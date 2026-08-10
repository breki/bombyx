//! bombyx CLI entry point.
//!
//! Thin by design: parse arguments, hand off to the library
//! to build the command list, then run it. The mapping from a
//! subcommand to its commands lives in `bombyx::plan`, where
//! it is covered by tests.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

use anyhow::{Context, Result, anyhow, bail};
use bombyx::config::Config;
use bombyx::doctor::{
    self, Finding, HostProbe, Outcome, ProbeResult, Report, VersionAnswer,
};
use bombyx::name::ScratchName;
use bombyx::plan::{Action, plan};
use bombyx::remote::{PushArchive, RemoteCommand};
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

    /// Print the command that would run, without running it
    #[arg(long)]
    dry_run: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Push the Vagrant dir and boot the project VM
    Up,
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
        Ok(code) => code,
        Err(err) => {
            eprintln!("bombyx: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)
        .with_context(|| format!("loading {}", cli.config.display()))?;
    let action = action_of(&cli.command, &cfg)?;

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
fn action_of(cmd: &Cmd, cfg: &Config) -> Result<Action> {
    Ok(match cmd {
        Cmd::Up => Action::Up,
        Cmd::Down => Action::Down,
        Cmd::Shell => Action::Shell,
        Cmd::Status => Action::Status,
        Cmd::Reset => Action::Reset,
        Cmd::Doctor => Action::Doctor,
        Cmd::Destroy { project } => {
            confirm_destroy(project.as_deref(), cfg)?;
            Action::Destroy
        }
        Cmd::Scratch { name } => Action::Scratch(vm_name(name)?),
        Cmd::Discard { name } => Action::Discard(vm_name(name)?),
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

/// Runs (or, for a dry run, prints) each command in order,
/// stopping at the first failure.
///
/// A failing step's exit code is passed through rather than
/// flattened, so `bombyx status` stays scriptable and an
/// `ssh` transport failure (255) remains distinguishable from
/// whatever the remote `vagrant` returned.
fn execute(commands: &[RemoteCommand], dry_run: bool) -> Result<ExitCode> {
    if dry_run {
        for cmd in commands {
            println!("{cmd}");
        }
        return Ok(ExitCode::SUCCESS);
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
            return Ok(exit_code(status));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Runs every precondition probe and prints the report.
///
/// Deliberately thin. Every decision -- the probe list, reading a
/// result, the skip cascade, rendering, the exit code -- lives in
/// `bombyx::doctor`, for the reason its module doc gives. What is
/// left here is process spawning.
fn doctor_run(cfg: &Config, local_dir: &Path) -> ExitCode {
    let mut report = Report::default();
    report.add(local_tool("tar", Some("--version")));
    report.add(local_tool("ssh", Some("-V")));
    report.add(local_tool("scp", None));
    report.add(doctor::vagrantfile_finding(local_dir));
    report.add_all(doctor::run_probes(&doctor::host_probes(cfg), spawn_probe));

    print!("{}", report.render(&cfg.host));
    if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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

/// Maps a child's exit status onto this process's exit code,
/// falling back to 1 for a signal or an out-of-range code.
fn exit_code(status: ExitStatus) -> ExitCode {
    let code = status.code().unwrap_or(1);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
