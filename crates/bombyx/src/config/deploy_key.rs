//! What `deploy_key` may be: the path, on the workstation, of the
//! private key the guest clones a private repository with.
//!
//! The field is optional. A public repository needs no
//! credential, so most `[source]` tables leave it out. The key can
//! come from the vault instead, as an attachment; `super::vault`
//! holds that form.
//!
//! **The path names a file on the machine bombyx runs on**, as
//! `env_file`'s does, and follows the same rules: bombyx opens the
//! file itself, reads it, and carries its bytes to the VM host,
//! which stages them only for the `vagrant` run. So the VM host
//! stores no key between runs. `docs/trust-boundary.md` holds the
//! rule and what it costs.
//!
//! Two things live here, as in `super::env_file`. [`DeployKeyPath`]
//! is the value the config states, checked while the config is
//! read. [`DeployKey`] is the key itself, which bombyx reads later
//! and hands to a pipe.

use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use super::error::FieldError;
use super::workstation_path::{
    self, WorkstationFileError, read_capped, resolve_file,
};
use crate::newtype::{checked_str_newtype, checked_str_try_from};

/// A private key file on the workstation, as the operator wrote it.
///
/// This is a *newtype*: a struct wrapping one `String`, where
/// the `String` inside is private. You cannot build one
/// directly. You have to call [`DeployKeyPath::parse`], which
/// runs the private `check` first. So holding one is the proof
/// that every rule in this module ran.
///
/// A `String` rather than a `PathBuf` for the reason
/// `super::EnvFilePath` gives: the value is stored as the operator
/// typed it, and its leading `~/` is expanded when the file is
/// opened.
///
/// `#[serde(try_from = "String")]` is what connects the type to
/// the config file. Without it serde would assign the private
/// field directly and skip every rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct DeployKeyPath(String);

/// A private key, read from the workstation or from the vault.
///
/// Holding one is the proof the bytes look like a private key:
/// `DeployKey::from_bytes` is the only way in. The check is
/// about shape, not validity. It refuses the two mistakes an
/// operator makes, naming the `.pub` half and naming a file that
/// is not a key at all, and it does not parse the key.
///
/// **Nothing renders the bytes.** There is no `Display`, and
/// `Debug` reports a length, for the reason `super::Secrets`
/// gives.
#[derive(Clone, PartialEq, Eq)]
pub struct DeployKey(Vec<u8>);

/// The bytes did not start the way a private key file does.
///
/// A unit error, because each caller names its own source: the
/// file `deploy_key` names, or the vault attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NotAKey;

/// What every refusal of a [`NotAKey`] tells the operator, so the
/// file's message and the vault's give the one reason
/// `DeployKey::from_bytes` checks.
pub(super) const NOT_A_KEY: &str = "does not start with a \
    `-----BEGIN ... PRIVATE KEY-----` line; it has to be the private \
    half of the key pair, not the `.pub` file";

/// Why bombyx could not read the key `deploy_key` names.
#[derive(Debug, Error)]
pub enum DeployKeyError {
    /// The file could not be read, for any reason but a missing
    /// file, including a `~` with no home directory to expand it
    /// against.
    #[error(transparent)]
    File(#[from] WorkstationFileError),

    /// This machine has no file at the path.
    ///
    /// The message says where bombyx looked, because a key kept on
    /// the VM host is the likely reason: bombyx reads it here and
    /// carries it there for the run.
    #[error(
        "`deploy_key` names {path}, which this machine does not have; \
         bombyx reads the key on the workstation and carries it to \
         the VM host for the run, so the key belongs here"
    )]
    Missing {
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
    },

    /// The file does not hold a private key.
    #[error("`deploy_key` names {path}, which {reason}", reason = NOT_A_KEY)]
    NotAKey {
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
    },
}

impl DeployKeyPath {
    /// The config key this type reads, and the name every one
    /// of its errors reports against.
    pub const FIELD: &'static str = "deploy_key";

    /// Checks `raw` against every rule here and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] naming `deploy_key` when it breaks
    /// any other rule `check` holds.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// Reads the key this path names.
    ///
    /// `getenv` reads this machine's environment, for the `~` in
    /// the path, as [`super::EnvFilePath::read`]'s does.
    ///
    /// # Errors
    ///
    /// Returns [`DeployKeyError::Missing`] when the file is not
    /// there, [`DeployKeyError::NotAKey`] when it does not hold a
    /// private key, and [`DeployKeyError::File`] for every other
    /// reason the read failed, which
    /// [`super::EnvFilePath::read`] lists.
    pub fn read<F>(&self, getenv: F) -> Result<DeployKey, DeployKeyError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let path = resolve_file(Self::FIELD, &self.0, getenv)?;
        let bytes = match read_capped(Self::FIELD, path.clone()) {
            Ok(bytes) => bytes,
            Err(WorkstationFileError::Read { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Err(DeployKeyError::Missing { path });
            }
            Err(e) => return Err(e.into()),
        };
        DeployKey::from_bytes(bytes)
            .map_err(|NotAKey| DeployKeyError::NotAKey { path })
    }
}

