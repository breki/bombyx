//! What can go wrong while reading a configuration, and what
//! can go wrong with one field of it.
//!
//! The module has two error types, and they are separate for a
//! reason.
//!
//! [`ConfigError`] belongs to *loading* a config. The registry
//! was missing, unreadable or not TOML. It carries no table for
//! the project asked for. Neither of its `host` keys named a VM
//! host. Most of the variants are about a file. No count here,
//! because a stale one costs the next reader a recount.
//!
//! [`FieldError`] belongs to *one value*: it was blank, or it
//! broke a rule. It has two variants, and neither mentions a
//! file.
//!
//! The reason for two is that [`RepoUrl`](super::RepoUrl) and
//! [`ScriptPath`] can be built by anyone, on a string from
//! anywhere, with no config file in sight. Handing their callers
//! an error type with a "config file is larger than 64 KiB"
//! variant would make matching on the result meaningless.
//!
//! **There is no blanket conversion from one to the other**,
//! and exactly one value converts by hand.
//!
//! Every checked type but `config::HostName` and
//! `name::ScratchName` runs its constructor while serde
//! deserializes, so a refusal it raises is wrapped by serde and
//! reaches the caller inside [`ConfigError::Parse`], which names
//! the line as well as the field. A blanket `From` would have
//! produced that same message with the position thrown away.
//! (`name::ProjectName` raises a `name::NameError` rather than a
//! [`FieldError`], because it shares its rule with
//! `name::ScratchName`, which has nothing to do with a config
//! file. Serde wraps either one the same way.)
//!
//! The two exceptions differ from each other. `ScratchName` is
//! built from the command line and never appears in the
//! registry, so no config error is involved at all. `HostName`
//! is the one that converts by hand: the value is ranked after
//! the file parses, and `config::host::with_origin` turns its
//! `FieldError` into [`ConfigError::InvalidHost`] rather than
//! `Parse`. The registry holds a `host` key per project and one
//! below them all, so that message names the source that
//! supplied the value instead of the field. A caller matching
//! only `Parse` to catch a bad config value will miss it.

use std::path::PathBuf;

use thiserror::Error;

