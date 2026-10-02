//! A path to a file on the workstation: the machine bombyx runs
//! on, as opposed to the VM host.
//!
//! Three fields name one, `env_file`, `deploy_key` and
//! `vault.database`. All three follow the same spelling rules and
//! the same `~` expansion, so those live here, and each field's
//! module states only what differs.
//!
//! `env_file` and `deploy_key` are also read the same way, through
//! `read_capped`, which refuses a file over `MAX_FILE_BYTES` and
//! reports every failure as a [`WorkstationFileError`].
//! `vault.database` is not read here: bombyx hands its path to
//! another program.

use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::error::FieldError;
use super::guards;

/// A `~/` value with no home directory to expand it against.
///
/// Its own type, rather than a variant of one field's error, so
/// each field's module can report it in its own terms: `vault`
/// turns it into `VaultError::NoHome`, and `resolve_file` turns it
/// into `WorkstationFileError::NoHome` for `env_file` and
/// `deploy_key`.
#[derive(Debug)]
pub(super) struct NoHome {
    /// The config key naming the value.
    pub(super) field: &'static str,
    /// The value, as the operator wrote it.
    pub(super) value: String,
}

/// Expands a leading `~/` in `value`, the path of a file on this
/// machine that the config key `field` names.
///
/// `HOME` is consulted before `USERPROFILE`. Git Bash on Windows
/// sets both, and its `HOME` is the POSIX form, which is the one
/// that joins onto the rest of a `~/` value without a separator
/// disagreement. A variable that is set but empty counts as
/// unset.
///
/// # Errors
///
/// Returns [`NoHome`] naming `field` when the value needs a home
/// directory and the environment names none.
pub(super) fn resolve_home<F>(
    field: &'static str,
    value: &str,
    getenv: F,
) -> Result<PathBuf, NoHome>
where
    F: Fn(&str) -> Option<String>,
{
    let Some(rest) = value.strip_prefix("~/") else {
        return Ok(PathBuf::from(value));
    };
    let home = getenv("HOME")
        .or_else(|| getenv("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .ok_or_else(|| NoHome {
            field,
            value: value.to_owned(),
        })?;
    Ok(Path::new(&home).join(rest))
}

/// Checks the path of a file on this machine, which the config
/// key `field` names.
///
/// Every such field refuses the same spellings: those that name a
/// directory, and those whose meaning depends on the directory
/// bombyx was started in.
///
/// # Errors
///
/// Returns [`FieldError::Empty`] when the value is blank, and
/// [`FieldError::Invalid`] naming `field` when the value names a
/// directory -- a bare `~`, a trailing separator, or a final `.`
/// or `..` segment -- or is neither `~/`-anchored nor absolute
/// on this machine.
pub(super) fn check_file(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    guards::check_not_empty(field, value)?;

    // No charset rule, unlike the values written into the
    // generated Vagrantfile or quoted into a remote shell, where
    // a `"` or a `$` changes what runs. This path reaches
    // neither: bombyx opens it on this machine, or hands it to a
    // program as one argument, and no shell reads it. A file
    // name holding a space or a quote is legal here.

    // Every spelling that names a directory rather than a file,
    // reported together and separately from the anchoring rule
    // below. These values do name something real, so "must be
    // absolute" would send the operator looking for the wrong
    // mistake.
    //
    // The whole family: a bare `~`, a trailing separator, and a
    // final `.` or `..` segment. `~/` and an absolute `/tmp/` are
    // both caught as a trailing separator.
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
        return Err(FieldError::invalid(
            field,
            "names a directory rather than a file; it has to name \
             the file itself",
        ));
    }

    // `Path::is_absolute` answers for the machine bombyx was
    // compiled for, and that is the machine that opens this file,
    // so it is the right question here. On Windows it wants a
    // drive, so `C:\secrets\x.env` passes and `\secrets\x.env`
    // does not -- the second is relative to whichever drive the
    // process is on.
    if !value.starts_with("~/") && !Path::new(value).is_absolute() {
        return Err(FieldError::invalid(
            field,
            "must start with `~/` or be an absolute path; a relative \
             path resolves against whatever directory bombyx was \
             started in",
        ));
    }

    Ok(())
}

