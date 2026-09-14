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
use crate::newtype::{checked_str_newtype, checked_str_try_from};

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
///
/// A `Vec<u8>` rather than a `String`, and that is the second
/// thing it shares with `crate::remote::Stdin`. A password may
/// be latin-1, and a token need not be text at all. Nothing
/// between this type and the guest reads the contents as
/// characters: they go into a pipe and come out of a `cat` on
/// the far side. A `String` would refuse such a file with a
/// message about invalid UTF-8, and an operator reading that
/// goes looking at permissions.
#[derive(Clone, PartialEq, Eq)]
pub struct Secrets(Vec<u8>);

impl Secrets {
    /// The contents themselves, for whoever writes them into a
    /// pipe.
    ///
    /// Crate-private, so the "nothing renders them" rule above
    /// is something the compiler holds outside this crate.
    #[must_use]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Secrets holding `bytes`, for a test.
    ///
    /// Production code gets these out of the operator's file
    /// through [`EnvFilePath::read`], and a test wanting a
    /// [`super::Staged`] goes through `Config::staged_for_tests`
    /// rather than calling this directly.
    #[cfg(test)]
    pub(crate) fn for_tests(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
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
    ///
    /// "names no directory" rather than "is not set", because an
    /// exported-but-empty `HOME` takes this branch too. An
    /// operator told the variable is unset runs `echo $HOME`,
    /// sees an empty line, and cannot tell whether bombyx read a
    /// different environment.
    #[error(
        "`{field}` is `{value}`, and neither HOME nor USERPROFILE \
         names a directory -- both are unset or empty -- so `~` \
         names nothing"
    )]
    NoHome {
        /// Name of the offending field.
        field: &'static str,
        /// The value, as the operator wrote it.
        value: String,
    },

    /// The path names something that is not a regular file.
    ///
    /// A directory is the reachable case: `check` refuses every
    /// spelling that reads as one, and a path spelled as a file
    /// can still be a directory on disk.
    ///
    /// It also stands between bombyx and a character device or
    /// a fifo. Reading `/dev/zero` returns bytes for as long as
    /// bombyx is willing to hold them, and none of them is a
    /// secret.
    #[error("`{field}` names {path}, which is not a regular file")]
    NotAFile {
        /// Name of the offending field.
        field: &'static str,
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
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
    /// expanded, [`EnvFileError::NotAFile`] when the path names
    /// something other than a regular file, and
    /// [`EnvFileError::Read`] when the file is missing or cannot
    /// be opened.
    pub fn read<F>(&self, getenv: F) -> Result<Secrets, EnvFileError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let path = self.resolve(getenv)?;
        let read_error = |source| EnvFileError::Read {
            field: Self::FIELD,
            path: path.clone(),
            source,
        };

        // Asked before the read rather than after it. A read of
        // a fifo or a character device does not return, so a
        // check made afterwards is one that never runs.
        //
        // `metadata` follows a symlink, which is the right
        // question here: what matters is what bombyx will end
        // up reading, not how it was named.
        let meta = std::fs::metadata(&path).map_err(read_error)?;
        if !meta.is_file() {
            return Err(EnvFileError::NotAFile {
                field: Self::FIELD,
                path,
            });
        }

        let bytes = std::fs::read(&path).map_err(read_error)?;
        Ok(Secrets(bytes))
    }
}

checked_str_newtype!(
    EnvFilePath,
    "The value, as the operator wrote it in the config file."
);

checked_str_try_from!(
    /// What serde calls. It already owns the `String`, so the
    /// rules run against a borrow of it rather than a copy.
    EnvFilePath,
    FieldError,
    check
);

