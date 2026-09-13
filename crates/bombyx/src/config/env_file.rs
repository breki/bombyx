//! What `env_file` may be: the path, on the workstation, of a
//! file holding the project's secrets.
//!
//! The field is optional. A project whose provisioning needs no
//! credential leaves it out.
//!
//! Two things live here, and they are separate on purpose.
//! [`EnvFilePath`] is the value the config file states, and it
//! is checked the moment the config is read. [`Secrets`] is the
//! file's contents, which bombyx reads later and hands to a
//! pipe.
//!
//! **The path names a file on the machine bombyx runs on**,
//! unlike `super::DeployKeyPath`, whose path names one on the VM
//! host. That difference decides every rule below. bombyx opens
//! this file itself, so it can say what a value resolved to; it
//! never gives the path to a shell, so nothing here is about
//! quoting.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use super::error::FieldError;
use super::guards;
use crate::newtype::checked_str_newtype;

/// A secrets file on the workstation, as the operator wrote it.
///
/// This is a *newtype*: a struct wrapping one `String`, where
/// the `String` inside is private. You cannot build one
/// directly. You have to call [`EnvFilePath::parse`], which
/// runs the private `check` first. So holding one is the proof
/// that every rule in this module ran.
///
/// A `PathBuf` would be the wrong representation, for a reason
/// the opposite of `super::DeployKeyPath`'s. That path is
/// expanded on the VM host, so `PathBuf` would answer for the
/// wrong machine. This one is expanded here, and `PathBuf`
/// would answer correctly -- but it has no idea what a leading
/// `~/` means, and expanding that is [`EnvFilePath::read`]'s
/// job. The value is stored as the operator typed it and turns
/// into a `PathBuf` at the moment it is opened.
///
/// `#[serde(try_from = "String")]` is what connects the type to
/// the config file. Without it serde would assign the private
/// field directly and skip every rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct EnvFilePath(String);

/// The contents of the file [`EnvFilePath`] names.
///
/// Any bytes at all are legal, so the rule this type carries is
/// not about their shape: **nothing renders them.** There is no
/// `Display`, and `Debug` reports a length. The same rule
/// `crate::remote::Stdin` carries, for the same reason -- a
/// payload that reached the screen through a `{:?}` in an error
/// message would undo the whole design.
#[derive(Clone, PartialEq, Eq)]
pub struct Secrets(String);

impl Secrets {
    /// The contents themselves, for whoever writes them into a
    /// pipe.
    ///
    /// Crate-private, so the "nothing renders them" rule above
    /// is something the compiler holds outside this crate.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// How many bytes there are, which is all any render says.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the file was empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secrets({} bytes)", self.0.len())
    }
}

/// Why bombyx could not read the file `env_file` names.
///
/// Separate from [`FieldError`], which belongs to a value's
/// shape and is raised while the config is being parsed. These
/// happen later, when the file is opened, and every one of them
/// names the path bombyx actually tried.
#[derive(Debug, Error)]
pub enum EnvFileError {
    /// The value starts with `~/` and this machine's
    /// environment names no home directory.
    #[error(
        "`{field}` is `{value}`, and neither HOME nor USERPROFILE \
         is set, so `~` names nothing"
    )]
    NoHome {
        /// Name of the offending field.
        field: &'static str,
        /// The value, as the operator wrote it.
        value: String,
    },

    /// The file could not be opened.
    ///
    /// The operating system's own words are left to `source`
    /// rather than written into this line. The binary prints an
    /// error together with its causes, so interpolating it here
    /// would report "No such file or directory" twice.
    #[error("`{field}` names {path}, which could not be read")]
    Read {
        /// Name of the offending field.
        field: &'static str,
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
}

impl EnvFilePath {
    /// The config key this type reads, and the name every one
    /// of its errors reports against.
    pub const FIELD: &'static str = "env_file";

