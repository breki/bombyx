//! End-to-end CLI tests.
//!
//! These drive the real binary with `--dry-run`, so they
//! assert the commands bombyx *would* run without needing a
//! VM host.

use assert_cmd::Command;
use bombyx::config::{CONFIG_DIR_ENV, HOST_ENV, USER_CONFIG_FILE};
use bombyx::remote::{VM_HOST_ENV, VM_HOSTNAME_ENV};
use predicates::prelude::*;
use tempfile::TempDir;

/// Name of the per-developer config directory inside a fixture.
const CONFIG_HOME: &str = "config-home";

/// The VM-host identity prefix, as `--dry-run` prints it.
///
/// Every vagrant invocation carries it so the guest can find out
/// which machine it is running on. The command substitution
/// appears escaped here because that is what makes the printed
/// line honest: pasted into a shell it asks the *host* for its
/// name, which is the whole point of the variable.
///
/// Built from the library's own constants, for the same reason
/// [`write_user_config`] uses them: renamed, a hardcoded copy
/// would leave this suite green while bombyx exported something
/// else entirely.
fn vm_env() -> String {
    format!(r"{VM_HOST_ENV}='vmhost' {VM_HOSTNAME_ENV}=\$(hostname -s)")
}

/// Writes a `bombyx.toml` and a per-developer config naming
/// `vmhost` into a fresh temp dir.
///
/// The project file carries no `host`: bombyx refuses one there.
/// The host comes from the per-developer file, which is the
/// ordinary arrangement and the lowest-precedence source, so a
/// test can override it from any of the other three.
///
/// The returned guard removes the directory on drop, so a
/// failing assertion cannot leak it into the system temp dir.
fn project_dir() -> TempDir {
    let dir = project_dir_with("project = \"myproject\"\n");
    write_user_config(&dir, "host = \"vmhost\"\n");
    dir
}

/// A `Config` for `myproject` on `vmhost`, built the way the
/// binary builds one.
///
/// Through `Config::load` against a real file, not a shortcut past
/// it: `Config::parse` used to be public with no production caller,
/// so these tests were exercising a path nothing shipped.
fn load_cfg(dir: &std::path::Path) -> bombyx::config::Config {
    let path = dir.join("bombyx.toml");
    std::fs::write(&path, "project = \"myproject\"\n").unwrap();
    let (cfg, _) = bombyx::config::Config::load(
        &path,
        &bombyx::config::HostSources {
            flag: Some("vmhost"),
            ..bombyx::config::HostSources::default()
        },
    )
    .unwrap();
    cfg
}

/// A fixture whose `bombyx.toml` is exactly `source`.
fn project_dir_with(source: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("bombyx.toml"), source).unwrap();
    std::fs::create_dir(dir.path().join(CONFIG_HOME)).unwrap();
    dir
}

/// Writes the per-developer config inside a fixture.
///
/// The file name and both environment variables come from the
/// library's own constants. Hardcoded, a rename would leave this
/// suite green while it wrote a file bombyx no longer reads and
/// cleared a variable it no longer honours -- so the hermeticity
/// below would quietly stop holding.
fn write_user_config(dir: &TempDir, source: &str) {
    std::fs::write(dir.path().join(CONFIG_HOME).join(USER_CONFIG_FILE), source)
        .unwrap();
}

