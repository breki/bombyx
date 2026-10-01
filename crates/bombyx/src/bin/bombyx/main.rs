//! bombyx CLI entry point.
//!
//! Parse arguments, hand off to the library to build the command
//! list, then run it. The mapping from a subcommand to its commands
//! lives in `bombyx::plan`, where it is covered by tests.
//!
//! Four things live here and nowhere else: argument parsing, the
//! config-precedence reporting on stderr, printing a plan or a
//! failure, and the ordering of the `self-update` sequence.
//! `self_update` is the largest of them. Starting a process is
//! `run`'s job.
//!
//! It sits outside the coverage gate (`src/bin/`), so anything that
//! stays here ships untested. That is the standing reason to put
//! each new decision in the library instead: the wording of an
//! update outcome belongs in `update::Decision::outcome` and a
//! post-extraction re-check in `update::asset::confirm_unchanged`,
//! because both are decisions and neither needs a process.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

use anyhow::{Context, Result, anyhow, bail};
use bombyx::config::{Config, HostOrigin, Staged, Transport};
use bombyx::confirm::{Consent, DestroyFlags, Terminals, confirm_destroy};
use bombyx::doctor::{
    self, Finding, HostProbe, Outcome, ProbeResult, Report, VersionAnswer,
};
use bombyx::listing;
use bombyx::name::{ProjectName, ScratchName};
use bombyx::plan::{self, Action, StagedRead, plan};
use bombyx::remote::{self, RemoteCommand, Tty};
use bombyx::term;
use bombyx::update::{self, asset};
use clap::{Args, Parser, Subcommand};
use tempfile::TempDir;

#[derive(Parser)]
#[command(name = "bombyx", version, about)]
struct Cli {
    /// Path to your `config.toml`, the project registry; defaults
    /// to the one in your config directory
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Print the command that would run, without running it
    #[arg(long, global = true)]
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
    /// release. Needs `git`, `curl` and `tar`.
    SelfUpdate,

    /// List the registered projects and what their VMs are doing
    ///
    /// Reads your config file and asks each machine named in it
    /// what its projects are doing. A machine that does not
    /// answer leaves its projects `unknown` and a note on
    /// stderr; it does not stop the listing.
    ///
    /// Any project whose state could not be established makes
    /// the command exit non-zero, so a script can tell a
    /// complete table from one with gaps in it. That covers a
    /// machine bombyx could not reach and a machine that
    /// answered without naming a state.
    List {
        /// List what the config file holds, contacting no machine
        ///
        /// For a workstation away from the hosts. The `STATE`
        /// column is left out rather than filled with dashes,
        /// because no machine was asked.
        #[arg(long)]
        offline: bool,
    },

    // Everything else. Flattened, so the *invocation* surface is
    // identical -- `bombyx up`, not `bombyx vm up`. The `--help`
    // listing does change: `self-update` now heads it instead of
    // sitting between `destroy` and `scratch`, because a flattened
    // variant contributes its subcommands at its own position.
    // The type says what the code relies on: `self-update` is the
    // one subcommand that is not about a VM and does not read a
    // config. Splitting the two means `action_of` is total over
    // `VmCmd`, so a second config-less subcommand is a compile
    // error rather than an unreachable bail arm held in place by
    // a `matches!` somewhere else in the file.
    #[command(flatten)]
    Vm(VmCmd),
}