impl DeployKey {
    /// Wraps `bytes` once they look like a private key file.
    ///
    /// # Errors
    ///
    /// Returns [`NotAKey`] unless the first line is a PEM or
    /// OpenSSH armour line for a private key.
    pub(super) fn from_bytes(bytes: Vec<u8>) -> Result<Self, NotAKey> {
        // Both armours OpenSSH reads put the type on the first
        // line: `-----BEGIN OPENSSH PRIVATE KEY-----` for its own
        // format, and `-----BEGIN RSA PRIVATE KEY-----` and the
        // like for PEM. A `.pub` file starts with the algorithm
        // name instead, so it fails here rather than as an
        // authentication failure inside the guest.
        let first = bytes.split(|&b| b == b'\n').next().unwrap_or_default();
        let first = first.strip_suffix(b"\r").unwrap_or(first);
        if first.starts_with(b"-----BEGIN ")
            && first.ends_with(b"PRIVATE KEY-----")
        {
            Ok(Self(bytes))
        } else {
            Err(NotAKey)
        }
    }

    /// The key itself, for whoever writes it into a pipe.
    ///
    /// Crate-private, so the "nothing renders it" rule above is
    /// something the compiler holds outside this crate.
    #[must_use]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// A key holding a fixed, fake body, for a test.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self(TEST_KEY.to_vec())
    }
}

/// A key-shaped body that is no real key.
#[cfg(test)]
const TEST_KEY: &[u8] = b"-----BEGIN OPENSSH PRIVATE KEY-----\n\
    b3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----\n";

impl fmt::Debug for DeployKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeployKey({} bytes)", self.0.len())
    }
}

checked_str_newtype!(
    DeployKeyPath,
    "The value, as the operator wrote it in the config file."
);

checked_str_try_from!(
    /// What serde calls. It already owns the `String`, so the
    /// rules run against a borrow of it rather than a copy.
    DeployKeyPath,
    FieldError,
    check
);

/// Checks a `deploy_key` value against every rule here.
///
/// # Errors
///
/// Returns what `super::workstation_path::check_file` returns,
/// naming `deploy_key`.
fn check(value: &str) -> Result<(), FieldError> {
    workstation_path::check_file(DeployKeyPath::FIELD, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory holding one file per test, removed when the
    /// guard drops.
    fn scratch(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(name);
        (dir, path)
    }

    /// The path of `path`, as a config value.
    fn parsed(path: &std::path::Path) -> DeployKeyPath {
        DeployKeyPath::parse(&path.display().to_string())
            .expect("an absolute path")
    }

    fn nothing(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_workstation_path_is_accepted_and_a_relative_one_is_not() {
        assert!(DeployKeyPath::parse("~/.ssh/p-deploy-key").is_ok());
        let err = DeployKeyPath::parse("keys/k").expect_err("relative");
        assert!(err.to_string().contains("deploy_key"), "{err}");
        assert!(err.to_string().contains("must start with `~/`"), "{err}");
    }

    #[test]
    fn a_path_naming_a_directory_is_refused() {
        for bad in ["~", "~/", "~/.ssh/.."] {
            let err = DeployKeyPath::parse(bad).expect_err("a directory");
            assert!(err.to_string().contains("names a directory"), "{err}");
        }
    }

    #[test]
    fn a_key_file_is_read_whole() {
        let (_dir, path) = scratch("k");
        std::fs::write(&path, TEST_KEY).expect("write");
        let key = parsed(&path).read(nothing).expect("a key");
        assert_eq!(key.as_bytes(), TEST_KEY);
    }

    #[test]
    fn a_missing_file_says_the_key_belongs_on_the_workstation() {
        let (_dir, path) = scratch("absent");
        let err = parsed(&path).read(nothing).expect_err("missing");
        assert!(matches!(err, DeployKeyError::Missing { .. }), "{err:?}");
        assert!(err.to_string().contains("belongs here"), "{err}");
    }

    #[test]
    fn a_public_key_or_an_empty_file_is_not_a_key() {
        let (_dir, path) = scratch("k.pub");
        for body in [&b"ssh-ed25519 AAAAC3Nz op@ws\n"[..], b""] {
            std::fs::write(&path, body).expect("write");
            let err = parsed(&path).read(nothing).expect_err("not a key");
            assert!(matches!(err, DeployKeyError::NotAKey { .. }), "{err:?}");
        }
    }

    #[test]
    fn the_armour_line_must_name_a_private_key() {
        let good: [&[u8]; 4] = [
            TEST_KEY,
            b"-----BEGIN RSA PRIVATE KEY-----\nx\n",
            b"-----BEGIN PRIVATE KEY-----\nx\n",
            b"-----BEGIN EC PRIVATE KEY-----\r\nx\r\n",
        ];
        for body in good {
            assert!(DeployKey::from_bytes(body.to_vec()).is_ok(), "{body:?}");
        }
        let bad: [&[u8]; 5] = [
            b"",
            b"-----BEGIN PUBLIC KEY-----\nx\n",
            b"-----BEGIN CERTIFICATE-----\nx\n",
            b"PuTTY-User-Key-File-3: ssh-ed25519\n",
            b"x-----BEGIN OPENSSH PRIVATE KEY-----\n",
        ];
        for body in bad {
            assert_eq!(DeployKey::from_bytes(body.to_vec()), Err(NotAKey));
        }
    }

    #[test]
    fn debug_reports_a_length_and_no_bytes() {
        let shown = format!("{:?}", DeployKey::for_tests());
        assert_eq!(shown, format!("DeployKey({} bytes)", TEST_KEY.len()));
    }
}
