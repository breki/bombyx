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
//! Both can name a host, and `super::host` ranks the two: the
//! named project's own `host` key wins, and the top-level one is
//! the default below it. An operator who keeps one project on a
//! different machine writes `host` inside that project's table.
//! The example below shows an entry without one, which is what
//! most entries look like:
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
//!
//! [`parse`] applies the host rule to every `host` key as the
//! file is read, so holding a [`Registry`] proves every host in
//! it passed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{
    ConfigError, RemoteRoot, Source, Vm, default_remote_root, from_toml,
    read_optional,
};
use crate::name::{ProjectName, check_segment};

/// File name of the per-developer configuration, inside the
/// directory [`super::user_config_dir`] returns.
pub const USER_CONFIG_FILE: &str = "config.toml";

/// One project's settings, exactly as its table parses.
///
/// Everything that describes one project except its name: the
/// table key supplies that, so a project cannot disagree with
/// itself about what it is called.
///
/// The fields are checked in three different places, and which
/// one depends on what carries the rule.
///
/// `remote_root`, `repo` and `script` are checked by their own
/// types as the table parses: they are a [`super::RemoteRoot`],
/// a [`super::RepoUrl`] and a [`super::ScriptPath`], so a bad
/// value fails the parse and names the line.
///
/// The optional `host` is checked by `parse`, once the table
/// has parsed and before any `Registry` exists, along with every
/// other `host` in the file.
///
/// The rest -- `box`, `cpus`, `memory` and `ref` inside the two
/// tables -- are checked by `Project::validate`, which
/// [`Registry::project`] calls before handing an entry out.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Root directory on the VM host under which this
    /// project's directories are created.
    ///
    /// Per project rather than a top-level default with an
    /// override, so there is one place to look for the value.
    ///
    /// A [`super::RemoteRoot`], whose constructor holds every
    /// rule the value has, so serde refuses a bad one while the
    /// table is parsing and the message names the line.
    #[serde(default = "default_remote_root")]
    pub remote_root: RemoteRoot,

    /// The VM host this one project runs on, when it is not the
    /// machine the file-wide `host` names. Absent from most
    /// entries.
    ///
    /// A `String` rather than a [`super::HostName`], which is
    /// the one primitive here that is argued for. An entry keeps
    /// the value as the table spelled it, because a bad host is
    /// reported by the key that carried it and this struct
    /// cannot say which key that was. `super::host::rank` builds
    /// the winner into a `HostName`.
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
    /// empty map, so a file part-way through being written gets
    /// the error naming the table to add rather than a TOML
    /// error about a missing one.
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
    /// Assembles a [`super::Config`] from this entry.
    ///
    /// `name` is the table key this entry was found under, and
    /// becomes `Config::project`. The entry does not carry the
    /// name itself, so a project cannot disagree with itself
    /// about what it is called.
    ///
    /// `host` comes from the caller because two keys can supply
    /// one and this entry carries only the first, and
    /// `transport` for the same reason: it is derived from that
    /// winning host, which this entry may not hold.
    /// `super::Config::load_project` ranks them and passes the
    /// winner.
    ///
    /// The fields are cloned. The registry is read once per run
    /// and one entry out of it is small, so borrowing them into
    /// the `Config` -- and giving that type a lifetime every
    /// module holding one would carry -- buys nothing.
    pub(super) fn to_config(
        &self,
        name: &str,
        host: super::HostName,
        transport: super::Transport,
    ) -> super::Config {
        super::Config {
            host,
            project: name.to_owned(),
            remote_root: self.remote_root.clone(),
            vm: self.vm.clone(),
            source: self.source.clone(),
            transport,
        }
    }

    /// Runs the rules no type on these fields carries: the
    /// `[vm]` and `[source]` checks.
    ///
    /// **`remote_root` is not among them.** It is a
    /// [`super::RemoteRoot`], so serde ran every rule it has
    /// while the table parsed.
    ///
    /// **`host` is not among them.** [`parse`] applies that rule
    /// to every key in the file as it is read, so by the time
    /// anything calls this, the entry's host has passed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Empty`] or
    /// [`ConfigError::Invalid`], naming the field that broke its
    /// rule.
    fn validate(&self) -> Result<(), ConfigError> {
        super::vm::validate(&self.vm)?;
        super::source::validate(&self.source)?;
        Ok(())
    }
}

