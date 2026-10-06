//! Mapping a user action to the commands that implement it.
//!
//! This is the tool's policy -- which steps run, and in what
//! order -- so it lives in the library where it is covered by
//! tests, not in `src/bin/`.

use crate::config::{Config, Staged};
use crate::doctor;
use crate::name::ScratchName;
use crate::remote::{self, CloneUpdate, RemoteCommand, Tty};
use crate::vagrantfile::{self, GuestHomeFile};

/// What the user asked bombyx to do.
///
/// Separate from the CLI's own subcommand enum so the library
/// does not depend on the argument parser, and so a scratch
/// name is already validated by the time it gets here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write the generated files on the VM host and boot the
    /// project VM.
    ///
    /// The binary follows the boot with [`refresh_secrets`] when
    /// the machine already existed, because vagrant does not
    /// provision it then, and with [`refresh_after_provisioning`]
    /// when this boot created it. `plan` returns the boot alone,
    /// since it cannot see the machine's state.
    Up,
    /// Write the generated files and re-run provisioning in the
    /// guest.
    ///
    /// Separate from [`Action::Up`] because vagrant provisions
    /// a machine only when it first creates it. Every later
    /// `vagrant up` skips the provisioners -- whether the VM
    /// was halted or running -- so the guest stays on the
    /// commit it checked out when it was created, while `up`
    /// reports success. This re-runs `bootstrap.sh`, which
    /// updates the clone the guest already has as the
    /// [`remote::CloneUpdate`] says, then runs the project's
    /// script from it.
    ///
    /// The default mode fetches `source.ref` and checks it out,
    /// and the guest refuses when that would overwrite the
    /// agent's uncommitted edits or untracked files git does not
    /// ignore. It refuses in the same way to delete a clone of
    /// another repository -- left by a change to `source.repo` --
    /// that holds any. Ignored files are not protected.
    /// The checkout detaches HEAD, as a fresh clone does, so a
    /// commit made in the guest ends up on no branch after the
    /// next provision.
    /// `crates/bombyx/templates/bootstrap.sh` decides all of
    /// this and explains how loosely it compares the URLs.
    ///
    /// Requires a machine that already exists: `vagrant
    /// provision` has nothing to provision on a VM that was
    /// never booted, so `up` comes first.
    ///
    /// The binary follows a successful run with
    /// [`refresh_after_provisioning`], which runs the project's
    /// `secrets_refreshed` hook when one is configured.
    Provision(CloneUpdate),
    /// Halt the project VM.
    Down,
    /// Open a shell inside the project VM, in the project clone.
    ///
    /// A plan stops at its first failing command, and a failed
    /// secrets refresh must not stop the shell from opening. So with
    /// [`ShellSecrets::Refresh`] the binary runs [`refresh_secrets`]
    /// itself, before this plan, and warns when the refresh fails;
    /// [`Action::staged_read`] says [`StagedRead::BestEffort`] for
    /// the same reason: `shell --refresh-secrets` reads the files
    /// when it can and opens the shell when it cannot. With
    /// [`ShellSecrets::Leave`] it says [`StagedRead::Skip`], and
    /// nothing is read.
    Shell(ShellSecrets),
    /// Show VM status on the host.
    Status,
    /// Restore the project VM's `fresh-install` snapshot.
    Reset,
    /// Save the project VM's `fresh-install` snapshot, replacing
    /// one that is already there.
    ///
    /// [`Action::Up`] takes the snapshot too, but only when the
    /// machine has none, so the reset cycle works without anyone
    /// running this. The case for running it deliberately is in
    /// `docs/usage.md`.
    Snapshot,
    /// Check bombyx's preconditions without changing anything.
    Doctor,
    /// Destroy the project VM and remove its directory.
    Destroy,
    /// Boot a throwaway VM.
    Scratch(ScratchName),
    /// Destroy a throwaway VM.
    Discard(ScratchName),
}

/// Whether [`Action::Shell`] sends the project's secrets to the
/// guest again before the shell opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellSecrets {
    /// Read no secrets and send none: no vault, no `env_file`, no
    /// deploy key, no `secrets_refreshed` hook. The guest keeps the
    /// copies it has. What a plain `shell` gets, so it asks for no
    /// vault password.
    Leave,
    /// Send them and run the hook, as `up` does on a running
    /// machine: `shell --refresh-secrets`.
    Refresh,
}

impl ShellSecrets {
    /// The choice `shell`'s `--refresh-secrets` flag makes: set, it
    /// refreshes; absent, it leaves the guest's secrets alone. The
    /// one place that decides what a plain `shell` does.
    #[must_use]
    pub fn from_flag(refresh_secrets: bool) -> Self {
        if refresh_secrets {
            Self::Refresh
        } else {
            Self::Leave
        }
    }
}

/// How an [`Action`] treats the secrets `source.env_file` or
/// `source.vault` supplies, and the git credential bombyx builds
/// out of them.
///
/// [`Action::staged_read`] gives the answer for each action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedRead {
    /// The action needs the files. A read that fails stops it
    /// before anything runs.
    Required,
    /// The action uses the files when it can read them. A read
    /// that fails is a warning, and the action runs with nothing
    /// staged.
    BestEffort,
    /// The action never reads the files.
    Skip,
}

impl Action {
    /// How this action treats the secrets `source.env_file` or
    /// `source.vault` supplies, and the git credential bombyx
    /// builds out of them.
    ///
    /// The caller reads that file, and reading it can fail --
    /// the operator rotated it, or moved it, or the config names
    /// a path this machine never had. The file may also lack the
    /// variable that `repo_token` names. So this decides which
    /// actions either failure is allowed to stop: only the
    /// [`StagedRead::Required`] ones.
    ///
    /// A plain `shell` is [`StagedRead::Skip`]: it sends nothing, so
    /// it reads nothing, and a `vault` asks for no master password.
    /// `shell --refresh-secrets` is [`StagedRead::BestEffort`]. It
    /// refreshes the guest's copies before the shell opens, and
    /// `shell` is the command an operator reaches for when something
    /// is already wrong, so a missing secrets file must not also
    /// cost them the shell.
    ///
    /// **The teardown verbs are one reason `Skip` exists.** A
    /// `destroy` refused because the secrets file has gone would
    /// leave the VM and the directory it was asked to remove,
    /// with no bombyx command able to clear either. The deploy
    /// key is read the same way, so the same rule covers it.
    ///
    /// `doctor` is on the same side of the line, and for a
    /// sharper reason: its job is to report what is wrong with a
    /// configuration, so failing before it starts would answer
    /// the question by refusing to look.
    ///
    /// Written as an exhaustive match rather than a `matches!`,
    /// so a new variant is a decision somebody makes here.
    #[must_use]
    pub fn staged_read(&self) -> StagedRead {
        match self {
            Self::Up | Self::Provision(_) | Self::Scratch(_) => {
                StagedRead::Required
            }
            Self::Shell(ShellSecrets::Refresh) => StagedRead::BestEffort,
            Self::Shell(ShellSecrets::Leave)
            | Self::Down
            | Self::Status
            | Self::Reset
            | Self::Snapshot
            | Self::Doctor
            | Self::Destroy
            | Self::Discard(_) => StagedRead::Skip,
        }
    }
}

/// Returns the ordered commands that carry out `action`.
///
/// `tty` is threaded through to every vagrant invocation this
/// builds, rather than being added while spawning, so the printed
/// plan and the executed plan come from one place -- see [`Tty`].
/// `doctor` ignores it: its probes are parsed, and
/// [`doctor::probe_commands`] builds them without a PTY.
///
/// **A dry run prints the argv for *its own* stdio, not for a later
/// live run.** `bombyx status --dry-run | grep ssh` has a piped
/// stdout, so it prints the plan without `-t`, while
/// `bombyx status` in that same terminal will use it. Deliberate --
/// the flag depends on how the process is invoked, and a dry run
/// that claimed otherwise would be guessing about a future
/// invocation -- but it does mean a captured plan is not a script
/// you can paste and expect byte-identical behaviour from.
///
/// **The file writes are the exception, and cannot be
/// otherwise.** Each carries a whole file -- the generated
/// Vagrantfile, the two guest scripts, the project's secrets
/// when the config names an `env_file` or a `vault`, and the git
/// credential when it names a `repo_token` -- and no file is in
/// the command at all: it travels on the command's standard
/// input, which is a pipe and not text a printed line can hold.
/// The line says how many bytes bombyx will send; see
/// [`RemoteCommand::with_stdin`]. That is also what keeps a
/// secret out of a printed plan.
///
/// The git credential is the one exception, and its line gives
/// no count. That file is fixed text plus one token, so a count
/// would measure the token; `write_then` below says so where it
/// chooses the writer.
///
/// `staged` is what the caller read off the workstation: the
/// secrets from the file `source.env_file` names or the vault
/// `source.vault` names, and the git credential built from one
/// variable inside them. Reading and parsing them here would put
/// a file open in the one module whose job is to decide which
/// commands run, and would make every test of that decision need
/// a file on disk.
#[must_use]
pub fn plan(
    action: &Action,
    cfg: &Config,
    tty: Tty,
    staged: &Staged,
) -> Vec<RemoteCommand> {
    match action {
        // The `fresh-install` snapshot is *not* appended here. It is
        // taken only when this `up` creates the machine, or cannot
        // tell -- a clean install is what the name promises -- and
        // `plan` cannot see whether the machine already exists,
        // because it returns the whole list before anything runs. So
        // `up::up_plan` decides from the probed state (issue #89) when
        // the snapshot is taken, and the binary's `up_run` supplies
        // `remote::save_snapshot_if_absent` for that step. `provision`
        // and `scratch` share `write_then` and want no snapshot at
        // all, the other reason it is not in the helper.
        Action::Up => write_then(
            cfg,
            &cfg.remote_project_dir(),
            &["up"],
            None,
            remote::SecretStaging::UnlessProvisioned,
            tty,
            staged,
        ),
        Action::Provision(clone_update) => write_then(
            cfg,
            &cfg.remote_project_dir(),
            &["provision"],
            Some(*clone_update),
            remote::SecretStaging::Always,
            tty,
            staged,
        ),
        Action::Down => vec![remote::vagrant(cfg, &["halt"], tty)],
        Action::Shell(_) => vec![remote::shell_into_vm(cfg)],
        Action::Status => vec![remote::status_or_never_built(cfg, tty)],
        Action::Reset => {
            let dir = cfg.remote_project_dir();
            vec![remote::restore_snapshot(cfg, &dir, tty)]
        }
        Action::Snapshot => {
            let dir = cfg.remote_project_dir();
            vec![remote::save_snapshot(cfg, &dir, tty)]
        }
        // Host probes only. The local checks read this
        // filesystem and spawn a `--version` call, so there is no
        // command line a dry run could print that would describe
        // them honestly.
        Action::Doctor => doctor::probe_commands(&doctor::host_probes(cfg)),
        Action::Destroy => tear_down(cfg, &cfg.remote_project_dir(), tty),
        Action::Scratch(name) => write_then(
            cfg,
            &cfg.remote_scratch_dir(name),
            &["up"],
            None,
            remote::SecretStaging::UnlessProvisioned,
            tty,
            staged,
        ),
        Action::Discard(name) => {
            tear_down(cfg, &cfg.remote_scratch_dir(name), tty)
        }
    }
}

