//! A project's configuration, out of the operator's own
//! `config.toml`.
//!
//! [`Config::load_project`] takes a project name and reads one
//! `[projects.<name>]` table for every setting. That file is the
//! only one bombyx opens: `docs/trust-boundary.md` says the
//! workstation reads nothing from the project's own repository,
//! so there is no committed file to consult and no
//! project-directory lookup to explain.
//!
//! **Which machine runs the VMs comes from the same file**, and
//! the entry may name its own. `host` is a key inside a
//! project's table, with a file-wide `host` as the default below
//! them all; see [`HostOrigin`] for the two and which wins.
//!
//! The registry is the operator's own private file, so its
//! values are not attacker-controlled the way a committed file
//! would be. Every field is still checked against an explicit
//! allowlist, because a `repo` or a `script` reaches `git` on
//! the guest and a typo there is worth reporting where the
//! operator is editing.
//!
//! Two of them are checked by their *type* and cannot be built
//! wrong at all: see [`RepoUrl`]. The rest are checked by
//! `Config::validate`, which the loading path runs. `Config`
//! has public fields, so a hand-built one skips that check.
//! That is a gap rather than a decision, argued once in
//! `docs/architecture.md` under "What config values are
//! checked". (`validate` is named rather than linked because
//! it is private, and rustdoc rejects a public page pointing
//! at a private item.)
//!
//! # Where each rule lives
//!
//! This module owns [`Config`] itself. Everything else is a
//! private child module, so rustdoc will not list them:
//!
//! - `read` -- getting a config file off disk: whether the path
//!   may be a symlink, how large a file may be, and how a TOML
//!   error is summarised.
//! - `registry` -- the operator's own `config.toml`: the VM
//!   host, and a table per project.
//! - `error` -- the two error types, and why there are two.
//! - `guards` -- the rules more than one field shares.
//! - `host` -- where the VM host name comes from, and its shape.
//! - `root` -- every rule `remote_root` must pass.
//! - `source` -- the `[source]` table and its two checked types.
//! - `vm` -- the `[vm]` table.
//!
//! A new field rule belongs in the module that owns the field.
//! Put it in `guards` only once a second field needs it.

use std::path::Path;

use crate::name::{ScratchName, check_segment};

mod error;
mod guards;
mod host;
mod read;
mod registry;
mod root;
mod source;
mod vm;

/// The `[vm]` and `[source]` tables every project file needs.
///
/// At module scope rather than inside a test module because two
/// sibling test modules share it, `tests` and
/// `load_project_tests`, and neither can reach into the other.
///
/// Writing the eleven lines out per test module would mean
/// editing each module to add a required field, and a module
/// somebody missed would fail in a test about something else
/// entirely.
#[cfg(test)]
const REQUIRED_TABLES: &str = "\n[vm]\n\
     provider = \"libvirt\"\n\
     box = \"generic/ubuntu2204\"\n\
     cpus = 2\n\
     memory = 2048\n\
     \n\
     [source]\n\
     repo = \"https://example.invalid/myproject.git\"\n\
     ref = \"main\"\n\
     script = \"vagrant/provision.sh\"\n";

/// One `[projects.<name>]` table, for a test.
///
/// `project_host` is the entry's own `host`, and `None` writes
/// an entry with no `host` line, which is what most entries look
/// like.
///
/// The `[vm]` and `[source]` tables come from
/// [`REQUIRED_TABLES`], renamed into the project's namespace,
/// because a `[projects.<name>]` table without them does not
/// parse. Renaming rather than writing them out again means a
/// field added to [`Vm`] or [`Source`] does not break a test
/// about something else.
///
/// The heading is written out rather than asked of
/// `registry::heading`: a fixture and the message checked
/// against it must not come from the same code.
#[cfg(test)]
fn test_entry(name: &str, project_host: Option<&str>) -> String {
    let keys =
        project_host.map_or_else(String::new, |h| format!("host = {h:?}\n"));
    test_entry_with(name, &keys)
}

/// One `[projects.<name>]` table carrying `keys`, for a test.
///
/// `keys` are the entry's own bare keys -- `host`,
/// `remote_root` -- and they go *before* the two tables. A bare
/// key written after a table header joins that table instead of
/// the entry and still parses, so a test meaning to set
/// `remote_root` would silently set `vm.remote_root` and fail on
/// `deny_unknown_fields` somewhere unrelated.
#[cfg(test)]
fn test_entry_with(name: &str, keys: &str) -> String {
    let tables = REQUIRED_TABLES
        .replace("[vm]", &format!("[projects.{name}.vm]"))
        .replace("[source]", &format!("[projects.{name}.source]"));
    format!("[projects.{name}]\n{keys}{tables}")
}