fn bombyx_in(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("bombyx").unwrap();
    cmd.current_dir(dir.path());
    // Hermetic on purpose. The host now comes from outside the
    // project, so without these two lines every assertion below
    // would depend on the developer's own `config.toml` and on
    // whether their shell exports BOMBYX_HOST -- passing on one
    // machine and failing on the next.
    cmd.env(CONFIG_DIR_ENV, dir.path().join(CONFIG_HOME));
    cmd.env_remove(HOST_ENV);
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
    assert!(lines[0].contains("mkdir -p ~/'vms/myproject'"));
    assert!(lines[1].contains("--exclude=./.vagrant"));
    assert!(lines[3].contains("tar -xzf"));
    assert!(
        lines[4].ends_with(&format!(
            "cd ~/'vms/myproject' && {} vagrant 'up'\"",
            vm_env()
        )),
        "{}",
        lines[4]
    );
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
fn a_local_config_overrides_the_user_config_host() {
    // The per-project escape hatch: one repo that needs a
    // different machine from the one in your `config.toml`.
    let dir = project_dir();
    std::fs::write(
        dir.path().join("bombyx.local.toml"),
        "host = \"my-vmhost\"\n",
    )
    .unwrap();

    let lines = dry_run(&dir, &["--dry-run", "status"]);
    assert!(lines[0].starts_with("ssh my-vmhost "), "{}", lines[0]);
    // The committed project config still applies.
    assert!(lines[0].contains("~/'vms/myproject'"), "{}", lines[0]);
}

#[test]
fn a_local_config_is_optional() {
    // The overwhelmingly common case is not having one, so its
    // absence must not be an error.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "status"]);
    assert!(lines[0].starts_with("ssh vmhost "), "{}", lines[0]);
}

#[test]
fn a_broken_local_config_is_reported() {
    // Silently ignoring it would send commands to the host the
    // override exists to replace.
    let dir = project_dir();
    std::fs::write(dir.path().join("bombyx.local.toml"), "host = ").unwrap();

    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("bombyx.local.toml"));
}

#[test]
fn provision_pushes_then_runs_vagrant_provision() {
    // The gap this command closes: `up` ships an edited
    // provisioning script and `vagrant up` on a running VM
    // never executes it, so the push reports success while
    // nothing was applied.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "provision"]);
    assert_eq!(programs(&lines), vec!["ssh", "tar", "scp", "ssh", "ssh"]);
    assert!(lines[0].contains("mkdir -p ~/'vms/myproject'"));
    assert!(
        lines[4].ends_with(&format!(
            "cd ~/'vms/myproject' && {} vagrant 'provision'\"",
            vm_env()
        )),
        "{}",
        lines[4]
    );
}

#[test]
fn scratch_pushes_into_a_project_scoped_dir() {
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "scratch", "pr-1234"]);
    assert_eq!(programs(&lines), vec!["ssh", "tar", "scp", "ssh", "ssh"]);
    assert!(lines[0].contains("mkdir -p ~/'vms/scratch/myproject/pr-1234'"));
    assert!(
        lines[4].ends_with(&format!(
            "cd ~/'vms/scratch/myproject/pr-1234' && {} vagrant 'up'\"",
            vm_env()
        )),
        "{}",
        lines[4]
    );
}