impl Registry {
    /// Reads the registry from `path`, if the file is there.
    ///
    /// Absence is `None` so that the caller can word the
    /// message: `super::Config::load_project` names the project
    /// whose table the operator has to write, which this
    /// function does not know. Anything else -- unreadable, not
    /// a plain file, not TOML -- is an error, because a file
    /// that exists and cannot be understood is a mistake to
    /// report rather than a reason to carry on with different
    /// settings.
    ///
    /// Symlinks are followed. `super::read::read_optional` owns
    /// that decision and is where it is written down; do not
    /// restate it here, because the copy in this module would
    /// then have to be corrected too.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Read`], [`ConfigError::NotAFile`],
    /// [`ConfigError::TooLarge`] or [`ConfigError::Parse`], each
    /// naming the registry file.
    pub fn read(path: &Path) -> Result<Option<Self>, ConfigError> {
        let Some(source) = read_optional(path)? else {
            return Ok(None);
        };
        Ok(Some(parse(&source, path)?))
    }

    /// The file this registry was read from.
    ///
    /// Every message naming the file asks for it here rather
    /// than being handed a path separately, so no message can
    /// name a file these settings did not come from.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the VM host this file names, if it names one.
    ///
    /// **Already checked.** `parse` applies the host rule to
    /// every `host` key in the file as it is read, so holding a
    /// [`Registry`] is the proof this value passed.
    ///
    /// The rules themselves live in `super::host`, which is
    /// where they are written down.
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
    /// **The host it returns has passed the host rule**, applied
    /// by [`parse`] to every key in the file. So the guarantee
    /// that a leading `-` never reaches `ssh` does not rest on
    /// what this method's callers remember to do.
    ///
    /// **The rest of `Project::validate` has not run.** An entry
    /// whose `cpus` is zero still supplies its host here,
    /// deliberately: refusing would demote that project to the
    /// file-wide host and boot its VM on the wrong machine,
    /// while the broken `cpus` is reported anyway the moment
    /// anything asks [`Registry::project`] for the entry.
    ///
    /// `pub(crate)` all the same. Nothing outside the crate has
    /// a use for one project's preferred host without the rest
    /// of its entry, and [`Registry::project`] is the way to ask
    /// for that.
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
    /// was asked for, and so does a bad `host` anywhere in it.
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

/// One project's table heading, as an operator must write it.
///
/// `tail` goes inside the brackets, so `""` gives
/// `[projects."x"]` and `".vm"` gives `[projects."x".vm]`.
///
/// The name is quoted because it may contain a `.`, which TOML
/// reads as nesting. `docs/architecture.md` under **The heading
/// spelling has one owner** says why every message asks this
/// rather than spelling it.
pub(super) fn heading(name: &str, tail: &str) -> String {
    format!("[projects.{name:?}{tail}]")
}

/// Parses `source` as the registry read from `path`.
///
/// The only way to build a [`Registry`], so the path in an
/// error message is always the path the text came from.
///
/// Private, so [`Registry::read`] is the only way production
/// code reaches a `Registry`. That is what makes the type's
/// promise -- every one in existence knows which file it came
/// from -- hold: a second route from arbitrary text would let a
/// caller name a path bombyx never opened.
///
/// # Errors
///
/// Returns [`ConfigError::Parse`] when `source` is not valid
/// TOML or carries a key the registry does not define.
fn parse(source: &str, path: &Path) -> Result<Registry, ConfigError> {
    let file: RegistryFile = from_toml(source, path)?;

    // Every `host` in the file, not only the one a later
    // command turns out to want: a typo in a project nobody
    // asked about is reported while the operator has the file
    // open. `docs/architecture.md` carries the argument.
    if let Some(host) = &file.host {
        super::host::refuse_if_bad(
            host,
            &super::HostOrigin::UserFile,
            Some(path),
        )?;
    }
    for (key, project) in &file.projects {
        if let Some(host) = &project.host {
            super::host::refuse_if_bad(
                host,
                &super::HostOrigin::ProjectEntry(key.clone()),
                Some(path),
            )?;
        }
    }

    Ok(Registry {
        path: path.to_path_buf(),
        host: file.host,
        projects: file.projects,
    })
}

/// [`parse`], for a test that would otherwise need a directory.
///
/// Gated on `cfg(test)`, so the widened visibility exists only
/// in a test build and production keeps the single route above.
/// The callers are `super::Config::parse_registry` and the
/// helper in `super`'s own test module.
///
/// # Errors
///
/// Whatever [`parse`] returns.
#[cfg(test)]
pub(super) fn parse_for_tests(
    source: &str,
    path: &Path,
) -> Result<Registry, ConfigError> {
    parse(source, path)
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

    /// Writes `source` as a registry file in a fresh directory,
    /// and returns the directory and the file inside it.
    ///
    /// The directory comes back too, because dropping a
    /// `TempDir` deletes the tree: keeping only the path would
    /// hand the test a file that no longer exists.
    fn registry_file(source: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USER_CONFIG_FILE);
        std::fs::write(&path, source).unwrap();
        (dir, path)
    }