    /// Checks `raw` against every rule here and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] naming `env_file` when it breaks
    /// any other rule `check` holds.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// Expands a leading `~/` and returns the path bombyx will
    /// open.
    ///
    /// `getenv` reads this machine's environment. It is a
    /// parameter rather than a call to
    /// [`std::env::var`] so the tests below can
    /// state a home directory instead of depending on the one
    /// the test runner happens to have.
    ///
    /// `HOME` is consulted before `USERPROFILE`. Git Bash on
    /// Windows sets both, and its `HOME` is the POSIX form,
    /// which is the one that joins onto the rest of a `~/`
    /// value without a separator disagreement.
    ///
    /// # Errors
    ///
    /// Returns [`EnvFileError::NoHome`] when the value needs a
    /// home directory and the environment names none.
    pub fn resolve<F>(&self, getenv: F) -> Result<PathBuf, EnvFileError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let Some(rest) = self.0.strip_prefix("~/") else {
            return Ok(PathBuf::from(&self.0));
        };
        let home = getenv("HOME")
            .or_else(|| getenv("USERPROFILE"))
            .filter(|h| !h.is_empty())
            .ok_or_else(|| EnvFileError::NoHome {
                field: Self::FIELD,
                value: self.0.clone(),
            })?;
        Ok(Path::new(&home).join(rest))
    }

    /// Reads the file this path names.
    ///
    /// The refusal is what the operator sees when the path is
    /// wrong, so it names the expanded path rather than the
    /// value: `~/secrets/x.env` tells nobody which directory
    /// was searched.
    ///
    /// # Errors
    ///
    /// Returns [`EnvFileError::NoHome`] when `~` cannot be
    /// expanded, and [`EnvFileError::Read`] when the file is
    /// missing, is a directory, or cannot be opened.
    pub fn read<F>(&self, getenv: F) -> Result<Secrets, EnvFileError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let path = self.resolve(getenv)?;
        let text = std::fs::read_to_string(&path).map_err(|source| {
            EnvFileError::Read {
                field: Self::FIELD,
                path: path.clone(),
                source,
            }
        })?;
        Ok(Secrets(text))
    }
}

checked_str_newtype!(
    EnvFilePath,
    "The value, as the operator wrote it in the config file."
);

impl TryFrom<String> for EnvFilePath {
    type Error = FieldError;

    /// What serde calls. It already owns the `String`, so the
    /// rules run against a borrow of it rather than a copy.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check(&raw)?;
        Ok(Self(raw))
    }
}

/// Checks an `env_file` value against every rule here.
///
/// # Errors
///
/// Returns [`FieldError::Empty`] when the value is blank, and
/// [`FieldError::Invalid`] naming `env_file` when the value is
/// a bare `~`, or is neither `~/`-anchored nor absolute on this
/// machine.
fn check(value: &str) -> Result<(), FieldError> {
    guards::check_not_empty(EnvFilePath::FIELD, value)?;

    // No charset rule, unlike `deploy_key`. That path is pasted
    // into the generated Vagrantfile and quoted into a remote
    // shell, so a `"` or a `$` in it changes what runs. This one
    // reaches neither: bombyx hands it to `std::fs::read_to_string`
    // on this machine, and the file's contents travel on a pipe.
    // A file name holding a space or a quote is legal here.

    // A bare `~` is the home directory, which is not a file.
    // Reported on its own because the value does name something
    // real, and the general message below would mislead.
    if value == "~" {
        return Err(invalid(
            "names the home directory, which is a directory rather \
             than a file",
        ));
    }

    // `Path::is_absolute` answers for the machine bombyx was
    // compiled for, and that is the machine that opens this file,
    // so it is the right question here. On Windows it wants a
    // drive, so `C:\secrets\x.env` passes and `\secrets\x.env`
    // does not -- the second is relative to whichever drive the
    // process is on.
    if !value.starts_with("~/") && !Path::new(value).is_absolute() {
        return Err(invalid(
            "must start with `~/` or be an absolute path; a relative \
             path resolves against whatever directory bombyx was \
             started in",
        ));
    }

    Ok(())
}