/// A registry naming one project, for a test.
///
/// `host` is the file-wide one. A test needing a second project
/// appends another [`test_entry`], which is why the file-wide
/// key is written here and not there: a file may carry only one.
#[cfg(test)]
fn test_registry(name: &str, host: &str, project_host: Option<&str>) -> String {
    format!("host = {host:?}\n\n{}", test_entry(name, project_host))
}

pub use error::{ConfigError, FieldError};
pub use host::{CONFIG_DIR_ENV, HostOrigin, registry_file, user_config_dir};
pub(crate) use host::{is_anchored_dir, registry_place};
pub use registry::{Project, Registry, USER_CONFIG_FILE};
pub use source::{RepoUrl, ScriptPath, Source};
pub use vm::{Provider, Vm};

use read::{MAX_CONFIG_BYTES, from_toml, read_optional};
pub(crate) use root::path_segments;

/// Default root on the VM host under which project
/// directories are created.
const DEFAULT_REMOTE_ROOT: &str = "~/vms";

/// [`DEFAULT_REMOTE_ROOT`] as serde wants it.
///
/// `#[serde(default = "...")]` names a function rather than a
/// constant, so `config::registry`'s `remote_root` reaches the
/// value through this.
fn default_remote_root() -> String {
    DEFAULT_REMOTE_ROOT.to_owned()
}

/// A resolved bombyx configuration.
///
/// Every field comes out of one `[projects.<name>]` table,
/// except that `host` may come from the file-wide key instead
/// (see [`HostOrigin`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// SSH host alias of the VM host, e.g. `vmhost`.
    ///
    /// Resolved through the user's `~/.ssh/config`, so
    /// bombyx never deals with addresses or usernames.
    ///
    /// **Only the operator's own file names it.** The VM host is
    /// a property of the person driving bombyx, not of the
    /// project: everyone on a team has their own machine on
    /// their own network, and a value inside the repository
    /// could name only one of them -- pointing everyone else's
    /// `destroy` at a colleague's host.
    pub host: String,

    /// Project name. Doubles as the directory name on the
    /// VM host.
    pub project: String,

    /// Root directory on the VM host under which project
    /// directories are created.
    pub remote_root: String,

    /// The machine to build. Rendered into the Vagrantfile
    /// bombyx writes on the VM host.
    pub vm: Vm,

    /// Where the guest clones the project from.
    pub source: Source,
}

impl Config {
    /// Loads a project out of a registry given as a string.
    ///
    /// [`Config::load_project`] without the file: `source` is a
    /// whole `config.toml`, and `path` is only what an error
    /// message names.
    ///
    /// **Test-only.** Production code calls
    /// [`Config::load_project`].
    ///
    /// Named for the file it parses rather than `parse`, so a
    /// reader of a test can see which of the two files a fixture
    /// is meant to be.
    ///
    /// # Errors
    ///
    /// Every error [`Config::load_project`] lists except the
    /// ones about reading a file.
    #[cfg(test)]
    pub(crate) fn parse_registry(
        source: &str,
        path: &Path,
        name: &str,
    ) -> Result<Self, ConfigError> {
        let registry = registry::parse_for_tests(source, path)?;
        Self::from_registry(&registry, name).map(|(cfg, _origin)| cfg)
    }

