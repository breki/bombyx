//! Mapping a user action to the commands that implement it.
//!
//! This is the tool's policy -- which steps run, and in what
//! order -- so it lives in the library where it is covered by
//! tests, not in `src/bin/`.

use std::path::Path;

use crate::config::Config;
use crate::doctor;
use crate::name::ScratchName;
use crate::remote::{self, PushArchive, RemoteCommand};

/// What the user asked bombyx to do.
///
/// Separate from the CLI's own subcommand enum so the library
/// does not depend on the argument parser, and so a scratch
/// name is already validated by the time it gets here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Push the Vagrant dir and boot the project VM.
    Up,
    /// Push the Vagrant dir and re-run provisioning on the
    /// project VM.
    ///
    /// Separate from [`Action::Up`] because vagrant provisions
    /// a machine only when it first creates it. Every later
    /// `vagrant up` skips the provisioners -- whether the VM
    /// was halted or running -- so an edited script reaches the
    /// host and nothing executes it, while the push reports
    /// success.
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
    /// Check bombyx's preconditions without changing anything.
    Doctor,
    /// Destroy the project VM and remove its directory.
    Destroy,
    /// Boot a throwaway VM.
    Scratch(ScratchName),
    /// Destroy a throwaway VM.
    Discard(ScratchName),
}

impl Action {
    /// Whether carrying out this action pushes the Vagrant
    /// directory, and so needs a local archive.
    #[must_use]
    pub fn pushes(&self) -> bool {
        matches!(self, Self::Up | Self::Provision | Self::Scratch(_))
    }
}

/// Returns the ordered commands that carry out `action`.
///
/// `local_dir` is the absolute path of the project's Vagrant
/// directory; `archive` names the transient push archive.
#[must_use]
pub fn plan(
    action: &Action,
    cfg: &Config,
    local_dir: &Path,
    archive: &PushArchive,
) -> Vec<RemoteCommand> {
    match action {
        Action::Up => push_then(
            cfg,
            &cfg.remote_project_dir(),
            local_dir,
            archive,
            &["up"],
        ),
        Action::Provision => push_then(
            cfg,
            &cfg.remote_project_dir(),
            local_dir,
            archive,
            &["provision"],
        ),
        Action::Down => vec![remote::vagrant(cfg, &["halt"])],
        Action::Shell => vec![remote::shell_into_vm(cfg)],
        Action::Status => vec![remote::vagrant(cfg, &["status"])],
        Action::Reset => vec![remote::vagrant(
            cfg,
            &["snapshot", "restore", "fresh-install"],
        )],
        // Host probes only. The local checks read this
        // filesystem and spawn a `--version` call, so there is no
        // command line a dry run could print that would describe
        // them honestly.
        Action::Doctor => doctor::probe_commands(&doctor::host_probes(cfg)),
        Action::Destroy => tear_down(cfg, &cfg.remote_project_dir()),
        Action::Scratch(name) => push_then(
            cfg,
            &cfg.remote_scratch_dir(name),
            local_dir,
            archive,
            &["up"],
        ),
        Action::Discard(name) => tear_down(cfg, &cfg.remote_scratch_dir(name)),
    }
}

/// Destroys the VM defined in `dir`, then removes `dir`.
///
/// Shared by `destroy` and `discard`, which differ only in
/// which directory they target. The order is load-bearing:
/// `vagrant` runs *inside* the directory, so removing it first
/// would leave nothing to run in.
///
/// The destroy step tolerates a directory with no Vagrantfile,
/// which is reachable without any unusual input -- an
/// interrupted first push leaves the directory created but
/// empty. A bare `vagrant destroy -f` fails there, and since
/// `execute` stops at the first failure the removal would never
/// run, leaving a directory no bombyx command could clear.
/// Skipping the destroy instead makes teardown re-runnable.
fn tear_down(cfg: &Config, dir: &str) -> Vec<RemoteCommand> {
    vec![
        remote::destroy_vm_if_present(cfg, dir),
        remote::remove_dir(cfg, dir),
    ]
}