/// Returns the commands that write the staged files over their
/// copies inside the running project VM.
///
/// Provisioning is the only other thing that writes them, and it
/// also re-runs `bootstrap.sh`, which checks `ref` out in the
/// guest's clone and refuses when that would overwrite the agent's
/// work. So `up` and `shell --refresh-secrets` send the files this
/// way instead: nothing but those files changes, and a token or key
/// rotated on the workstation reaches the guest without the
/// operator committing or pushing anything first.
///
/// One command per file, and none for a file `staged` lacks, so a
/// project with no `env_file` pays no round trip. The credential
/// is refreshed alongside the secrets because it is built from
/// one variable inside them, and a rotated repo token would leave
/// `git` pushing with the old one otherwise. The deploy key is
/// refreshed on its own, since an ssh clone may stage no secrets.
///
/// **The commands are independent**, so a caller runs every one
/// and reports each failure, rather than stopping at the first:
/// a secrets file the guest would not take is no reason to leave
/// the credential stale.
///
/// Only those copies are rewritten. A project script that
/// copied the secrets somewhere else during provisioning keeps
/// that copy, unless the project names a `secrets_refreshed` hook
/// to make it again, and a process that read them keeps its
/// values until it restarts; `docs/usage.md` says so to the
/// operator.
///
/// **The hook rides on the secrets command**, in the same guest
/// command as the write, so it costs no round trip of its own and
/// runs only once the write has succeeded;
/// [`remote::refresh_secrets_then_hook`] holds how. The credential
/// and the key go first for the hook's sake: a hook that runs
/// `git` then finds the token or key the operator just rotated.
#[must_use]
pub fn refresh_secrets(cfg: &Config, staged: &Staged) -> Vec<RemoteCommand> {
    let mut cmds = Vec::new();
    if let Some(credential) = staged.credential() {
        cmds.push(remote::refresh_in_guest(
            cfg,
            GuestHomeFile::Credential,
            credential.as_bytes(),
        ));
    }
    if let Some(key) = staged.deploy_key() {
        cmds.push(remote::refresh_in_guest(
            cfg,
            GuestHomeFile::DeployKey,
            key.as_bytes(),
        ));
    }
    cmds.extend(secrets_command(cfg, staged));
    cmds
}

/// The command that rewrites the secrets file, carrying the
/// `secrets_refreshed` hook when one is configured, or `None` when
/// nothing was staged.
///
/// Shared by [`refresh_secrets`] and [`refresh_after_provisioning`],
/// which differ only in whether the credential and the key go too.
fn secrets_command(cfg: &Config, staged: &Staged) -> Option<RemoteCommand> {
    let secrets = staged.secrets()?;
    Some(match &cfg.hooks.secrets_refreshed {
        Some(hook) => remote::refresh_secrets_then_hook(cfg, secrets, hook),
        None => remote::refresh_in_guest(
            cfg,
            GuestHomeFile::Secrets,
            secrets.as_bytes(),
        ),
    })
}

/// Returns the refresh that follows a provisioning run: the `up`
/// that creates the machine, and `provision`.
///
/// The project's `secrets_refreshed` hook is the one place it
/// copies its secrets, so it runs after provisioning as well as
/// after a rewrite in an existing guest. It rides on the secrets
/// command, so this is that one command when a hook is configured,
/// and nothing otherwise.
///
/// Provisioning has just written every file, so rewriting the
/// secrets here serves only to carry the hook. The credential and
/// the key have no hook, so rewriting them would cost a `vagrant
/// ssh` each and change nothing; they are left out.
///
/// The hook runs *after* the project's own provisioning script, so
/// that script cannot rely on the copy the hook makes; one that
/// needs a secret during its run reads `BOMBYX_ENV_FILE`, which
/// provisioning exports. Running it before the script needs the
/// hook inside `bootstrap.sh`, which is left to issue #12
/// (`provision-lifecycle-hooks`).
#[must_use]
pub fn refresh_after_provisioning(
    cfg: &Config,
    staged: &Staged,
) -> Vec<RemoteCommand> {
    if cfg.hooks.secrets_refreshed.is_some() {
        secrets_command(cfg, staged).into_iter().collect()
    } else {
        Vec::new()
    }
}

/// Destroys the VM defined in `dir`, then removes `dir`.
///
/// Shared by `destroy` and `discard`, which differ only in
/// which directory they target. The steps cannot be swapped:
/// `vagrant` runs *inside* the directory, so removing it first
/// would leave nothing to run in.
///
/// The destroy step tolerates a directory with no Vagrantfile,
/// which is reachable without any unusual input -- an
/// `up` interrupted between the `mkdir` and the Vagrantfile
/// write leaves the directory created but empty. A bare
/// `vagrant destroy -f` fails there, and since `execute` stops
/// at the first failure the removal would never run, leaving a
/// directory no bombyx command could clear. Skipping the
/// destroy instead makes teardown re-runnable.
///
/// The destroy step is also a gate. It refuses, and so keeps the
/// directory, when a machine is still recorded after it, because
/// removing the Vagrantfile then would leave that machine running
/// with nothing to point `vagrant` at.
/// `remote::destroy_vm_if_present` holds why.
fn tear_down(cfg: &Config, dir: &str, tty: Tty) -> Vec<RemoteCommand> {
    vec![
        remote::destroy_vm_if_present(cfg, dir, tty),
        remote::remove_dir(cfg, dir),
    ]
}

