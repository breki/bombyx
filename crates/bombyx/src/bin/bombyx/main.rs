//! bombyx CLI entry point.
//!
//! Parse arguments, hand off to the library to build the command
//! list, then run it. The mapping from a subcommand to its commands
//! lives in `bombyx::plan`, where it is covered by tests.
//!
//! It used to say "thin by design", and that is worth not claiming.
//! Four things genuinely live here and nowhere else: argument
//! parsing, the config-precedence reporting on stderr, spawning
//! processes, and the ordering of the `self-update` sequence -- and
//! this file is outside the coverage gate (`src/bin/`), so anything
//! that stays is untested. That is the reason to keep moving
//! decisions out of it, most recently the wording of the update
//! decision (`update::Decision::outcome`) and the post-extraction
//! re-check (`update::asset::confirm_unchanged`). `self_update` is
//! still the largest thing here.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

use anyhow::{Context, Result, anyhow, bail};
use bombyx::config::{Config, HostOrigin};
use bombyx::doctor::{
    self, Finding, HostProbe, Outcome, ProbeResult, Report, VersionAnswer,
};
use bombyx::name::ScratchName;
use bombyx::plan::{Action, plan};
use bombyx::remote::{PushArchive, RemoteCommand};
use bombyx::update::{self, asset};
use clap::{Parser, Subcommand};
use tempfile::TempDir;

/// Default configuration file name, looked up in the
/// current directory.
const CONFIG_FILE: &str = "bombyx.toml";

#[derive(Parser)]
#[command(name = "bombyx", version, about)]
struct Cli {
    /// Path to the project config
    #[arg(short, long, default_value = CONFIG_FILE)]
    config: PathBuf,

    /// SSH alias of the VM host, overriding your config.toml
    #[arg(long)]
    host: Option<String>,

    /// Print the command that would run, without running it
    #[arg(long)]
    dry_run: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Update this bombyx binary to the newest release
    ///
    /// Downloads the release archive for this platform and
    /// verifies it against the release's `SHA256SUMS` before
    /// replacing the binary. Refuses rather than installing
    /// anything it cannot verify, never installs a pre-release,
    /// and never downgrades a local build that is newer than any
    /// release. Needs `curl` and `tar`.
    SelfUpdate,

    // Everything else. Flattened, so the *invocation* surface is
    // identical -- `bombyx up`, not `bombyx vm up`. The `--help`
    // listing does change: `self-update` now heads it instead of
    // sitting between `destroy` and `scratch`, because a flattened
    // variant contributes its subcommands at its own position.
    // The type says what the code relies on: `self-update` is the
    // one subcommand that is not about a VM and does not read a
    // config. `action_of` used to carry a `Cmd::SelfUpdate =>
    // bail!("internal error")` arm kept unreachable by a
    // `matches!` four hundred lines away, and the next
    // config-less subcommand would have added a second one.
    #[command(flatten)]
    Vm(VmCmd),
}