#[test]
fn destroy_needs_the_project_name_to_confirm() {
    // A bare `destroy` must refuse, name what to type, and name
    // the target -- the target is the part the operator can
    // check, since `project` comes from the same file that
    // picks the directory.
    let dir = project_dir();
    bombyx_in(&dir)
        .args(["--dry-run", "destroy"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("bombyx destroy myproject"));
}

#[test]
fn destroy_rejects_a_mismatched_project_name() {
    let dir = project_dir();
    bombyx_in(&dir)
        .args(["--dry-run", "destroy", "not-myproject"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not match"));
}

#[test]
fn destroy_wires_the_subcommand_through_to_a_teardown() {
    // The unit tests pin the exact command strings; this checks
    // the CLI reaches them and prints the resolved target.
    let dir = project_dir();
    let out = bombyx_in(&dir)
        .args(["--dry-run", "destroy", "myproject"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 2, "{stdout:?}");
    assert!(stdout.contains("rm -rf ~/'vms/myproject'"));
    // The target the operator can check against reality.
    assert!(
        stderr.contains("vmhost:~/vms/myproject"),
        "must name the target: {stderr:?}"
    );
}

#[test]
fn doctor_dry_run_lists_read_only_probes() {
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "doctor"]);
    // One line per probe, and every probe accounted for. An
    // embedded newline in a script would split the dry run and,
    // worse, could smuggle a second command past a reader.
    assert_eq!(lines.len(), 7, "{lines:?}");
    for l in &lines {
        // Asserted per line rather than "some line has each
        // option": the loose form is satisfied by seven different
        // lines each carrying one option. What each option
        // prevents is documented at `remote::probe::probe`.
        for opt in [
            "BatchMode=yes",
            "ConnectTimeout=10",
            "LogLevel=ERROR",
            "ServerAliveInterval=5",
            "ServerAliveCountMax=3",
        ] {
            assert!(l.contains(opt), "{opt} missing from {l:?}");
        }
        // And none of them may change the host. The list of what
        // that means lives in the library, so this test and the
        // unit test over the builders cannot disagree.
        assert_eq!(bombyx::doctor::mutating_token(l), None, "{l:?}");
    }
    // The probe the command exists for.
    assert!(
        lines.iter().any(|l| l.contains("command -v 'vagrant'")),
        "{lines:?}"
    );
}

#[test]
fn the_push_archive_really_excludes_dot_vagrant_and_dot_git() {
    // The exclusions are the reason a stale local `.vagrant`
    // cannot overwrite the host's copy and orphan a running VM.
    // Asserting the `--exclude` flags appear in the argv proves
    // only that bombyx asked; `tar` implementations disagree
    // about pattern matching, so this runs the real one and
    // reads the archive back.
    let project = TempDir::new().unwrap();
    let vagrant_dir = project.path().join("vagrant");
    std::fs::create_dir_all(vagrant_dir.join(".vagrant")).unwrap();
    std::fs::create_dir_all(vagrant_dir.join(".git")).unwrap();
    std::fs::write(vagrant_dir.join("Vagrantfile"), "# vm").unwrap();
    std::fs::write(vagrant_dir.join(".vagrant/machine-id"), "x").unwrap();
    std::fs::write(vagrant_dir.join(".git/HEAD"), "y").unwrap();

    let work = TempDir::new().unwrap();
    let archive = bombyx::remote::PushArchive::new(work.path(), "test");
    let cfg_dir = TempDir::new().unwrap();
    let cfg = load_cfg(cfg_dir.path());
    let cmds = bombyx::plan::plan(
        &bombyx::plan::Action::Up,
        &cfg,
        &vagrant_dir,
        &archive,
        bombyx::remote::Tty::NoPty,
    );
    let pack = cmds
        .iter()
        .find(|c| c.program == "tar")
        .expect("up must pack an archive");

    // Resolved the same way bombyx resolves it, so this exercises
    // the `tar` a real push would use rather than whichever one
    // the OS search happens to prefer.
    let tar = bombyx::tool::resolve("tar").expect("tar must be on PATH");
    let status = std::process::Command::new(&tar)
        .args(&pack.args)
        .current_dir(pack.dir.as_ref().unwrap())
        .status()
        .unwrap();
    assert!(status.success(), "packing failed: {status}");

    let listed = std::process::Command::new(&tar)
        .args(["-tzf", &archive.name])
        .current_dir(work.path())
        .output()
        .unwrap();
    assert!(listed.status.success());
    let names = String::from_utf8_lossy(&listed.stdout);
    assert!(names.contains("Vagrantfile"), "{names}");
    assert!(!names.contains(".vagrant"), "{names}");
    assert!(!names.contains(".git"), "{names}");
}

/// The tag in the report row for `scope` and `name`.
///
/// Asserting on the row rather than on substrings of the whole
/// report: `contains("Vagrantfile")` matches the name column,
/// which is printed for a pass, a failure and a skip alike, and
/// `contains("FAIL")` is satisfied by any other failing check. An
/// earlier version of this test asserted both and passed whatever
/// `vagrantfile_finding` returned.
///
/// The scope is part of the key because it has to be: `ssh` and
/// `tar` each name both a local check and a host probe, so
/// matching on the name alone silently answers about the local
/// one.
fn row_tag<'a>(
    lines: &'a [String],
    scope: &str,
    name: &str,
) -> Option<&'a str> {
    lines.iter().find_map(|l| {
        // `<scope>  <name>  <tag>  [detail]`, columns padded.
        let rest = l.trim_start().strip_prefix(scope)?;
        let rest = rest.strip_prefix(' ')?.trim_start();
        // Guard against `tar` matching a `tar-x` row.
        let after = rest.strip_prefix(name)?.strip_prefix(' ')?;
        after.split_whitespace().next()
    })
}

