//! Mapping a user action to the commands that implement it.
//!
//! This is the tool's policy -- which steps run, and in what
//! order -- so it lives in the library where it is covered by
//! tests, not in `src/bin/`.

use std::path::Path;

use crate::config::Config;
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
    /// Halt the project VM.
    Down,
    /// Open a shell inside the project VM.
    Shell,
    /// Show VM status on the host.
    Status,
    /// Restore the project VM's `fresh-install` snapshot.
    Reset,
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
        matches!(self, Self::Up | Self::Scratch(_))
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
        Action::Up => boot(cfg, &cfg.remote_project_dir(), local_dir, archive),
        Action::Down => vec![remote::vagrant(cfg, &["halt"])],
        Action::Shell => vec![remote::shell_into_vm(cfg)],
        Action::Status => vec![remote::vagrant(cfg, &["status"])],
        Action::Reset => vec![remote::vagrant(
            cfg,
            &["snapshot", "restore", "fresh-install"],
        )],
        Action::Destroy => tear_down(cfg, &cfg.remote_project_dir()),
        Action::Scratch(name) => {
            boot(cfg, &cfg.remote_scratch_dir(name), local_dir, archive)
        }
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
/// Vagrant directory into it, and boots the VM.
///
/// Shared by `up` and `scratch`: a scratch VM needs the same
/// Vagrantfile as a persistent one, just in a throwaway
/// directory. Routing both through one helper is what keeps
/// `scratch` from drifting back into booting an empty
/// directory.
fn boot(
    cfg: &Config,
    dir: &str,
    local_dir: &Path,
    archive: &PushArchive,
) -> Vec<RemoteCommand> {
    let mut cmds = vec![remote::ensure_dir(cfg, dir)];
    cmds.extend(remote::push_dir(cfg, local_dir, dir, archive));
    cmds.push(remote::vagrant_in(cfg, dir, &["up"]));
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::parse(
            "host = \"frosti\"\nproject = \"phren\"\n",
            Path::new("bombyx.toml"),
        )
        .unwrap()
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
    fn only_booting_actions_need_an_archive() {
        assert!(Action::Up.pushes());
        assert!(Action::Scratch(scratch("x")).pushes());
        for a in [
            Action::Down,
            Action::Status,
            Action::Shell,
            Action::Reset,
            Action::Destroy,
            Action::Discard(scratch("x")),
        ] {
            assert!(!a.pushes(), "{a:?} must not need an archive");
        }
    }
}
