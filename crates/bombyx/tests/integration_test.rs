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
    std::fs::write(&path, completed("project = \"myproject\"\n")).unwrap();
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

/// The `[vm]` and `[source]` tables every project file needs.
///
/// bombyx generates the Vagrantfile, so it has to be told what
/// machine to build and where the guest clones the project
/// from. Neither section has a default.
const REQUIRED_TABLES: &str = "\n[vm]\n\
     provider = \"libvirt\"\n\
     box = \"generic/ubuntu2204\"\n\
     cpus = 2\n\
     memory = 2048\n\
     \n\
     [source]\n\
     repo = \"https://example.invalid/myproject.git\"\n\
     ref = \"main\"\n\
     script = \"vagrant/provision.sh\"\n";

/// Appends [`REQUIRED_TABLES`] when `source` omits them.
///
/// These tests vary one top-level key and care nothing about
/// the machine description, so spelling it out in each would
/// bury the line under test. A test *about* a missing section
/// includes `[vm]` itself and is left alone.
fn completed(source: &str) -> String {
    if source.contains("[vm]") {
        source.to_owned()
    } else {
        format!("{source}{REQUIRED_TABLES}")
    }
}

/// A fixture whose `bombyx.toml` is `source`, with the required
/// `[vm]` and `[source]` tables appended when absent.
fn project_dir_with(source: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("bombyx.toml"), completed(source)).unwrap();
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
fn up_makes_the_dir_writes_the_files_then_boots() {
    // Order is the assertion: a `contains` check would pass
    // even if the boot ran before the writes.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "up"]);
    assert_eq!(programs(&lines), vec!["ssh", "ssh", "ssh", "ssh"]);
    assert!(lines[0].contains("mkdir -p ~/'vms/myproject'"));
    // Each generated file prints as one elided line.
    assert!(lines[1].contains("cat > ~/'vms/myproject/Vagrantfile'"));
    assert!(lines[1].contains("lines elided"), "{}", lines[1]);
    assert!(lines[2].contains("cat > ~/'vms/myproject/bootstrap.sh'"));
    assert!(
        lines[3].ends_with(&format!(
            "cd ~/'vms/myproject' && {} vagrant 'up'\"",
            vm_env()
        )),
        "{}",
        lines[3]
    );
}

#[test]
fn up_keeps_the_tilde_expandable() {
    // A single-quoted `~` makes a directory literally named
    // `~`, so the boot would run somewhere the generated
    // Vagrantfile is not.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "up"]);
    assert!(
        lines.iter().all(|l| !l.contains("'~/")),
        "no path may be quoted with the tilde inside: {lines:?}"
    );
}

#[test]
fn up_runs_nothing_on_the_workstation() {
    // Every step is an `ssh`, so no path from this machine
    // reaches a program's argv. That matters on Windows, where a
    // local path starts with a drive letter and any program
    // reading `host:file` -- `scp`, GNU `tar` -- would take the
    // `C` for a host name.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "up"]);
    assert!(programs(&lines).iter().all(|p| *p == "ssh"), "{lines:?}");
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
fn provision_writes_the_files_then_runs_vagrant_provision() {
    // The gap this command closes: `vagrant up` on a machine
    // that already exists skips the provisioners, so the guest
    // keeps running the script it cloned when it was created and
    // `up` reports success.
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "provision"]);
    assert_eq!(programs(&lines), vec!["ssh", "ssh", "ssh", "ssh"]);
    assert!(lines[0].contains("mkdir -p ~/'vms/myproject'"));
    assert!(
        lines[3].ends_with(&format!(
            "cd ~/'vms/myproject' && {} vagrant 'provision'\"",
            vm_env()
        )),
        "{}",
        lines[3]
    );
}

