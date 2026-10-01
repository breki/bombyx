//! A path to a file on the workstation: the machine bombyx runs
//! on, as opposed to the VM host.
//!
//! Three fields name one, `env_file`, `deploy_key` and
//! `vault.database`, and all three are spelled and opened the same
//! way. So their rules live here, and each field's module states
//! only what differs.

use std::path::{Path, PathBuf};

use super::error::FieldError;
use super::guards;

/// A `~/` value with no home directory to expand it against.
///
/// Its own type, rather than a variant of one field's error, so
/// each field's module can report it in its own terms.
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
