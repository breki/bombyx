//! The per-developer file (`config.toml`), and the project
//! entries in it.
//!
//! This is the file the operator owns. It is not part of any
//! project's repository, so bombyx trusts it the way it trusts
//! a command-line argument: whoever writes it is whoever runs
//! bombyx.
//!
//! It carries two things. A top-level `host` names the machine
//! the VMs run on, and `super::host` ranks that value against
//! `--host` and the environment. A `[projects.<name>]` table
//! per project carries the settings that describe one VM:
//!
//! ```toml
//! host = "vmhost"
//!
//! [projects.myproject]
//! remote_root = "~/vms"
//!
//! [projects.myproject.vm]
//! provider = "libvirt"
//! box = "generic/ubuntu2204"
//! cpus = 4
//! memory = 8192
//!
//! [projects.myproject.source]
//! repo = "https://github.com/you/myproject"
//! ref = "main"
//! script = "vagrant/provision.sh"
//! ```
//!
//! One type parses the whole file. Two structs reading the same
//! file would each carry `deny_unknown_fields`, so each would
//! refuse the other's keys, and the two would drift as the file
//! grows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{
    ConfigError, Source, Symlinks, Vm, default_remote_root, from_toml,
    read_optional,
};

/// File name of the per-developer configuration, inside the
/// directory [`super::user_config_dir`] returns.
pub const USER_CONFIG_FILE: &str = "config.toml";

/// One project's settings, exactly as its table parses.
///
/// These are the values a `bombyx.toml` carries today, minus
/// `project`: the table key supplies that name, so a project
/// cannot disagree with itself about what it is called.
///
/// Nothing here is checked while parsing. `super::Config` owns
/// the rules for these values and runs them in one place, so a
/// second copy of a rule here could disagree with it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Root directory on the VM host under which this
    /// project's directories are created.
    ///
    /// Per project rather than a top-level default with an
    /// override, so there is one place to look for the value.
    #[serde(default = "default_remote_root")]
    pub remote_root: String,

    /// The machine to build.
    pub vm: Vm,

    /// Where the guest clones the project from.
    pub source: Source,
}

/// The per-developer file, exactly as it parses.
///
/// `deny_unknown_fields` refuses every key this struct does not
/// name, so a misspelt setting is reported instead of quietly
/// doing nothing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    /// The VM host, when the operator put one here.
    pub host: Option<String>,

    /// One entry per project, keyed by project name.
    ///
    /// A file with no `[projects.*]` table at all parses to an
    /// empty map. That is the state of every registry written
    /// before project entries existed, and it is still a
    /// perfectly good file for supplying a host.
    #[serde(default)]
    projects: BTreeMap<String, Project>,
}

impl Registry {
    /// Returns the entry for `name`.
    ///
    /// `path` is the file this registry was read from, and it
    /// appears in the error so the operator knows which file to
    /// edit.
    ///
    /// A missing entry is an error rather than something bombyx
    /// fills in. Writing an entry on the operator's behalf means
    /// guessing a repository address and a provisioning script,
    /// and a guessed entry that boots the wrong VM is worse than
    /// a message saying what to type.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ProjectNotFound`] when the file
    /// has no table for `name`.
    pub fn project(
        &self,
        name: &str,
        path: &Path,
    ) -> Result<&Project, ConfigError> {
        self.projects
            .get(name)
            .ok_or_else(|| ConfigError::ProjectNotFound {
                name: name.to_owned(),
                path: path.to_path_buf(),
            })
    }
}

/// The registry file inside `dir`.
///
/// One spelling of the join, so an error message and the file
/// actually opened cannot name different paths.
pub(super) fn path(dir: &Path) -> PathBuf {
    dir.join(USER_CONFIG_FILE)
}