/// Ensures `dir` exists on the host, pushes the project's
/// Vagrant directory into it, then runs `vagrant` with `args`
/// there.
///
/// Shared by `up`, `scratch` and `provision`, which differ only
/// in the directory they target and the vagrant arguments they
/// end with. Routing all three through one helper is what keeps
/// the push from drifting: every caller ships the local
/// directory first, so vagrant always acts on the working tree
/// rather than on whatever a previous run left behind.
///
/// `args` is a slice rather than one string, matching
/// [`remote::vagrant_in`]. A single string would turn a
/// two-word invocation into one quoted argument, which fails on
/// the host after the push has already changed state.
fn push_then(
    cfg: &Config,
    dir: &str,
    local_dir: &Path,
    archive: &PushArchive,
    args: &[&str],
) -> Vec<RemoteCommand> {
    let mut cmds = vec![remote::ensure_dir(cfg, dir)];
    cmds.extend(remote::push_dir(cfg, local_dir, dir, archive));
    cmds.push(remote::vagrant_in(cfg, dir, args));
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::for_tests()
    }

    fn run(action: &Action) -> Vec<RemoteCommand> {
        plan(
            action,
            &cfg(),
            Path::new("/repo/vagrant"),
            &PushArchive::new(Path::new("/work"), "42"),
        )
    }

    fn scripts(action: &Action) -> Vec<String> {
        run(action).iter().map(ToString::to_string).collect()
    }

    fn scratch(name: &str) -> ScratchName {
        ScratchName::parse(name).unwrap()
    }

    #[test]
    fn up_makes_the_dir_then_pushes_then_boots() {
        // Order is the point: booting before the push would
        // run `vagrant up` with no Vagrantfile in place.
        assert_eq!(
            scripts(&Action::Up),
            vec![
                "ssh frosti \"mkdir -p ~/'vms/phren'\"",
                "cd /work && tar -czf .bombyx-push-42.tar.gz -C \
                 /repo/vagrant --exclude=./.vagrant \
                 --exclude=./.git .",
                "cd /work && scp .bombyx-push-42.tar.gz \
                 frosti:.bombyx-push-42.tar.gz",
                "ssh frosti \"{ cd ~/'vms/phren' && tar -xzf \
                 ~/'.bombyx-push-42.tar.gz'; }; rc=\\$?; rm -f \
                 ~/'.bombyx-push-42.tar.gz'; exit \\$rc\"",
                "ssh frosti \"cd ~/'vms/phren' && vagrant 'up'\"",
            ]
        );
    }

    #[test]
    fn scratch_pushes_before_booting() {
        // Without the push, `scratch` boots an empty dir.
        let cmds = run(&Action::Scratch(scratch("pr-1234")));
        let programs: Vec<&str> =
            cmds.iter().map(|c| c.program.as_str()).collect();
        assert_eq!(programs, vec!["ssh", "tar", "scp", "ssh", "ssh"]);
        assert!(cmds[0].args[1].contains("mkdir -p"));
        assert!(cmds.last().unwrap().args[1].ends_with("vagrant 'up'"));
    }

    #[test]
    fn provision_pushes_then_reprovisions() {
        // Pins the literal shell, so the command's whole effect
        // on the host is readable in one place.
        assert_eq!(
            scripts(&Action::Provision),
            vec![
                "ssh frosti \"mkdir -p ~/'vms/phren'\"",
                "cd /work && tar -czf .bombyx-push-42.tar.gz -C \
                 /repo/vagrant --exclude=./.vagrant \
                 --exclude=./.git .",
                "cd /work && scp .bombyx-push-42.tar.gz \
                 frosti:.bombyx-push-42.tar.gz",
                "ssh frosti \"{ cd ~/'vms/phren' && tar -xzf \
                 ~/'.bombyx-push-42.tar.gz'; }; rc=\\$?; rm -f \
                 ~/'.bombyx-push-42.tar.gz'; exit \\$rc\"",
                "ssh frosti \"cd ~/'vms/phren' && vagrant 'provision'\"",
            ]
        );
    }

    #[test]
    fn provision_and_up_take_the_same_shape() {
        // The invariant the shared helper exists to keep: the
        // two differ in their last step and nowhere else. A
        // `provision` that grew its own push logic could skip
        // the push and re-run the stale copy on the host --
        // the bug the command was added to fix.
        let up = run(&Action::Up);
        let pr = run(&Action::Provision);
        assert_eq!(up.len(), pr.len());
        assert_eq!(up[..up.len() - 1], pr[..pr.len() - 1]);
        assert_eq!(
            up.last().unwrap().args[1],
            "cd ~/'vms/phren' && vagrant 'up'"
        );
        assert_eq!(
            pr.last().unwrap().args[1],
            "cd ~/'vms/phren' && vagrant 'provision'"
        );
    }

    #[test]
    fn scratch_and_up_take_the_same_shape() {
        // The two lifecycles must not drift apart again.
        let up = run(&Action::Up);
        let sc = run(&Action::Scratch(scratch("x")));
        let names = |cmds: &[RemoteCommand]| -> Vec<String> {
            cmds.iter().map(|c| c.program.clone()).collect()
        };
        assert_eq!(names(&up), names(&sc));
    }

    #[test]
    fn scratch_targets_a_project_scoped_dir() {
        let cmds = run(&Action::Scratch(scratch("pr-1234")));
        assert_eq!(cmds[0].args[1], "mkdir -p ~/'vms/scratch/phren/pr-1234'");
    }

    #[test]
    fn down_halts_without_pushing() {
        let cmds = run(&Action::Down);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].args[1], "cd ~/'vms/phren' && vagrant 'halt'");
    }

    #[test]
    fn status_queries_the_project_dir() {
        let cmds = run(&Action::Status);
        assert_eq!(cmds[0].args[1], "cd ~/'vms/phren' && vagrant 'status'");
    }

    #[test]
    fn reset_restores_the_fresh_install_snapshot() {
        let cmds = run(&Action::Reset);
        assert_eq!(
            cmds[0].args[1],
            "cd ~/'vms/phren' && vagrant 'snapshot' 'restore' \
             'fresh-install'"
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
            "cd ~/'vms/scratch/phren/pr-1234' && if [ -f Vagrantfile ]; \
             then vagrant destroy -f; fi"
        );
        assert_eq!(cmds[1].args[1], "rm -rf ~/'vms/scratch/phren/pr-1234'");
    }

    #[test]
    fn destroy_destroys_the_vm_then_removes_the_dir() {
        let cmds = run(&Action::Destroy);
        assert_eq!(cmds.len(), 2);
        assert_eq!(
            cmds[0].args[1],
            "cd ~/'vms/phren' && if [ -f Vagrantfile ]; then \
             vagrant destroy -f; fi"
        );
        assert_eq!(cmds[1].args[1], "rm -rf ~/'vms/phren'");
    }

    #[test]
    fn destroy_and_discard_take_the_same_shape() {
        // Compare step *kinds*, not program names: both plans
        // are two ssh calls, so comparing programs would pass
        // through exactly the drift this guards against.
        let kinds = |cmds: &[RemoteCommand]| -> Vec<&'static str> {
            cmds.iter()
                .map(|c| {
                    if c.args[1].contains("vagrant destroy") {
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

    #[test]
    fn only_pushing_actions_need_an_archive() {
        // Named for pushing rather than booting: `provision`
        // needs the archive without booting anything, so tying
        // the rule to "boots" would have made it the exception
        // instead of a third member of the set.
        assert!(Action::Up.pushes());
        assert!(Action::Scratch(scratch("x")).pushes());
        assert!(Action::Provision.pushes());
        for a in [
            Action::Down,
            Action::Status,
            Action::Shell,
            Action::Reset,
            Action::Destroy,
            Action::Doctor,
            Action::Discard(scratch("x")),
        ] {
            assert!(!a.pushes(), "{a:?} must not need an archive");
        }
    }
}