/// Checks an `env_file` value against every rule here.
///
/// # Errors
///
/// Returns [`FieldError::Empty`] when the value is blank, and
/// [`FieldError::Invalid`] naming `env_file` when the value
/// names a directory -- a bare `~`, a trailing separator, or a
/// final `.` or `..` segment -- or is neither `~/`-anchored nor
/// absolute on this machine.
fn check(value: &str) -> Result<(), FieldError> {
    guards::check_not_empty(EnvFilePath::FIELD, value)?;

    // No charset rule, unlike `deploy_key`. That path is pasted
    // into the generated Vagrantfile and quoted into a remote
    // shell, so a `"` or a `$` in it changes what runs. This one
    // reaches neither: bombyx hands it to `std::fs::read` on this
    // machine, and the file's contents travel on a pipe. A file
    // name holding a space or a quote is legal here.

    // Every spelling that names a directory rather than a file,
    // reported together and separately from the anchoring rule
    // below. These values do name something real, so "must be
    // absolute" would send the operator looking for the wrong
    // mistake.
    //
    // The whole family, not only the case that prompted the
    // rule: a bare `~`, a trailing separator, and a final `.` or
    // `..` segment. `~/` is both the first and the second, and
    // an absolute `/tmp/` is the second on its own.
    //
    // `std::path::is_separator` rather than `'/'`, because this
    // value is resolved on the machine bombyx was compiled for
    // and that machine may be Windows, where `\` separates too.
    // The rule beneath this one asks `Path::is_absolute`, which
    // already answers per platform; a `/`-only rule here would
    // let `C:\secrets\` through on Windows and refuse it later
    // with a message about a regular file.
    let last = value.rsplit(std::path::is_separator).next();
    let names_a_directory = value == "~"
        || value.ends_with(std::path::is_separator)
        || matches!(last, Some("." | ".."));
    if names_a_directory {
        return Err(invalid(
            "names a directory rather than a file; `env_file` has \
             to name the secrets file itself",
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

    /// The separator this platform writes its own paths with.
    ///
    /// Not always `/`. `check` asks `Path::is_absolute`, which
    /// answers for the machine bombyx was compiled for, so every
    /// other rule about the value has to answer for that machine
    /// too.
    const SEP: char = if cfg!(windows) { '\\' } else { '/' };

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
        for bad in ["secrets/x.env", "x.env", "./x.env", "../x.env"] {
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
    fn the_whole_family_of_directory_spellings_is_refused() {
        // Enumerated before the guard was written, because a
        // guard fixed only for the case that prompted it claims
        // more than it does. Three shapes name a directory: a
        // bare `~`, a trailing slash, and a final `.` or `..`
        // segment -- each in its `~/` form and its absolute one.
        //
        // These get their own message. They do name something
        // real, so the anchoring message above would send the
        // operator looking for the wrong mistake.
        let abs = HOME;
        let cases: Vec<String> = [
            "~".to_owned(),
            "~/".to_owned(),
            "~/.".to_owned(),
            "~/..".to_owned(),
            "~/secrets/".to_owned(),
            "~/secrets/.".to_owned(),
            "~/secrets/..".to_owned(),
            ".".to_owned(),
            "..".to_owned(),
            format!("{abs}/"),
            format!("{abs}/."),
            format!("{abs}/.."),
            // The platform's own separator, which on Windows is
            // not the one above. `check` asks
            // `Path::is_absolute`, which answers for the machine
            // bombyx was compiled for, so the directory rule has
            // to answer for that machine too.
            format!("{abs}{SEP}"),
            format!("{abs}{SEP}."),
            format!("{abs}{SEP}.."),
        ]
        .into();
        for bad in cases {
            let err = EnvFilePath::parse(&bad)
                .expect_err("a directory spelling must be refused");
            assert!(err.to_string().contains("directory"), "{bad}: {err}");
        }
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
    fn a_secrets_file_that_is_not_utf8_is_carried_all_the_same() {
        // A password is bytes. A latin-1 one, or a token that
        // is not text at all, has to reach the guest -- and the
        // contents never pass through anything that reads them
        // as characters, so nothing here needs them to be.
        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("x.env");
        std::fs::write(&file, b"PASS=\xff\xfe\n").expect("write");

        let p = EnvFilePath::parse(&file.display().to_string())
            .expect("a temp path is absolute");
        let secrets = p.read(nothing).expect("bytes are bytes");
        assert_eq!(secrets.as_bytes(), b"PASS=\xff\xfe\n");
    }

    #[test]
    fn a_path_that_is_not_a_regular_file_is_refused_before_the_read() {
        // A directory is the reachable case: `check` refuses
        // every spelling that looks like one, and a path
        // spelled as a file can still be a directory on disk.
        //
        // The check also stands between bombyx and a character
        // device or a fifo, where a read returns bytes for as
        // long as bombyx is willing to hold them.
        let dir = tempfile::tempdir().expect("a temp dir");
        let inner = dir.path().join("notafile");
        std::fs::create_dir(&inner).expect("mkdir");

        let p = EnvFilePath::parse(&inner.display().to_string())
            .expect("a temp path is absolute");
        let err = p.read(nothing).expect_err("a directory is not a file");
        assert!(matches!(err, EnvFileError::NotAFile { .. }), "{err}");
        assert!(err.to_string().contains("env_file"), "{err}");
    }

    #[test]
    fn the_contents_reach_secrets_and_no_render_of_it_holds_them() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("x.env");
        std::fs::write(&file, "TOKEN=hunter2\n").expect("write");

        let p = EnvFilePath::parse(&file.display().to_string())
            .expect("a temp path is absolute");
        let secrets = p.read(nothing).expect("the file is there");
        assert_eq!(secrets.as_bytes(), b"TOKEN=hunter2\n");
        assert_eq!(secrets.as_bytes().len(), 14);

        // The whole reason for the type: a payload that reached
        // the screen through a `{:?}` would undo the pipe.
        assert_eq!(format!("{secrets:?}"), "Secrets(14 bytes)");
    }
}