    /// The config every module's tests use.
    ///
    /// It lives next to the type it builds, so every test module
    /// shares one copy. Written out per module, adding a required
    /// field would mean the same edit in each of them, and the
    /// two literals would be pinned in as many places.
    ///
    /// It comes out of a registry because that is the only file
    /// bombyx reads, so this helper builds a `Config` the same
    /// way production does.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_tests() -> Self {
        Self::parse_registry(
            &test_registry("myproject", "vmhost", None),
            Path::new(USER_CONFIG_FILE),
            "myproject",
        )
        .expect("the shared test config must be valid")
    }

    /// Loads the settings for one project out of the registry.
    ///
    /// `registry` is the file to read, which is `--config` when
    /// the operator passed it and [`registry_file`] otherwise.
    /// `None` means the environment names no config directory,
    /// so bombyx has nowhere to look at all.
    ///
    /// Reads the file once and ranks the host from that same
    /// copy, so the settings and the host cannot come from two
    /// different reads.
    ///
    /// Returns the winning [`HostOrigin`] alongside the config,
    /// so a caller reporting which host is in force does not
    /// re-derive the precedence rule.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Invalid`] with `field: "project"`
    /// if `name` is not a single path segment -- checked before
    /// anything else, including before the registry is looked
    /// for, so no message can advise a table heading that the
    /// parser refuses. Then
    /// [`ConfigError::RegistryNotFound`] if there is no
    /// registry file, [`ConfigError::ProjectNotFound`] if it has
    /// no table for `name`, [`ConfigError::Read`],
    /// [`ConfigError::NotAFile`], [`ConfigError::TooLarge`] or
    /// [`ConfigError::Parse`] if the file cannot be read or
    /// understood, [`ConfigError::HostMissing`] if neither
    /// `host` key names one, and [`ConfigError::Empty`] /
    /// [`ConfigError::Invalid`] if a field fails validation.
    pub fn load_project(
        name: &str,
        registry: Option<&Path>,
    ) -> Result<(Self, HostOrigin), ConfigError> {
        // Before the file is opened, because the errors below
        // quote `name` back as a table heading and must not
        // advise an impossible one.
        check_segment(name).map_err(|e| ConfigError::Invalid {
            field: "project",
            reason: e.to_string(),
        })?;

        let missing = || ConfigError::RegistryNotFound {
            name: name.to_owned(),
            place: registry_place(registry),
        };
        let path = registry.ok_or_else(missing)?;
        let registry = Registry::read(path)?.ok_or_else(missing)?;
        Self::from_registry(&registry, name)
    }

    /// [`Config::load_project`] against a registry already read.
    ///
    /// Split out so nothing has to write a file to test the
    /// assembly: the test-only `Config::parse_registry` parses a
    /// string literal and calls this. (Named rather than linked
    /// because it is `#[cfg(test)]`, so the doc build has no
    /// page for it.)
    ///
    /// # Errors
    ///
    /// Every error [`Config::load_project`] lists except the
    /// ones about reading the file.
    fn from_registry(
        registry: &Registry,
        name: &str,
    ) -> Result<(Self, HostOrigin), ConfigError> {
        let project = registry.project(name)?;

        // Ranked for the same `name` the entry came from, so the
        // host and the settings always come from one project.
        let (host, origin) = host::rank(registry, name)?;

        let cfg = project.to_config(name, host);

        // Deliberately checks the entry's fields a second
        // time: a field added to `Config` with no matching
        // entry check is still refused here.
        cfg.validate()?;
        Ok((cfg, origin))
    }

    /// Rejects values that are empty or outside their allowed
    /// shape.
    ///
    /// **`host` is not among them**, on purpose.
    /// `config::registry`'s parse checks every `host` key as it
    /// reads the file, so there is nothing left to run here.
    fn validate(&self) -> Result<(), ConfigError> {
        // Each field specifies the program it will reach.
        //
        // `project` is checked here; `remote_root` gets the same
        // two rules inside `root::check` below, along with its
        // own four.
        //
        // **For both the rule is a precaution, not a live hole.**
        // Only `host` is handed to a program as a bare argument
        // that a leading `-` could turn into an option. `project`
        // and `remote_root` go through `quote_remote_path` into a
        // shell script that `ssh` runs, so they arrive quoted.
        // The rule is kept anyway, because each of those
        // protections lives in a different file from the value,
        // and either could be rewritten by somebody who does not
        // know it is what makes the value safe.
        guards::check_not_empty("project", &self.project)?;
        guards::check_not_an_option("project", &self.project, "ssh")?;
        guards::check_not_empty("remote_root", &self.remote_root)?;

        // `project` becomes one directory name on the host.
        check_segment(&self.project).map_err(|e| ConfigError::Invalid {
            field: "project",
            reason: e.to_string(),
        })?;

        // Every `remote_root` rule lives in `config::root`,
        // because bombyx deletes the directory it derives from
        // that value. See the module for what each rule stops.
        root::check(&self.remote_root)?;

        self.validate_generated()
    }

    /// Runs the `[vm]` and `[source]` checks that types cannot.
    ///
    /// Each table's rules live in the module that owns it, and
    /// this is the only place either is called from, so no
    /// caller can run one half of the checks without the other.
    ///
    /// Split out of [`Config::validate`] because that function
    /// outgrew the 100-line limit.
    fn validate_generated(&self) -> Result<(), ConfigError> {
        // The `?` widens each `FieldError` into a `ConfigError`.
        // See the `From` impl in `config::error`.
        vm::validate(&self.vm)?;
        source::validate(&self.source)?;
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::host::config_dir_from;
    use super::*;

    /// A registry whose only entry is `myproject`, with `keys`
    /// inside that entry.
    ///
    /// The file-wide `host` is always `vmhost`, because these
    /// tests are about the entry's other settings. The two
    /// `host` keys and the ranking between them belong to
    /// `load_project_tests` below.
    fn registry_with(keys: &str) -> String {
        format!(
            "host = \"vmhost\"\n\n{}",
            test_entry_with("myproject", keys)
        )
    }

    /// Loads `myproject` out of a registry carrying `keys`.
    fn parse(keys: &str) -> Result<Config, ConfigError> {
        parse_whole(&registry_with(keys))
    }

    /// Loads `myproject` out of a whole registry given as text.
    ///
    /// A test about a missing table, or about text that is not
    /// TOML at all, has to write the file itself: [`parse`]
    /// would append the tables the test means to omit.
    fn parse_whole(source: &str) -> Result<Config, ConfigError> {
        Config::parse_registry(source, Path::new(USER_CONFIG_FILE), "myproject")
    }

    fn good() -> Config {
        parse("").unwrap()
    }

    fn scratch(name: &str) -> ScratchName {
        ScratchName::parse(name).unwrap()
    }

    #[test]
    fn a_parse_error_names_the_position_and_not_the_line() {
        // The disclosure this replaced: `toml`'s own `Display`
        // quotes the offending source line, and bombyx printed
        // it to stderr. `--config` takes any path at all, so a
        // mistyped `--config ~/.ssh/id_ed25519` aims the parser
        // at a private key and has a line echoed. Measured
        // against the built binary before and after.
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
                   b3BlbnNzaC1rZXktdjEAAAAA\n";
        let err = parse_whole(key).unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, ConfigError::Parse { .. }), "{text}");
        // The position and the reason are what correct a
        // malformed config, and they stay.
        assert!(text.contains("line 1"), "{text}");
        assert!(text.contains("column"), "{text}");
        // None of the file's own bytes do.
        assert!(!text.contains("BEGIN"), "{text}");
        assert!(!text.contains("OPENSSH"), "{text}");
        assert!(!text.contains("b3Blb"), "{text}");
    }

    #[test]
    fn a_shape_error_still_names_the_field() {
        // An unknown key carries no span, so the summary is the
        // reason alone -- which names the key, and that is the
        // whole answer.
        let err = parse("hsot = \"x\"\n").unwrap_err();
        let text = err.to_string();
        assert!(text.contains("hsot"), "{text}");
    }

    #[test]
    fn a_registry_key_outside_every_entry_is_refused() {
        // A typo in a file nobody reads twice must be an error,
        // not a setting that silently does nothing. The
        // file-wide keys get the same rule as an entry's, and
        // this is the seam between the two.
        let source = registry_with("").replace("host = ", "hsot = ");
        let err = parse_whole(&source).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
        assert!(err.to_string().contains("hsot"), "{err}");
    }

    #[test]
    fn an_entry_may_state_its_own_remote_root() {
        let cfg = parse("remote_root = \"/srv/vms\"\n").unwrap();
        assert_eq!(cfg.remote_root, "/srv/vms");
    }

    #[test]
    fn a_bad_remote_root_surfaces_as_a_field_error() {
        // Every `remote_root` rule, and the tests enumerating
        // the family it refuses, live in `config::root`. What
        // this covers is the seam: a value refused there has to
        // come back out of the load naming the field, so an
        // operator is told which key to edit.
        for (root, want) in [
            ("", "must not be empty"),
            ("vms", "must start with"),
            ("~vms", "must start with"),
            ("/.", "`.` segment"),
            ("~", "at least 1 directory"),
        ] {
            let err =
                parse(&format!("remote_root = \"{root}\"\n")).unwrap_err();
            assert!(
                matches!(
                    &err,
                    ConfigError::Empty {
                        field: "remote_root"
                    } | ConfigError::Invalid {
                        field: "remote_root",
                        ..
                    }
                ),
                "remote_root {root:?} must be refused, got {err:?}"
            );
            assert!(err.to_string().contains(want), "{root:?}: {err}");
        }
    }

    #[test]
    fn builds_remote_project_dir() {
        assert_eq!(good().remote_project_dir(), "~/vms/myproject");
    }

    #[test]
    fn remote_project_dir_ignores_trailing_slash() {
        let cfg = parse("remote_root = \"/srv/\"\n").unwrap();
        assert_eq!(cfg.remote_project_dir(), "/srv/myproject");
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
        let source =
            format!("host = \"vmhost\"\n\n{}", test_entry("ledgerstone", None));
        let b = Config::parse_registry(
            &source,
            Path::new(USER_CONFIG_FILE),
            "ledgerstone",
        )
        .unwrap();
        let name = scratch("pr-1");
        assert_ne!(a.remote_scratch_dir(&name), b.remote_scratch_dir(&name));
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

    /// The registry file `registry_with` describes, written into
    /// a fresh directory.
    fn registry_file_in_a_dir(source: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USER_CONFIG_FILE);
        std::fs::write(&path, source).unwrap();
        (dir, path)
    }

    #[test]
    fn an_oversized_registry_is_refused() {
        // `usize::try_from` rather than `as`: the cap is a
        // `u64`, and a bare cast is a truncation waiting for a
        // 32-bit target.
        let over = usize::try_from(MAX_CONFIG_BYTES).unwrap() + 1;
        let (_dir, path) = registry_file_in_a_dir(&"#".repeat(over));
        let err = Config::load_project("myproject", Some(&path)).unwrap_err();
        assert!(matches!(err, ConfigError::TooLarge(_)), "{err:?}");
    }

    #[test]
    fn a_directory_named_as_the_registry_is_not_a_file() {
        // Was a `Read` error carrying whatever the OS said,
        // which differs per platform ("Is a directory" on Unix,
        // "Access is denied" on Windows) and explains nothing.
        // The type check answers this case first and says what
        // is actually wrong.
        let dir = tempfile::tempdir().unwrap();
        let err =
            Config::load_project("myproject", Some(dir.path())).unwrap_err();
        assert!(matches!(err, ConfigError::NotAFile(_)), "{err:?}");
    }

    #[test]
    fn a_symlinked_registry_is_followed() {
        // Every ordinary dotfile manager (stow, chezmoi, a
        // hand-made `ln -s`) symlinks exactly this file, and
        // refusing one made bombyx fail on every subcommand with
        // a message that never mentioned symlinks.
        let (dir, link) = registry_file_in_a_dir("");
        std::fs::remove_file(&link).unwrap();
        let real = dir.path().join("real-config.toml");
        std::fs::write(&real, registry_with("")).unwrap();
        if !symlink_file(&real, &link) {
            // Windows needs a privilege for this; the guarantee
            // is asserted wherever the test can create a link.
            return;
        }

        let (cfg, _origin) =
            Config::load_project("myproject", Some(&link)).unwrap();
        assert_eq!(cfg.host, "vmhost");
    }

    /// Creates a file symlink, or `false` where not permitted.
    fn symlink_file(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(target, link);
        #[cfg(not(windows))]
        let result = std::os::unix::fs::symlink(target, link);
        result.is_ok()
    }

    /// Every `[vm]` and `[source]` field, with the line that
    /// states it.
    ///
    /// The requiredness tests below omit one entry at a time
    /// and write the rest, so both tables come from here rather
    /// than from a literal in each test. A literal sibling
    /// table is the hazard: adding a required `[source]` field
    /// later would make the `[vm]` test fail on the new field
    /// while its message blamed whichever `[vm]` field it had
    /// omitted.
    const TABLE_FIELDS: [(&str, &str, &str); 7] = [
        ("vm", "provider", "provider = \"libvirt\""),
        ("vm", "box", "box = \"generic/ubuntu2204\""),
        ("vm", "cpus", "cpus = 2"),
        ("vm", "memory", "memory = 2048"),
        ("source", "repo", "repo = \"https://example.invalid/p.git\""),
        ("source", "ref", "ref = \"main\""),
        ("source", "script", "script = \"vagrant/provision.sh\""),
    ];

    /// A registry stating every required field except `omitted`,
    /// which names one entry of [`TABLE_FIELDS`].
    ///
    /// Pass `""` to omit nothing, which is what the tests about
    /// a *missing table* start from.
    fn entry_without(omitted: &str) -> String {
        let mut src = String::from("host = \"vmhost\"\n\n");
        src.push_str("[projects.myproject]\n");
        for table in ["vm", "source"] {
            use std::fmt::Write as _;
            let _ = writeln!(src, "\n[projects.myproject.{table}]");
            for (owner, field, line) in TABLE_FIELDS {
                if owner == table && field != omitted {
                    src.push_str(line);
                    src.push('\n');
                }
            }
        }
        src
    }

    #[test]
    fn every_vm_and_source_field_is_required() {
        // A table present but incomplete must be an error, not
        // a VM built from a mix of stated and invented values.
        // Nothing enforces this except every field of `Vm` and
        // `Source` being required: a `#[serde(default)]` added
        // to one of them later would turn a half-stated table
        // into a silent default, and the operator would read one
        // value in the file while the guest ran with another.
        //
        // Both tables, because the rule protects a required
        // serde field with no default rather than a table name,
        // and `[source]`'s values are the ones reaching `git` on
        // the guest.
        //
        // Each field is omitted in turn rather than testing one
        // partial table, so a failure names the field that
        // stopped being required instead of only reporting that
        // something did.
        for (_, omitted, _) in TABLE_FIELDS {
            let err = parse_whole(&entry_without(omitted)).unwrap_err();
            let text = err.to_string();
            assert!(
                matches!(err, ConfigError::Parse { .. }),
                "omitting {omitted}: {text}"
            );
            assert!(
                text.contains("missing field") && text.contains(omitted),
                "omitting {omitted} must be refused by name, got {text}"
            );
        }
    }

    #[test]
    fn an_entry_requires_both_tables() {
        // Neither has a defensible default. A box is the one
        // thing bombyx cannot invent, and a repository bombyx
        // guessed at would be cloned into the guest and run as
        // root.
        //
        // One table removed at a time, and the loop below skips
        // only the named one. Building the input with
        // `take_while` instead would truncate the file at that
        // header, so removing the `[vm]` table would drop the
        // `[source]` one too and neither case would be isolated.
        for missing in ["vm", "source"] {
            let header = format!("[projects.myproject.{missing}]");
            let mut source = String::new();
            let mut skipping = false;
            for line in entry_without("").lines() {
                if line.starts_with('[') {
                    skipping = line == header;
                }
                if !skipping {
                    source.push_str(line);
                    source.push('\n');
                }
            }
            let kept = if missing == "vm" { "source" } else { "vm" };
            assert!(
                source.contains(&format!("[projects.myproject.{kept}]")),
                "only {missing} may be removed"
            );
            let err = parse_whole(&source).unwrap_err();
            assert!(
                matches!(err, ConfigError::Parse { .. }),
                "an entry without {missing} must be refused: {err:?}"
            );
        }
    }

    /// An entry stating every field, with `cpus` and `memory`
    /// raised off their defaults.
    ///
    /// Tests that exercise one bad field start from this and
    /// replace a line, so a failure names the field under test
    /// rather than a table someone forgot. The raised values are
    /// what let a test tell a stated number from a default one.
    fn full_registry() -> String {
        entry_without("")
            .replace("cpus = 2", "cpus = 4")
            .replace("memory = 2048", "memory = 8192")
    }

    #[test]
    fn reads_the_vm_and_source_tables() {
        let cfg = parse_whole(&full_registry()).unwrap();
        assert_eq!(cfg.vm.provider, Provider::Libvirt);
        assert_eq!(cfg.vm.box_name, "generic/ubuntu2204");
        assert_eq!(cfg.vm.cpus, 4);
        assert_eq!(cfg.vm.memory, 8192);
        assert_eq!(cfg.source.repo.as_str(), "https://example.invalid/p.git");
        assert_eq!(cfg.source.git_ref, "main");
        assert_eq!(cfg.source.script.as_str(), "vagrant/provision.sh");
    }

    #[test]
    fn rejects_an_unknown_provider() {
        // Failing at parse rather than at boot: an unknown
        // provider renders a Vagrantfile no vagrant can use,
        // and the error would arrive on the VM host after
        // bombyx had already created a directory there.
        let source = full_registry().replace("libvirt", "virtualbox");
        let err = parse_whole(&source).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err:?}");
    }

    #[test]
    fn rejects_a_machine_with_nothing_to_run_on() {
        // One field at a time, so a guard covering only `cpus`
        // cannot pass by way of the `memory` case.
        for (field, from, to) in [
            ("cpus", "cpus = 4", "cpus = 0"),
            ("memory", "memory = 8192", "memory = 0"),
        ] {
            let source = full_registry().replace(from, to);
            let err = parse_whole(&source).unwrap_err();
            assert!(
                matches!(&err, ConfigError::Invalid { field: f, .. }
                    if *f == field),
                "{field}: {err:?}"
            );
        }
    }

    #[test]
    fn rejects_characters_that_would_break_the_generated_file() {
        // These four fields are rendered into a Ruby file. The
        // whole family: a quote closes the literal early, a
        // backslash starts an escape, a control character is
        // never meant, and `#{}` is Ruby interpolation, which
        // is evaluated rather than printed.
        for field in ["box", "repo", "ref", "script"] {
            // `{bad:?}` renders a Rust literal, which is a valid
            // TOML basic string for these three. A control
            // character is not: Rust and TOML spell an escape
            // for one differently, so that case would fail as
            // a parse error before reaching the guard. It is
            // covered directly in `config::vm`'s tests instead.
            for (bad, reason) in [
                ("a\"b", "would end or escape"),
                ("a\\b", "would end or escape"),
                ("a#{1}b", "Ruby interpolation"),
            ] {
                let source: String = full_registry()
                    .lines()
                    .map(|l| {
                        if l.starts_with(&format!("{field} = ")) {
                            format!("{field} = {bad:?}\n")
                        } else {
                            format!("{l}\n")
                        }
                    })
                    .collect();
                // The substitution has to have happened, or this
                // loop would assert nothing at all.
                assert!(
                    source.contains(&format!("{field} = ")),
                    "{field} is not a key in the fixture"
                );
                // The message is asserted, not the variant,
                // because the two differ by field. `repo` and
                // `script` are newtypes checked while serde
                // deserializes, so a bad one arrives wrapped as
                // `Parse`, which also names the line. `box` and
                // `ref` are checked after parsing, as `Invalid`.
                // Both spellings carry the field and the reason.
                //
                // The reason has to be asserted as well as the
                // field. `toml` backticks field names in its own
                // messages, so a field-only check also passes on
                // `missing field` or `unknown field` -- and then
                // this test would go green on a fixture that had
                // drifted, while proving nothing about the guard.
                let err = parse_whole(&source).unwrap_err().to_string();
                assert!(
                    err.contains(&format!("`{field}`")) && err.contains(reason),
                    "{field} must refuse {bad:?} with {reason:?}, got {err}"
                );
            }
        }
    }
}