/// Builds a [`FieldError::Invalid`] naming `env_file`.
fn invalid(reason: impl Into<String>) -> FieldError {
    FieldError::Invalid {
        field: EnvFilePath::FIELD,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home directory every test below expands against, so
    /// the expected paths and the environment agree.
    const HOME: &str = if cfg!(windows) {
        r"C:\home\i"
    } else {
        "/home/i"
    };

    /// An environment naming `HOME` and nothing else.
    fn home_only(key: &str) -> Option<String> {
        (key == "HOME").then(|| HOME.to_owned())
    }

    /// An environment naming nothing at all.
    fn nothing(_: &str) -> Option<String> {
        None
    }

    /// A value every rule accepts on this platform.
    fn absolute() -> String {
        Path::new(HOME).join("x.env").display().to_string()
    }

    #[test]
    fn a_tilde_anchored_path_and_an_absolute_one_are_both_accepted() {
        assert_eq!(
            EnvFilePath::parse("~/secrets/x.env")
                .expect("a `~/` path must be accepted")
                .as_str(),
            "~/secrets/x.env"
        );
        let abs = absolute();
        assert_eq!(
            EnvFilePath::parse(&abs)
                .expect("an absolute path must be accepted")
                .as_str(),
            abs
        );
    }

    #[test]
    fn a_blank_value_is_refused_as_empty() {
        assert_eq!(
            EnvFilePath::parse("   "),
            Err(FieldError::Empty {
                field: EnvFilePath::FIELD
            })
        );
    }

    #[test]
    fn the_whole_family_of_unanchored_values_is_refused() {
        // Enumerated rather than fixed one at a time, so the
        // guard covers what its message claims. A relative path
        // resolves against bombyx's own working directory, which
        // nobody chose.
        for bad in [
            "secrets/x.env",
            "x.env",
            "./x.env",
            "../x.env",
            ".",
            "..",
            "~",
        ] {
            let err = EnvFilePath::parse(bad)
                .expect_err("an unanchored value must be refused");
            assert!(
                matches!(err, FieldError::Invalid { field, .. }
                    if field == EnvFilePath::FIELD),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn a_bare_tilde_says_it_is_a_directory() {
        // Separate from the message above: `~` does name
        // something real, and "must be absolute" would send the
        // operator looking for the wrong mistake.
        let err = EnvFilePath::parse("~").expect_err("`~` must be refused");
        assert!(err.to_string().contains("directory"), "{err}");
    }

    #[test]
    fn a_file_name_holding_a_quote_or_a_space_is_accepted() {
        // The value reaches no shell. bombyx opens it with
        // `std::fs`, and the contents travel on a pipe. A rule
        // refusing these would be protecting nothing and would
        // refuse a legal file name.
        for ok in ["~/my secrets/x.env", "~/it's/x.env", "~/a$b/x.env"] {
            assert!(
                EnvFilePath::parse(ok).is_ok(),
                "{ok} names a real file and must be accepted"
            );
        }
    }

    #[test]
    fn a_tilde_expands_against_home_and_an_absolute_path_is_left_alone() {
        let p = EnvFilePath::parse("~/secrets/x.env").expect("valid");
        assert_eq!(
            p.resolve(home_only).expect("HOME is set"),
            Path::new(HOME).join("secrets").join("x.env")
        );

        let abs = absolute();
        let p = EnvFilePath::parse(&abs).expect("valid");
        assert_eq!(
            p.resolve(nothing).expect("no home is needed"),
            PathBuf::from(&abs)
        );
    }

    #[test]
    fn userprofile_answers_when_home_does_not() {
        let p = EnvFilePath::parse("~/x.env").expect("valid");
        assert_eq!(
            p.resolve(|k| (k == "USERPROFILE").then(|| HOME.to_owned()))
                .expect("USERPROFILE is set"),
            Path::new(HOME).join("x.env")
        );
    }

    #[test]
    fn an_empty_home_is_treated_as_no_home_at_all() {
        // An exported-but-empty `HOME` is common in a service
        // account's environment, and joining onto it would give
        // a relative path -- the thing `check` exists to refuse.
        let p = EnvFilePath::parse("~/x.env").expect("valid");
        let err = p
            .resolve(|k| (k == "HOME").then(String::new))
            .expect_err("an empty HOME names nothing");
        assert!(matches!(err, EnvFileError::NoHome { .. }), "{err}");
        assert!(err.to_string().contains("env_file"), "{err}");
    }

    #[test]
    fn a_missing_file_names_the_expanded_path_and_the_field() {
        // The operator wrote `~/...` and has to be told which
        // directory bombyx actually searched.
        let p = EnvFilePath::parse("~/no-such-dir/x.env").expect("valid");
        let err = p.read(home_only).expect_err("the file is not there");
        let text = err.to_string();
        assert!(text.contains("env_file"), "{text}");
        assert!(text.contains("no-such-dir"), "{text}");
        assert!(
            !text.contains('~'),
            "the expanded path is the point: {text}"
        );
    }

    #[test]
    fn the_contents_reach_secrets_and_no_render_of_it_holds_them() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("x.env");
        std::fs::write(&file, "TOKEN=hunter2\n").expect("write");

        let p = EnvFilePath::parse(&file.display().to_string())
            .expect("a temp path is absolute");
        let secrets = p.read(nothing).expect("the file is there");
        assert_eq!(secrets.as_str(), "TOKEN=hunter2\n");
        assert_eq!(secrets.len(), 14);
        assert!(!secrets.is_empty());

        // The whole reason for the type: a payload that reached
        // the screen through a `{:?}` would undo the pipe.
        assert_eq!(format!("{secrets:?}"), "Secrets(14 bytes)");
    }
}