/// The subcommands that drive a VM, so all of them need a
/// project config and a host.
#[derive(Subcommand)]
enum VmCmd {
    /// Push the Vagrant dir and boot the project VM
    Up,
    /// Push the Vagrant dir and re-run provisioning
    ///
    /// Vagrant provisions only when it first creates a VM, so
    /// every later `up` ships an edited provisioning script to
    /// the host without executing it. This applies it. The VM
    /// must already exist: run `up` first.
    Provision,
    /// Halt the project VM
    Down,
    /// Open a shell inside the project VM
    Shell,
    /// Show VM status on the host
    Status,
    /// Restore the project VM to its `fresh-install` snapshot
    Reset,
    /// Check bombyx's preconditions, changing nothing
    Doctor,
    /// Destroy the project VM and remove its directory
    ///
    /// Takes the project name as confirmation, since this
    /// discards the warm caches the persistent lifecycle
    /// exists to keep.
    Destroy {
        /// Must match `project` in `bombyx.toml`
        project: Option<String>,
    },
    /// Boot a throwaway VM for untrusted work
    Scratch {
        /// Name for the scratch VM, e.g. `pr-1234`
        name: String,
    },
    /// Destroy a throwaway VM
    Discard {
        /// Name of the scratch VM to destroy
        name: String,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(ran) => ran.code(),
        Err(err) => {
            eprintln!("bombyx: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Ran> {
    let cli = Cli::parse();

    // Handled before anything reads a config, because it is the
    // one subcommand that is not about a VM. Every other command
    // needs a project and a host; `self-update` needs neither,
    // and loading the config first would make updating bombyx
    // fail in any directory without a `bombyx.toml` -- including
    // the home directory, which is where someone would naturally
    // run it.
    //
    // An exhaustive `match`, not a `let ... else`: the point of the
    // `Cmd`/`VmCmd` split is that a third config-less subcommand
    // fails to compile here rather than being routed silently into
    // `self_update`.
    let vm = match cli.command {
        Cmd::SelfUpdate => return self_update(cli.dry_run),
        Cmd::Vm(vm) => vm,
    };

    // The VM host is not in the project file: it belongs to
    // whoever is driving bombyx, not to the repo. Highest
    // precedence first -- flag, environment, then the two files
    // `Config::load` reads.
    let env_host = std::env::var(bombyx::config::HOST_ENV).ok();
    let user_dir = bombyx::config::user_config_dir();
    let sources = bombyx::config::HostSources {
        flag: cli.host.as_deref(),
        env: env_host.as_deref(),
        user_config_dir: user_dir.as_deref(),
    };

    // The `loading <file>` context is added only for errors that
    // are really about that file. A host error is not: the host
    // cannot come from the project config at all, so prefixing
    // `loading bombyx.toml:` told the operator to edit the one
    // file that must not carry a host.
    let (cfg, host_origin) =
        Config::load(&cli.config, &sources).map_err(|err| match err {
            e @ (bombyx::config::ConfigError::HostMissing { .. }
            | bombyx::config::ConfigError::HostInProjectFile { .. }
            | bombyx::config::ConfigError::InvalidHost { .. }) => anyhow!(e),
            e => anyhow::Error::new(e)
                .context(format!("loading {}", cli.config.display())),
        })?;

    // Say when the committed config is not the one in force.
    // A typo in the override's *filename* silently falls back
    // to the committed values, and one line on stderr makes the
    // two states distinguishable without reading either file.
    if let Some(local) = bombyx::config::local_config_path(&cli.config)
        && local.is_file()
    {
        eprintln!(
            "bombyx: {} overrides {}",
            local.display(),
            cli.config.display()
        );
    }

    // And say which source the host came from, using the winner
    // the library reports rather than re-testing the sources
    // here. Re-deriving it duplicated the precedence rule in the
    // one place no library test could reach, so a change to the
    // ranking would have left this line naming the old winner --
    // and `destroy` runs `rm -rf` on whichever host really won.
    //
    // Printed unless it came from the per-developer file, which
    // is the ordinary case and would be noise on every command.
    // That also covers the gap the notice above leaves: an
    // overlay setting only `remote_root` prints "overrides", and
    // the host it does *not* set is reported here instead of
    // being silently attributed to that file.
    if host_origin != HostOrigin::UserFile {
        eprintln!("bombyx: host {} from {host_origin}", cfg.host);
    }

    let action = action_of(&vm, &cfg)?;

    // `tar` runs from the archive directory, so the source
    // directory has to be absolute.
    let local_dir = std::env::current_dir()
        .context("resolving the current directory")?
        .join(&cfg.vagrant_dir);

    // Only a pushing action needs a workspace. It is a
    // private directory owned by this run and removed when
    // the guard drops, so concurrent runs cannot collide and
    // no file bombyx did not create is ever deleted.
    let workspace = if action.pushes() {
        Some(TempDir::new().context("creating a temp directory")?)
    } else {
        None
    };
    // A non-pushing action never reads the archive, so the
    // fallback directory is not used for anything.
    let archive = PushArchive::new(
        workspace
            .as_ref()
            .map_or_else(|| Path::new("."), TempDir::path),
        &run_id(),
    );

    // Every action renders its dry run the same way, through
    // `plan`, so no subcommand can describe a run it would not
    // perform -- doctor included. Ordered so the two doctor
    // paths are exclusive: a dry run never builds the probe
    // structs, and a live run never builds their command lines
    // twice.
    if cli.dry_run {
        return execute(&plan(&action, &cfg, &local_dir, &archive), true);
    }
    if matches!(action, Action::Doctor) {
        return Ok(doctor_run(&cfg, &local_dir));
    }
    execute(&plan(&action, &cfg, &local_dir, &archive), false)
}

/// Checks for a newer release and installs it.
///
/// Does not go through `plan`, because there is a decision in the
/// middle: the tag list has to be *read* before the rest can be
/// built at all. The steps that follow the decision come from
/// [`asset::plan`], so their order and their URLs are asserted in
/// the library rather than assembled here.
///
/// The tag list is fetched even for a dry run: it changes nothing
/// locally, and a dry run that skipped it could only print a
/// guess at the version.
fn self_update(dry_run: bool) -> Result<Ran> {
    let current = update::Version::parse(update::CURRENT).ok_or_else(|| {
        anyhow!("this build's version {:?} is not X.Y.Z", update::CURRENT)
    })?;

    let list = update::list_releases_command();
    if dry_run {
        println!("{list}");
    }
    let Some(latest) = newer_release(current, &list)? else {
        return Ok(Ran::Ok);
    };

    let triple = asset::target_triple().ok_or_else(|| {
        anyhow!(
            "no release is published for {}/{}; build from source \
             instead:\n  {}",
            std::env::consts::ARCH,
            std::env::consts::OS,
            update::install_command(latest)
        )
    })?;
    let dir = target_dir(latest)?;
    let work = TempDir::new().context("creating a temp directory")?;
    let plan = asset::plan(latest, triple, work.path());

    if dry_run {
        for cmd in plan.steps() {
            println!("{cmd}");
        }
        return Ok(Ran::Ok);
    }

    println!("bombyx: updating {current} -> {latest}");
    let sums = fetch_verified(&plan, latest)?;

    if !ran_ok(&plan.extract)? {
        bail!("extracting {} failed", plan.archive);
    }
    asset::confirm_unchanged(&plan.archive_path, &sums, &plan.archive)?;

    let placed = update::place(&plan.extracted, &dir, &run_id())?;
    // Both sentences come from the library, where a test can read
    // them. The wording of each is explained beside it.
    for notice in [placed.sweep_notice(), placed.leftover_notice()]
        .into_iter()
        .flatten()
    {
        eprintln!("bombyx: {notice}");
    }
    println!("bombyx: updated to {latest} in {}", dir.display());
    Ok(Ran::Ok)
}

/// The newest release when it is worth installing, else `None`.
///
/// Prints its own reason for the three no-op answers, so the
/// caller has nothing to decide. `None` is not a failure: being up
/// to date, and being ahead of every release, are both ordinary.
fn newer_release(
    current: update::Version,
    list: &RemoteCommand,
) -> Result<Option<update::Version>> {
    let tags = capture(list)?;
    let decision = update::decide(current, update::newest_release(&tags));
    // The three sentences live in the library, with the decision they
    // describe. They used to be written here, where the coverage gate
    // does not reach and no test asserted which version each names.
    match decision.outcome() {
        update::Outcome::Install(latest) => Ok(Some(latest)),
        update::Outcome::Nothing(why) => {
            println!("{why}");
            Ok(None)
        }
        update::Outcome::Refuse(why) => bail!("{why}"),
    }
}

/// Downloads the archive and refuses unless it verifies.
///
/// The checksum file is fetched **first**, so a release that
/// cannot be verified is discovered before an archive is
/// downloaded rather than after.
///
/// None of the failures here claim to know *why* the fetch
/// failed. An earlier version answered a non-zero `curl` with
/// "this release predates checksummed releases", which is one
/// cause among DNS failure, a proxy, a 403 and a dropped
/// connection -- so on a network that merely blocked the file,
/// bombyx confidently advised abandoning verification.
fn fetch_verified(
    plan: &asset::UpdatePlan,
    latest: update::Version,
) -> Result<String> {
    let by_hand = || {
        format!("or install by hand:\n  {}", update::install_command(latest))
    };

    if !ran_ok(&plan.get_sums)? {
        bail!(
            "could not fetch {} for {} (curl's error is above).\n  \
             If that release has none, it predates checksummed \
             releases and cannot be verified here -- {}",
            asset::SUMS_FILE,
            latest.tag(),
            by_hand()
        );
    }
    if !ran_ok(&plan.get_archive)? {
        bail!("could not download {} -- {}", plan.archive, by_hand());
    }

    let sums = std::fs::read_to_string(&plan.sums_path)
        .with_context(|| format!("reading {}", plan.sums_path.display()))?;
    // A zero-length body passes `curl -f` -- a 200 with nothing in
    // it, or a truncated transfer -- and would otherwise be
    // reported as "no entry for this asset", which is a claim about
    // the release rather than about the download.
    if sums.trim().is_empty() {
        bail!(
            "{} for {} is empty; the download was truncated",
            asset::SUMS_FILE,
            latest.tag()
        );
    }

    let bytes = std::fs::read(&plan.archive_path)
        .with_context(|| format!("reading {}", plan.archive_path.display()))?;
    asset::verify(&sums, &plan.archive, &bytes).with_context(by_hand)?;
    println!("bombyx: {} matches its published checksum", plan.archive);
    Ok(sums)
}

/// Where the binary being replaced lives.
///
/// **The directory holding the *running* executable**, not a
/// directory guessed from the environment. Those differ more often
/// than they look: `cargo install --root`, a copy into `~/bin`, a
/// Scoop or winget shim, or simply running
/// `target\release\bombyx.exe`. Deriving it from `CARGO_HOME`
/// alone wrote a fresh binary into `~/.cargo/bin`, printed
/// `updated`, and left the binary the operator actually invokes
/// untouched -- a success message for a no-op.
///
/// [`update::install_dir`] is the fallback for the platforms where
/// `current_exe` can fail, and it is only a fallback.
fn target_dir(latest: update::Version) -> Result<PathBuf> {
    if let Some(dir) = update::running_dir() {
        return Ok(dir);
    }
    update::install_dir().ok_or_else(|| {
        anyhow!(
            "cannot tell which directory holds this binary, and \
             the environment names no cargo home either: install \
             by hand:\n  {}",
            update::install_command(latest)
        )
    })
}

/// Runs one command, reporting only whether it succeeded.
///
/// `execute` passes a failing status through rather than turning it
/// into an error, which is right for the VM commands whose status is
/// the tool's own answer. Here the two failures need different
/// messages, so the caller decides -- and a bare `execute` result
/// would make "no such asset" and "network down" look identical.
fn ran_ok(cmd: &RemoteCommand) -> Result<bool> {
    Ok(execute(std::slice::from_ref(cmd), false)?.ok())
}

/// Runs a command and returns its stdout.
///
/// Separate from `execute`, which streams and keeps only the exit
/// status. Resolution goes through `tool` for the same reason
/// every other program does -- see that module; the working
/// directory is never searched.
fn capture(cmd: &RemoteCommand) -> Result<String> {
    let program = bombyx::tool::resolve(&cmd.program)
        .ok_or_else(|| anyhow!("{}", doctor::not_on_path(&cmd.program)))?;
    let out = std::process::Command::new(program)
        .args(&cmd.args)
        .output()
        .with_context(|| format!("running {}", cmd.program))?;
    if !out.status.success() {
        // The program's own stderr is the useful part -- for
        // `git ls-remote` it distinguishes "no network" from
        // "repository not found".
        let reason = String::from_utf8_lossy(&out.stderr);
        bail!("{} failed: {}\n{}", cmd.program, out.status, reason.trim());
    }
    String::from_utf8(out.stdout)
        .with_context(|| format!("{} printed invalid UTF-8", cmd.program))
}

/// Returns a string distinguishing this run from any other
/// pushing to the same host.
fn run_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!("{}-{nanos}", std::process::id())
}

/// Converts the parsed subcommand into a library action,
/// validating any user-supplied VM name.
///
/// Total over [`VmCmd`], which is the point of that type existing.
/// It used to take `Cmd` and carry a `Cmd::SelfUpdate =>
/// bail!("internal error: ...")` arm, unreachable only because of a
/// `matches!` four hundred lines away -- an invariant maintained by
/// a comment instead of by the types.
fn action_of(cmd: &VmCmd, cfg: &Config) -> Result<Action> {
    Ok(match cmd {
        VmCmd::Up => Action::Up,
        VmCmd::Provision => Action::Provision,
        VmCmd::Down => Action::Down,
        VmCmd::Shell => Action::Shell,
        VmCmd::Status => Action::Status,
        VmCmd::Reset => Action::Reset,
        VmCmd::Doctor => Action::Doctor,
        VmCmd::Destroy { project } => {
            confirm_destroy(project.as_deref(), cfg)?;
            Action::Destroy
        }
        VmCmd::Scratch { name } => Action::Scratch(vm_name(name)?),
        VmCmd::Discard { name } => Action::Discard(vm_name(name)?),
    })
}

fn vm_name(raw: &str) -> Result<ScratchName> {
    ScratchName::parse(raw).with_context(|| format!("invalid VM name {raw:?}"))
}

/// Requires `given` to name the configured project, and always
/// reports the target being destroyed.
///
/// `down` halts a VM and `reset` rolls it back; `destroy`
/// throws away the warm caches and installed tooling that make
/// the persistent lifecycle worth having, so it asks for a
/// deliberate act rather than a flag.
///
/// Printing the resolved target is the more important half.
/// The name alone confirms nothing an attacker could not have
/// chosen: `project` comes from the same `bombyx.toml` that
/// decides which directory is deleted, so a repo can name
/// itself after a VM you care about. What the operator can
/// check against reality is `<host>:<dir>`, so that is shown on
/// both the refusal and the confirmed path.
fn confirm_destroy(given: Option<&str>, cfg: &Config) -> Result<()> {
    let target = format!("{}:{}", cfg.host, cfg.remote_project_dir());
    match given {
        Some(name) if name == cfg.project => {
            eprintln!("bombyx: destroying {target}");
            Ok(())
        }
        Some(name) => bail!(
            "{name:?} does not match the project in this directory \
             ({:?}); refusing to destroy {target}",
            cfg.project
        ),
        None => bail!(
            "destroy needs the project name to confirm: run \
             `bombyx destroy {}` -- target is {target}",
            cfg.project
        ),
    }
}

/// What running a command list came to.
///
/// Not an [`ExitCode`]: that is a *process-exit* type, and using it
/// as the domain answer made every caller re-derive "did it work"
/// by comparing against `ExitCode::SUCCESS`, leaning on a
/// `PartialEq` that opaque type does not exist to provide. It
/// carries a raw status byte instead, so this type *can* be
/// compared, and the single conversion happens in [`main`].
#[derive(Debug, PartialEq, Eq)]
enum Ran {
    /// Every command succeeded.
    Ok,
    /// One failed; this is the status to exit with.
    Failed(u8),
}

impl Ran {
    /// Whether every command succeeded.
    fn ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    /// The code this process should exit with.
    fn code(self) -> ExitCode {
        match self {
            Self::Ok => ExitCode::SUCCESS,
            Self::Failed(status) => ExitCode::from(status),
        }
    }
}

/// Runs (or, for a dry run, prints) each command in order,
/// stopping at the first failure.
///
/// A failing step's exit code is passed through rather than
/// flattened, so `bombyx status` stays scriptable and an
/// `ssh` transport failure (255) remains distinguishable from
/// whatever the remote `vagrant` returned.
fn execute(commands: &[RemoteCommand], dry_run: bool) -> Result<Ran> {
    if dry_run {
        for cmd in commands {
            println!("{cmd}");
        }
        return Ok(Ran::Ok);
    }

    // Resolve every program before running any of them, through
    // `tool` -- never the working directory; see that module.
    //
    // Up front, because resolving inside the loop would let `up`
    // create the remote directory and only then discover that
    // `tar` is missing: the change-state-then-fail behaviour the
    // whole `doctor` command exists to prevent.
    let mut resolved: HashMap<&str, PathBuf> = HashMap::new();
    for cmd in commands {
        if let Entry::Vacant(slot) = resolved.entry(&cmd.program) {
            let found =
                bombyx::tool::resolve(&cmd.program).ok_or_else(|| {
                    anyhow!("{}", doctor::not_on_path(&cmd.program))
                })?;
            slot.insert(found);
        }
    }

    for cmd in commands {
        let mut child =
            std::process::Command::new(&resolved[cmd.program.as_str()]);
        child.args(&cmd.args);
        if let Some(dir) = &cmd.dir {
            child.current_dir(dir);
        }
        let status = child
            .status()
            .with_context(|| format!("running {}", cmd.program))?;
        if !status.success() {
            eprintln!("bombyx: {cmd} failed: {status}");
            return Ok(Ran::Failed(exit_status_byte(status)));
        }
    }
    Ok(Ran::Ok)
}

/// Runs every precondition probe and prints the report.
///
/// Deliberately thin. Every decision -- the probe list, reading a
/// result, the skip cascade, rendering, the exit code -- lives in
/// `bombyx::doctor`, for the reason its module doc gives. What is
/// left here is process spawning.
fn doctor_run(cfg: &Config, local_dir: &Path) -> Ran {
    let mut report = Report::default();
    report.add(local_tool("tar", Some("--version")));
    report.add(local_tool("ssh", Some("-V")));
    report.add(local_tool("scp", None));
    report.add(doctor::vagrantfile_finding(local_dir));
    report.add_all(doctor::run_probes(&doctor::host_probes(cfg), spawn_probe));

    print!("{}", report.render(&cfg.host));
    if report.ok() { Ran::Ok } else { Ran::Failed(1) }
}

/// Runs one host probe, turning a spawn failure into a finding.
///
/// Propagating the error instead would discard the whole report
/// -- including findings already gathered -- for the most likely
/// local misconfiguration there is, `ssh` missing from `PATH`.
/// A diagnostic that refuses to diagnose is worse than a wrong
/// answer.
fn spawn_probe(p: &HostProbe) -> Outcome {
    // No bare-name fallback. Spawning the unresolved name goes
    // straight back through the OS search that `tool` exists to
    // avoid -- and doctor is the command run first in a fresh
    // clone, so it is the worst place to reintroduce it.
    let Some(program) = bombyx::tool::resolve(&p.command.program) else {
        return Outcome::Fail(doctor::not_on_path(&p.command.program));
    };
    match std::process::Command::new(&program)
        .args(&p.command.args)
        .output()
    {
        Ok(o) => doctor::classify(&ProbeResult::from_output(&o), p.verdict),
        Err(e) => Outcome::Fail(doctor::cannot_run(
            &p.command.program,
            &e.to_string(),
        )),
    }
}

/// Looks a tool up on this workstation and, where there is a
/// version to ask for, asks.
///
/// Spawning only. What the results *mean* -- absent, present,
/// present but unusable, present but uncommunicative -- is
/// `doctor::local_tool_finding`'s job, for the same reason
/// `doctor_run` is thin.
///
/// `version_arg` is per tool: `tar` answers `--version`, OpenSSH
/// `ssh` answers `-V`, and `scp` answers neither -- asking it
/// prints a usage message that would land in the report as noise.
fn local_tool(name: &str, version_arg: Option<&str>) -> Finding {
    let resolved = bombyx::tool::resolve(name);
    let version = match (resolved.as_deref(), version_arg) {
        (Some(path), Some(arg)) => {
            match std::process::Command::new(path).arg(arg).output() {
                Ok(o) => VersionAnswer::Answered(ProbeResult::from_output(&o)),
                Err(e) => VersionAnswer::WouldNotStart(e.to_string()),
            }
        }
        _ => VersionAnswer::NotAsked,
    };
    doctor::local_tool_finding(name, resolved.as_deref(), &version)
}

/// Maps a child's exit status onto a status byte, falling back to
/// 1 for a signal or an out-of-range code.
fn exit_status_byte(status: ExitStatus) -> u8 {
    let code = status.code().unwrap_or(1);
    u8::try_from(code).unwrap_or(1)
}