    #[test]
    fn an_entry_carries_the_settings_that_describe_one_vm() {
        let registry = parsed(&registry_toml());
        let project = registry.project("myproject").unwrap();
        assert_eq!(project.remote_root.as_str(), "~/vms");
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
        assert!(err.contains("[projects.\"myproject\"]"), "{err}");
        assert!(err.contains("[projects.\"myproject\".vm]"), "{err}");
        assert!(err.contains("[projects.\"myproject\".source]"), "{err}");
        assert!(err.contains("remote_root"), "{err}");
    }

    #[test]
    fn a_looked_up_entry_has_had_its_values_checked() {
        // Holding a `&Project` has to be proof the values
        // passed, the way holding a `Config` is. Each of these
        // breaks a rule owned by a different module.
        //
        // `host` and `remote_root` are not in the table, and
        // their absence is the point: both are refused earlier,
        // while the file is read, so a registry carrying a bad
        // one never becomes a `Registry` for anything to look an
        // entry up in. `reading_the_file_refuses_a_bad_host_in_an_entry`
        // and `reading_the_file_refuses_a_bad_remote_root` cover
        // them. Between the three tests every value in an entry
        // is checked before a caller can act on it.
        for (from, to) in [
            ("cpus = 4", "cpus = 0"),
            ("memory = 8192", "memory = 0"),
            (
                "box = \"generic/ubuntu2204\"",
                "box = \"generic/ubuntu\\\"2204\"",
            ),
            ("ref = \"main\"", "ref = \"\""),
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
    fn reading_the_file_refuses_a_bad_remote_root() {
        // `remote_root` is a `RemoteRoot`, and serde runs its
        // constructor, so the value is refused while the table
        // parses rather than by `Project::validate`. An entry
        // holding one never exists.
        //
        // The whole family of bad roots is enumerated in
        // `super::root`. What this covers is the seam: the
        // reason has to reach the operator naming the field and
        // the line to edit.
        let source = registry_toml()
            .replace("remote_root = \"~/vms\"", "remote_root = \"/\"");
        let err = parse_at(&source, "/home/dev/config.toml").unwrap_err();
        let ConfigError::Parse { summary, .. } = &err else {
            panic!("must be refused by the parser, got {err:?}");
        };
        assert!(summary.contains("remote_root"), "{summary}");
        assert!(summary.contains("at least 1 directory"), "{summary}");
        assert!(summary.contains("line "), "{summary}");
    }

    #[test]
    fn a_project_entry_may_name_its_own_host() {
        // An operator who keeps one project on another machine
        // has no other way to record that choice.
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
        // A file part-way through being written has to parse,
        // so the projects table is optional.
        let registry = parsed("host = \"vmhost\"\n");
        assert_eq!(registry.host.as_deref(), Some("vmhost"));
        assert!(registry.projects.is_empty());
    }

    #[test]
    fn a_missing_registry_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USER_CONFIG_FILE);
        assert!(Registry::read(&path).unwrap().is_none());
    }

    #[test]
    fn read_parses_the_file_it_is_pointed_at() {
        let (_dir, path) = registry_file(&registry_toml());
        let registry = Registry::read(&path).unwrap().unwrap();
        assert!(registry.project("myproject").is_ok());
    }

    #[test]
    fn a_miss_names_the_file_the_registry_was_read_from() {
        // The registry carries its own path, so no caller can
        // hand the error a different one.
        let (_dir, path) = registry_file(&registry_toml());
        let registry = Registry::read(&path).unwrap().unwrap();
        let err = registry.project("other").unwrap_err().to_string();
        assert!(err.contains(&path.display().to_string()), "{err}");
    }

    #[test]
    fn a_registry_that_is_not_toml_names_the_file() {
        let (_dir, path) = registry_file("host = \n");
        let err = Registry::read(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains(USER_CONFIG_FILE), "{err}");
    }

    /// Every host value the whole family of guards refuses.
    ///
    /// One table, used for the file-wide key and for a project's
    /// key, so the two cannot come to disagree about what a
    /// legal host looks like. The reasons come from
    /// `super::host::HostName::parse`, which is where the rules
    /// are; this only asserts that reading the file applies
    /// them.
    const BAD_HOSTS: [(&str, &str); 4] = [
        ("", "must not be empty"),
        ("   ", "must not be empty"),
        ("-oProxyCommand=x", "must not start with"),
        ("vm host", "letters, digits"),
    ];

