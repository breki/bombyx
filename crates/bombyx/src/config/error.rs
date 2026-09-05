//! What can go wrong while reading a configuration, and what
//! can go wrong with one field of it.
//!
//! The module has two error types, and they are separate for a
//! reason.
//!
//! [`ConfigError`] belongs to *loading* a config. The project
//! file was missing, unreadable, not TOML, or carried a
//! forbidden key. No source at all named a VM host -- not the
//! flag, not the environment, not the registry. Or the registry
//! has no table for the project asked for. Most of the variants
//! are about a file. No count here: one lands most times this
//! module is touched, and a stale number costs the next reader a
//! recount.
//!
//! [`FieldError`] belongs to *one value*: it was blank, or it
//! broke a rule. It has two variants, and neither mentions a
//! file.
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

use super::host::HOST_ENV;
use super::registry::heading;
use super::{DEFAULT_REMOTE_ROOT, MAX_CONFIG_BYTES};

/// A single configuration value broke its own rule.
///
/// Returned by the field guards and by the newtype
/// constructors, none of which knows or cares whether a config
/// file is involved.
///
/// It derives equality and [`ConfigError`] does not. That is not
/// an oversight in either direction: every field here is a
/// string, so a test can compare two of these directly, while
/// `ConfigError::Read` holds a `std::io::Error`, which has no
/// `PartialEq` at all. Deriving it there would not compile.
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
    /// Shorthand for [`FieldError::Invalid`], which is how every
    /// guard builds one.
    ///
    /// `impl Into<String>` rather than `&str` so both kinds of
    /// caller pay once. A guard with a fixed sentence passes a
    /// `&'static str` and this copies it; a guard building its
    /// sentence with `format!` passes the `String` it already
    /// owns, and this takes it as it is. Asking for `&str` would
    /// have made the second kind allocate, hand over a borrow,
    /// and then allocate again.
    pub(crate) fn invalid(
        field: &'static str,
        reason: impl Into<String>,
    ) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
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
    /// message, so printing it to stderr echoes a line of the
    /// file:
    ///
    /// ```text
    /// bombyx: loading bombyx.toml: invalid config in bombyx.toml:
    /// TOML parse error at line 1, column 12
    ///   |
    /// 1 | -----BEGIN OPENSSH PRIVATE KEY-----
    /// ```
    ///
    /// Reproduced against the built binary. Two routes put a
    /// file bombyx should not quote in front of the parser.
    /// `bombyx.toml` arrives inside a clone and nobody reads it
    /// before running bombyx. And `--config` takes any path at
    /// all, so a mistyped or pasted `--config ~/.ssh/id_ed25519`
    /// hands the parser a private key. Neither needs a symlink,
    /// which is refused separately.
    ///
    /// So `summary` keeps the position and the reason and drops
    /// the quoted line. That is enough to correct a malformed
    /// config, and it is not bombyx's responsibility to print
    /// the file contents.
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
    /// Refused rather than ignored. A committed host identifies one
    /// developer's machine, and a stale one that still parses
    /// is how `destroy` ends up deleting a directory on a
    /// colleague's host.
    #[error(
        "`host` is not allowed in {}: it names one developer's \
         machine, and this file is committed. Move that line to \
         {place}",
        .path.display()
    )]
    HostInProjectFile {
        /// The project file carrying the key.
        path: PathBuf,
        /// Where the value belongs instead.
        place: String,
    },

    /// No source supplied a VM host.
    #[error(
        "no VM host configured -- set it in {place}, pass \
         --host, or set {HOST_ENV}"
    )]
    HostMissing {
        /// The file that would supply one.
        place: String,
    },

    /// The registry has no table for the named project.
    ///
    /// bombyx never edits the registry for the operator, so the
    /// message names the file and the tables the entry needs.
    /// Guessing a repository address and a provisioning script
    /// would boot a VM the operator did not describe.
    ///
    /// `super::registry::heading` spells the heading, so this
    /// message and the two others showing one cannot differ.
    ///
    /// The tables, not every key inside them. `[vm]` and
    /// `[source]` require seven keys between them, and listing
    /// all seven turns a one-line error into a config sample.
    /// Once the tables exist the parser names each missing key
    /// in turn, which is the same information delivered where
    /// the operator is already editing.
    #[error(
        "no `{}` in {} -- add that table with `{}` and `{}`, \
         and a `remote_root` if `{DEFAULT_REMOTE_ROOT}` is not \
         where this project belongs",
        heading(.name, ""),
        .path.display(),
        heading(.name, ".vm"),
        heading(.name, ".source")
    )]
    ProjectNotFound {
        /// Project name that was looked up.
        name: String,
        /// The registry file that has no table for it.
        path: PathBuf,
    },

    /// A project's settings were asked for and there is no
    /// registry file to hold them.
    ///
    /// Separate from [`ConfigError::NotFound`], which says a
    /// file is absent and stops there, and from
    /// [`ConfigError::ProjectNotFound`], whose message claims
    /// bombyx looked inside a file. The operator here has to
    /// create the file *and* know what to put in it, so the
    /// message says both.
    ///
    /// `place` is a `String` rather than a `PathBuf` because a
    /// machine whose environment names no config directory has
    /// no path to print. `config::host::registry_place` decides
    /// the wording for both cases and is where it is written
    /// down.
    #[error(
        "no registry file -- create {place} with a `{}` table",
        heading(.name, "")
    )]
    RegistryNotFound {
        /// Project name that was looked up.
        name: String,
        /// The registry file bombyx would have read.
        place: String,
    },

    /// The winning source supplied an unusable host.
    ///
    /// Separate from [`ConfigError::Invalid`] so the message can
    /// name *where the value came from*. A plain field error
    /// carries a field name and, at most, the project file's
    /// path -- and the project file is the one file forbidden to
    /// carry a host, so that message would send the operator to
    /// edit a file that cannot hold the value.
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