#[test]
fn doctor_judges_the_local_vagrantfile_check_on_its_own() {
    // The local checks must be evaluated independently of the
    // host, so a typo in `vagrant_dir` is caught whether or not
    // the VM host answers. Both cases are asserted, because only
    // the pair rules out a check that always returns the same
    // thing.
    let dir = project_dir_with("project = \"myproject\"\n");
    write_user_config(&dir, "host = \"nosuchhost.invalid\"\n");

    let missing = bombyx_in(&dir).args(["doctor"]).assert().failure();
    let out = String::from_utf8(missing.get_output().stdout.clone()).unwrap();
    let lines: Vec<String> = out.lines().map(str::to_owned).collect();
    assert_eq!(
        row_tag(&lines, "local", "Vagrantfile"),
        Some("FAIL"),
        "{out}"
    );

    std::fs::create_dir(dir.path().join("vagrant")).unwrap();
    std::fs::write(dir.path().join("vagrant/Vagrantfile"), "# vm").unwrap();
    let present = bombyx_in(&dir).args(["doctor"]).assert().failure();
    let out = String::from_utf8(present.get_output().stdout.clone()).unwrap();
    let lines: Vec<String> = out.lines().map(str::to_owned).collect();
    assert_eq!(row_tag(&lines, "local", "Vagrantfile"), Some("ok"), "{out}");
    // Still a failing run overall -- the host does not resolve --
    // which is what makes the row assertion the load-bearing one.
    assert_eq!(
        row_tag(&lines, "nosuchhost.invalid", "ssh"),
        Some("FAIL"),
        "{out}"
    );
}

#[test]
fn doctor_dry_run_prints_exactly_the_commands_a_live_run_sends() {
    // The invariant behind "--dry-run goes through `plan`": the
    // binary must not be able to advertise a probe list the live
    // runner would not use. Compared against the library's own
    // rendering of `host_probes`, through the real CLI.
    let dir = project_dir();
    let printed = dry_run(&dir, &["--dry-run", "doctor"]);
    let cfg_dir = TempDir::new().unwrap();
    let cfg = load_cfg(cfg_dir.path());
    let expected: Vec<String> =
        bombyx::doctor::probe_commands(&bombyx::doctor::host_probes(&cfg))
            .iter()
            .map(ToString::to_string)
            .collect();
    assert_eq!(printed, expected);
}

#[test]
fn a_dangerous_remote_root_is_refused_at_load() {
    // Each of these once produced a teardown outside the
    // configured root, or at a depth the floor claimed to
    // forbid. They must fail before any command is built, on
    // every subcommand -- not just the destructive ones.
    for root in ["~/..", "/.", "~/.", "/", "~", "vms"] {
        let dir = project_dir_with(&format!(
            "project = \"etc\"\nremote_root = {root:?}\n"
        ));
        // A host has to be resolvable, or the run fails on the
        // missing host before it ever validates `remote_root`.
        write_user_config(&dir, "host = \"vmhost\"\n");
        bombyx_in(&dir)
            .args(["--dry-run", "up"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("remote_root"));
    }
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
fn a_host_cannot_smuggle_an_ssh_option() {
    // `host` reaches `ssh` as its first positional argument and
    // neither ssh nor scp honours `--`, so a leading `-` is read
    // as an option. It can no longer arrive from a repo, but a
    // per-developer file or a mistyped flag still reaches the
    // same argv.
    let dir = project_dir();
    write_user_config(&dir, "host = \"-oProxyCommand=curl evil\"\n");
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must not start with"));
}

#[test]
fn a_typo_in_the_config_is_reported() {
    let dir = project_dir_with("project = \"p\"\nvagrantdir = \"x\"\n");
    write_user_config(&dir, "host = \"vmhost\"\n");
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("vagrantdir"));
}

#[test]
fn a_host_in_the_project_file_is_refused() {
    // The rule the design turns on: the VM host belongs to the
    // developer, and `bombyx.toml` is committed. Refused rather
    // than ignored, and the message has to say where the line
    // goes instead.
    let dir = project_dir_with("host = \"vmhost\"\nproject = \"myproject\"\n");
    write_user_config(&dir, "host = \"mine\"\n");
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("`host` is not allowed"))
        .stderr(predicate::str::contains("bombyx.local.toml"));
}