#[cfg(test)]
mod load_project_tests {
    use std::path::Path;

    use super::*;

    /// [`Config::load_project`] against a registry given as
    /// text, keeping the [`HostOrigin`] `Config::parse_registry`
    /// drops.
    ///
    /// No file is written. `Config::load_project`'s own reading
    /// is covered by the tests that do write one, and every test
    /// about ranking or assembly goes through here.
    fn load(
        source: &str,
        name: &str,
    ) -> Result<(Config, HostOrigin), ConfigError> {
        let registry =
            registry::parse_for_tests(source, Path::new(USER_CONFIG_FILE))?;
        Config::from_registry(&registry, name)
    }

    /// A registry naming one project and nothing unusual.
    fn plain() -> String {
        test_registry("myproject", "vmhost", None)
    }

    /// The `myproject` entry as [`HostOrigin`] names it.
    fn entry_origin() -> HostOrigin {
        HostOrigin::ProjectEntry(
            crate::name::ProjectName::parse("myproject").unwrap(),
        )
    }

    #[test]
    fn every_setting_comes_out_of_the_entry() {
        let (cfg, origin) = load(&plain(), "myproject").unwrap();

        // The table key becomes the project name: the entry does
        // not carry one, so the two cannot disagree.
        assert_eq!(cfg.project, "myproject");
        assert_eq!(cfg.host, "vmhost");
        // Absent from the entry, so the default applies.
        assert_eq!(cfg.remote_root, DEFAULT_REMOTE_ROOT);
        assert_eq!(cfg.vm.provider, Provider::Libvirt);
        assert_eq!(cfg.vm.box_name, "generic/ubuntu2204");
        assert_eq!(cfg.vm.cpus, 2);
        assert_eq!(cfg.vm.memory, 2048);
        assert_eq!(cfg.source.git_ref, "main");
        assert_eq!(cfg.source.script.as_str(), "vagrant/provision.sh");
        // The entry names no host of its own, so the file-wide
        // key won.
        assert_eq!(origin, HostOrigin::UserFile);
    }