/// Largest file [`read_capped`] will read: a secrets file or a
/// deploy key.
///
/// A secrets file holds a handful of `NAME=value` lines and a
/// private key a few kilobytes, so the limit costs a real one
/// nothing. What it buys: the path is checked with `metadata` and
/// opened afterwards, and whoever can write the containing
/// directory can swap a regular file for something that never ends
/// between those two calls. The cap bounds what bombyx holds in
/// memory either way. `super::read::MAX_CONFIG_BYTES` is the same
/// number for the same reason.
pub(super) const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Why bombyx could not read a file on the workstation that
/// `env_file` or `deploy_key` names.
///
/// Every variant carries the field it was read for, so the one
/// type serves both.
///
/// Separate from [`FieldError`], which belongs to a value's
/// shape and is raised while the config is being parsed. These
/// happen later, when the file is opened, and every one of them
/// names the path bombyx actually tried.
#[derive(Debug, Error)]
pub enum WorkstationFileError {
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
    /// A directory is the reachable case: `check_file` refuses every
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

    /// The file is larger than `MAX_FILE_BYTES`.
    ///
    /// The limit is in the message because the operator cannot
    /// otherwise tell how far over the file is, and the number
    /// is the one thing they can act on.
    #[error(
        "`{field}` names {path}, which is larger than the \
         {limit} byte limit on a file bombyx reads"
    )]
    TooLarge {
        /// Name of the offending field.
        field: &'static str,
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
        /// The limit, in bytes.
        limit: u64,
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

/// Expands a leading `~/` in `value`, the file the config key
/// `field` names, reporting a missing home as [`WorkstationFileError`].
///
/// `super::env_file` and `super::deploy_key` both call it, so the
/// two fields report it the same way.
///
/// # Errors
///
/// Returns [`WorkstationFileError::NoHome`] when the value needs a home
/// directory and the environment names none.
pub(super) fn resolve_file<F>(
    field: &'static str,
    value: &str,
    getenv: F,
) -> Result<PathBuf, WorkstationFileError>
where
    F: Fn(&str) -> Option<String>,
{
    resolve_home(field, value, getenv).map_err(|e| {
        WorkstationFileError::NoHome {
            field: e.field,
            value: e.value,
        }
    })
}

/// Reads the regular file at `path`, which the config key `field`
/// names, refusing anything larger than `MAX_FILE_BYTES`.
///
/// `super::env_file` and `super::deploy_key` both open their file
/// through it. Every error names `field`, so each caller's refusal
/// points at its own config line. Its tests drive it through
/// `EnvFilePath::read`, in `env_file`, which owns the fixtures.
///
/// # Errors
///
/// Returns [`WorkstationFileError::NotAFile`] when the path names
/// something other than a regular file,
/// [`WorkstationFileError::Read`] when the file is missing or
/// cannot be opened, and [`WorkstationFileError::TooLarge`] when
/// it is bigger than the cap.
pub(super) fn read_capped(
    field: &'static str,
    path: PathBuf,
) -> Result<Vec<u8>, WorkstationFileError> {
    let read_error = |source| WorkstationFileError::Read {
        field,
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
        return Err(WorkstationFileError::NotAFile { field, path });
    }

    // One byte past the cap, so a file *at* the limit is
    // read whole and anything beyond it is detectable
    // rather than silently truncated into a secrets file
    // the guest would accept and half-understand.
    //
    // `take` rather than a length taken from `meta`: the
    // path is re-opened here, so the file the read gets is
    // not provably the file `metadata` answered about.
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(read_error)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(WorkstationFileError::TooLarge {
            field,
            path,
            limit: MAX_FILE_BYTES,
        });
    }
    Ok(bytes)
}