/// Ensures `dir` exists on the host, writes the generated files
/// into it, then runs `vagrant` with `args` there.
///
/// Shared by `up`, `scratch` and `provision`, which differ only
/// in the directory they target and the vagrant arguments they
/// end with. Routing all three through one helper is what stops
/// them drifting: `vagrant` needs the Vagrantfile bombyx
/// generates, so every caller has to write it before booting.
///
/// `clone_update` is `Some` for `provision` alone. `up` and
/// `scratch` leave the mode to the guest's fallback;
/// [`remote::CloneUpdate`] says why that is safe for them.
///
/// `args` is a slice rather than one string, matching
/// [`remote::vagrant_in`]. A single string would turn a
/// two-word invocation into one quoted argument, which fails on
/// the host after the directory has already been created.
///
/// The three verbs sharing this helper are the three that boot
/// or provision, and so the three that stage the deploy key. The
/// teardown verbs go through [`tear_down`] and stage nothing.
///
/// `staging` says when the secrets are written. `provision` passes
/// `Always`, because it always provisions. `up` and `scratch` pass
/// `UnlessProvisioned`, so their plan holds each secret's write but
/// the VM host's shell skips it on a machine vagrant has already
/// provisioned, where nothing would read it.
fn write_then(
    cfg: &Config,
    dir: &str,
    args: &[&str],
    clone_update: Option<CloneUpdate>,
    staging: remote::SecretStaging,
    tty: Tty,
    staged: &Staged,
) -> Vec<RemoteCommand> {
    let mut cmds = vec![remote::ensure_dir(cfg, dir)];
    for (name, contents) in vagrantfile::files(cfg, staged) {
        cmds.push(remote::write_file(cfg, dir, name, contents.as_bytes()));
    }

    // The three secret-carrying files are written after the
    // generated ones, so the window in which the VM host holds
    // them is the `vagrant` run and nothing more. A guarded write
    // tests vagrant's provision marker, and the marker's presence can
    // be trusted only after vagrant has loaded the machine, because
    // the load deletes a marker whose machine no longer exists;
    // `remote::load_machine` says more. A project that stages nothing
    // has nothing to guard, so it does not pay for the load.
    let stages_any = staged.secrets().is_some()
        || staged.credential().is_some()
        || staged.deploy_key().is_some();
    if staging == remote::SecretStaging::UnlessProvisioned && stages_any {
        cmds.push(remote::load_machine(cfg, dir));
    }
    if let Some(secrets) = staged.secrets() {
        cmds.push(remote::write_secret(
            cfg,
            dir,
            vagrantfile::ENV_FILE_NAME,
            secrets.as_bytes(),
            staging,
        ));
    }
    // `write_secret_of_hidden_size`, not `write_secret`, and the
    // difference is only in what a dry run prints. This file is
    // `https://` plus the username, the host and two
    // separators, all of which the reader already has -- so a
    // byte count would measure the token. `remote::Stdin` holds
    // the rule.
    if let Some(credential) = staged.credential() {
        cmds.push(remote::write_secret_of_hidden_size(
            cfg,
            dir,
            vagrantfile::CREDENTIAL_FILE_NAME,
            credential.as_bytes(),
            staging,
        ));
    }
    if let Some(key) = staged.deploy_key() {
        cmds.push(remote::write_secret(
            cfg,
            dir,
            vagrantfile::DEPLOY_KEY_FILE_NAME,
            key.as_bytes(),
            staging,
        ));
    }

    // The removal runs whether one was staged or not, and that
    // is not tidiness. A run interrupted after the vagrant step
    // began leaves the file on the VM host; take `env_file` out
    // of the config afterwards and the write above stops
    // happening, so a removal conditional on it would never
    // collect what the earlier run left. `bootstrap.sh` does the
    // matching cleanup inside the guest, and this is its
    // sibling. `rm -f` on a file that was never there costs
    // nothing.
    //
    // `remote::vagrant_in_then_remove` holds why the removal is
    // inside the vagrant step rather than after it.
    cmds.push(remote::vagrant_in_then_remove(
        cfg,
        dir,
        args,
        clone_update,
        tty,
        &[
            vagrantfile::ENV_FILE_NAME,
            vagrantfile::CREDENTIAL_FILE_NAME,
            vagrantfile::DEPLOY_KEY_FILE_NAME,
        ],
    ));
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeployKey, DeployKeyPath, Provider};

    fn cfg() -> Config {
        Config::for_tests()
    }

    /// How many commands in a plan carry `-t`.
    fn dash_t_count(cmds: &[RemoteCommand]) -> usize {
        cmds.iter()
            .filter(|c| c.args.iter().any(|a| a == "-t"))
            .count()
    }

    fn plan_for(action: &Action, tty: Tty) -> Vec<RemoteCommand> {
        plan(action, &cfg(), tty, &Staged::default())
    }

    fn local_cfg() -> Config {
        Config::for_tests_local()
    }

    #[test]
    fn every_action_carries_the_tty_choice_it_should() {
        // Classifying every action is what makes a new one a
        // decision rather than an omission. A test that checked
        // only the actions it remembered would leave `destroy`
        // and `discard` silently un-threaded.
        for action in all_actions() {
            // The rule is per command: a step gets a terminal
            // when it runs vagrant, because that is the step with
            // output to render. The `mkdir`, the file writes and
            // the `rm -rf` have none worth one.
            //
            // Doctor is the exemption. Its probes are parsed, and
            // a PTY would fold control characters into the text
            // being compared.
            let allocate = plan_for(&action, Tty::Allocate);
            for c in &allocate {
                let runs_vagrant =
                    c.args[c.args.len() - 1].contains(" vagrant '");
                let want = runs_vagrant && action != Action::Doctor;
                assert_eq!(
                    c.args.iter().any(|a| a == "-t"),
                    want,
                    "{action:?} under Allocate: {:?}",
                    c.args
                );
            }

            // The per-command rule above is satisfied by a plan
            // with no vagrant step in it at all, so it cannot
            // notice one that lost its boot. Counting the
            // vagrant steps is what does.
            //
            // `doctor` is excluded because its probes spell the
            // program differently -- `command -v 'vagrant'` and
            // `vagrant plugin list` -- so none of them matches
            // the test above.
            if action != Action::Doctor {
                assert!(
                    allocate.iter().any(|c| {
                        c.args[c.args.len() - 1].contains(" vagrant '")
                    }),
                    "{action:?} runs vagrant nowhere"
                );
            }

            // Under NoPty only `shell` keeps its `-t`, because it
            // asks for one regardless of the local stdio.
            let without = usize::from(matches!(action, Action::Shell(_)));
            assert_eq!(
                dash_t_count(&plan_for(&action, Tty::NoPty)),
                without,
                "{action:?} under NoPty"
            );
        }
    }

    #[test]
    fn a_plan_runs_one_program_and_only_ssh_is_handed_dash_t() {
        // `-t` is an `ssh` option, and the tty tests above assert
        // where it appears. This is the premise those rest on:
        // no plan contains a program that could be handed `-t`
        // meaning something else. `-t` is a `tar` option and is
        // not an `scp` option at all.
        //
        // bombyx has two routes and each uses one program, so
        // this states both: `ssh`, which takes `-t`, and `sh`,
        // which is never given one because a local shell already
        // has whatever terminal bombyx was started with. A third
        // program appearing on either route is a step whose
        // relationship to `-t` nobody has decided yet.
        for action in all_actions() {
            for c in &plan_for(&action, Tty::Allocate) {
                assert_eq!(c.program, "ssh", "{action:?} over ssh");
            }
            let here =
                plan(&action, &local_cfg(), Tty::Allocate, &Staged::default());
            for c in &here {
                assert_eq!(c.program, "sh", "{action:?} here");
                assert!(
                    !c.args.iter().any(|a| a == "-t"),
                    "{action:?} here: {:?}",
                    c.args
                );
            }
        }
    }

    /// The identity prefix `remote` puts on every vagrant script,
    /// pinned in full by `remote`'s own tests.
    ///
    /// Built from the exported constants so a rename cannot leave
    /// this module green while bombyx sets a different variable.
    fn vm_env() -> String {
        format!(
            "{}='vmhost' {}=$(hostname -s)",
            remote::VM_HOST_ENV,
            remote::VM_HOSTNAME_ENV
        )
    }

    /// Whether [`plan`] writes the Vagrantfile and the bootstrap
    /// script before `action`'s own commands.
    ///
    /// `plan` branches per action and reads no such flag, so
    /// this is the second statement of that policy and
    /// `writes_files_agrees_with_the_commands_built` is what
    /// holds the two together. The match is exhaustive, so a new
    /// variant fails to compile here rather than joining the
    /// non-writing set unnoticed.
    fn writes_files(action: &Action) -> bool {
        match action {
            Action::Up | Action::Provision(_) | Action::Scratch(_) => true,
            Action::Down
            | Action::Shell(_)
            | Action::Status
            | Action::Reset
            | Action::Snapshot
            | Action::Doctor
            | Action::Destroy
            | Action::Discard(_) => false,
        }
    }

    /// Every action, for the tests that must cover all of them.
    ///
    /// Listed here once. A new variant is a compile error in the
    /// `match` below rather than a case silently missed by every
    /// test in the module -- which is what a hand-written list in
    /// each test would have allowed.
    fn all_actions() -> Vec<Action> {
        let variants = [
            Action::Up,
            Action::Provision(CloneUpdate::Checkout),
            Action::Down,
            Action::Shell(ShellSecrets::Leave),
            Action::Shell(ShellSecrets::Refresh),
            Action::Status,
            Action::Reset,
            Action::Snapshot,
            Action::Doctor,
            Action::Destroy,
            Action::Scratch(scratch("pr-1")),
            Action::Discard(scratch("pr-1")),
        ];
        // Exhaustiveness check: adding a variant fails to compile
        // here, which is the point of writing it out.
        for action in &variants {
            match action {
                Action::Up
                | Action::Provision(_)
                | Action::Down
                | Action::Shell(_)
                | Action::Status
                | Action::Reset
                | Action::Snapshot
                | Action::Doctor
                | Action::Destroy
                | Action::Scratch(_)
                | Action::Discard(_) => {}
            }
        }
        variants.to_vec()
    }

    fn run(action: &Action) -> Vec<RemoteCommand> {
        plan(action, &cfg(), Tty::NoPty, &Staged::default())
    }

    /// Each command of `action`'s plan as `--dry-run` prints
    /// it, without the `unset` prefix.
    fn scripts(action: &Action) -> Vec<String> {
        run(action)
            .iter()
            .map(remote::rendered_without_disarm)
            .collect()
    }

    /// The script of one command, without the `unset` prefix.
    ///
    /// `remote` does the stripping, so this module and
    /// `remote::tests` cannot disagree about how strict it is.
    /// `remote` owns the prefix and asserts it on every builder
    /// and every probe; repeating it in the pins here would put
    /// a hundred characters of `unset` in front of every
    /// expected string and hide the command order these tests
    /// are about.
    fn script(c: &RemoteCommand) -> String {
        remote::script_without_disarm(c)
    }

    /// Like [`scripts`], with the file writes' payloads cleared
    /// before rendering.
    ///
    /// A write carries a whole generated file, and
    /// [`Display`](std::fmt::Display) ends such a command with the
    /// payload's size in bytes. Pinning that number here would
    /// fail whenever a comment in either guest script was
    /// reworded, in a test about command order. The contents are
    /// pinned where they belong: `vagrantfile::tests` for what is
    /// rendered, and `remote::write::tests` for what reaches the
    /// pipe. What these tests own is the shell shape and the order.
    ///
    /// [`RemoteCommand::without_payload`] beats cutting the
    /// rendered string, which would need a parser for the note
    /// it is removing.
    fn scripts_without_payloads(action: &Action) -> Vec<String> {
        run(action)
            .iter()
            .map(|c| remote::rendered_without_disarm(&c.without_payload()))
            .collect()
    }

    fn scratch(name: &str) -> ScratchName {
        ScratchName::parse(name).unwrap()
    }

    // A dumb pin, on purpose: it reads as the exact shell
    // bombyx emits. `provision_writes_the_files_then_reprovisions`
    // spells out almost the same script, and the duplication is
    // the point -- two expectations written independently cannot
    // drift the same wrong way, which one shared builder can.
    #[test]
    fn up_makes_the_dir_writes_the_files_then_boots() {
        // Order is the point. `vagrant up` reads the Vagrantfile
        // from the directory it runs in, so every generated file
        // has to be written before the boot, into a directory
        // that already exists.
        //
        // The `fresh-install` snapshot is not part of this plan:
        // `up::up_plan` decides whether `up` takes it, and the
        // binary's `up_run` supplies its command. So `plan(Up)` is the
        // five boot steps, and the length assertion catches one going
        // missing.
        let s = scripts_without_payloads(&Action::Up);
        assert_eq!(s.len(), 5, "up lost or gained a step: {s:?}");
        assert_eq!(
            s[..5],
            vec![
                "ssh vmhost \"mkdir -p ~/'vms/myproject'\"",
                "ssh vmhost \"umask 077; \
                 cat > ~/'vms/myproject/Vagrantfile' && \
                 chmod 600 ~/'vms/myproject/Vagrantfile'\"",
                "ssh vmhost \"umask 077; \
                 cat > ~/'vms/myproject/bootstrap.sh' && \
                 chmod 600 ~/'vms/myproject/bootstrap.sh'\"",
                "ssh vmhost \"umask 077; \
                 cat > ~/'vms/myproject/account.sh' && \
                 chmod 600 ~/'vms/myproject/account.sh'\"",
                "ssh vmhost \"cd ~/'vms/myproject' && \
                 BOMBYX_VM_HOST='vmhost' \
                 BOMBYX_VM_HOSTNAME=\\$(hostname -s) \
                 VAGRANT_DEFAULT_PROVIDER='libvirt' \
                 vagrant 'up'; rc=\\$?; \
                 rm -f ~/'vms/myproject/bombyx.env' || \
                 { printf 'bombyx: could not remove %s from the VM \
                 host; it may hold secrets for this project\\\\n' \
                 ~/'vms/myproject/bombyx.env' >&2; \
                 [ \\\"\\$rc\\\" = 0 ] && rc=1; }; \
                 rm -f ~/'vms/myproject/bombyx.git-credentials' || \
                 { printf 'bombyx: could not remove %s from the VM \
                 host; it may hold secrets for this project\\\\n' \
                 ~/'vms/myproject/bombyx.git-credentials' >&2; \
                 [ \\\"\\$rc\\\" = 0 ] && rc=1; }; \
                 rm -f ~/'vms/myproject/bombyx.deploy-key' || \
                 { printf 'bombyx: could not remove %s from the VM \
                 host; it may hold secrets for this project\\\\n' \
                 ~/'vms/myproject/bombyx.deploy-key' >&2; \
                 [ \\\"\\$rc\\\" = 0 ] && rc=1; }; exit \\$rc\"",
            ]
        );
    }

    /// Index of the one script containing `needle`.
    fn only_at(scripts: &[String], needle: &str) -> usize {
        let hits: Vec<usize> = scripts
            .iter()
            .enumerate()
            .filter(|(_, s)| s.contains(needle))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(hits.len(), 1, "{needle} appears {} times", hits.len());
        hits[0]
    }

    #[test]
    fn both_generated_files_are_written_before_booting() {
        // Order is the whole point. `vagrant` reads the
        // Vagrantfile when it starts, so a file written after the
        // boot command would never be read, and the boot would
        // fail on a directory holding no Vagrantfile at all.
        //
        // The directory has to exist first as well, which is what
        // pins `mkdir` at index 0. The needle carries the
        // quote the remote path is wrapped in, so it matches
        // the command bombyx builds and not a `mkdir` that
        // happens to appear inside a file being written by one
        // of these very commands.
        //
        // The boot is found by its own vagrant verb rather than by
        // position, so the one loop covers all three actions
        // whatever else their plans carry.
        for (action, verb) in [
            (Action::Up, "vagrant 'up'"),
            (
                Action::Provision(CloneUpdate::Checkout),
                "vagrant 'provision'",
            ),
            (Action::Scratch(scratch("pr-1234")), "vagrant 'up'"),
        ] {
            let s = scripts(&action);
            let vagrantfile = only_at(&s, "/Vagrantfile'");
            let bootstrap = only_at(&s, "/bootstrap.sh'");
            let boot = only_at(&s, verb);
            assert_eq!(only_at(&s, "mkdir -p ~/'"), 0, "{action:?}");
            assert!(
                vagrantfile < boot,
                "{action:?}: Vagrantfile written out of order"
            );
            assert!(
                bootstrap < boot,
                "{action:?}: bootstrap written out of order"
            );
        }
    }

    #[test]
    fn only_provision_tells_the_guest_how_to_update_the_clone() {
        // `provision` names its mode, the default included, so a
        // dry run shows which one the guest will act on. `up` and
        // `scratch` provision only a machine they create, which
        // has no clone, so naming a mode there would be noise.
        let named = |action: &Action| {
            scripts(action)
                .iter()
                .find(|s| s.contains(" vagrant '"))
                .map(|s| s.contains(remote::CLONE_UPDATE_ENV))
                .unwrap()
        };
        for mode in CloneUpdate::ALL {
            let boot = scripts(&Action::Provision(mode))
                .into_iter()
                .find(|s| s.contains("vagrant 'provision'"))
                .unwrap();
            let want = format!(
                "{}='{}' vagrant 'provision'",
                remote::CLONE_UPDATE_ENV,
                mode.as_str()
            );
            assert!(boot.contains(&want), "{mode:?}: {boot}");
        }
        assert!(!named(&Action::Up));
        assert!(!named(&Action::Scratch(scratch("pr-1234"))));
    }

    /// The staged key's name in the project directory.
    const KEY_FILE: &str = vagrantfile::DEPLOY_KEY_FILE_NAME;

    /// [`cfg`] carrying a `deploy_key`, and what it stages.
    fn cfg_with_key() -> (Config, Staged) {
        let mut cfg = cfg();
        cfg.source.deploy_key = Some(
            DeployKeyPath::parse("~/.secrets/k").expect("a valid fixture path"),
        );
        let staged = cfg.staged_for_tests();
        (cfg, staged)
    }

    #[test]
    fn a_deploy_key_is_staged_owner_only_before_the_vagrant_run() {
        // The key reaches the VM host the way the secrets do: a
        // write into the project directory, at 0600, ahead of the
        // `vagrant` step that uploads it.
        let (cfg, staged) = cfg_with_key();
        for action in [
            Action::Up,
            Action::Provision(CloneUpdate::Checkout),
            Action::Scratch(scratch("pr-1234")),
        ] {
            let cmds = plan(&action, &cfg, Tty::NoPty, &staged);
            let scripts: Vec<String> = cmds.iter().map(script).collect();
            let write = scripts
                .iter()
                .position(|s| s.contains("cat > ") && s.contains(KEY_FILE))
                .unwrap_or_else(|| panic!("{action:?}: the key is not staged"));
            assert!(scripts[write].contains("umask 077"), "{action:?}");
            assert_eq!(write, scripts.len() - 2, "{action:?}: not last write");
            assert_eq!(
                cmds[write].stdin.as_ref().map(remote::Stdin::bytes),
                staged.deploy_key().map(DeployKey::as_bytes),
                "{action:?}: the write does not carry the key"
            );
        }
    }

    #[test]
    fn the_staged_key_is_removed_whether_or_not_one_was_staged() {
        // The same rule as the secrets file: a run interrupted
        // after the write leaves the key on the VM host, and a
        // config that dropped `deploy_key` since would otherwise
        // never collect it.
        let (with_key, staged) = cfg_with_key();
        for (cfg, staged) in
            [(&with_key, &staged), (&cfg(), &Staged::default())]
        {
            let cmds = plan(&Action::Up, cfg, Tty::NoPty, staged);
            let last = script(cmds.last().expect("a vagrant step"));
            assert!(
                last.contains(&format!("rm -f ~/'vms/myproject/{KEY_FILE}'")),
                "{last}"
            );
        }
    }

    #[test]
    fn no_verb_checks_the_vm_host_for_a_key() {
        // The key lives on the workstation, so there is nothing on
        // the VM host to look for. The needle is the shell-quoted
        // field name a host-side check would pass to `printf`.
        let (cfg, staged) = cfg_with_key();
        for action in all_actions() {
            let cmds = plan(&action, &cfg, Tty::NoPty, &staged);
            assert!(
                !cmds.iter().any(|c| script(c).contains("'deploy_key'")),
                "{action:?}: a host-side key check was built"
            );
        }
    }

    #[test]
    fn writes_files_agrees_with_the_commands_built() {
        // A write on `down` or `destroy` would recreate the
        // directory teardown had just removed, and a missing
        // write on `scratch` boots a directory with no
        // Vagrantfile.
        //
        // Both halves are checked from `all_actions()` against
        // `writes_files`, whose match is exhaustive, so a new
        // action reaches this test by existing.
        for action in all_actions() {
            let wrote = scripts(&action).iter().any(|s| s.contains("cat > "));
            assert_eq!(
                wrote,
                writes_files(&action),
                "{action:?}: writes_files disagrees with the commands"
            );
        }
    }

    #[test]
    fn scratch_writes_the_files_before_booting() {
        // Without the writes, `scratch` boots a directory
        // holding no Vagrantfile.
        let cmds = run(&Action::Scratch(scratch("pr-1234")));
        let programs: Vec<&str> =
            cmds.iter().map(|c| c.program.as_str()).collect();
        // Five, and every one of them is `ssh`: a VM action
        // runs no program on the workstation.
        assert_eq!(programs, vec!["ssh"; 5]);
        assert!(script(&cmds[0]).contains("mkdir -p"));
        assert!(script(cmds.last().unwrap()).contains("vagrant 'up';"));
    }

    #[test]
    fn provision_writes_the_files_then_reprovisions() {
        // Pins the literal shell, so the command's whole effect
        // on the host is readable in one place.
        assert_eq!(
            scripts_without_payloads(&Action::Provision(CloneUpdate::Checkout)),
            vec![
                "ssh vmhost \"mkdir -p ~/'vms/myproject'\"",
                "ssh vmhost \"umask 077; \
                 cat > ~/'vms/myproject/Vagrantfile' && \
                 chmod 600 ~/'vms/myproject/Vagrantfile'\"",
                "ssh vmhost \"umask 077; \
                 cat > ~/'vms/myproject/bootstrap.sh' && \
                 chmod 600 ~/'vms/myproject/bootstrap.sh'\"",
                "ssh vmhost \"umask 077; \
                 cat > ~/'vms/myproject/account.sh' && \
                 chmod 600 ~/'vms/myproject/account.sh'\"",
                "ssh vmhost \"cd ~/'vms/myproject' && \
                 BOMBYX_VM_HOST='vmhost' \
                 BOMBYX_VM_HOSTNAME=\\$(hostname -s) \
                 VAGRANT_DEFAULT_PROVIDER='libvirt' \
                 BOMBYX_CLONE_UPDATE='checkout' \
                 vagrant 'provision'; rc=\\$?; \
                 rm -f ~/'vms/myproject/bombyx.env' || \
                 { printf 'bombyx: could not remove %s from the VM \
                 host; it may hold secrets for this project\\\\n' \
                 ~/'vms/myproject/bombyx.env' >&2; \
                 [ \\\"\\$rc\\\" = 0 ] && rc=1; }; \
                 rm -f ~/'vms/myproject/bombyx.git-credentials' || \
                 { printf 'bombyx: could not remove %s from the VM \
                 host; it may hold secrets for this project\\\\n' \
                 ~/'vms/myproject/bombyx.git-credentials' >&2; \
                 [ \\\"\\$rc\\\" = 0 ] && rc=1; }; \
                 rm -f ~/'vms/myproject/bombyx.deploy-key' || \
                 { printf 'bombyx: could not remove %s from the VM \
                 host; it may hold secrets for this project\\\\n' \
                 ~/'vms/myproject/bombyx.deploy-key' >&2; \
                 [ \\\"\\$rc\\\" = 0 ] && rc=1; }; exit \\$rc\"",
            ]
        );
    }

    #[test]
    fn provision_and_up_take_the_same_shape() {
        // The invariant the shared helper exists to keep: the two
        // write the same commands and differ only in the vagrant
        // call that follows them. A `provision` that grew its own
        // file-writing logic could boot against a stale Vagrantfile
        // on the host.
        //
        // `up`'s snapshot is not part of the plan -- `up::up_plan`
        // decides it and the binary supplies its command -- so the two
        // plans are the same length and the comparison runs to the
        // last step.
        let up = run(&Action::Up);
        let pr = run(&Action::Provision(CloneUpdate::Checkout));
        assert_eq!(up.len(), pr.len());
        let writes = up.len() - 1;
        assert_eq!(up[..writes], pr[..writes]);
        // One prefix on both, which
        // `every_other_project_vagrant_call_names_the_configured_provider`
        // states as a rule across every action but the
        // teardown.
        //
        // The removal of the staged secrets file rides on the
        // same step, so the two scripts do not end at the verb.
        // What this test owns is that the two verbs differ and
        // nothing else does, but for the clone mode `provision`
        // names, which
        // `only_provision_tells_the_guest_how_to_update_the_clone`
        // owns. `remote`'s own tests pin the removal's spelling,
        // and
        // `provision_writes_the_files_then_reprovisions` above
        // pins the whole line.
        for (script, verb) in [
            (script(up.last().unwrap()), "vagrant 'up'"),
            (
                script(pr.last().unwrap()),
                "BOMBYX_CLONE_UPDATE='checkout' vagrant 'provision'",
            ),
        ] {
            assert_eq!(
                script.split_once("; rc=$?; ").map(|(head, _)| head),
                Some(
                    format!("cd ~/'vms/myproject' && {} {verb}", vagrant_env())
                        .as_str()
                ),
                "{verb}"
            );
            assert!(
                script.contains("rm -f ~/'vms/myproject/bombyx.env'"),
                "{verb}: the staged file is not collected"
            );
        }
    }

    #[test]
    fn scratch_and_up_take_the_same_shape() {
        // The two lifecycles must not drift apart in how they write
        // and boot. `up`'s snapshot is not in the plan -- `up::up_plan`
        // decides it -- and a scratch VM never gets one anyway (it is
        // discarded, not reset), so the two plans match step for step
        // and both end at the boot.
        let up = run(&Action::Up);
        let sc = run(&Action::Scratch(scratch("x")));
        let names = |cmds: &[RemoteCommand]| -> Vec<String> {
            cmds.iter().map(|c| c.program.clone()).collect()
        };
        assert_eq!(names(&up), names(&sc));
        assert!(
            script(up.last().unwrap()).contains("vagrant 'up';"),
            "up must end at the boot, not a snapshot: {:?}",
            up.last().unwrap().args
        );
    }

    #[test]
    fn scratch_targets_a_project_scoped_dir() {
        let cmds = run(&Action::Scratch(scratch("pr-1234")));
        assert_eq!(
            script(&cmds[0]),
            "mkdir -p ~/'vms/scratch/myproject/pr-1234'"
        );
    }

    #[test]
    fn down_only_halts() {
        let cmds = run(&Action::Down);
        let env = vagrant_env();
        assert_eq!(cmds.len(), 1);
        assert_eq!(
            script(&cmds[0]),
            format!("cd ~/'vms/myproject' && {env} vagrant 'halt'")
        );
    }

    #[test]
    fn status_guards_a_never_built_project() {
        // Status maps to the guarded builder, so a project whose VM
        // was never built is answered rather than `cd`-ed into.
        // `remote` owns the exact script; here we confirm the wiring.
        let cmds = run(&Action::Status);
        let s = script(&cmds[0]);
        assert!(s.contains("if [ -f ~/'vms/myproject/Vagrantfile' ]"), "{s}");
        assert!(s.contains("vagrant 'status'"), "{s}");
        assert!(s.contains("has no VM yet"), "{s}");
    }

    // The snapshot is not part of `plan(Up)`: `up::up_plan` decides
    // whether `up` takes it, and the binary's `up_run` supplies its
    // command. The boot-then-snapshot order is pinned end-to-end by
    // the `up --dry-run` integration test and by `up.rs`'s tests, and
    // the rule -- a machine absent or in an unknown state gets the
    // snapshot -- by `listing`'s
    // `up_snapshots_only_when_creating_or_unsure`.

    #[test]
    fn snapshot_replaces_the_snapshot_without_consulting_the_listing() {
        // The on-demand command exists to re-take, so it must
        // overwrite. Sharing `up`'s guard would make it do
        // nothing on exactly the machine an operator runs it on.
        let cmds = run(&Action::Snapshot);
        assert_eq!(cmds.len(), 1);
        assert_eq!(
            script(&cmds[0]),
            format!(
                "cd ~/'vms/myproject' && {} vagrant 'snapshot' 'save' \
                 '-f' 'fresh-install'",
                vagrant_env()
            )
        );
    }

    #[test]
    fn reset_restores_the_name_snapshot_writes() {
        // The pairing this action set exists for: `reset` returns to
        // the name `snapshot` writes. Asserted across the plans rather
        // than inside `remote`, because `plan` chooses which builder
        // each action gets and could hand `reset` a different one.
        // `up` writes that same snapshot too, but `up::up_plan`
        // decides that step, not `plan`.
        let restored = script(&run(&Action::Reset)[0]);
        assert!(restored.contains("'fresh-install'"), "{restored}");
        let saved = scripts(&Action::Snapshot);
        let save = saved.last().unwrap();
        assert!(save.contains("vagrant 'snapshot' 'save'"), "{save}");
        assert!(save.contains("'fresh-install'"), "{save}");
    }

    #[test]
    fn reset_restores_the_fresh_install_snapshot() {
        let cmds = run(&Action::Reset);
        let env = vagrant_env();
        assert_eq!(
            script(&cmds[0]),
            format!(
                "cd ~/'vms/myproject' && {env} vagrant 'snapshot' \
                 'restore' 'fresh-install'"
            )
        );
    }

    #[test]
    fn the_refresh_flag_alone_decides_the_shell_secrets() {
        // A plain `shell` leaves the guest's secrets alone, so it
        // opens no vault; only `--refresh-secrets` sends them (#180).
        assert_eq!(ShellSecrets::from_flag(false), ShellSecrets::Leave);
        assert_eq!(ShellSecrets::from_flag(true), ShellSecrets::Refresh);
    }

    #[test]
    fn shell_forces_a_tty() {
        let cmds = run(&Action::Shell(ShellSecrets::Leave));
        assert_eq!(cmds[0].args[0], "-t");
    }

    #[test]
    fn discard_destroys_the_vm_then_removes_the_dir() {
        // Order is the assertion. `vagrant` runs *inside* the
        // directory, so removing it first would leave nothing
        // to run in.
        //
        // `remote`'s own tests pin the destroy's spelling, so
        // this one asserts only where it runs and what it runs.
        let cmds = run(&Action::Discard(scratch("pr-1234")));
        assert_eq!(cmds.len(), 2);
        let destroy = script(&cmds[0]);
        assert!(
            destroy.starts_with("cd ~/'vms/scratch/myproject/pr-1234' && ")
                && destroy.contains("vagrant 'destroy' '-f'"),
            "{destroy}"
        );
        assert_eq!(
            script(&cmds[1]),
            "rm -rf ~/'vms/scratch/myproject/pr-1234'"
        );
    }

    #[test]
    fn destroy_destroys_the_vm_then_removes_the_dir() {
        // The spelling is pinned in `remote`, as in the `discard`
        // test above.
        let cmds = run(&Action::Destroy);
        assert_eq!(cmds.len(), 2);
        let destroy = script(&cmds[0]);
        assert!(
            destroy.starts_with("cd ~/'vms/myproject' && ")
                && destroy.contains("vagrant 'destroy' '-f'"),
            "{destroy}"
        );
        assert_eq!(script(&cmds[1]), "rm -rf ~/'vms/myproject'");
    }

    #[test]
    fn destroy_and_discard_take_the_same_shape() {
        // Compare step *kinds*, not program names: both plans
        // are two ssh calls, so comparing programs would pass
        // through exactly the drift this guards against.
        let kinds = |cmds: &[RemoteCommand]| -> Vec<&'static str> {
            cmds.iter()
                .map(|c| {
                    if script(c).contains("vagrant 'destroy'") {
                        "destroy"
                    } else if script(c).starts_with("rm -rf") {
                        "remove"
                    } else {
                        "other"
                    }
                })
                .collect()
        };
        assert_eq!(kinds(&run(&Action::Destroy)), vec!["destroy", "remove"]);
        assert_eq!(
            kinds(&run(&Action::Discard(scratch("x")))),
            vec!["destroy", "remove"]
        );
    }

    #[test]
    fn doctor_delegates_rather_than_listing_probes_itself() {
        // All this arm may do is delegate. Open-coding a list
        // here is what would let `--dry-run` advertise a probe
        // the live runner does not send. The CLI-level test
        // asserts the binary's own output against the same
        // function, which is the half that constrains the
        // binary rather than the library.
        assert_eq!(
            run(&Action::Doctor),
            doctor::probe_commands(&doctor::host_probes(&cfg()))
        );
        assert!(
            !run(&Action::Doctor).is_empty(),
            "{:?}",
            run(&Action::Doctor)
        );
    }

    /// The whole prefix on every vagrant call: the identity and
    /// the provider.
    ///
    /// [`vm_env`] is the identity half alone, which is what
    /// `every_project_vagrant_call_carries_the_vm_host_identity`
    /// asserts on its own.
    fn vagrant_env() -> String {
        format!(
            "{} {}='{}'",
            vm_env(),
            remote::PROVIDER_ENV,
            cfg().vm.provider
        )
    }

    /// Every script that runs `vagrant` on a project, paired
    /// with the action whose plan produced it.
    ///
    /// Derived from `all_actions` rather than a hand-written
    /// list of builders, so a new action's script is covered
    /// without editing this list. `doctor` is left out: its probes
    /// inspect the host's vagrant installation rather than a
    /// project's VM.
    ///
    /// The filter matches `" vagrant '"` rather than the bare
    /// word, because the commands that *write* the generated
    /// files mention vagrant too -- one carries
    /// `vagrant/provision.sh` in its payload.
    ///
    /// The list is asserted non-empty, so a caller cannot pass
    /// by matching nothing.
    fn project_vagrant_scripts() -> Vec<(Action, String)> {
        let mut found = vec![];
        for action in all_actions() {
            if action == Action::Doctor {
                continue;
            }
            for cmd in run(&action) {
                // Stripped, so an assertion about a variable
                // being set cannot pass on the `unset` that
                // clears the same name two words earlier.
                let s = script(&cmd);
                if s.contains(" vagrant '") {
                    found.push((action.clone(), s));
                }
            }
        }
        assert!(!found.is_empty(), "no plan runs vagrant at all");
        found
    }

    #[test]
    fn every_teardown_destroys_under_the_provider_it_finds_recorded() {
        // `remote::destroy_vm_if_present` holds the argument.
        // Each destroy that names a provider sits behind a test
        // for the id file vagrant writes under that provider,
        // because on a WSL2 host a destroy naming none is refused
        // while a machine exists (issue #111). Exactly one names
        // none: the last, behind the test for any recorded
        // machine, for a machine bombyx cannot place. Every
        // branch is guarded by an id test and there is no
        // `else`, so with no id recorded no vagrant runs, which
        // keeps a misconfigured project removable.
        //
        // Counted, not just filtered. A loop that skips every
        // script it does not recognise asserts nothing at all
        // once the builder stops emitting the literal it
        // matches on, and `project_vagrant_scripts` guards
        // itself the same way for the same reason.
        let mut teardowns = 0;
        for (action, script) in project_vagrant_scripts() {
            if !script.contains("vagrant 'destroy'") {
                continue;
            }
            teardowns += 1;
            let unnamed = vagrant_calls(&script)
                .iter()
                .filter(|c| !c.contains(&format!("{}='", remote::PROVIDER_ENV)))
                .count();
            assert_eq!(unnamed, 1, "{action:?}: {script}");
            let fallback = format!(
                "elif {}; then {} vagrant 'destroy'",
                remote::ANY_RECORDED_MACHINE,
                vm_env()
            );
            let fallback_at = script.find(&fallback).unwrap_or_else(|| {
                panic!("{action:?} has no fallback destroy: {script}")
            });
            // One contiguous substring per provider, so the id
            // test and the destroy it guards must name the same
            // provider.
            for p in Provider::ALL {
                let paired = format!(
                    "[ -f {id} ]; then {env} {}='{p}' vagrant 'destroy'",
                    remote::PROVIDER_ENV,
                    id = remote::shell_quote(&remote::recorded_machine_id(p)),
                    env = vm_env(),
                );
                // Before the fallback, so a machine bombyx can
                // place is never destroyed with no provider named.
                assert!(
                    script.find(&paired).is_some_and(|i| i < fallback_at),
                    "{action:?} does not destroy a {p} machine as {p} \
                     first: {script}"
                );
            }
        }
        // `destroy` and `discard`, the two actions that tear a
        // machine down.
        assert_eq!(teardowns, 2, "the teardown scripts went missing");
    }

    #[test]
    fn every_other_project_vagrant_call_names_the_configured_provider() {
        // `remote::PROVIDER_ENV` holds the argument. The short
        // version: bombyx clears the operator's exported value
        // before every script, so a verb that does not write
        // the configured one back leaves vagrant choosing for
        // itself -- and on a host whose other provider cannot
        // even answer a probe, that refuses the command.
        //
        // Split on the vagrant invocation rather than on the
        // whole script, because one script can hold two calls
        // -- `save_snapshot_if_absent` already emits a listing
        // and a save in one -- and a whole-script skip would
        // then exempt the call beside a teardown.
        //
        // The teardown is skipped because it names the provider
        // vagrant recorded rather than the configured one;
        // `every_teardown_destroys_under_the_provider_it_finds_recorded`
        // holds that.
        let want = format!("{}='{}'", remote::PROVIDER_ENV, cfg().vm.provider);
        let mut calls = 0;
        for (action, script) in project_vagrant_scripts() {
            for call in vagrant_calls(&script) {
                if call.contains("vagrant 'destroy'") {
                    continue;
                }
                calls += 1;
                assert!(
                    call.contains(&want),
                    "{action:?} runs vagrant without the provider: {call}"
                );
            }
        }
        // Ten calls today, and this is a floor rather than
        // that number: a new action adding one should not have
        // to edit this line, while a builder that stops
        // emitting a call still fails here. The exact figure
        // is not written into the assertion for the reason
        // `config-tests-own-file` in `docs/todo.md` gives about
        // counts in prose.
        assert!(calls >= 7, "only {calls} vagrant calls seen");
    }

    /// Each `vagrant` invocation inside one script.
    ///
    /// A script can hold more than one: `save_snapshot_if_absent`
    /// puts a listing and a save in a single `if`. Each slice
    /// runs from the identity prefix that opens an invocation up
    /// to the next one, so an assertion about a call reads only
    /// that call.
    fn vagrant_calls(script: &str) -> Vec<String> {
        let opener = format!("{}=", remote::VM_HOST_ENV);
        let mut starts: Vec<usize> =
            script.match_indices(&opener).map(|(i, _)| i).collect();
        starts.push(script.len());
        starts
            .windows(2)
            .map(|w| script[w[0]..w[1]].to_owned())
            .collect()
    }

    #[test]
    fn every_project_vagrant_call_carries_the_vm_host_identity() {
        // The guest cannot work out which machine it runs on, so
        // the two names ride in on the commands that cross the
        // boundary. Asserted apart from the provider above
        // because the two answer different readers: the guest
        // reads these through the Vagrantfile, and vagrant reads
        // the provider.
        let env = vm_env();
        for (action, script) in project_vagrant_scripts() {
            assert!(
                script.contains(&env),
                "{action:?} runs vagrant without the identity: {script}"
            );
        }
    }

    #[test]
    fn doctor_probes_stay_outside_the_identity_arrangement() {
        // Asserted rather than left implicit, so the exemption
        // above is a decision on record instead of an oversight
        // someone later "fixes" without knowing why.
        let has_vagrant = run(&Action::Doctor)
            .iter()
            .any(|c| c.args[c.args.len() - 1].contains("vagrant"));
        assert!(has_vagrant, "doctor should probe vagrant at all");
        for cmd in run(&Action::Doctor) {
            let script = &cmd.args[cmd.args.len() - 1];
            assert!(
                !script.contains(remote::VM_HOST_ENV),
                "doctor probe should not carry the identity: {script}"
            );
        }
    }

    /// A project naming an `env_file`, and what a caller stages
    /// for it -- with a token value distinctive enough to
    /// search for.
    ///
    /// Built by writing a real file and going through
    /// `Config::read_staged`, rather than by assembling the
    /// parts here. That is the only supported way to pair them,
    /// and it means these tests exercise the same path a run
    /// takes.
    ///
    /// The config comes back with the `Staged`, because the two
    /// belong together: the Vagrantfile `plan` writes is
    /// rendered from the second, and handing it the first
    /// alongside a `Staged` read for some other project is the
    /// pairing this signature exists to prevent.
    ///
    /// `with_token` decides whether the config also names a
    /// `repo_token`, which is what makes bombyx build the git
    /// credential as well.
    fn staged_project(with_token: bool) -> (Config, Staged) {
        use crate::config::{EnvFilePath, RepoToken, RepoTokenVar, RepoUser};

        let dir = tempfile::tempdir().expect("a temp dir");
        let file = dir.path().join("x.env");
        std::fs::write(&file, "TOKEN=hunter2\n").expect("write");

        let mut cfg = cfg();
        cfg.source.env_file = Some(
            EnvFilePath::parse(&file.display().to_string())
                .expect("a temp path is absolute"),
        );
        if with_token {
            cfg.source.repo_token = Some(RepoToken {
                var: RepoTokenVar::parse("TOKEN").expect("a plain name"),
                user: RepoUser::parse("x-token-auth")
                    .expect("a plain username"),
            });
        }
        let staged = cfg.read_staged(|_| None).expect("the file is there");
        (cfg, staged)
    }

    #[test]
    fn a_boot_stages_secrets_only_when_vagrant_will_provision() {
        // `vagrant up` provisions a machine only while its
        // `action_provision` marker is missing, so on an existing,
        // provisioned machine nothing reads the staged secrets. Under
        // `up` and `scratch` each secret-carrying write therefore
        // checks the marker on the VM host and, when it is there,
        // drains its input without writing. `provision` always
        // provisions, so it writes unconditionally. The generated
        // files are written either way.
        const MARKER: &str =
            ".vagrant/machines/default/libvirt/action_provision";
        let secret_names = [
            vagrantfile::ENV_FILE_NAME,
            vagrantfile::CREDENTIAL_FILE_NAME,
            KEY_FILE,
        ];
        for (cfg, staged) in [staged_project(true), cfg_with_key()] {
            for (action, guarded) in [
                (Action::Up, true),
                (Action::Scratch(scratch("pr-1234")), true),
                (Action::Provision(CloneUpdate::Checkout), false),
            ] {
                let cmds = plan(&action, &cfg, Tty::NoPty, &staged);
                let mut secret_writes = 0;
                for s in cmds.iter().map(script) {
                    let Some(at) = s.find("cat > ") else { continue };
                    let target = &s[at..];
                    let is_secret =
                        secret_names.iter().any(|n| target.contains(n));
                    if is_secret {
                        secret_writes += 1;
                        assert_eq!(
                            s.contains(MARKER) && s.contains("cat > /dev/null"),
                            guarded,
                            "{action:?}: {s}"
                        );
                    } else if s.contains("Vagrantfile") {
                        assert!(!s.contains(MARKER), "{action:?}: {s}");
                    }
                }
                assert!(secret_writes > 0, "{action:?}: no secret written");
            }
        }
    }

    #[test]
    fn a_guarded_boot_lets_vagrant_load_the_machine_before_the_check() {
        // Loading a machine whose provider no longer has it makes
        // vagrant wipe its data directory, marker included. Without
        // that load first, a marker left by a vanished machine would
        // hold back the secrets from the provision vagrant then
        // runs. So the load sits after the Vagrantfile is written and
        // before the first guarded write; `provision`, which stages
        // unconditionally, has no need of it.
        let (cfg, staged) = staged_project(false);
        for (action, loads) in [
            (Action::Up, true),
            (Action::Scratch(scratch("pr-1234")), true),
            (Action::Provision(CloneUpdate::Checkout), false),
        ] {
            let scripts: Vec<String> = plan(&action, &cfg, Tty::NoPty, &staged)
                .iter()
                .map(script)
                .collect();
            let find =
                |needle: &str| scripts.iter().position(|s| s.contains(needle));
            let load = find("vagrant 'status' >/dev/null 2>&1 || true");
            assert_eq!(load.is_some(), loads, "{action:?}: {scripts:#?}");
            if let Some(load) = load {
                let vagrantfile = find("/Vagrantfile'").expect("written");
                let secret = find(vagrantfile::ENV_FILE_NAME).expect("staged");
                assert!(vagrantfile < load && load < secret, "{action:?}");
            }
        }
    }

    #[test]
    fn only_the_verbs_that_boot_need_the_secrets_file() {
        // Classifying every action is what makes a new one a
        // decision rather than an omission.
        //
        // The teardown verbs are the ones that matter. `destroy` must
        // work after the operator has rotated or deleted the secrets
        // file on the workstation, or the VM and its directory become
        // unremovable by bombyx. That is the rule `plan::write_then`
        // already states for the deploy key, aimed at the other
        // machine. `shell --refresh-secrets` reads the file to
        // refresh the guest's copy, and must open all the same.
        for action in all_actions() {
            let want = match action {
                Action::Up | Action::Provision(_) | Action::Scratch(_) => {
                    StagedRead::Required
                }
                Action::Shell(ShellSecrets::Refresh) => StagedRead::BestEffort,
                _ => StagedRead::Skip,
            };
            assert_eq!(action.staged_read(), want, "{action:?}");
        }
    }

    #[test]
    fn a_configured_env_file_is_written_and_then_removed() {
        // Three properties, and the order between them is the
        // whole design: the file is written before vagrant runs,
        // vagrant is what reads it, and the removal is inside
        // that same step rather than after it.
        for action in [
            Action::Up,
            Action::Provision(CloneUpdate::Checkout),
            Action::Scratch(scratch("pr-1234")),
        ] {
            let (cfg, staged) = staged_project(true);
            let cmds = plan(&action, &cfg, Tty::NoPty, &staged);
            let write = cmds
                .iter()
                .position(|c| script(c).contains("bombyx.env"))
                .unwrap_or_else(|| panic!("{action:?}: nothing writes it"));
            let vagrant = cmds
                .iter()
                .position(|c| script(c).contains("rm -f"))
                .unwrap_or_else(|| panic!("{action:?}: nothing removes it"));
            assert!(
                write < vagrant,
                "{action:?}: the write must come before the removal"
            );
            let removing = script(&cmds[vagrant]);
            assert!(
                removing.contains(" vagrant '"),
                "{action:?}: the removal must be in the vagrant step, so a \
                 failed boot still clears the file: {removing}"
            );
            assert!(
                removing.contains("exit $rc"),
                "{action:?}: the vagrant step must hand back its own \
                 status: {removing}"
            );
        }
    }

    #[test]
    fn a_windows_guest_is_refreshed_as_a_linux_guest_is() {
        // The same commands in the same order: the credential first when
        // a token is staged, then the secrets file, each on stdin; and
        // the hook's refresh after provisioning. `remote` holds how each
        // reaches a Windows guest.
        for credential in [false, true] {
            let (mut cfg, staged) = staged_project(credential);
            cfg.vm.guest = crate::config::Guest::Windows;
            let cmds = refresh_secrets(&cfg, &staged);
            assert_eq!(cmds.len(), if credential { 2 } else { 1 }, "{cmds:?}");
            let last = cmds.last().expect("the secrets command");
            let stdin = last.stdin.as_ref().expect("the file is on stdin");
            assert_eq!(stdin.bytes(), b"TOKEN=hunter2\n");
            assert!(
                refresh_after_provisioning(&cfg, &staged).is_empty(),
                "{:?}",
                refresh_after_provisioning(&cfg, &staged)
            );
            cfg.hooks.secrets_refreshed = Some(
                crate::config::HookPath::parse(".bombyx/refresh.ps1")
                    .expect("a valid hook path"),
            );
            assert_eq!(refresh_after_provisioning(&cfg, &staged).len(), 1);
        }
    }

    #[test]
    fn a_refresh_rewrites_the_secrets_file_in_the_agents_home() {
        // One command, carrying the file exactly as the workstation
        // holds it, aimed at the path `account.sh` writes. No token,
        // so no credential and no second command.
        let (cfg, staged) = staged_project(false);
        let cmds = refresh_secrets(&cfg, &staged);
        assert_eq!(cmds.len(), 1, "{cmds:?}");
        let script = script(&cmds[0]);
        assert!(script.contains(GuestHomeFile::Secrets.path()), "{script}");
        let stdin = cmds[0].stdin.as_ref().expect("the file is on stdin");
        assert_eq!(stdin.bytes(), b"TOKEN=hunter2\n");
        assert!(stdin.size_may_be_shown());
    }

    #[test]
    fn a_refresh_rewrites_the_git_credential_too_and_hides_its_size() {
        // A rotated repo token lives in the same file, so refreshing
        // only the file would leave `git` pushing with the old one.
        let (cfg, staged) = staged_project(true);
        let cmds = refresh_secrets(&cfg, &staged);
        assert_eq!(cmds.len(), 2, "{cmds:?}");
        let script = script(&cmds[0]);
        assert!(
            script.contains(GuestHomeFile::Credential.path()),
            "{script}"
        );
        let stdin = cmds[0].stdin.as_ref().expect("the file is on stdin");
        let credential = staged.credential().expect("a token was configured");
        assert_eq!(stdin.bytes(), credential.as_bytes());
        assert!(!stdin.size_may_be_shown());
        for c in &cmds {
            assert!(
                c.args.iter().all(|a| !a.contains("hunter2")),
                "the token reached an argument: {c}"
            );
        }
    }

    #[test]
    fn a_refresh_rewrites_the_deploy_key_on_its_own() {
        // An ssh clone needs the key and may need no secrets, so
        // the key is refreshed whether or not secrets were staged.
        let (cfg, staged) = cfg_with_key();
        let cmds = refresh_secrets(&cfg, &staged);
        assert_eq!(cmds.len(), 1, "{cmds:?}");
        let script = script(&cmds[0]);
        assert!(script.contains(GuestHomeFile::DeployKey.path()), "{script}");
        let stdin = cmds[0].stdin.as_ref().expect("the key is on stdin");
        assert_eq!(
            Some(stdin.bytes()),
            staged.deploy_key().map(DeployKey::as_bytes)
        );
    }

    #[test]
    fn the_deploy_key_is_refreshed_before_the_secrets_and_their_hook() {
        // The hook may run `git`, so it has to find the key the
        // operator just rotated, as it finds the credential.
        let (mut cfg, _) = staged_project_with_hook(false);
        cfg.source.deploy_key = Some(
            DeployKeyPath::parse("~/.secrets/k").expect("a valid fixture path"),
        );
        let staged = cfg.staged_for_tests();
        let cmds = refresh_secrets(&cfg, &staged);
        assert_eq!(cmds.len(), 2, "{cmds:?}");
        assert!(script(&cmds[0]).contains(GuestHomeFile::DeployKey.path()));
        assert!(script(&cmds[1]).contains(GuestHomeFile::Secrets.path()));
    }

    /// [`staged_project`] with a `secrets_refreshed` hook as well.
    fn staged_project_with_hook(with_token: bool) -> (Config, Staged) {
        let (mut cfg, staged) = staged_project(with_token);
        cfg.hooks.secrets_refreshed = Some(
            crate::config::HookPath::parse(".bombyx/refresh-env.sh")
                .expect("a good hook"),
        );
        (cfg, staged)
    }

    #[test]
    fn a_configured_hook_rides_on_the_secrets_command() {
        // One round trip for the write and the hook, so no extra
        // command, and the secrets command is the one carrying it.
        let (cfg, staged) = staged_project_with_hook(false);
        let cmds = refresh_secrets(&cfg, &staged);
        assert_eq!(cmds.len(), 1, "{cmds:?}");
        let script = script(&cmds[0]);
        assert!(script.contains(".bombyx/refresh-env.sh"), "{script}");
        assert!(script.contains(GuestHomeFile::Secrets.path()), "{script}");
        let stdin = cmds[0].stdin.as_ref().expect("the file is on stdin");
        assert_eq!(stdin.bytes(), b"TOKEN=hunter2\n");
    }

    #[test]
    fn the_credential_is_refreshed_before_the_secrets_and_their_hook() {
        // A hook that runs `git` must find the token the operator
        // just rotated, so the credential goes first and the hook
        // runs last. Nothing else in the plan carries the hook.
        let (cfg, staged) = staged_project_with_hook(true);
        let cmds = refresh_secrets(&cfg, &staged);
        assert_eq!(cmds.len(), 2, "{cmds:?}");
        assert!(
            script(&cmds[0]).contains(GuestHomeFile::Credential.path()),
            "{}",
            cmds[0]
        );
        assert!(!script(&cmds[0]).contains("refresh-env.sh"), "{}", cmds[0]);
        assert!(script(&cmds[1]).contains("refresh-env.sh"), "{}", cmds[1]);
    }

    #[test]
    fn provisioning_is_followed_by_the_hook_when_one_is_configured() {
        // The hook is the one place a project copies its secrets,
        // so it follows a provisioning run too, on the secrets
        // command. The credential is not sent again: provisioning
        // has just written it, and a second `vagrant ssh` would buy
        // nothing.
        let (cfg, staged) = staged_project_with_hook(true);
        let cmds = refresh_after_provisioning(&cfg, &staged);
        assert_eq!(cmds.len(), 1, "{cmds:?}");
        let script = script(&cmds[0]);
        assert!(script.contains("refresh-env.sh"), "{script}");
        assert!(
            !script.contains(GuestHomeFile::Credential.path()),
            "{script}"
        );
    }

    #[test]
    fn provisioning_without_a_hook_costs_no_round_trip() {
        // Provisioning has just placed both files, so rewriting them
        // with no hook to follow would repeat `account.sh` for the
        // price of one `vagrant ssh`.
        let (cfg, staged) = staged_project(true);
        assert!(
            refresh_after_provisioning(&cfg, &staged).is_empty(),
            "{:?}",
            refresh_after_provisioning(&cfg, &staged)
        );
    }

    #[test]
    fn a_project_without_a_hook_refreshes_as_before() {
        let (cfg, staged) = staged_project(false);
        let cmds = refresh_secrets(&cfg, &staged);
        assert!(
            !script(&cmds[0]).contains("secrets_refreshed"),
            "{}",
            cmds[0]
        );
    }

    #[test]
    fn nothing_is_refreshed_when_nothing_was_staged() {
        // A project without an `env_file`, or a
        // `shell --refresh-secrets` that could not read it, sends
        // nothing and costs no round trip.
        assert!(
            refresh_secrets(&cfg(), &Staged::default()).is_empty(),
            "{:?}",
            refresh_secrets(&cfg(), &Staged::default())
        );
    }

    #[test]
    fn a_staged_file_is_removed_even_when_none_was_configured() {
        // The leftover case, and it is reachable without
        // anything unusual: a connection dropped after the
        // vagrant step started leaves `bombyx.env` on the VM
        // host, and the operator then takes `env_file` out of
        // the config. Nothing would collect it, while `README.md`
        // and `docs/trust-boundary.md` both say the VM host does
        // not keep the file.
        //
        // `bootstrap.sh` already does the matching cleanup on the
        // guest side. This is its sibling.
        for action in [
            Action::Up,
            Action::Provision(CloneUpdate::Checkout),
            Action::Scratch(scratch("pr-1234")),
        ] {
            let cmds = plan(&action, &cfg(), Tty::NoPty, &Staged::default());
            for name in ["bombyx.env", "bombyx.git-credentials"] {
                assert!(
                    cmds.iter().any(|c| script(c).contains("rm -f")
                        && script(c).contains(name)),
                    "{action:?}: nothing collects a leftover {name}"
                );
            }
        }
    }

    #[test]
    fn no_env_file_stages_nothing() {
        // The removal above is not a write. A project with no
        // `env_file` must still send nothing.
        for action in all_actions() {
            for c in plan(&action, &cfg(), Tty::NoPty, &Staged::default()) {
                assert!(
                    c.stdin.is_none() || !script(&c).contains("bombyx.env"),
                    "{action:?}: a file nobody configured was written"
                );
            }
        }
    }

    #[test]
    fn a_configured_repo_token_stages_a_credential_file() {
        // A second file travelling the same way as the secrets,
        // and it has to be there before vagrant runs: the clone
        // it authenticates is the first thing the guest does
        // with the network.
        for action in [
            Action::Up,
            Action::Provision(CloneUpdate::Checkout),
            Action::Scratch(scratch("pr-1234")),
        ] {
            let (cfg, staged) = staged_project(true);
            let cmds = plan(&action, &cfg, Tty::NoPty, &staged);
            let write = cmds
                .iter()
                .position(|c| script(c).contains("bombyx.git-credentials"))
                .unwrap_or_else(|| panic!("{action:?}: nothing writes it"));
            // The boot is the last vagrant call; `up` and `scratch`
            // run a `vagrant status` before the writes as well.
            let vagrant = cmds
                .iter()
                .rposition(|c| script(c).contains(" vagrant '"))
                .unwrap_or_else(|| panic!("{action:?}: nothing runs vagrant"));
            assert!(
                write < vagrant,
                "{action:?}: the credential must be staged before the boot"
            );
        }
    }

    #[test]
    fn the_credential_write_does_not_print_its_size() {
        // The byte count is harmless for `bombyx.env`, where it
        // is a whole file's size. This file is fixed text plus
        // one token, so a count measures the token -- and
        // `docs/usage.md` invites the operator to paste a dry
        // run into a bug report.
        let (cfg, staged) = staged_project(true);
        let cmds = plan(&Action::Up, &cfg, Tty::NoPty, &staged);
        let cred = cmds
            .iter()
            .find(|c| script(c).contains("bombyx.git-credentials"))
            .expect("the credential must be staged");
        let shown = cred.to_string();
        assert!(
            !shown.contains("bytes on stdin"),
            "a count measures the token: {shown}"
        );
        assert!(
            shown.contains("contents on stdin, not shown"),
            "the reader must still be told a payload is sent: {shown}"
        );

        // The secrets file keeps its count, so this is a
        // decision about one file rather than the render losing
        // the information everywhere.
        let env = cmds
            .iter()
            .find(|c| script(c).contains("bombyx.env"))
            .expect("the secrets file must be staged");
        assert!(
            env.to_string().contains("bytes on stdin"),
            "the secrets file keeps its size: {env}"
        );
    }

    #[test]
    fn an_env_file_without_a_repo_token_stages_no_credential() {
        // The secrets travel and the credential does not. A
        // project cloning a public repository over https, or one
        // cloning over ssh with a deploy key, is in this case.
        for action in all_actions() {
            let (cfg, staged) = staged_project(false);
            for c in plan(&action, &cfg, Tty::NoPty, &staged) {
                assert!(
                    c.stdin.is_none()
                        || !script(&c).contains("bombyx.git-credentials"),
                    "{action:?}: a credential nobody configured was written"
                );
            }
        }
    }

    #[test]
    fn the_secrets_reach_no_command_line_and_no_printed_plan() {
        // The reason the contents travel on a pipe at all. Every
        // account on the VM host can read another's arguments,
        // and a dry run prints the plan to a terminal.
        for action in [
            Action::Up,
            Action::Provision(CloneUpdate::Checkout),
            Action::Scratch(scratch("pr-1234")),
        ] {
            let (cfg, staged) = staged_project(true);
            for c in plan(&action, &cfg, Tty::NoPty, &staged) {
                for arg in &c.args {
                    assert!(!arg.contains("hunter2"), "{action:?}: {arg}");
                }
                assert!(
                    !c.to_string().contains("hunter2"),
                    "{action:?}: a printed plan holds the secret: {c}"
                );
            }
        }
    }

    #[test]
    fn the_teardown_verbs_write_no_secrets_file() {
        // They take the whole directory with `rm -rf`, so the
        // staged file goes with it and there is nothing for
        // these plans to write or remove on their own.
        for action in [Action::Destroy, Action::Discard(scratch("pr-1234"))] {
            let (cfg, staged) = staged_project(true);
            for c in plan(&action, &cfg, Tty::NoPty, &staged) {
                assert!(
                    !script(&c).contains("bombyx.env"),
                    "{action:?}: teardown must not stage anything"
                );
            }
        }
    }
}