    #[test]
    fn reading_the_file_refuses_a_bad_file_wide_host() {
        for (bad, reason) in BAD_HOSTS {
            let source = registry_toml()
                .replace("host = \"vmhost\"", &format!("host = {bad:?}"));
            let err = parse_at(&source, "/home/dev/config.toml").unwrap_err();
            let text = err.to_string();
            assert!(
                matches!(err, ConfigError::InvalidHost { .. }),
                "{bad:?} must be refused, got {text}"
            );
            assert!(text.contains(reason), "{bad:?}: {text}");
            // The operator has to find the value, so the message
            // names the file it is in.
            assert!(text.contains("/home/dev/config.toml"), "{text}");
        }
    }

    #[test]
    fn reading_the_file_refuses_a_bad_host_in_an_entry() {
        for (bad, reason) in BAD_HOSTS {
            let source = registry_toml().replace(
                "[projects.myproject]\n",
                &format!("[projects.myproject]\nhost = {bad:?}\n"),
            );
            let err = parse_at(&source, "/home/dev/config.toml").unwrap_err();
            let text = err.to_string();
            assert!(
                matches!(err, ConfigError::InvalidHost { .. }),
                "{bad:?} must be refused, got {text}"
            );
            assert!(text.contains(reason), "{bad:?}: {text}");
            // The table, not just the file: an operator with
            // twenty projects in one file needs the heading.
            assert!(text.contains("[projects.\"myproject\"].host"), "{text}");
            assert!(text.contains("/home/dev/config.toml"), "{text}");
        }
    }

    #[test]
    fn a_bad_host_in_another_project_still_fails_the_file() {
        // The rule is about what the file says, not about which
        // project a command asked for. A file bombyx will not
        // stand behind is refused whole, the way a table that
        // does not parse fails whichever project was wanted.
        let source = format!(
            "{}\n[projects.other]\nhost = \"-oProxyCommand=x\"\n{}",
            registry_toml(),
            "[projects.other.vm]\n\
             provider = \"libvirt\"\n\
             box = \"generic/ubuntu2204\"\n\
             cpus = 2\n\
             memory = 2048\n\n\
             [projects.other.source]\n\
             repo = \"https://example.invalid/other.git\"\n\
             ref = \"main\"\n\
             script = \"vagrant/provision.sh\"\n"
        );
        let err = parse_at(&source, "/home/dev/config.toml").unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, ConfigError::InvalidHost { .. }), "{text}");
        assert!(text.contains("[projects.\"other\"].host"), "{text}");
    }

    #[test]
    fn every_message_spells_a_project_heading_the_same_way() {
        // The heading is quoted in one place, `heading`, and
        // these are the three messages that show one to an
        // operator. A fourth that spelled it itself would pass
        // its own test and fail this one.
        let name = "web.api";
        let want = "[projects.\"web.api\"]";
        let key = ProjectName::parse(name).unwrap();
        let messages = [
            ConfigError::ProjectNotFound {
                name: name.to_owned(),
                path: PathBuf::from("/home/dev/config.toml"),
            }
            .to_string(),
            ConfigError::RegistryNotFound {
                name: name.to_owned(),
                place: "/home/dev/config.toml".to_owned(),
            }
            .to_string(),
            super::super::HostOrigin::ProjectEntry(key)
                .describe(Some(Path::new("/home/dev/config.toml"))),
        ];
        for text in messages {
            assert!(text.contains(want), "want {want} in: {text}");
        }
    }

    #[test]
    fn a_dotted_project_name_is_named_as_a_quoted_table() {
        // A project may be named `web.api`, and its heading in
        // the file then has to be `[projects."web.api"]`: TOML
        // reads a bare dot as nesting, so the unquoted spelling
        // declares `api` inside `web` and the whole file is
        // refused. The message telling the operator where the
        // bad host is has to name the heading their file
        // actually contains.
        //
        // A project name reaches an operator-readable heading
        // in three places: this message and the two in
        // `super::super::error`. This asserts the third.
        let source = registry_toml()
            .replace("[projects.myproject", "[projects.\"web.api\"")
            .replace(
                "[projects.\"web.api\"]\n",
                "[projects.\"web.api\"]\nhost = \"-oProxyCommand=x\"\n",
            );
        let err = parse_at(&source, "/home/dev/config.toml").unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("[projects.\"web.api\"].host"),
            "the heading must be quoted, got {text}"
        );
    }

    #[test]
    fn a_good_host_in_every_place_is_accepted() {
        // The other side of the guard: the shapes an operator
        // actually writes must survive it.
        for good in ["vmhost", "frosti", "user@10.0.0.4", "vm-host_1.lan"] {
            let source = registry_toml().replace(
                "[projects.myproject]\n",
                &format!("[projects.myproject]\nhost = {good:?}\n"),
            );
            let registry = parse_at(&source, "/home/dev/config.toml")
                .unwrap_or_else(|e| panic!("{good:?} must be accepted: {e}"));
            assert_eq!(host_of(&registry, "myproject"), Some(good));
        }
    }
}