    #[test]
    fn the_entrys_own_host_outranks_the_file_wide_one() {
        let source = test_registry("myproject", "file-wide", Some("entry"));
        let (cfg, origin) = load(&source, "myproject").unwrap();
        assert_eq!(cfg.host, "entry");
        assert_eq!(origin, entry_origin());
    }

    #[test]
    fn the_reported_entry_names_its_table_and_its_file() {
        // The startup notice is built from `Display`, and it
        // exists so the operator can see which machine `destroy`
        // will run `rm -rf` on. A project's host and the
        // file-wide host come out of the same `config.toml`, so
        // naming the file alone would not say which of the two
        // won.
        let text = entry_origin().to_string();
        assert!(text.contains("myproject"), "{text}");
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
    }

    #[test]
    fn the_host_comes_from_the_entry_being_loaded() {
        // Two entries, each naming its own machine. Taking the
        // host from the other one would boot `myproject`'s VM on
        // `other`'s machine, and `destroy` would `rm -rf` there.
        let source = format!(
            "{}\n{}",
            test_registry("myproject", "file-wide", Some("mine")),
            test_entry("other", Some("theirs"))
        );
        let (cfg, origin) = load(&source, "myproject").unwrap();
        assert_eq!(cfg.host, "mine");
        assert_eq!(origin, entry_origin());
    }