#[test]
fn scratch_writes_into_a_project_scoped_dir() {
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "scratch", "pr-1234"]);
    assert_eq!(programs(&lines), vec!["ssh", "ssh", "ssh", "ssh"]);
    assert!(lines[0].contains("mkdir -p ~/'vms/scratch/myproject/pr-1234'"));
    assert!(
        lines[3].ends_with(&format!(
            "cd ~/'vms/scratch/myproject/pr-1234' && {} vagrant 'up'\"",
            vm_env()
        )),
        "{}",
        lines[3]
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

/// `doctor` run for real, not as a dry run.
///
/// The dry-run tests below cover which commands `doctor` would
/// send. This one covers the half they cannot reach: spawning a
/// local tool, rendering the report, and the exit code. That
/// code is the whole point of the command -- an operator or a
/// script reads it to decide whether `up` is worth trying -- and
/// `src/bin/` is excluded from the coverage gate, so without a
/// test here a change to it fails nothing.
///
/// It points at a host that cannot resolve, so it needs no VM
/// host and no network.
#[test]
fn doctor_fails_and_says_which_check_failed() {
    let dir = project_dir();
    write_user_config(&dir, "host = \"nosuchhost.invalid\"\n");

    // `~/.ssh/config` is not consulted. A `Host *` block with a
    // `ProxyCommand` is common on a work laptop, and
    // `ConnectTimeout` is not inherited by one, so a proxy that
    // accepts the connection and then goes quiet would hang this
    // test. Pointing HOME at the fixture is the same precaution
    // `-F /dev/null` gives a hand-run `ssh`.
    let out = bombyx_in(&dir)
        .env("HOME", dir.path())
        .env("USERPROFILE", dir.path())
        .args(["doctor"])
        .assert()
        .failure();
    let text = String::from_utf8(out.get_output().stdout.clone()).unwrap();

    // Exactly one check failed, and it is the host one. The
    // count is what rules out a blanket failure: on a machine
    // with no `ssh` at all every row would say FAIL, and a test
    // asserting only "some row says FAIL" would stay green
    // through exactly the regression it exists to catch.
    assert!(text.contains("1 check failed"), "{text}");
    assert!(
        text.lines().any(|l| {
            l.contains("local") && l.contains("ssh") && l.contains("ok")
        }),
        "the local ssh row must pass: {text}"
    );
    assert!(
        text.lines()
            .any(|l| l.contains("nosuchhost.invalid") && l.contains("FAIL")),
        "{text}"
    );

    // `ssh` is the only local program checked. `git`, `curl` and
    // `tar` belong to self-update, and a red row for any of them
    // would make the exit code say nothing about whether `up`
    // works.
    for absent in ["tar", "scp", "curl", "git"] {
        assert!(
            !text
                .lines()
                .any(|l| l.contains("local") && l.contains(absent)),
            "doctor must not check {absent} locally: {text}"
        );
    }
}

#[test]
fn doctor_dry_run_lists_read_only_probes() {
    let dir = project_dir();
    let lines = dry_run(&dir, &["--dry-run", "doctor"]);
    // One line per probe, and every probe accounted for. An
    // embedded newline in a script would split the dry run and,
    // worse, could smuggle a second command past a reader.
    //
    // Five because the fixture is a libvirt project. A Hyper-V
    // one sends four -- see
    // `probes::tests::the_libvirt_probe_is_only_sent_for_a_libvirt_project`.
    assert_eq!(lines.len(), 5, "{lines:?}");
    for l in &lines {
        // Asserted per line rather than "some line has each
        // option": the loose form is satisfied by five different
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
        // A host has to be configured, or the run fails with
        // `no VM host configured` before it ever validates
        // `remote_root`.
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
    // Quoting stops injection but not traversal. The name
    // becomes a directory on the VM host, and without validation
    // that directory is the one `mkdir -p` creates, the one the
    // generated files are written into, and the one `discard`
    // hands to `rm -rf`.
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
    // `ssh` does not honour `--`, so a leading `-` is read
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
    // `remote_rot`, a near-miss of a key that exists. A typo of
    // a key that never existed would pass for the same reason
    // whether or not `deny_unknown_fields` were set.
    let dir = project_dir_with("project = \"p\"\nremote_rot = \"x\"\n");
    write_user_config(&dir, "host = \"vmhost\"\n");
    bombyx_in(&dir)
        .args(["--dry-run", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("remote_rot"));
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
fn status_sends_exactly_one_command() {
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

/// Parses the generated Vagrantfile with the real `vagrant`.
///
/// Ignored by default because it needs vagrant installed with a
/// provider plugin, which CI does not have. Run it with
/// `cargo xtask test --ignored`.
///
/// Worth having because every unit test around the renderer
/// asserts that bombyx produced the text it meant to, and none
/// of them can say whether vagrant accepts it. A syntax error
/// would otherwise surface on the VM host, after bombyx has
/// already created the directory and written both files there.
///
/// It lives here rather than beside the renderer so its body,
/// which never runs under coverage, does not count against that
/// small module's per-module floor.
#[test]
#[ignore = "needs vagrant and a provider plugin installed"]
fn the_generated_vagrantfile_is_one_vagrant_accepts() {
    let dir = TempDir::new().unwrap();
    let cfg = load_cfg(dir.path());
    std::fs::write(
        dir.path().join(bombyx::vagrantfile::VAGRANTFILE_NAME),
        bombyx::vagrantfile::render(&cfg),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(bombyx::vagrantfile::BOOTSTRAP_NAME),
        bombyx::vagrantfile::BOOTSTRAP,
    )
    .unwrap();

    let out = std::process::Command::new("vagrant")
        .arg("validate")
        .current_dir(dir.path())
        .output()
        .expect("vagrant must be on PATH for this test");
    assert!(
        out.status.success(),
        "vagrant rejected the generated file:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