/// Reads the registry from `dir`, if there is one.
///
/// Absence is `None`: an operator who passes `--host` and names
/// no project needs no such file. Anything else -- unreadable,
/// not a plain file, not TOML -- is an error, because a file
/// that exists and cannot be understood is a mistake to report
/// rather than a reason to carry on with different settings.
///
/// Symlinks are followed. Dotfile managers such as `stow` and
/// `chezmoi` put exactly this kind of file in place as a link,
/// and nothing inside a project's repository can create or
/// retarget a file in the operator's own config directory.
///
/// # Errors
///
/// Returns [`ConfigError::Read`], [`ConfigError::NotAFile`],
/// [`ConfigError::TooLarge`] or [`ConfigError::Parse`], each
/// naming the registry file.
pub(super) fn read(dir: &Path) -> Result<Option<Registry>, ConfigError> {
    let path = path(dir);
    let Some(source) = read_optional(&path, Symlinks::Follow)? else {
        return Ok(None);
    };
    Ok(Some(from_toml(&source, &path)?))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::Provider;

    /// A registry naming one project, `myproject`.
    ///
    /// Copied by hand from the example in this module's header,
    /// key for key. The header is what a reader writes their own
    /// file from, so a field renamed here without renaming it
    /// there fails a test rather than misleading them.
    fn registry_toml() -> String {
        "host = \"vmhost\"\n\n\
         [projects.myproject]\n\
         remote_root = \"~/vms\"\n\n\
         [projects.myproject.vm]\n\
         provider = \"libvirt\"\n\
         box = \"generic/ubuntu2204\"\n\
         cpus = 4\n\
         memory = 8192\n\n\
         [projects.myproject.source]\n\
         repo = \"https://github.com/you/myproject\"\n\
         ref = \"main\"\n\
         script = \"vagrant/provision.sh\"\n"
            .to_owned()
    }

    fn parse(source: &str) -> Result<Registry, ConfigError> {
        from_toml(source, Path::new(USER_CONFIG_FILE))
    }

    /// Writes `source` as the registry inside a fresh directory.
    fn registry_dir(source: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), source).unwrap();
        dir
    }

    #[test]
    fn an_entry_carries_the_settings_that_describe_one_vm() {
        let registry = parse(&registry_toml()).unwrap();
        let project = registry
            .project("myproject", Path::new(USER_CONFIG_FILE))
            .unwrap();
        assert_eq!(project.remote_root, "~/vms");
        assert_eq!(project.vm.provider, Provider::Libvirt);
        assert_eq!(project.vm.cpus, 4);
        assert_eq!(project.vm.memory, 8192);
        assert_eq!(project.source.git_ref, "main");
        assert_eq!(project.source.script.as_str(), "vagrant/provision.sh");
    }

    #[test]
    fn the_table_key_is_what_names_a_project() {
        // A name nobody wrote a table for must not find the
        // one table that is there.
        let registry = parse(&registry_toml()).unwrap();
        let err = registry
            .project("other", Path::new(USER_CONFIG_FILE))
            .unwrap_err();
        assert!(matches!(err, ConfigError::ProjectNotFound { .. }));
    }

    #[test]
    fn a_missing_entry_names_the_file_and_the_keys_it_needs() {
        // The operator has to type the entry themselves, so the
        // message has to say what to type and where.
        let registry = parse("host = \"vmhost\"\n").unwrap();
        let err = registry
            .project("myproject", Path::new("/home/dev/config.toml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("/home/dev/config.toml"), "{err}");
        assert!(err.contains("[projects.myproject]"), "{err}");
        assert!(err.contains("[projects.myproject.vm]"), "{err}");
        assert!(err.contains("[projects.myproject.source]"), "{err}");
        assert!(err.contains("remote_root"), "{err}");
    }

    #[test]
    fn remote_root_falls_back_to_the_default() {
        let source = registry_toml().replace("remote_root = \"~/vms\"\n", "");
        let registry = parse(&source).unwrap();
        let project = registry
            .project("myproject", Path::new(USER_CONFIG_FILE))
            .unwrap();
        assert_eq!(project.remote_root, default_remote_root());
    }

    #[test]
    fn a_project_table_refuses_a_key_it_does_not_know() {
        // `project` is the table key, so writing it inside the
        // table is a mistake worth reporting rather than a
        // second opinion on the name.
        let source = registry_toml().replace(
            "[projects.myproject]\n",
            "[projects.myproject]\nproject = \"other\"\n",
        );
        let err = parse(&source).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("project"), "{err}");
    }

    #[test]
    fn a_registry_with_no_projects_at_all_still_parses() {
        // Every registry written before project entries existed
        // looks like this, and it is still a good file.
        let registry = parse("host = \"vmhost\"\n").unwrap();
        assert_eq!(registry.host.as_deref(), Some("vmhost"));
        assert!(registry.projects.is_empty());
    }

    #[test]
    fn a_missing_registry_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).unwrap().is_none());
    }

    #[test]
    fn read_parses_the_file_in_the_directory() {
        let dir = registry_dir(&registry_toml());
        let registry = read(dir.path()).unwrap().unwrap();
        assert!(registry.project("myproject", &path(dir.path())).is_ok());
    }

    #[test]
    fn a_registry_that_is_not_toml_names_the_file() {
        let dir = registry_dir("host = \n");
        let err = read(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains(USER_CONFIG_FILE), "{err}");
    }
}
