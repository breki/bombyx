//! Mapping a user action to the commands that implement it.
//!
//! This is the tool's policy -- which steps run, and in what
//! order -- so it lives in the library where it is covered by
//! tests, not in `src/bin/`.

use crate::config::Config;
use crate::doctor;
use crate::name::ScratchName;
use crate::remote::{self, RemoteCommand, Tty};
use crate::vagrantfile;

/// What the user asked bombyx to do.
///
/// Separate from the CLI's own subcommand enum so the library
/// does not depend on the argument parser, and so a scratch
/// name is already validated by the time it gets here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write the generated files on the VM host and boot the
    /// project VM.
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
    /// fetches and checks out `source.ref` again in the clone
    /// the guest already has. The checkout is forced, so it
    /// overwrites edits to tracked files and any untracked file
    /// the fetched commit adds at the same path. It also
    /// detaches HEAD, so a commit made in the guest ends up on
    /// no branch after the next provision. Changing
    /// `source.repo` to a different repository removes the
    /// clone outright, which loses everything -- see
    /// `crates/bombyx/templates/bootstrap.sh`, which decides
    /// that and explains how loosely it compares the URLs.
    ///
    /// Requires a machine that already exists: `vagrant
    /// provision` has nothing to provision on a VM that was
    /// never booted, so `up` comes first.
    Provision,
    /// Halt the project VM.
    Down,
    /// Open a shell inside the project VM.
    Shell,
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
/// **A dry run also elides the two file writes' payloads.** Each
/// carries a whole file -- the generated Vagrantfile and the
/// bootstrap script -- and printing both in full buries the plan
/// they belong to. The printed line identifies the heredoc and how
/// many lines it dropped, and what is written to the host is the
/// full content regardless; see
/// [`RemoteCommand::abbreviated`].
#[must_use]
pub fn plan(action: &Action, cfg: &Config, tty: Tty) -> Vec<RemoteCommand> {
    match action {
        // The snapshot save is here rather than inside
        // `write_then` because `provision` and `scratch` share
        // that helper and neither wants one: `provision` runs on
        // a machine already in arbitrary use, and a scratch VM is
        // discarded rather than reset.
        Action::Up => {
            let dir = cfg.remote_project_dir();
            let mut cmds = write_then(cfg, &dir, &["up"], tty);
            cmds.push(remote::save_snapshot_if_absent(cfg, &dir, tty));
            cmds
        }
        Action::Provision => {
            write_then(cfg, &cfg.remote_project_dir(), &["provision"], tty)
        }
        Action::Down => vec![remote::vagrant(cfg, &["halt"], tty)],
        Action::Shell => vec![remote::shell_into_vm(cfg)],
        Action::Status => vec![remote::vagrant(cfg, &["status"], tty)],
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
        Action::Scratch(name) => {
            write_then(cfg, &cfg.remote_scratch_dir(name), &["up"], tty)
        }
        Action::Discard(name) => {
            tear_down(cfg, &cfg.remote_scratch_dir(name), tty)
        }
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
/// `args` is a slice rather than one string, matching
/// [`remote::vagrant_in`]. A single string would turn a
/// two-word invocation into one quoted argument, which fails on
/// the host after the directory has already been created.
fn write_then(
    cfg: &Config,
    dir: &str,
    args: &[&str],
    tty: Tty,
) -> Vec<RemoteCommand> {
    let mut cmds = vec![remote::ensure_dir(cfg, dir)];
    for (name, contents) in vagrantfile::files(cfg) {
        cmds.push(remote::write_file(cfg, dir, name, &contents));
    }
    cmds.push(remote::vagrant_in(cfg, dir, args, tty));
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;

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
        plan(action, &cfg(), tty)
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
            // output to render. The `mkdir`, the two file writes
            // and the `rm -rf` have none worth one.
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
            // notice one that lost its boot. The count the old
            // version of this test asserted is what caught that,
            // and this is that half kept.
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
            let without = usize::from(matches!(action, Action::Shell));
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
            let here = plan(&action, &local_cfg(), Tty::Allocate);
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

    /// Every action, for the tests that must cover all of them.
    ///
    /// Listed here once. A new variant is a compile error in the
    /// `match` below rather than a case silently missed by every
    /// test in the module -- which is what a hand-written list in
    /// each test would have allowed.
    fn all_actions() -> Vec<Action> {
        let variants = [
            Action::Up,
            Action::Provision,
            Action::Down,
            Action::Shell,
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
                | Action::Provision
                | Action::Down
                | Action::Shell
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
        plan(action, &cfg(), Tty::NoPty)
    }

    fn scripts(action: &Action) -> Vec<String> {
        run(action).iter().map(ToString::to_string).collect()
    }

    /// Like [`scripts`], with each entry cut at its first line.
    ///
    /// Only the two file writes span more than one line, and
    /// what they carry is a whole Vagrantfile and a whole shell
    /// script. Pinning those here would put forty lines of
    /// another file's prose inside a test about command order,
    /// and would fail whenever a comment in `bootstrap.sh` was
    /// reworded. Their contents are pinned where they belong:
    /// `vagrantfile::tests` for what is rendered, and
    /// `remote::tests` for the heredoc that carries it. What
    /// this test owns is the shell shape and the order, and the
    /// first line carries both.
    fn scripts_head(action: &Action) -> Vec<String> {
        scripts(action)
            .iter()
            .map(|s| s.lines().next().unwrap_or_default().to_owned())
            .collect()
    }

    fn scratch(name: &str) -> ScratchName {
        ScratchName::parse(name).unwrap()
    }

    // This test and `provision_writes_the_files_then_reprovisions`
    // spell out
    // the same four-command script, differing only in the trailing
    // `vagrant 'up'` versus `vagrant 'provision'`. A review proposed
    // an `expected_script(dir, subcommand)` helper and the duplication
    // is kept deliberately: these blocks are meant to be dumb pins
    // that read as the exact shell bombyx emits, and two
    // independently written expectations cannot both drift the same
    // wrong way, which one shared builder can.
    // `provision_and_up_take_the_same_shape` below carries the
    // "these two differ only in their last step" claim the helper
    // would have made visible. Revisit if a third caller of
    // `write_then` gains its own exact-script test -- three copies
    // change the judgement.
    #[test]
    fn up_makes_the_dir_writes_the_files_then_boots() {
        // Order is the point. `vagrant up` reads the Vagrantfile
        // from the directory it runs in, so both generated files
        // have to be written before the boot, into a directory
        // that already exists.
        //
        // The snapshot save that follows the boot is not spelled
        // out here. Its shell is pinned twice already -- in
        // `remote` for what the builder emits, and by
        // `up_takes_the_snapshot_after_booting` for the fact that
        // `up` ends with that builder -- and a third escaped copy
        // was rewritten by hand three times while this change was
        // being reviewed, wrongly on each of them. The length
        // assertion below is what still catches a step that goes
        // missing.
        let s = scripts_head(&Action::Up);
        assert_eq!(s.len(), 5, "up lost or gained a step: {s:?}");
        assert_eq!(
            s[..4],
            vec![
                "ssh vmhost \"mkdir -p ~/'vms/myproject'\"",
                "ssh vmhost \"cat > ~/'vms/myproject/Vagrantfile' \
                 <<'BOMBYX_EOF'",
                "ssh vmhost \"cat > ~/'vms/myproject/bootstrap.sh' \
                 <<'BOMBYX_EOF'",
                "ssh vmhost \"cd ~/'vms/myproject' && \
                 BOMBYX_VM_HOST='vmhost' \
                 BOMBYX_VM_HOSTNAME=\\$(hostname -s) \
                 VAGRANT_DEFAULT_PROVIDER='libvirt' vagrant 'up'\"",
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
        // pins `mkdir` at index 0.
        //
        // The boot is found by its own vagrant verb rather than
        // taken as the last step, because `up` has one step after
        // it: the snapshot save.
        for (action, verb) in [
            (Action::Up, "vagrant 'up'"),
            (Action::Provision, "vagrant 'provision'"),
            (Action::Scratch(scratch("pr-1234")), "vagrant 'up'"),
        ] {
            let s = scripts(&action);
            let vagrantfile = only_at(&s, "/Vagrantfile'");
            let bootstrap = only_at(&s, "/bootstrap.sh'");
            let boot = only_at(&s, verb);
            assert_eq!(only_at(&s, "mkdir -p"), 0, "{action:?}");
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
    fn every_other_action_writes_nothing() {
        // A write on `down` or `destroy` would recreate the
        // directory teardown had just removed.
        //
        // Derived from `all_actions()` rather than listed, so a
        // new action joins this test by existing. Listing the
        // set by hand is how `shell` and `doctor` came to be
        // absent from both halves of the rule.
        let writes = |a: &Action| {
            matches!(a, Action::Up | Action::Provision | Action::Scratch(_))
        };
        for action in all_actions().iter().filter(|a| !writes(a)) {
            for s in scripts(action) {
                assert!(!s.contains("cat > "), "{action:?} writes: {s}");
            }
        }
    }

    #[test]
    fn scratch_writes_the_files_before_booting() {
        // Without the two writes, `scratch` boots a directory
        // holding no Vagrantfile.
        let cmds = run(&Action::Scratch(scratch("pr-1234")));
        let programs: Vec<&str> =
            cmds.iter().map(|c| c.program.as_str()).collect();
        // Four, and every one of them is `ssh`: a VM action
        // runs no program on the workstation.
        assert_eq!(programs, vec!["ssh", "ssh", "ssh", "ssh"]);
        assert!(cmds[0].args[1].contains("mkdir -p"));
        assert!(cmds.last().unwrap().args[1].ends_with("vagrant 'up'"));
    }

    #[test]
    fn provision_writes_the_files_then_reprovisions() {
        // Pins the literal shell, so the command's whole effect
        // on the host is readable in one place.
        assert_eq!(
            scripts_head(&Action::Provision),
            vec![
                "ssh vmhost \"mkdir -p ~/'vms/myproject'\"",
                "ssh vmhost \"cat > ~/'vms/myproject/Vagrantfile' \
                 <<'BOMBYX_EOF'",
                "ssh vmhost \"cat > ~/'vms/myproject/bootstrap.sh' \
                 <<'BOMBYX_EOF'",
                "ssh vmhost \"cd ~/'vms/myproject' && \
                 BOMBYX_VM_HOST='vmhost' \
                 BOMBYX_VM_HOSTNAME=\\$(hostname -s) vagrant 'provision'\"",
            ]
        );
    }

    #[test]
    fn provision_and_up_take_the_same_shape() {
        // The invariant the shared helper exists to keep: the two
        // write the same three commands, and differ only in the
        // vagrant call that follows them. A `provision` that grew
        // its own file-writing logic could boot against a stale
        // Vagrantfile on the host.
        //
        // `up` then has one step neither shares, the snapshot
        // save, so the comparison stops at the vagrant call
        // rather than at the end of the plan.
        let up = run(&Action::Up);
        let pr = run(&Action::Provision);
        assert_eq!(up.len(), pr.len() + 1);
        let writes = pr.len() - 1;
        assert_eq!(up[..writes], pr[..writes]);
        // Two prefixes, not one: the boot names the provider
        // and `provision` does not, which
        // `only_the_calls_that_create_a_machine_name_the_provider`
        // states as a rule across every action.
        assert_eq!(
            up[writes].args[1],
            format!("cd ~/'vms/myproject' && {} vagrant 'up'", boot_env())
        );
        assert_eq!(
            pr.last().unwrap().args[1],
            format!("cd ~/'vms/myproject' && {} vagrant 'provision'", vm_env())
        );
    }

    #[test]
    fn scratch_and_up_take_the_same_shape() {
        // The two lifecycles must not drift apart again in how
        // they write and boot. They differ in one step and the
        // difference is deliberate: `up` ends by saving the
        // snapshot `reset` restores, and a scratch VM has no
        // `reset` -- it is discarded instead.
        let up = run(&Action::Up);
        let sc = run(&Action::Scratch(scratch("x")));
        let names = |cmds: &[RemoteCommand]| -> Vec<String> {
            cmds.iter().map(|c| c.program.clone()).collect()
        };
        assert_eq!(up.len(), sc.len() + 1);
        assert_eq!(names(&up[..sc.len()]), names(&sc));
        assert!(
            up.last().unwrap().args[1].contains("vagrant 'snapshot' 'save'"),
            "{:?}",
            up.last().unwrap().args
        );
    }

    #[test]
    fn scratch_targets_a_project_scoped_dir() {
        let cmds = run(&Action::Scratch(scratch("pr-1234")));
        assert_eq!(
            cmds[0].args[1],
            "mkdir -p ~/'vms/scratch/myproject/pr-1234'"
        );
    }

    #[test]
    fn down_only_halts() {
        let cmds = run(&Action::Down);
        let env = vm_env();
        assert_eq!(cmds.len(), 1);
        assert_eq!(
            cmds[0].args[1],
            format!("cd ~/'vms/myproject' && {env} vagrant 'halt'")
        );
    }

    #[test]
    fn status_queries_the_project_dir() {
        let cmds = run(&Action::Status);
        let env = vm_env();
        assert_eq!(
            cmds[0].args[1],
            format!("cd ~/'vms/myproject' && {env} vagrant 'status'")
        );
    }

    #[test]
    fn up_takes_the_snapshot_after_booting() {
        // Order is the assertion. The snapshot has to record a
        // machine that has finished booting, so the save is the
        // last step and never an earlier one.
        let cmds = run(&Action::Up);
        // Compared against the builder rather than a third copy
        // of the shell. What this test owns is which builder ends
        // the plan; `remote` owns what that builder emits.
        assert_eq!(
            cmds.last().unwrap(),
            &remote::save_snapshot_if_absent(
                &cfg(),
                &cfg().remote_project_dir(),
                Tty::NoPty
            )
        );
        assert!(
            cmds[cmds.len() - 2].args[1].ends_with("vagrant 'up'"),
            "the boot should come directly before the save: {:?}",
            cmds[cmds.len() - 2].args
        );
    }

    #[test]
    fn snapshot_replaces_the_snapshot_without_consulting_the_listing() {
        // The on-demand command exists to re-take, so it must
        // overwrite. Sharing `up`'s guard would make it do
        // nothing on exactly the machine an operator runs it on.
        let cmds = run(&Action::Snapshot);
        assert_eq!(cmds.len(), 1);
        assert_eq!(
            cmds[0].args[1],
            format!(
                "cd ~/'vms/myproject' && {} vagrant 'snapshot' 'save' \
                 '-f' 'fresh-install'",
                vm_env()
            )
        );
    }

    #[test]
    fn reset_restores_the_name_the_two_saves_write() {
        // The pairing this action set exists for. Asserted across
        // the plans rather than inside `remote`, because `plan`
        // chooses which builder each action gets and could hand
        // `reset` a different one.
        let restored = run(&Action::Reset)[0].args[1].clone();
        assert!(restored.contains("'fresh-install'"), "{restored}");
        for action in [Action::Up, Action::Snapshot] {
            let saved = scripts(&action);
            let save = saved.last().unwrap();
            assert!(
                save.contains("vagrant 'snapshot' 'save'"),
                "{action:?} should end by saving: {save}"
            );
            assert!(save.contains("'fresh-install'"), "{action:?}: {save}");
        }
    }

    #[test]
    fn reset_restores_the_fresh_install_snapshot() {
        let cmds = run(&Action::Reset);
        let env = vm_env();
        assert_eq!(
            cmds[0].args[1],
            format!(
                "cd ~/'vms/myproject' && {env} vagrant 'snapshot' \
                 'restore' 'fresh-install'"
            )
        );
    }

    #[test]
    fn shell_forces_a_tty() {
        let cmds = run(&Action::Shell);
        assert_eq!(cmds[0].args[0], "-t");
    }

    #[test]
    fn discard_destroys_the_vm_then_removes_the_dir() {
        // Order is the assertion. `vagrant` runs *inside* the
        // directory, so removing it first would leave nothing
        // to run in.
        let cmds = run(&Action::Discard(scratch("pr-1234")));
        assert_eq!(cmds.len(), 2);
        assert_eq!(
            cmds[0].args[1],
            format!(
                "cd ~/'vms/scratch/myproject/pr-1234' && if [ -f \
                 Vagrantfile ]; then {} vagrant 'destroy' '-f'; fi",
                vm_env()
            )
        );
        assert_eq!(cmds[1].args[1], "rm -rf ~/'vms/scratch/myproject/pr-1234'");
    }

    #[test]
    fn destroy_destroys_the_vm_then_removes_the_dir() {
        let cmds = run(&Action::Destroy);
        assert_eq!(cmds.len(), 2);
        assert_eq!(
            cmds[0].args[1],
            format!(
                "cd ~/'vms/myproject' && if [ -f Vagrantfile ]; then \
                 {} vagrant 'destroy' '-f'; fi",
                vm_env()
            )
        );
        assert_eq!(cmds[1].args[1], "rm -rf ~/'vms/myproject'");
    }

    #[test]
    fn destroy_and_discard_take_the_same_shape() {
        // Compare step *kinds*, not program names: both plans
        // are two ssh calls, so comparing programs would pass
        // through exactly the drift this guards against.
        let kinds = |cmds: &[RemoteCommand]| -> Vec<&'static str> {
            cmds.iter()
                .map(|c| {
                    if c.args[1].contains("vagrant 'destroy'") {
                        "destroy"
                    } else if c.args[1].starts_with("rm -rf") {
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
        assert!(!run(&Action::Doctor).is_empty());
    }

    /// The prefix on the boot, which is the one verb that names
    /// a provider.
    ///
    /// Every other verb carries [`vm_env`] alone;
    /// `only_the_calls_that_create_a_machine_name_the_provider`
    /// is what holds the two apart across the actions.
    fn boot_env() -> String {
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
    /// list of builders, which once enumerated four call sites
    /// where there were six. `doctor` is left out: its probes
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
                let script = cmd.args[cmd.args.len() - 1].clone();
                if script.contains(" vagrant '") {
                    found.push((action.clone(), script));
                }
            }
        }
        assert!(!found.is_empty(), "no plan runs vagrant at all");
        found
    }

    #[test]
    fn only_the_calls_that_create_a_machine_name_the_provider() {
        // `remote::PROVIDER_ENV` holds the argument and the
        // measurements. The short version: on any verb but the
        // boot the variable never changes which provider
        // vagrant uses, and on `destroy` it can refuse the
        // command that clears the directory a refused boot left
        // behind.
        let want = format!("{}='{}'", remote::PROVIDER_ENV, cfg().vm.provider);
        for (action, script) in project_vagrant_scripts() {
            let creates = script.contains(" vagrant 'up'");
            assert_eq!(
                script.contains(&want),
                creates,
                "{action:?}: a provider belongs on the boot and \
                 nowhere else: {script}"
            );
        }
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

    #[test]
    fn only_the_three_writing_actions_write() {
        // `provision` writes without booting anything, so a rule
        // phrased around booting would have made it an exception
        // instead of a third member of the set.
        for action in
            [Action::Up, Action::Provision, Action::Scratch(scratch("x"))]
        {
            assert!(
                scripts(&action).iter().any(|s| s.contains("cat > ")),
                "{action:?} must write the generated files"
            );
        }
    }
}