    #[test]
    fn no_host_anywhere_in_the_file_says_to_add_one() {
        // The file parses and names a machine nowhere, which
        // must read as a setting to add rather than as a broken
        // file.
        let err =
            load(&test_entry("myproject", None), "myproject").unwrap_err();
        assert!(matches!(err, ConfigError::HostMissing { .. }), "{err}");
        let text = err.to_string();
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
        assert!(text.contains("`host`"), "{text}");
    }

    #[test]
    fn a_broken_entry_is_reported_before_a_missing_host() {
        // Both are wrong at once: the entry says `cpus = 0` and
        // neither `host` key names a machine. The entry is read
        // first, so the entry's problem is what the operator is
        // told about. It is the one they can act on with the file
        // already open, and the host message would send them to
        // that same file for a second edit.
        let source =
            test_entry("myproject", None).replace("cpus = 2", "cpus = 0");
        let err = load(&source, "myproject").unwrap_err();
        let text = err.to_string();
        assert!(text.contains("cpus"), "{text}");
        assert!(
            !matches!(err, ConfigError::HostMissing { .. }),
            "the host message wins only if the entry is read second: {text}"
        );
    }

    #[test]
    fn a_name_with_no_table_names_the_table_to_write() {
        let err = load("host = \"vmhost\"\n", "myproject").unwrap_err();
        assert!(matches!(err, ConfigError::ProjectNotFound { .. }), "{err}");
        let text = err.to_string();
        assert!(text.contains("[projects.\"myproject\"]"), "{text}");
    }

