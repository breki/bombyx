//! What can go wrong while reading a configuration, and what
//! can go wrong with one field of it.
//!
//! Two types, and the split is the point.
//!
//! [`ConfigError`] belongs to *loading* a config: the file was
//! missing, unreadable, not TOML, carried a forbidden key, or
//! named no VM host. Ten variants, most of them about a file.
//!
//! [`FieldError`] belongs to *one value*: it was blank, or it
//! broke a rule. Two variants, and no mention of files.
//!
//! The reason for two is that [`RepoUrl`](super::RepoUrl) and
//! [`ScriptPath`](super::ScriptPath) can be built by anyone, on
//! a string from anywhere, with no config file in sight. Handing
//! their callers an error type with a "config file is larger
//! than 64 KiB" variant would make matching on the result
//! meaningless. A `FieldError` converts into a `ConfigError`
//! when one does turn up during loading, so nothing downstream
//! has to know which kind it started as.

use std::path::PathBuf;

use thiserror::Error;

use super::MAX_CONFIG_BYTES;
use super::host::HOST_ENV;

/// A single configuration value broke its own rule.
///
/// Returned by the field guards and by the newtype
/// constructors, none of which knows or cares whether a config
/// file is involved.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FieldError {
    /// A required field was present but empty.
    #[error("`{field}` must not be empty")]
    Empty {
        /// Name of the offending field.
        field: &'static str,
    },

    /// A field held a value outside its allowed shape.
    #[error("invalid `{field}`: {reason}")]
    Invalid {
        /// Name of the offending field.
        field: &'static str,
        /// What rule the value broke.
        reason: String,
    },
}

impl FieldError {
    /// Shorthand for [`FieldError::Invalid`] with an owned
    /// reason, which is how every guard builds one.
    pub(crate) fn invalid(field: &'static str, reason: &str) -> Self {
        Self::Invalid {
            field,
            reason: reason.to_owned(),
        }
    }
}

/// Errors produced while loading a project configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The configuration file does not exist.
    #[error("config file not found: {}", .0.display())]
    NotFound(PathBuf),

    /// The configuration file could not be read.
    #[error("failed to read {}: {source}", .path.display())]
    Read {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// A configuration path exists but is not a regular file.
    #[error("{} is not a regular file", .0.display())]
    NotAFile(PathBuf),

    /// A configuration file is implausibly large.
    #[error("{} is larger than {MAX_CONFIG_BYTES} bytes", .0.display())]
    TooLarge(PathBuf),

    /// The configuration file is not valid TOML, or does not
    /// match the expected shape.
    ///
    /// **Carries a summary, not the `toml` crate's `Display`.**
    /// That rendering quotes the offending *source line* into the
    /// message, and bombyx printed it straight to stderr:
    ///
    /// ```text
    /// bombyx: loading bombyx.toml: invalid config in bombyx.toml:
    /// TOML parse error at line 1, column 12
    ///   |
    /// 1 | -----BEGIN OPENSSH PRIVATE KEY-----
    /// ```
    ///
    /// Reproduced against the built binary. `bombyx.toml` can be a
    /// symlink -- the overlay path refuses one, the base path does
    /// not, and nobody inspects a config after a clone -- so a
    /// hostile repo could aim it at `~/.ssh/id_ed25519` and have a
    /// line of it echoed.
    ///
    /// What is kept is the part that helps: the position and the
    /// reason. What is dropped is the quoted line. Naming
    /// `line 1, column 12` and "expected an equals" is enough to
    /// correct a malformed config; the file's own contents are not
    /// bombyx's to print.
    #[error("invalid config in {}: {summary}", .path.display())]
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Position and reason, without the source snippet.
        summary: String,
    },

    /// A required field was present but empty.
    #[error("`{field}` must not be empty")]
    Empty {
        /// Name of the offending field.
        field: &'static str,
    },

    /// A field held a value outside its allowed shape.
    #[error("invalid `{field}`: {reason}")]
    Invalid {
        /// Name of the offending field.
        field: &'static str,
        /// What rule the value broke.
        reason: String,
    },

    /// The committed project file carried a `host` key.
    ///
    /// Refused rather than ignored. A committed host names one
    /// developer's machine, and a stale one that still parses
    /// is how `destroy` ends up deleting a directory on a
    /// colleague's host.
    #[error(
        "`host` is not allowed in {}: it names one developer's \
         machine, and this file is committed. Move that line to \
         {places}",
        .path.display()
    )]
    HostInProjectFile {
        /// The project file carrying the key.
        path: PathBuf,
        /// Where the value belongs instead.
        places: String,
    },

    /// No source supplied a VM host.
    #[error(
        "no VM host configured -- set it in {places}, pass \
         --host, or set {HOST_ENV}"
    )]
    HostMissing {
        /// The files that would supply one.
        places: String,
    },

    /// The winning source supplied an unusable host.
    ///
    /// Separate from [`ConfigError::Invalid`] so the message can
    /// name *where the value came from*. As a plain field error
    /// the only path in it was the project file's -- the one file
    /// now forbidden to carry a host -- so the message sent the
    /// operator to edit the wrong thing.
    #[error("invalid VM host from {origin}: {reason}")]
    InvalidHost {
        /// Which source supplied it.
        origin: String,
        /// What rule the value broke.
        reason: String,
    },
}

/// Widens a one-field failure into a loading failure.
///
/// The two variants map across unchanged, so a caller matching
/// `ConfigError::Invalid { field, .. }` sees the same thing
/// whether the check ran during loading or inside a newtype
/// constructor.
impl From<FieldError> for ConfigError {
    fn from(err: FieldError) -> Self {
        match err {
            FieldError::Empty { field } => Self::Empty { field },
            FieldError::Invalid { field, reason } => {
                Self::Invalid { field, reason }
            }
        }
    }
}