/// The subcommands that drive a VM, so all of them need a
/// project config and a host.
#[derive(Subcommand)]
enum VmCmd {
    /// Write the generated files on the VM host and boot the
    /// project VM
    ///
    /// A VM that already exists is not provisioned again, so its
    /// copy of the secrets from `env_file` or `vault`, and the git
    /// credential when `repo_token` is set, are rewritten in the
    /// guest once it is up. On a running VM, that is all `up`
    /// does. Nothing is fetched or checked out, so work in the
    /// guest's clone is untouched.
    ///
    /// The project's `secrets_refreshed` hook, when its `[hooks]`
    /// table names one, runs from the clone after the rewrite, and
    /// after provisioning on the `up` that creates the VM. A
    /// failed rewrite or hook makes `up` exit non-zero.
    Up(ProjectArg),
    /// Write the generated files and re-run provisioning in
    /// the guest
    ///
    /// Vagrant provisions only when it first creates a VM, so
    /// every later `up` leaves the guest on the commit it
    /// checked out then. This re-runs the bootstrap, which
    /// fetches your repository and checks out `ref` again in
    /// the clone the guest already has.
    ///
    /// `bootstrap.sh` forces that checkout, so it overwrites
    /// your edits to tracked files. It also overwrites an
    /// untracked file when the fetched commit adds one at the
    /// same path. An untracked file survives only where the
    /// commit has nothing at that path.
    ///
    /// A forced checkout of `FETCH_HEAD` detaches HEAD, so
    /// committing in the guest does not protect work either: the
    /// next provision moves HEAD away and leaves that commit on
    /// no branch, findable only through `git reflog`. Push it to
    /// survive a provision.
    ///
    /// Pointing `source.repo` at a different repository removes
    /// the clone and starts over, which loses everything.
    /// Rewriting the same URL with or without a trailing `/` or
    /// `.git` keeps the clone.
    ///
    /// The project's `secrets_refreshed` hook, when its `[hooks]`
    /// table names one, runs after a successful provision, and a
    /// failed hook makes `provision` exit non-zero.
    ///
    /// The VM must already exist: run `up` first.
    Provision(ProjectArg),
    /// Halt the project VM
    Down(ProjectArg),
    /// Open a shell inside the project VM, in the project clone
    ///
    /// First rewrites the guest's copy of the secrets from
    /// `env_file` or `vault`, and the git credential when
    /// `repo_token` is set, and runs the `secrets_refreshed` hook,
    /// as `up` does. If any of that fails, bombyx warns and opens
    /// the shell anyway.
    Shell(ProjectArg),
    /// Show VM status on the host
    Status(ProjectArg),
    /// Restore the project VM to its `fresh-install` snapshot
    Reset(ProjectArg),
    /// Save the project VM's `fresh-install` snapshot
    ///
    /// `up` already takes this snapshot on a machine that has
    /// none, so the `reset` cycle works without running this.
    /// Use it to move the point `reset` returns to, or on a VM
    /// that was already in use before bombyx took snapshots at
    /// all -- there the snapshot `up` saved records that state
    /// rather than a fresh install.
    ///
    /// Replaces an existing snapshot without asking, which
    /// discards the state `reset` would have returned to. The VM
    /// and its caches are untouched.
    Snapshot(ProjectArg),
    /// Check bombyx's preconditions, changing nothing
    Doctor(ProjectArg),
    /// Destroy the project VM and remove its directory
    ///
    /// Prints the host and directory it is about to remove, then
    /// asks you to type the project name, since this discards
    /// the warm caches the persistent lifecycle exists to keep.
    /// A dry run asks nothing, because it destroys nothing.
    Destroy {
        #[command(flatten)]
        project: ProjectArg,
        /// Destroy without asking for the project name
        ///
        /// Needed where nobody can answer the question: when stdin
        /// or stderr is not a terminal, as under cron, in CI, from
        /// a pipe or with stderr redirected to a file, `destroy`
        /// refuses without it. A script started from a terminal
        /// still asks.
        #[arg(long)]
        yes: bool,
    },
    /// Boot a throwaway VM for untrusted work
    Scratch {
        #[command(flatten)]
        project: ProjectArg,
        /// Name for the scratch VM, e.g. `pr-1234`
        name: String,
    },
    /// Destroy a throwaway VM
    Discard {
        #[command(flatten)]
        project: ProjectArg,
        /// Name of the scratch VM to destroy
        name: String,
    },
}

/// The project a VM subcommand acts on, which each of them takes
/// as its first positional argument.
#[derive(Args)]
struct ProjectArg {
    /// The project to act on: a `[projects.<name>]` table in your
    /// config file
    ///
    /// bombyx reads nothing from the project's own directory, so
    /// it cannot work out which project you mean from where you
    /// are standing.
    //
    // Parsed by clap into the checked type, so a name no table
    // key could hold is a usage error naming this argument, and
    // no later message can advise a `[projects.<name>]` heading
    // the TOML parser refuses.
    #[arg(value_parser = ProjectName::parse)]
    project: ProjectName,
}