#[test]
fn no_host_anywhere_names_every_way_to_set_one() {
    // The first-run failure. It has to be actionable: an
    // operator who has never configured a host learns all four
    // sources from this one message.
    let dir = project_dir_with("project = \"myproject\"\n");
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no VM host configured"))
        .stderr(predicate::str::contains("--host"))
        .stderr(predicate::str::contains("BOMBYX_HOST"));
}

#[test]
fn the_host_flag_outranks_every_file() {
    // Both files name a host, and the flag still wins.
    let dir = project_dir();
    std::fs::write(
        dir.path().join("bombyx.local.toml"),
        "host = \"from-overlay\"\n",
    )
    .unwrap();
    let lines = dry_run(&dir, &["--dry-run", "--host", "from-flag", "status"]);
    assert!(lines[0].starts_with("ssh from-flag "), "{}", lines[0]);
}

#[test]
fn a_flag_host_says_where_the_host_came_from() {
    // With a `bombyx.local.toml` present, the "overrides" notice
    // otherwise reads as "the host in that file is in force"
    // when the flag actually won. `destroy` runs `rm -rf` on the
    // winner, so the two notices must not disagree.
    let dir = project_dir();
    std::fs::write(
        dir.path().join("bombyx.local.toml"),
        "host = \"from-overlay\"\n",
    )
    .unwrap();
    bombyx_in(&dir)
        .args(["--dry-run", "--host", "from-flag", "status"])
        .assert()
        .success()
        .stderr(predicate::str::contains("host from-flag from --host"));
}

#[test]
fn an_overlay_without_a_host_does_not_claim_the_host() {
    // The gap between the two notices. A `bombyx.local.toml`
    // setting only `remote_root` still prints "overrides", which
    // reads as "the host in that file is in force" -- while the
    // host actually came from the per-developer file. The
    // provenance line has to name the real source.
    let dir = project_dir();
    std::fs::write(
        dir.path().join("bombyx.local.toml"),
        "remote_root = \"/srv/vms\"\n",
    )
    .unwrap();

    let out = bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();

    // The overlay is in force for the field it does set.
    assert!(stdout.contains("/srv/vms/myproject"), "{stdout}");
    // The host came from the per-developer file, and nothing may
    // attribute it to the overlay.
    assert!(stdout.starts_with("ssh vmhost "), "{stdout}");
    assert!(
        !stderr.contains("host vmhost from bombyx.local.toml"),
        "{stderr}"
    );
}

#[test]
fn an_invalid_host_does_not_blame_the_project_file() {
    // The project file is the one file forbidden to carry a host,
    // so an error about a bad host must not be prefixed with
    // "loading bombyx.toml" -- that sends the operator to edit
    // the wrong thing.
    let dir = project_dir();
    write_user_config(&dir, "host = \"-oProxyCommand=curl evil\"\n");
    let out = bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("must not start with"), "{stderr}");
    assert!(stderr.contains(USER_CONFIG_FILE), "{stderr}");
    assert!(!stderr.contains("loading bombyx.toml"), "{stderr}");
}

#[test]
fn the_host_env_var_outranks_the_files() {
    // Between the flag and the files: useful in CI, or for an
    // agent driving bombyx with no config directory of its own.
    let dir = project_dir();
    let out = bombyx_in(&dir)
        .env("BOMBYX_HOST", "from-env")
        .args(["--dry-run", "status"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.starts_with("ssh from-env "), "{stdout}");
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