    #[test]
    fn the_registry_is_read_from_the_path_it_is_given() {
        // The one test that goes through the file, so
        // `Config::load_project`'s own reading is exercised
        // rather than only the assembly below it. The file is
        // named `elsewhere.toml` because `--config` takes any
        // path, so nothing may depend on the default name.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("elsewhere.toml");
        std::fs::write(&path, plain()).unwrap();
        let (cfg, origin) =
            Config::load_project("myproject", Some(&path)).unwrap();
        assert_eq!(cfg.project, "myproject");
        assert_eq!(cfg.host, "vmhost");
        assert_eq!(origin, HostOrigin::UserFile);
    }

    #[test]
    fn no_registry_file_says_which_file_to_create() {
        // The directory exists and the file does not, which is
        // every machine bombyx has never run on.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USER_CONFIG_FILE);
        let err = Config::load_project("myproject", Some(&path)).unwrap_err();
        assert!(matches!(err, ConfigError::RegistryNotFound { .. }), "{err}");
        let text = err.to_string();
        // The path to create, and what goes in it. A message
        // with only the first leaves the operator with an empty
        // file.
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
        assert!(text.contains("[projects.\"myproject\"]"), "{text}");
    }

    #[test]
    fn no_config_directory_describes_the_file_instead() {
        // Nothing in the environment names a config directory,
        // so `registry_file` produced no path, and the message
        // describes the file rather than naming one.
        // `registry_place` decides both wordings.
        let err = Config::load_project("myproject", None).unwrap_err();
        assert!(matches!(err, ConfigError::RegistryNotFound { .. }), "{err}");
        let text = err.to_string();
        assert!(text.contains("config directory"), "{text}");
        assert!(text.contains("[projects.\"myproject\"]"), "{text}");
    }