impl VmCmd {
    /// The project this subcommand names.
    fn project(&self) -> &ProjectName {
        match self {
            Self::Up(p)
            | Self::Provision(p)
            | Self::Down(p)
            | Self::Shell(p)
            | Self::Status(p)
            | Self::Reset(p)
            | Self::Snapshot(p)
            | Self::Doctor(p)
            | Self::Destroy { project: p, .. }
            | Self::Scratch { project: p, .. }
            | Self::Discard { project: p, .. } => &p.project,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(ran) => ran.code(),
        Err(err) => {
            // Through `eprint_lines` because `{err:#}` is routinely
            // *multi-line* -- the "no release is published for
            // this platform" message embeds a newline and so does
            // any anyhow chain -- and it runs after arbitrary
            // children, `ssh` probes included. A multi-line message
            // is exactly the shape worth protecting.
            eprint_lines(&format!("bombyx: {err:#}\n"));
            ExitCode::FAILURE
        }
    }
}

/// Whether to ask `ssh` for a remote pseudo-terminal.
///
/// **Windows only, deliberately.** The reason to want a PTY here is
/// that the remote tty then translates `\n` to `\r\n`, and a Unix
/// terminal needs no such translation -- so on Linux and macOS this
/// would buy nothing by its own rationale while still paying every
/// cost [`Tty`] lists. The one that matters: `-t` merges the
/// remote's stderr into stdout, so `bombyx up 2> err.log` would
/// capture nothing from the remote. Leaving those platforms on
/// [`Tty::NoPty`] keeps their behaviour exactly as it was.
///
/// `shell_into_vm` is unaffected and still allocates on every
/// platform: an interactive shell needs a tty for its own sake.
///
/// The two-boolean rule itself lives in [`Tty::for_streams`], where
/// a test can reach it. `IsTerminal` is `std`, so reading the
/// streams costs no dependency and no `unsafe` -- which matters,
/// because production crates here are `#[forbid(unsafe_code)]` and
/// the Win32 console API is therefore not an option.
fn tty_choice() -> Tty {
    if !cfg!(windows) {
        return Tty::NoPty;
    }
    Tty::for_streams(
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )
}

/// Writes `text` to stdout, ending lines the way it needs them.
///
/// **On a Windows console a bare `\n` is sometimes not enough**, and
/// the cause is the honest gap in this fix. What is *measured*: the
/// output staircases -- each line starting at the column where the
/// previous ended -- after a command that runs `ssh`, and
/// `self-update`, which spawns children but never `ssh`, prints
/// cleanly. The leading explanation is that `ssh.exe` leaves a
/// console-mode bit set that suppresses the console's implicit
/// carriage return (`DISABLE_NEWLINE_AUTO_RETURN` is the bit with
/// that effect). **That cause is unverified**: a redirected stdout
/// cannot reproduce a console-mode change, so confirming it needs a
/// real console.
///
/// The translation lives here rather than in the library.
/// [`Report::render`] keeps emitting `\n`, which is what keeps its
/// expected-output tests identical on every platform -- a renderer
/// that emitted `\r\n` on Windows would need two expectations, and
/// this project has already shipped one test that passed on Windows
/// alone. The substitution itself is [`bombyx::term::line_endings`],
/// which is pure and tested.
fn print_lines(text: &str) {
    print!(
        "{}",
        term::line_endings(text, crlf_wanted(&std::io::stdout()))
    );
}

/// [`print_lines`] for stderr.
///
/// Reads **stderr's** own terminal state, not stdout's. The streams
/// are redirected independently and sampling the wrong one is wrong
/// in both directions: `bombyx up > out.log` would leave the failure
/// line bare on the terminal, which is the case this exists for, and
/// `bombyx up 2> err.log` would write carriage returns into a
/// captured log, which the change promises not to do.
fn eprint_lines(text: &str) {
    eprint!(
        "{}",
        term::line_endings(text, crlf_wanted(&std::io::stderr()))
    );
}

/// Whether `stream` wants `\r\n`: a Windows terminal, and nothing
/// else.
///
/// Redirected or piped, the bytes stay as the library produced them.
fn crlf_wanted(stream: &impl IsTerminal) -> bool {
    cfg!(windows) && stream.is_terminal()
}

fn run() -> Result<Ran> {
    let cli = Cli::parse();

    // `self-update` is handled before anything reads a config,
    // because it is the one subcommand that is not about a VM at
    // all. Loading the config first would make updating bombyx
    // fail on a machine with no registry -- which is exactly the
    // machine somebody is trying to install bombyx on.
    //
    // An exhaustive `match`, not a `let ... else`: the three arms
    // are three different requirements, and a fourth subcommand
    // has to say which of them it has rather than being routed
    // silently into one.
    //
    // `--config` when the operator passed one, and otherwise the
    // file in whichever config directory the environment names.
    // `list` needs it too, which is why it is read before the
    // `list` arm rather than after it.
    let registry = cli.config.or_else(bombyx::config::registry_file);
    let vm = match cli.command {
        Cmd::SelfUpdate => return self_update(cli.dry_run),
        Cmd::List { offline } => {
            return list_run(offline, registry.as_deref(), cli.dry_run);
        }
        Cmd::Vm(vm) => vm,
    };

    // Required and checked by clap, so it is a `ProjectName` by
    // now. `Config::load_project` takes the checked value and
    // cannot be reached with anything else.
    let project = vm.project();

    // `Config::load_project` takes a path, so the machine whose
    // environment names no config directory is answered here,
    // where the environment was read. The message for it lives
    // in the library beside the message for a registry file
    // that is simply absent.
    let registry =
        registry.ok_or_else(|| Config::no_config_directory(project))?;

    // No arm names the registry file here. Every error that
    // could want one names it already: a value breaking its
    // type's rule is refused by serde and arrives as
    // `ConfigError::Parse`, which carries the path and the
    // line.
    let (cfg, host_origin) = Config::load_project(project, &registry)?;

    // Say which source the host came from, using the winner
    // the library reports rather than re-testing the sources
    // here. Re-deriving it would put a second copy of the
    // precedence rule where no library test reaches, so a
    // change to the ranking would leave this line naming the
    // wrong winner -- and `destroy` runs `rm -rf` on whichever
    // host really won.
    //
    // Silent only for the *file-wide* `host`, which is the
    // ordinary case and would be noise on every command. A
    // project's own `host` key sits in the same `config.toml`
    // and is printed, because the two keys share a file and the
    // line has to say which of them won.
    //
    // That exemption has a cost worth knowing at this line.
    // `BOMBYX_CONFIG_HOME` decides *which* config directory gets
    // read, and a per-directory environment tool can set it from
    // inside a clone -- so staying quiet here also hides a
    // redirect the operator did not choose. `docs/todo.md`
    // tracks it as `config-home-env-provenance`.
    //
    // `describe` rather than `Display`, so the line names the
    // file bombyx read. `Display` renders the bare `config.toml`,
    // and `--config` can point at any path at all.
    match &host_origin {
        HostOrigin::ProjectEntry(_) => {
            eprintln!(
                "bombyx: host {} from {}",
                cfg.host,
                host_origin.describe(&registry)
            );
        }
        HostOrigin::UserFile => {}
    }

    // The local route is announced and the `ssh` route is
    // silent, because `ssh` is the ordinary case and a line on
    // every command would be noise nobody reads.
    //
    // The local one is announced every time it applies, which
    // is more than the line above manages: the route is derived
    // from `host` rather than written down, so the operator
    // never chose it, one `config.toml` behaves differently
    // depending on which machine reads it, and it is the
    // arrangement that gives up most of the isolation --
    // `docs/architecture.md` says what.
    match cfg.transport() {
        Transport::Ssh => {}
        Transport::Local => eprintln!(
            "bombyx: host {} is this machine; running vagrant \
             here rather than over ssh",
            cfg.host
        ),
    }

    let action = action_of(&vm)?;
    if let VmCmd::Destroy { yes, .. } = vm {
        let stdin = std::io::stdin();
        let flags = DestroyFlags {
            yes,
            dry_run: cli.dry_run,
        };
        let terminals = Terminals {
            stdin: stdin.is_terminal(),
            stderr: std::io::stderr().is_terminal(),
        };
        let consent = Consent::of(flags, terminals);
        confirm_destroy(
            &cfg.project,
            &cfg.destroy_target(),
            consent,
            &mut stdin.lock(),
            &mut std::io::stderr(),
        )?;
    }
    let tty = tty_choice();

    // Read here, at the edge, because this is the only place in
    // bombyx that is allowed to touch the filesystem on a whim --
    // `plan` decides which commands run and nothing else.
    //
    // Before the plan is built, so a config naming a file this
    // machine does not have stops the run with the path in the
    // message: nothing is created anywhere before the secrets and
    // the deploy key are known to be here.
    //
    // A dry run reads it too, and that is deliberate rather than
    // an oversight in the sentence above. `plan` renders the
    // write step only when it is handed the contents, so a dry
    // run given `None` would print a plan missing a step the
    // real run performs -- and a plan that describes a different
    // run is worse than one that refuses. The contents never
    // reach the printed output either way: the line carries a
    // byte count, or for the git credential no count at all --
    // `crate::remote::Stdin` says which payloads may not report
    // their size and why.
    //
    // Only for the actions that consume them, which
    // `Action::staged_read` decides and explains. The verbs it
    // skips are the ones that must keep working after the
    // operator has deleted the file, and a best-effort read turns
    // a failure into a warning and an empty `Staged`, which
    // refreshes nothing.
    let staged = match action.staged_read() {
        StagedRead::Skip => Staged::default(),
        read => match cfg.read_staged(|k| std::env::var(k).ok()) {
            Ok(staged) => staged,
            Err(e) if read == StagedRead::BestEffort => {
                eprint_lines(&format!(
                    "bombyx: not refreshing the secrets in the guest, \
                     which keeps the ones it has: {e}\n"
                ));
                Staged::default()
            }
            Err(e) => return Err(e.into()),
        },
    };

    // `up` and `shell` decide on the machine's live state before
    // they act, and `provision` follows its run with the secrets
    // hook, so each owns both its dry run and its live run -- see
    // `up_run`, `shell_run` and `provision_run`. They handle
    // `dry_run` themselves, so they come before the generic dry-run
    // line below.
    if matches!(action, Action::Up) {
        return up_run(&cfg, tty, &staged, cli.dry_run);
    }
    if matches!(action, Action::Shell) {
        return shell_run(&cfg, tty, &staged, cli.dry_run);
    }
    if matches!(action, Action::Provision) {
        return provision_run(&cfg, tty, &staged, cli.dry_run);
    }

    // Every other action renders its dry run the same way, through
    // `plan`, so no subcommand can describe a run it would not
    // perform -- doctor included. Ordered so the two doctor
    // paths are exclusive: a dry run never builds the probe
    // structs, and a live run never builds their command lines
    // twice.
    if cli.dry_run {
        return execute(&plan(&action, &cfg, tty, &staged), true);
    }
    if matches!(action, Action::Doctor) {
        return Ok(doctor_run(&cfg));
    }
    execute(&plan(&action, &cfg, tty, &staged), false)
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
    // The three sentences live in the library, with the decision
    // they describe, so a test can assert which version each one
    // names. Written here they would sit outside the coverage gate.
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
/// failed. A non-zero `curl` covers DNS failure, a proxy, a 403
/// and a dropped connection alike, so a message naming one of
/// them -- "this release predates checksummed releases", say --
/// would tell an operator on a merely blocked network to abandon
/// verification.
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
    let out = capture_output(cmd)?;
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

/// How much of each stream a refresh command may print before
/// bombyx stops keeping it. Well above the 64 KiB the guest relays
/// of a hook's output, so only a guest that has been changed to
/// print more reaches it.
const RELAY_LIMIT: usize = 1 << 20;

/// [`capture_output`] with each stream kept to [`RELAY_LIMIT`]
/// bytes, for output relayed from the guest rather than parsed.
fn capture_capped(cmd: &RemoteCommand) -> Result<bombyx::run::Capped> {
    let resolver = bombyx::run::Resolver::for_command(cmd)
        .map_err(|e| anyhow!("{}", doctor::not_on_path(e.program())))?;
    Ok(resolver.output_capped(cmd, RELAY_LIMIT)?)
}

/// Runs a command and returns everything it printed, and its exit
/// status, whatever that status is.
///
/// [`capture`] for a caller that reads a failure itself rather
/// than turning it into an error.
fn capture_output(cmd: &RemoteCommand) -> Result<std::process::Output> {
    let resolver = bombyx::run::Resolver::for_command(cmd)
        .map_err(|e| anyhow!("{}", doctor::not_on_path(e.program())))?;
    Ok(resolver.output(cmd)?)
}

/// Returns a string distinguishing this run from any other on
/// this machine.
///
/// `self-update` is the only caller. It renames the running
/// binary aside before writing the new one, and two updates
/// started at the same moment must not choose the same
/// rename-aside name -- see [`bombyx::update::swap`].
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
/// Taking `Cmd` would need an arm for `SelfUpdate`, which reads no
/// config and has no `Action` -- an arm reachable only if some
/// `matches!` elsewhere in the file stopped agreeing with it. The
/// types hold that invariant instead.
fn action_of(cmd: &VmCmd) -> Result<Action> {
    Ok(match cmd {
        VmCmd::Up(_) => Action::Up,
        VmCmd::Provision(_) => Action::Provision,
        VmCmd::Down(_) => Action::Down,
        VmCmd::Shell(_) => Action::Shell,
        VmCmd::Status(_) => Action::Status,
        VmCmd::Reset(_) => Action::Reset,
        VmCmd::Snapshot(_) => Action::Snapshot,
        VmCmd::Doctor(_) => Action::Doctor,
        VmCmd::Destroy { .. } => Action::Destroy,
        VmCmd::Scratch { name, .. } => Action::Scratch(vm_name(name)?),
        VmCmd::Discard { name, .. } => Action::Discard(vm_name(name)?),
    })
}

fn vm_name(raw: &str) -> Result<ScratchName> {
    ScratchName::parse(raw).with_context(|| format!("invalid VM name {raw:?}"))
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

    // `Resolver` looks every program up before any of them runs,
    // and `run`'s module doc says why that order matters. It
    // does not cover `self-update`, which reaches `execute` one
    // command at a time through `ran_ok` and so resolves `tar`
    // only after `curl` has already downloaded the archive.
    let resolver = bombyx::run::Resolver::for_commands(commands)
        .map_err(|e| anyhow!("{}", doctor::not_on_path(e.program())))?;

    for cmd in commands {
        let status = resolver.execute(cmd)?;
        if !status.success() {
            // Without the payload: `Display` would otherwise end
            // the command in a `#` comment, and the status --
            // the only new thing on the line -- would land
            // inside it.
            let shown = cmd.without_payload();
            eprint_lines(&format!("bombyx: {shown} failed: {status}\n"));
            return Ok(Ran::Failed(exit_status_byte(status)));
        }
    }
    Ok(Ran::Ok)
}

/// Runs `up`. On a running VM it only refreshes the secrets; on a
/// VM that exists but is stopped it boots and then refreshes.
///
/// `up` on a running machine would rewrite the generated files,
/// stage the project's secrets and take a mislabeled `fresh-install`
/// snapshot, while `vagrant up` itself does nothing -- it does not
/// re-provision a running machine (issue #89). So `up` asks the host
/// what the machine is doing first. When it is already running, `up`
/// writes the project's secrets over the guest's copies
/// ([`plan::refresh_secrets`]) and stops; with no secrets to send it
/// prints a note and exits 0 without touching anything.
///
/// A boot that does not create the machine usually does not
/// provision it either, so it is followed by the same refresh.
/// [`listing::refreshes_secrets_after_up`] decides when, and says
/// why the refresh is harmless where vagrant did provision.
///
/// The same probe decides the `fresh-install` snapshot. That
/// snapshot names a clean install, so it is taken only when this
/// `up` creates the machine -- an absent one
/// ([`listing::VmState::is_absent`]) or an unconfirmed state that
/// might be a first boot. A machine the probe reports as present but
/// stopped is only being booted, so snapshotting its in-use disk
/// would mislabel it (`snapshot-precondition-on-halt`). The snapshot
/// lives here rather than in `plan` for the same reason the running
/// check does: `plan` cannot see the machine's state.
///
/// The probe is one `vagrant status` round trip, cheap beside a
/// boot. The "is it running" test is
/// [`listing::VmState::is_running`], in the tested library because
/// this file is outside the coverage gate, and the step that pairs
/// each project with its live state ([`listing::entries`]) is the
/// same one `bombyx list` uses -- so `up` and `list` cannot disagree
/// about whether a machine is up.
///
/// A dry run contacts nothing, so it cannot know the state. It
/// prints the probe `up` would run first, then the boot, then the
/// refresh that follows provisioning -- empty unless a
/// `secrets_refreshed` hook is configured -- then the snapshot:
/// the shape of a first `up`, which is the honest description when
/// the state is unknown ahead of time. `bombyx shell --dry-run`
/// prints the refresh an existing guest gets.
fn up_run(
    cfg: &Config,
    tty: Tty,
    staged: &Staged,
    dry_run: bool,
) -> Result<Ran> {
    let boot = plan(&Action::Up, cfg, tty, staged);
    // Built here rather than in `plan`, and appended below only when
    // this `up` is creating the machine. `plan` cannot make that call
    // because it cannot see the machine's state.
    let snapshot =
        remote::save_snapshot_if_absent(cfg, &cfg.remote_project_dir(), tty);
    if dry_run {
        let mut cmds = listing::status_commands(std::slice::from_ref(cfg));
        cmds.extend(boot);
        cmds.extend(plan::refresh_after_provisioning(cfg, staged));
        cmds.push(snapshot);
        return execute(&cmds, true);
    }
    let state = probe_state(cfg);
    let refresh = plan::refresh_secrets(cfg, staged);
    if state.as_ref().is_some_and(listing::VmState::is_running) {
        if refresh.is_empty() {
            eprint_lines(&format!(
                "bombyx: {} is already running; up did nothing\n",
                cfg.project.as_str()
            ));
            return Ok(Ran::Ok);
        }
        eprint_lines(&format!(
            "bombyx: {} is already running; refreshing its secrets\n",
            cfg.project.as_str()
        ));
        return Ok(refreshed(run_refresh(&refresh)));
    }
    // A probe bombyx could not complete -- an unreachable host, a
    // reply that did not parse -- must not block the boot, but it is
    // said out loud: the machine might in fact be running, and then
    // this boot would re-stage its secrets and re-snapshot it, the
    // harm this command exists to prevent. A positive stopped state
    // boots without the note.
    if state.as_ref().is_none_or(listing::VmState::is_unknown) {
        eprint_lines(&format!(
            "bombyx: could not confirm whether {} is running; \
             running up anyway\n",
            cfg.project.as_str()
        ));
    }
    let booted = execute(&boot, false)?;
    if !booted.ok() {
        return Ok(booted);
    }
    // After the boot, because the guest must be up to take the
    // files. A boot that provisioned is followed, when a hook is
    // configured, by the secrets rewrite carrying the hook, without
    // the credential; one that did not provision gets the whole
    // refresh.
    let after = if listing::refreshes_secrets_after_up(state.as_ref()) {
        refresh
    } else {
        plan::refresh_after_provisioning(cfg, staged)
    };
    let refresh_ok = run_refresh(&after);
    // The refresh runs before the snapshot, so a `reset` returns to
    // a guest holding the copy the hook made.
    //
    // The snapshot is taken only when this `up` creates the
    // machine, or cannot tell -- never when the probe reports a
    // present but stopped machine, whose in-use disk the name would
    // mislabel. The policy is `listing::takes_fresh_snapshot`, in the
    // tested library because this branch is otherwise uncovered; see
    // its doc. This guard turns on the machine's state; the
    // `_if_absent` inside the command is a different test -- it
    // skips the save when the `fresh-install` name already exists --
    // so the two do not overlap.
    //
    // It is taken even when the refresh failed: a later `up` finds
    // the machine present and takes no `fresh-install` snapshot, so
    // skipping it here would leave `reset` with nothing to return to.
    if listing::takes_fresh_snapshot(state.as_ref()) {
        let saved = execute(std::slice::from_ref(&snapshot), false)?;
        if !saved.ok() {
            return Ok(saved);
        }
    }
    Ok(refreshed(refresh_ok))
}

/// Runs `provision`, then the project's `secrets_refreshed` hook
/// when one is configured.
///
/// [`plan::refresh_after_provisioning`] holds why the hook follows
/// a provisioning run. A provisioning run that failed keeps its own
/// status and runs no hook, and a hook that failed after a
/// successful run makes `provision` exit non-zero, as it makes `up`
/// do.
fn provision_run(
    cfg: &Config,
    tty: Tty,
    staged: &Staged,
    dry_run: bool,
) -> Result<Ran> {
    let mut cmds = plan(&Action::Provision, cfg, tty, staged);
    let after = plan::refresh_after_provisioning(cfg, staged);
    if dry_run {
        cmds.extend(after);
        return execute(&cmds, true);
    }
    let provisioned = execute(&cmds, false)?;
    if !provisioned.ok() {
        return Ok(provisioned);
    }
    Ok(refreshed(run_refresh(&after)))
}

/// The outcome of an `up` or a `provision` whose refresh
/// ([`run_refresh`]) reported `all_ok`, once every other step
/// succeeded.
///
/// Either fails when a refresh failed: the operator ran it to get
/// the guest current, and a zero exit would say it is. The status
/// is 1 because each failure was already printed with its own.
fn refreshed(all_ok: bool) -> Ran {
    if all_ok { Ran::Ok } else { Ran::Failed(1) }
}

/// Asks the VM host what the project's machine is doing.
///
/// One `vagrant status` round trip, through the same step
/// ([`listing::entries`]) `bombyx list` uses, so `up`, `shell` and
/// `list` cannot disagree about a machine's state. A probe that
/// failed returns `Some(VmState::Unknown(reason))`; `None` only
/// when `entries` returns no row for the project.
fn probe_state(cfg: &Config) -> Option<listing::VmState> {
    listing::entries(vec![cfg.clone()], run_command)
        .into_iter()
        .next()
        .and_then(|entry| entry.state)
}

/// Runs `shell`, but reports and stops if the project has no VM or
/// its VM is not running.
///
/// Without the probe, a project never brought up fails with the VM
/// host's `cd` into the project directory, a stopped VM with
/// vagrant's own message, and bombyx then prints the whole ssh
/// command it ran. [`listing::shell_refusal`] decides, and holds
/// why an unknown state still opens the shell.
///
/// Before the shell opens, the project's secrets are written over
/// the guest's copies ([`plan::refresh_secrets`]), so a token
/// rotated on the workstation reaches the guest with no provision.
///
/// A refresh that fails is a warning and the shell opens anyway,
/// because the operator may be opening the shell to find out why.
///
/// A dry run contacts nothing, so it prints the probe, the
/// refresh and then the shell, as `up_run` does.
fn shell_run(
    cfg: &Config,
    tty: Tty,
    staged: &Staged,
    dry_run: bool,
) -> Result<Ran> {
    let shell = plan(&Action::Shell, cfg, tty, staged);
    let refresh = plan::refresh_secrets(cfg, staged);
    if dry_run {
        let mut cmds = listing::status_commands(std::slice::from_ref(cfg));
        cmds.extend(refresh);
        cmds.extend(shell);
        return execute(&cmds, true);
    }
    if let Some(refusal) =
        listing::shell_refusal(cfg, probe_state(cfg).as_ref())
    {
        eprint_lines(&format!("{refusal}\n"));
        return Ok(Ran::Failed(1));
    }
    if !run_refresh(&refresh) {
        eprint_lines("bombyx: opening the shell anyway\n");
    }
    execute(&shell, false)
}

/// Runs each refresh command `plan` built -- from
/// [`plan::refresh_secrets`] or [`plan::refresh_after_provisioning`]
/// -- and says whether every one succeeded.
///
/// Every command runs whatever happened to the one before, which
/// is what `refresh_secrets` asks of a caller: a secrets file the
/// guest would not take is no reason to leave the credential
/// stale. A failure on this side -- a program that would not start,
/// a pipe that broke -- is reported the same way as a failed exit
/// status rather than returned, so `shell` can still open after
/// either. The caller decides what a failure costs.
///
/// **The output is captured and relayed, never streamed.** The
/// secrets command can carry the project's `secrets_refreshed`
/// hook, which is code from the branch checked out in the guest,
/// and it runs on every `shell`. [`term::relay`] keeps that code
/// from repainting the operator's terminal, the rule `doctor`
/// follows for the same reason. Nothing here is interactive, so
/// capturing costs only the order between the two streams:
/// standard output is printed first, then standard error.
///
/// **The exit status names the part that failed**, which
/// [`remote::RefreshOutcome`] reads. A failed write makes no claim
/// about what the guest now holds: a failure in the guest leaves
/// its copy alone, because the new file is renamed into place only
/// once whole, but a pipe broken on this side can end the guest's
/// `cat` as if the input were complete, and then a short file is
/// the one kept. A failed hook says the secrets are current, which
/// the guest guarantees by running the hook only after the rename.
///
/// **What is kept is bounded**, at [`RELAY_LIMIT`] bytes a stream.
/// The guest already relays at most 64 KiB of a hook's output, but
/// the agent has root in the guest and can change that, so the
/// bound that protects this machine's memory is the one here.
fn run_refresh(commands: &[RemoteCommand]) -> bool {
    let mut all_ok = true;
    for cmd in commands {
        let mut status = None;
        let outcome = match capture_capped(cmd) {
            Ok(capped) => {
                status = Some(capped.output.status);
                let out = &capped.output;
                print_lines(&term::relay(&out.stdout));
                eprint_lines(&term::relay(&out.stderr));
                if capped.truncated {
                    eprint_lines(&format!(
                        "bombyx: the guest printed more than \
                         {RELAY_LIMIT} bytes; the rest was not shown\n"
                    ));
                }
                remote::RefreshOutcome::from_code(out.status.code())
            }
            Err(e) => {
                eprint_lines(&format!("bombyx: {e:#}\n"));
                remote::RefreshOutcome::WriteFailed
            }
        };
        if let Some(message) = outcome.message() {
            all_ok = false;
            eprint_lines(&format!("{message}\n"));
            // The guest's own error, relayed above, names the file
            // it could not write. A failure before the guest ran --
            // `ssh` exits 255 when it cannot connect -- names
            // nothing, and the status is then the only clue.
            if let (remote::RefreshOutcome::WriteFailed, Some(status)) =
                (outcome, status)
            {
                eprint_lines(&format!(
                    "bombyx: that command ended with {status}\n"
                ));
            }
        }
    }
    all_ok
}

/// Runs every precondition probe and prints the report.
///
/// Deliberately thin. Every decision -- the probe list, reading a
/// result, the skip cascade, rendering, the exit code -- lives in
/// `bombyx::doctor`, for the reason its module doc gives. What is
/// left here is assembling the report and asking the local
/// programs for their versions.
fn doctor_run(cfg: &Config) -> Ran {
    let mut report = Report::default();
    // One local program per route, and only the one this run
    // will actually spawn: checking `ssh` where bombyx starts
    // `sh` reports on a program no VM command reaches, and a
    // report that turns red over it says nothing about whether
    // `up` works.
    //
    // `bombyx self-update` also needs `git`, `curl` and `tar`.
    // Those are its problem, for the same reason.
    match cfg.transport() {
        Transport::Ssh => report.add(local_tool("ssh", Some("-V"))),
        Transport::Local => report.add(local_tool("sh", None)),
    }
    report.add_all(doctor::host_findings(cfg, spawn_probe));

    print_lines(&report.render(cfg.host.as_str()));
    if report.ok() { Ran::Ok } else { Ran::Failed(1) }
}

/// Lists the registered projects, and what their VMs are doing.
///
/// Outside `plan`, which every VM subcommand goes through. A
/// plan is built from one `Config` and this command spans all of
/// them, so there is no `Action` shape for it. The property that
/// rule protects is kept another way: the dry run and the live
/// run both take their commands from
/// `listing::status_commands`, so neither can describe a run the
/// other would not perform.
///
/// `--offline` asks no machine anything, so its dry run prints
/// nothing. That is honest rather than empty: there is no
/// command to show.
fn list_run(
    offline: bool,
    registry: Option<&Path>,
    dry_run: bool,
) -> Result<Ran> {
    // The same split the project route makes: a machine whose
    // environment names no config directory is answered here,
    // and `Config::load_all` is handed a path.
    let registry = registry.ok_or_else(Config::no_config_directory_for_all)?;
    let configs = Config::load_all(registry)?;

    if dry_run {
        let commands = if offline {
            Vec::new()
        } else {
            listing::status_commands(&configs)
        };
        return execute(&commands, true);
    }

    // Two routes rather than a flag, so the offline one cannot
    // reach the code that spawns anything. The library owns the
    // join between a project and its state; that is the step
    // that could print `running` against the wrong machine, and
    // this file is outside the coverage gate.
    let entries = if offline {
        listing::offline_entries(configs)
    } else {
        listing::entries(configs, run_command)
    };

    print_lines(&listing::render(&entries));
    // stderr, because a note is a diagnostic rather than a row:
    // it carries the `bombyx:` prefix the rest of this file uses,
    // and it must not land in output somebody pipes into `awk`.
    let notes = listing::notes(&entries);
    for note in &notes {
        eprint_lines(&format!("{note}\n"));
    }

    // A machine that could not be asked leaves the question
    // half-answered, and a script reading the table has no other
    // way to learn that -- the `unknown` cells are prose and the
    // reason is on a stream it may not be reading. `doctor_run`
    // fails its run for the same reason.
    if notes.is_empty() {
        Ok(Ran::Ok)
    } else {
        Ok(Ran::Failed(1))
    }
}

/// Runs one command, turning a spawn failure into a reason.
///
/// `Err` carries what could not be started, so a caller can
/// report it as one row's problem instead of the whole run's.
/// Both callers want that: a diagnostic that refuses to
/// diagnose, and a listing that refuses to list because one
/// machine is asleep, are each worse than an answer with a gap
/// in it.
fn run_command(cmd: &RemoteCommand) -> Result<ProbeResult, String> {
    // Through `run` rather than a child built here, so both
    // callers read every field of a `RemoteCommand`. A second
    // runner built by hand is how `dir` and `stdin` come to be
    // honoured on one path and dropped on the other.
    //
    // `Resolver` also keeps the no-bare-name rule: spawning an
    // unresolved name goes straight back through the OS search
    // that `tool` exists to avoid, and `doctor` is the command
    // run first in a fresh clone, so it is the worst place to
    // reintroduce it.
    let resolver = bombyx::run::Resolver::for_command(cmd)
        .map_err(|e| doctor::not_on_path(e.program()))?;
    resolver
        .output(cmd)
        .map(|o| ProbeResult::from_output(&o))
        .map_err(|e| doctor::cannot_run(&cmd.program, &because(&e)))
}

/// An error and every cause behind it, joined with `: `.
///
/// `Display` on a `thiserror` enum prints that variant's message
/// and stops, so `run::Error`'s `#[source]` never reaches the
/// screen on its own. For a `doctor` row the operating system's
/// own words are the content: "could not start ssh" says nothing
/// a reader can act on, and "could not start ssh: Permission
/// denied (os error 13)" says what to go and fix.
fn because(e: &dyn std::error::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut cause = e.source();
    while let Some(c) = cause {
        parts.push(c.to_string());
        cause = c.source();
    }
    parts.join(": ")
}

/// Runs one host probe and reads its result as a finding.
///
/// Propagating the error instead would discard the whole report
/// -- including findings already gathered -- for the most likely
/// local misconfiguration there is, `ssh` missing from `PATH`.
fn spawn_probe(p: &HostProbe) -> Outcome {
    match run_command(&p.command) {
        Ok(result) => doctor::classify(&result, p.verdict),
        Err(why) => Outcome::Fail(why),
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
/// `version_arg` is per tool, and `None` means there is nothing
/// worth asking. OpenSSH `ssh` answers `-V`. `sh` gets `None`,
/// because `sh` is whatever the system links it to and `dash`
/// has no version flag at all -- asking would report a failure
/// about the question rather than about the shell.
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