use super::registry::heading;
use super::{
    DEFAULT_REMOTE_ROOT, EnvName, GuestUser, HookPath, MAX_CONFIG_BYTES,
    ScriptPath,
};
use crate::name::ProjectName;

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
    /// bombyx: invalid config in config.toml:
    /// TOML parse error at line 1, column 12
    ///   |
    /// 1 | -----BEGIN OPENSSH PRIVATE KEY-----
    /// ```
    ///
    /// Reproduced against the built binary. `--config` takes any
    /// path at all, so a mistyped or pasted
    /// `--config ~/.ssh/id_ed25519` hands the parser a private
    /// key, and a symlinked registry is followed rather than
    /// refused.
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

    /// Neither `host` key supplied a VM host.
    ///
    /// The message asks for the file-wide key rather than the
    /// project's own: one machine name written once covers every
    /// project.
    ///
    /// `place` is a `PathBuf` here and a `String` in
    /// [`ConfigError::RegistryNotFound`]: bombyx raises this one
    /// only after reading a registry, so a path always exists.
    #[error(
        "no VM host configured -- add a `host` line to {}",
        .place.display()
    )]
    HostMissing {
        /// The file that would supply one.
        place: PathBuf,
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
        name: ProjectName,
        /// The registry file that has no table for it.
        path: PathBuf,
    },

    /// A project's settings were asked for and there is no
    /// registry file to hold them.
    ///
    /// Separate from [`ConfigError::ProjectNotFound`], whose
    /// message claims bombyx looked inside a file. The operator
    /// here has to create the file *and* know what to put in it,
    /// so the message says both.
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
        name: ProjectName,
        /// The registry file bombyx would have read.
        place: String,
    },

    /// Every project's settings were asked for and there is no
    /// registry file to hold them.
    ///
    /// Separate from [`ConfigError::RegistryNotFound`], whose
    /// message names the one project that was looked up. Nothing
    /// here looked a project up, so there is no name to quote
    /// and the advice has to describe the file instead.
    #[error(
        "no registry file -- create {place} and give each project \
         you drive a `[projects.<name>]` table"
    )]
    NoRegistry {
        /// The registry file bombyx would have read.
        place: String,
    },

    /// A `host` key in the registry named an unusable host.
    ///
    /// The message names *which line carried the value*. Saying
    /// only `host` would not: the registry has one of those per
    /// project plus a file-wide one, so the operator would be
    /// told to fix a key without being told which.
    #[error("invalid VM host from {origin}: {reason}")]
    InvalidHost {
        /// Which source supplied it.
        origin: String,
        /// What rule the value broke.
        reason: String,
    },

    /// A project names a `secrets_refreshed` hook and no secrets:
    /// no `env_file`, and no `vault.entries`.
    ///
    /// The hook runs only after bombyx rewrites the secrets, so
    /// with no source of them it would never run. A rule spanning two
    /// tables, so no single type can hold it; the registry's
    /// `parse` checks it once both tables have parsed.
    #[error(
        "invalid config in {}: [projects.\"{project}\".hooks] names \
         `secrets_refreshed`, which runs after bombyx rewrites the \
         project's secrets, but [projects.\"{project}\".source] names \
         none: no `env_file`, and no `vault.entries` -- add one, or \
         remove the hook",
        .path.display()
    )]
    HookWithoutSecrets {
        /// The registry file holding the project.
        path: PathBuf,
        /// The project whose table pairs them.
        project: ProjectName,
    },

    /// A project with `guest = "windows"` names a
    /// `secrets_refreshed` hook.
    ///
    /// The Windows hook runner starts the hook with `powershell
    /// -File`, which refuses any file but a `.ps1`, and Windows
    /// refuses a path over 259 characters by default. Either way the
    /// refresh would write the secrets and then report that the hook
    /// did not run. A rule spanning `[vm]` and `[hooks]`, checked in the
    /// registry's `parse`, as [`ConfigError::WindowsGuestScript`] is
    /// for `script`.
    #[error(
        "invalid config in {}: project \"{project}\" sets \
         secrets_refreshed = \"{hook}\" in [projects.\"{project}\".hooks], \
         but [projects.\"{project}\".vm] sets guest = \"windows\", and \
         {reason}",
        .path.display()
    )]
    WindowsGuestHook {
        /// The registry file holding the project.
        path: PathBuf,
        /// The project naming the hook.
        project: ProjectName,
        /// The hook, as the config spells it.
        hook: HookPath,
        /// Why a Windows guest cannot run it.
        reason: super::WindowsHookRefusal,
    },

    /// A project with `guest = "windows"` names a `script` that is
    /// not a `.ps1` file.
    ///
    /// The Windows bootstrap script runs the project's script with
    /// `powershell -File`, which refuses any other file, so the VM
    /// would boot and its provisioning then fail. A rule spanning
    /// `[vm]` and `[source]`, checked in the registry's `parse`.
    #[error(
        "invalid config in {}: project \"{project}\" sets script = \
         \"{script}\", but [projects.\"{project}\".vm] sets guest = \
         \"windows\", and a Windows guest runs the script with \
         PowerShell, which needs a .ps1 file",
        .path.display()
    )]
    WindowsGuestScript {
        /// The registry file holding the project.
        path: PathBuf,
        /// The project naming the script.
        project: ProjectName,
        /// The script, as the config spells it.
        script: ScriptPath,
    },

    /// A project with `guest = "windows"` names a `guest_user`
    /// that a Windows guest cannot hold: longer than Windows allows
    /// a local account name, or one of the box's built-in accounts.
    ///
    /// A rule on `guest_user` that only `[vm]`'s `guest` switches
    /// on, so it runs in the registry's `parse`, after
    /// `GuestUser`'s own rules have passed.
    #[error(
        "invalid config in {}: project \"{project}\" sets guest_user = \
         \"{user}\", but [projects.\"{project}\".vm] sets guest = \
         \"windows\", and on Windows guest_user {reason}",
        .path.display()
    )]
    WindowsGuestUser {
        /// The registry file holding the project.
        path: PathBuf,
        /// The project naming the account.
        project: ProjectName,
        /// The account name, as the config spells it.
        user: GuestUser,
        /// Which Windows rule the name breaks.
        reason: super::WindowsUserRefusal,
    },

    /// A project with `guest = "windows"` names an `[env]` entry a
    /// Windows guest cannot take: one that, compared without regard
    /// to case as Windows compares, is bombyx's own or changes what
    /// its guest scripts do, or one that differs only in case from
    /// another `[env]` name, which Windows reads as the same variable.
    ///
    /// A rule on `[env]` that only `[vm]`'s `guest` switches on, so
    /// it runs in the registry's `parse`, after `EnvName`'s own
    /// rules have passed.
    #[error(
        "invalid config in {}: project \"{project}\" sets `{name}` in \
         [projects.\"{project}\".env], but [projects.\"{project}\".vm] \
         sets guest = \"windows\", and there the name {reason}",
        .path.display()
    )]
    WindowsGuestEnv {
        /// The registry file holding the project.
        path: PathBuf,
        /// The project naming the variable.
        project: ProjectName,
        /// The variable's name, as the config spells it.
        name: EnvName,
        /// Which rule the name breaks.
        reason: &'static str,
    },
}
