//! bombyx CLI entry point.
//!
//! Thin by design: parse arguments, hand off to the library
//! to build the command list, then run it. The mapping from a
//! subcommand to its commands lives in `bombyx::plan`, where
//! it is covered by tests.

use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

use anyhow::{Context, Result};
use bombyx::config::Config;
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
    let action = action_of(&cli.command)?;

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

    let commands = plan(&action, &cfg, &local_dir, &archive);
    execute(&commands, cli.dry_run)
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
fn action_of(cmd: &Cmd) -> Result<Action> {
    Ok(match cmd {
        Cmd::Up => Action::Up,
        Cmd::Down => Action::Down,
        Cmd::Shell => Action::Shell,
        Cmd::Status => Action::Status,
        Cmd::Reset => Action::Reset,
        Cmd::Scratch { name } => Action::Scratch(vm_name(name)?),
        Cmd::Discard { name } => Action::Discard(vm_name(name)?),
    })
}

fn vm_name(raw: &str) -> Result<ScratchName> {
    ScratchName::parse(raw).with_context(|| format!("invalid VM name {raw:?}"))
}

/// Runs (or, for a dry run, prints) each command in order,
/// stopping at the first failure.
///
/// A failing step's exit code is passed through rather than
/// flattened, so `bombyx status` stays scriptable and an
/// `ssh` transport failure (255) remains distinguishable from
/// whatever the remote `vagrant` returned.
fn execute(commands: &[RemoteCommand], dry_run: bool) -> Result<ExitCode> {
    for cmd in commands {
        if dry_run {
            println!("{cmd}");
            continue;
        }
        let mut child = std::process::Command::new(&cmd.program);
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

/// Maps a child's exit status onto this process's exit code,
/// falling back to 1 for a signal or an out-of-range code.
fn exit_code(status: ExitStatus) -> ExitCode {
    let code = status.code().unwrap_or(1);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