    #[test]
    fn a_name_with_a_dot_is_advised_as_a_quoted_table() {
        // `check_segment` allows `.` after the first character,
        // so `a.b` is a legal project name -- and a bare
        // `[projects.a.b]` is not the table it looks like. TOML
        // reads the dot as nesting, so that heading declares
        // `b` inside `projects.a`, and `deny_unknown_fields`
        // then refuses the file with `unknown field \`b\``. An
        // operator following that advice breaks every project in
        // the file, which is the harm the name check above
        // exists to prevent -- for `/` and `..` it does, and
        // this is the member of the family it let through.
        //
        // Quoting is what makes one spelling right for every
        // name `check_segment` accepts.
        for name in ["a.b", "a-b.c", "myproject"] {
            for err in [
                Config::load_project(name, None).unwrap_err(),
                load("host = \"vmhost\"\n", name).unwrap_err(),
            ] {
                let text = err.to_string();
                assert!(
                    text.contains(&format!("[projects.{name:?}]")),
                    "{name:?} must be advised quoted, got {text}"
                );
            }
        }
    }

    #[test]
    fn an_illegal_name_is_refused_before_the_registry_is_looked_for() {
        // The name rule runs first, so no message ever advises a
        // table heading the parser would refuse. `[projects...]`
        // with a `/` or a `..` in it cannot be written down: the
        // whole file fails to parse, so an operator following
        // the advice breaks every project rather than fixing
        // this one.
        //
        // The whole family, not the case that prompted it. Each
        // one is refused for its own reason inside
        // `name::check_segment`, and the point here is that
        // `load_project` consults that function at all.
        for name in ["", ".", "..", "../../etc", "-x", "a/b", "a/"] {
            // No registry path, which is the route that had no
            // check: with one, `Registry::project` runs the rule
            // before the map lookup.
            let err = Config::load_project(name, None).unwrap_err();
            let text = err.to_string();
            assert!(
                matches!(
                    err,
                    ConfigError::Invalid {
                        field: "project",
                        ..
                    }
                ),
                "{name:?} must be refused as a project name, got {text}"
            );
        }
    }
}
