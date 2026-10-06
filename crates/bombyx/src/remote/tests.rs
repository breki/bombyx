//! Tests for `super`, the commands bombyx sends to the VM host.

use super::*;
use crate::name::ScratchName;

/// The ssh options that precede the destination, in order.
///
/// Asserted as a whole rather than by index. A test pinning
/// `args[1]` as the host passes an argv where a new option
/// has pushed the host somewhere else: the index still holds
/// a string, just the wrong one. Comparing the whole list is
/// what notices.
fn opts_before_host(c: &RemoteCommand) -> Vec<String> {
    c.args
        .iter()
        .take_while(|a| *a != "vmhost")
        .cloned()
        .collect()
}

/// The remote script as built, whatever precedes it.
fn raw_script(c: &RemoteCommand) -> String {
    c.args.last().expect("a remote command").clone()
}

/// The remote script without the `unset` every route
/// carries.
///
/// Every test below asks about the part of the script the
/// builder wrote, so stripping here keeps the prefix out of
/// two dozen expected strings.
/// `every_route_disarms_the_vagrant_redirects` is the one
/// test that reads the prefix, and it uses `raw_script`.
fn remote_script(c: &RemoteCommand) -> String {
    script_without_disarm(c)
}

fn local_cfg() -> Config {
    Config::for_tests_local()
}

#[test]
fn the_local_route_runs_the_same_script_through_sh() {
    // The script is the delicate part: quoting, the `cd`,
    // the redirection and the `$(hostname -s)` the far
    // side must evaluate. `sh -c` is the same POSIX shell
    // `ssh` starts on the host, so every builder keeps one
    // script. The only difference on this route is the two
    // words in front of it, so the scripts must be equal
    // character for character.
    /// One builder, named for the error message.
    type Builder = (&'static str, fn(&Config) -> RemoteCommand);

    let builders: [Builder; 9] = [
        ("vagrant", |c| vagrant(c, &["status"], Tty::NoPty)),
        ("status_or_never_built", |c| {
            status_or_never_built(c, Tty::NoPty)
        }),
        // A row because this builder does not go through
        // `transport`: `unattended` matches on the route
        // itself, so a script made conditional there is
        // exactly what this test exists to catch.
        ("listing", |c| vagrant_status_many(c, &[])),
        ("ensure_dir", |c| ensure_dir(c, "~/vms")),
        ("remove_dir", |c| remove_dir(c, "~/vms/myproject")),
        ("destroy", |c| {
            destroy_vm_if_present(c, "~/vms/myproject", Tty::NoPty)
        }),
        ("snapshot", |c| {
            save_snapshot(c, &c.remote_project_dir(), Tty::NoPty)
        }),
        ("guarded snapshot", |c| {
            save_snapshot_if_absent(c, &c.remote_project_dir(), Tty::NoPty)
        }),
        ("write", |c| write_file(c, "~/vms", "Vagrantfile", b"x\n")),
    ];
    for (name, build) in builders {
        let over_ssh = build(&cfg());
        let here = build(&local_cfg());
        assert_eq!(here.program, "sh", "{name}");
        assert_eq!(here.args.len(), 2, "{name}: {:?}", here.args);
        assert_eq!(here.args[0], "-c", "{name}");
        assert_eq!(remote_script(&here), remote_script(&over_ssh), "{name}");
    }
}

#[test]
fn every_route_disarms_the_vagrant_redirects() {
    // Both routes hand the script a shell that may already
    // carry the operator's exported variables. `sh -c` is a
    // child of bombyx and inherits its whole environment.
    // Over `ssh` bombyx's own environment stays behind, but
    // the VM host builds one of its own: `pam_env` applies
    // `/etc/environment` to a non-interactive command, and
    // `zsh` sources `~/.zshenv` on every invocation, and
    // a `bash` export above the non-interactive return
    // guard in `~/.bashrc` survives. Either way
    // three vagrant variables override the directory the
    // script just `cd`'d into, so `destroy` would test
    // `[ -f Vagrantfile ]` in one project and destroy the
    // machine defined in another.
    for route in [cfg(), local_cfg()] {
        for c in [
            vagrant(&route, &["status"], Tty::NoPty),
            destroy_vm_if_present(&route, "~/vms/p", Tty::NoPty),
            save_snapshot(&route, "~/vms/p", Tty::NoPty),
            save_snapshot_if_absent(&route, "~/vms/p", Tty::NoPty),
            ensure_dir(&route, "~/vms"),
            write_file(&route, "~/vms", "Vagrantfile", b"x\n"),
            vagrant_status_many(&route, &[]),
        ] {
            // The prefix alone, not the whole script.
            // `vagrant_command` writes `PROVIDER_ENV` into
            // the same string, so a search over everything
            // would find that assignment and report the
            // variable disarmed with the `unset` gone.
            let raw = raw_script(&c);
            let prefix = raw
                .split_once("; ")
                .expect("the script carries the unset prefix")
                .0;
            assert!(prefix.starts_with("unset "), "{raw}");
            for var in [
                "VAGRANT_CWD",
                "VAGRANT_VAGRANTFILE",
                "VAGRANT_DOTFILE_PATH",
                PROVIDER_ENV,
                "VAGRANT_PREFERRED_PROVIDERS",
            ] {
                assert!(prefix.contains(var), "{var} not disarmed: {prefix}");
            }
        }
    }
}

#[test]
fn the_local_route_asks_for_no_pty() {
    // `-t` is an `ssh` option. `sh -c` inherits whatever
    // stdio bombyx itself was given, so an interactive
    // `bombyx shell` still gets the operator's terminal and
    // there is nothing to request.
    for c in [
        vagrant(&local_cfg(), &["status"], Tty::Allocate),
        destroy_vm_if_present(&local_cfg(), "~/vms/p", Tty::Allocate),
        shell_into_vm(&local_cfg()),
    ] {
        assert_eq!(c.program, "sh");
        assert!(!c.args.iter().any(|a| a == "-t"), "{:?}", c.args);
    }
}

#[test]
fn a_tty_run_asks_for_a_pty_and_silences_the_closing_notice() {
    // Order matters to ssh: options come before the destination,
    // and everything after it is the remote command, so `-t`
    // landing after the host would be handed to the remote
    // shell instead.
    //
    // `LogLevel=ERROR` is measured, not decoration: a tty
    // session makes ssh print `Connection to <host> closed.` to
    // stderr, which would end every status and up with a
    // spurious line. A genuine failure still reports at this
    // level.
    let c = vagrant(&cfg(), &["status"], Tty::Allocate);
    assert_eq!(c.program, "ssh");
    assert_eq!(opts_before_host(&c), vec!["-t", "-o", "LogLevel=ERROR"]);
    assert!(remote_script(&c).contains("vagrant 'status'"));
}

#[test]
fn no_tty_passes_no_options_at_all() {
    // The default for a pipe or a redirect: the remote's bytes
    // arrive unchanged, which is what a captured log needs, and
    // ssh emits no pseudo-terminal warning.
    let c = vagrant(&cfg(), &["status"], Tty::NoPty);
    assert!(opts_before_host(&c).is_empty(), "{:?}", c.args);
    assert_eq!(c.args.len(), 2);
}

#[test]
fn the_tty_choice_does_not_disturb_the_remote_script() {
    // Only the argv ahead of the host differs. If the script
    // itself changed with the tty, the printed plan and the
    // executed one would describe different work.
    let with = vagrant(&cfg(), &["status"], Tty::Allocate);
    let without = vagrant(&cfg(), &["status"], Tty::NoPty);
    assert_eq!(remote_script(&with), remote_script(&without));
}

#[test]
fn an_interactive_shell_always_gets_a_tty() {
    // Unconditional here, unlike every other vagrant call:
    // `vagrant ssh` needs a TTY through a non-interactive SSH
    // command, and a shell without one is unusable whatever the
    // local stdio looks like.
    let c = shell_into_vm(&cfg());
    assert_eq!(opts_before_host(&c), vec!["-t", "-o", "LogLevel=ERROR"]);
}

#[test]
fn an_interactive_shell_starts_in_the_project_clone() {
    // `vagrant ssh` logs in as the box's own account, and the
    // clone belongs to the agent's, so the guest switches
    // with `sudo -u` first. The clone is `$HOME/<project>` of
    // that account; `|| cd` falls back to its home when the
    // clone is missing, so the operator still gets a shell to
    // look into why. The project travels as an argument
    // rather than inside the script, so the script is the
    // same text for every project. The whole guest command is
    // one argument, quoted once more for the VM host.
    let guest = "if id -u 'agent' >/dev/null 2>&1; \
                     then exec sudo -u 'agent' -H -- sh -c \
                     'cd \"$HOME/$1\" || cd; exec \"$SHELL\" -l' \
                     sh 'myproject'; \
                     else echo \"bombyx: this guest has no account \
                     'agent', so it was never provisioned for it; \
                     run provision for this project, or destroy, \
                     then up, if provisioning refuses. Opening a \
                     shell as $(id -un) instead.\" >&2; \
                     exec \"$SHELL\" -l; fi";
    let c = shell_into_vm(&cfg());
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {} vagrant 'ssh' '-c' {}",
            vagrant_env(),
            shell_quote(guest)
        )
    );
}

#[test]
fn the_shell_opens_where_the_bootstrap_script_clones() {
    // Two spellings of one path, in two files that cannot see
    // each other: `bootstrap.sh` clones into
    // `$HOME/$BOMBYX_PROJECT`, and the shell `cd`s into
    // `$HOME/$1` with the project as `$1`. A change to either
    // would open the shell outside the clone and fail nothing
    // else.
    assert!(
        crate::vagrantfile::BOOTSTRAP.contains(
            "readonly CLONE_DIR=\"$HOME/${BOMBYX_PROJECT:-project}\""
        ),
        "bootstrap.sh no longer clones into $HOME/<project>"
    );
    let script = remote_script(&shell_into_vm(&cfg()));
    assert!(script.contains(r#"cd "$HOME/$1""#), "{script}");
    assert!(script.contains("sh '\\''myproject'\\''"), "{script}");
}

#[test]
fn a_windows_shell_logs_in_as_the_agent_in_one_hop() {
    // A Windows guest has no `sudo -u` that keeps the terminal, and
    // a second SSH login inside the guest splits an arrow key's
    // escape sequence (#182). So the VM host logs in as the agent
    // itself, with the machine key vagrant uses, which account.ps1
    // authorizes for the agent. `vagrant ssh -- -l` cannot do it,
    // because vagrant's own user wins; plain ssh with vagrant's
    // config can. A probe that cannot log in says why it might have
    // failed and stops, rather than open another account's shell.
    let mut cfg = cfg();
    cfg.vm.guest = crate::config::Guest::Windows;
    let c = shell_into_vm(&cfg);
    assert_eq!(opts_before_host(&c), vec!["-t", "-o", "LogLevel=ERROR"]);
    let encoded = crate::powershell::encoded_command(
        "Set-Location -LiteralPath (Join-Path $env:USERPROFILE 'myproject')",
    );
    let env = vagrant_env();
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && c=$(mktemp) && \
             trap 'rm -f \"$c\"' EXIT && trap 'exit 129' HUP && \
             trap 'exit 130' INT && trap 'exit 143' TERM && {{ \
             VAGRANT_CHECKPOINT_DISABLE=1 \
             {env} vagrant 'ssh-config' '--host' 'guest' > \"$c\" || exit; \
             ssh -n -F \"$c\" -o BatchMode=yes -l 'agent' guest exit; \
             rc=$?; if [ \"$rc\" = 0 ]; then \
             ssh -F \"$c\" -t -l 'agent' guest powershell.exe -NoLogo \
             -NoExit -EncodedCommand {encoded}; \
             else \
             printf 'bombyx: could not log in to the guest as %s: %s\\n' \
             'agent' {advice} >&2; exit \"$rc\"; fi; }}",
            advice = shell_quote(WINDOWS_SHELL_ADVICE),
        )
    );
}

/// How long a Windows guest command may be once vagrant has
/// wrapped it. vagrant 2.4.9's `ssh_run.rb` prefixes the text and
/// encodes it as UTF-16LE base64 for `powershell -encodedCommand`,
/// and the guest's sshd runs that through `cmd.exe`, whose
/// documented limit is 8191 characters; this leaves room for the
/// `cmd.exe /c` around it.
const WINDOWS_COMMAND_BUDGET: usize = 7800;

/// The length of `text` as vagrant sends it to a Windows guest.
fn sent_by_vagrant(text: &str) -> usize {
    let text = format!("$ProgressPreference = \"SilentlyContinue\"; {text}");
    let utf16: Vec<u8> =
        text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -encodedCommand {}",
        crate::powershell::base64(&utf16)
    )
    .len()
}

/// A Windows config with the longest names the config accepts, so
/// a length test measures the worst case rather than the fixture.
fn longest_windows_cfg() -> Config {
    let user =
        crate::config::GuestUser::parse(&"a".repeat(20)).expect("a plain name");
    assert!(user.windows_refusal().is_none());
    let longer =
        crate::config::GuestUser::parse(&"a".repeat(21)).expect("a plain name");
    assert!(longer.windows_refusal().is_some(), "20 is the limit");
    let mut cfg = cfg();
    cfg.vm.guest = crate::config::Guest::Windows;
    cfg.vm.guest_user = user;
    cfg.project =
        crate::name::ProjectName::parse(&"a".repeat(crate::name::MAX_NAME_LEN))
            .expect("a name at the limit");
    cfg
}

#[test]
fn the_longest_windows_refresh_command_fits_the_guest_command_line() {
    // The hook path is the one long value the call carries, so it
    // is at `MAX_WINDOWS_HOOK_LEN`, the most any Windows project may
    // name. The longest names here would leave a real hook less
    // room, so this is an upper bound.
    let cfg = longest_windows_cfg();
    let limit = crate::config::MAX_WINDOWS_HOOK_LEN;
    let hook =
        HookPath::parse(&format!("{}.ps1", "a".repeat(limit - ".ps1".len())))
            .expect("a hook path at the cap");
    let text = windows::refresh_command(
        &cfg,
        GuestHomeFile::Credential.path(),
        Some((&hook, HOOK_TIMEOUT_SECS)),
    );
    let sent = sent_by_vagrant(&text);
    assert!(
        sent <= WINDOWS_COMMAND_BUDGET,
        "{sent} characters, over the {WINDOWS_COMMAND_BUDGET} budget"
    );
}

#[test]
fn an_interactive_shell_opens_as_the_configured_account() {
    let mut cfg = cfg();
    cfg.vm.guest_user =
        crate::config::GuestUser::parse("dev").expect("a plain name");
    let script = remote_script(&shell_into_vm(&cfg));
    assert!(script.contains("sudo -u '\\''dev'\\''"), "{script}");
}

#[test]
fn a_refresh_writes_the_file_as_the_agent_in_its_home() {
    // The agent's own account writes the file, as in
    // `account.sh`'s `place`; the two `sh` tests below hold what
    // `REFRESH_SCRIPT` does. The name travels as `$1`, so the
    // script is one text for both files. A guest without the
    // account says so and fails rather than writing anywhere
    // else.
    let guest = format!(
        "if id -u 'agent' >/dev/null 2>&1; \
             then exec sudo -u 'agent' -H -- sh -c {} sh '.bombyx-env'; \
             else echo \"bombyx: this guest has no account \
             'agent', so it was never provisioned for it; \
             run provision for this project.\" >&2; exit 1; fi",
        shell_quote(REFRESH_SCRIPT)
    );
    let c = refresh_in_guest(&cfg(), GuestHomeFile::Secrets, b"K=v\n");
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {} vagrant 'ssh' '--no-tty' \
                 '-c' {}",
            vagrant_env(),
            shell_quote(&guest)
        )
    );
}

/// The text a Windows refresh sends: the prefix lines naming the
/// helper's arguments, then `refresh-call.ps1`'s code, encoded.
/// No hook goes with a timeout of 0, which nothing reads.
fn windows_refresh_text(file: &str, hook: &str, timeout: u32) -> String {
    let script = format!(
        "$Interface = {}\n$User = 'agent'\n$File = '{file}'\n\
             $Project = 'myproject'\n$Hook = '{hook}'\n\
             $Timeout = {timeout}\n{}",
        windows::HELPER_CALL,
        crate::powershell::code_lines(windows::REFRESH_CALL)
    );
    crate::powershell::run_encoded(&script)
}

#[test]
fn a_windows_refresh_calls_the_installed_helper_with_the_file_on_stdin() {
    // The helper is too long to carry on the command line, so
    // provisioning installs it and this names it. The file travels
    // on stdin, with no terminal on either hop, as on Linux.
    let mut cfg = cfg();
    cfg.vm.guest = crate::config::Guest::Windows;
    for (file, bytes) in [
        (GuestHomeFile::Secrets, &b"K=v\r\n"[..]),
        (GuestHomeFile::Credential, &b"https://x:t@h\n"[..]),
    ] {
        let c = refresh_in_guest(&cfg, file, bytes);
        assert!(opts_before_host(&c).is_empty(), "no terminal");
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && {} vagrant 'ssh' '--no-tty' \
                     '-c' {}",
                vagrant_env(),
                shell_quote(&windows_refresh_text(file.path(), "", 0))
            )
        );
        let stdin = c.stdin.as_ref().expect("the file is on stdin");
        assert_eq!(stdin.bytes(), bytes);
        assert_eq!(stdin.size_may_be_shown(), !file.hides_size());
    }
}

#[test]
fn a_windows_refresh_with_a_hook_names_it_for_the_helper() {
    let mut cfg = cfg();
    cfg.vm.guest = crate::config::Guest::Windows;
    let hook = HookPath::parse(".bombyx/refreshed.ps1").expect("a path");
    let secrets = Secrets::for_tests(b"K=v\n");
    let c = refresh_secrets_then_hook(&cfg, &secrets, &hook);
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {} vagrant 'ssh' '--no-tty' '-c' {}",
            vagrant_env(),
            shell_quote(&windows_refresh_text(
                ".bombyx-env",
                ".bombyx/refreshed.ps1",
                60
            ))
        )
    );
    let stdin = c.stdin.as_ref().expect("the file is on stdin");
    assert_eq!(stdin.bytes(), b"K=v\n");
}

/// Runs [`REFRESH_SCRIPT`] under `sh` with `HOME` at `home`,
/// feeding it `input` for `file`, and returns whether it
/// succeeded.
///
/// Unix only, like its callers: the script needs a POSIX `sh`,
/// and a helper nothing calls is a dead-code error.
#[cfg(unix)]
fn run_refresh_script_for(
    home: &std::path::Path,
    file: GuestHomeFile,
    input: &[u8],
) -> bool {
    use std::io::Write as _;
    let mut child = std::process::Command::new("sh")
        .args(["-c", REFRESH_SCRIPT, "sh", file.path()])
        .env("HOME", home)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("sh starts");
    child
        .stdin
        .take()
        .expect("a piped stdin")
        .write_all(input)
        .expect("the script reads its input");
    child.wait().expect("sh finishes").success()
}

/// [`run_refresh_script_for`] the secrets file.
#[cfg(unix)]
fn run_refresh_script(home: &std::path::Path, input: &[u8]) -> bool {
    run_refresh_script_for(home, GuestHomeFile::Secrets, input)
}

#[cfg(unix)]
#[test]
fn a_refresh_creates_the_ssh_directory_a_key_needs_at_mode_700() {
    // A guest provisioned before its config named a key has no
    // `~/.ssh` for the agent, and `ssh` refuses a directory other
    // accounts can write.
    use std::os::unix::fs::PermissionsExt as _;
    let home = tempfile::tempdir().expect("a temp dir");
    let key = b"-----BEGIN OPENSSH PRIVATE KEY-----\n";
    assert!(run_refresh_script_for(
        home.path(),
        GuestHomeFile::DeployKey,
        key
    ));
    let target = home.path().join(GuestHomeFile::DeployKey.path());
    assert_eq!(std::fs::read(&target).expect("the key"), key);
    let mode = |p: &std::path::Path| {
        std::fs::metadata(p).expect("meta").permissions().mode() & 0o777
    };
    assert_eq!(mode(&home.path().join(".ssh")), 0o700);
    assert_eq!(mode(&target), 0o600);
}

#[cfg(unix)]
#[test]
fn a_refresh_replaces_the_file_whole_at_mode_600() {
    use std::os::unix::fs::PermissionsExt as _;
    let home = tempfile::tempdir().expect("a temp dir");
    let target = home.path().join(".bombyx-env");
    std::fs::write(&target, "OLD=1\n").expect("an earlier copy");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))
        .expect("loosened by the agent");
    assert!(run_refresh_script(home.path(), b"NEW=2\n"));
    assert_eq!(std::fs::read(&target).expect("the file"), b"NEW=2\n");
    let mode = std::fs::metadata(&target)
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "mode {mode:o}");
    let left: Vec<_> = std::fs::read_dir(home.path())
        .expect("the home")
        .map(|e| e.expect("an entry").file_name())
        .collect();
    assert_eq!(left, [".bombyx-env"], "a temporary file was left");
}

#[cfg(unix)]
#[test]
fn a_refresh_that_cannot_write_keeps_the_copy_it_had() {
    // A write that fails in the guest -- a full disk, here a
    // directory where the new file would go -- must leave the
    // guest's working copy alone, because `bombyx shell` opens
    // anyway and the secrets it had must still work there.
    let home = tempfile::tempdir().expect("a temp dir");
    let target = home.path().join(".bombyx-env");
    std::fs::write(&target, "OLD=1\n").expect("an earlier copy");
    std::fs::create_dir(home.path().join(".bombyx-env.new"))
        .expect("a directory in the way");
    assert!(!run_refresh_script(home.path(), b"NEW=2\n"));
    assert_eq!(std::fs::read(&target).expect("the file"), b"OLD=1\n");
}

#[test]
fn a_refresh_sends_the_file_down_a_pipe_with_no_terminal() {
    // A terminal on either hop would put the pipe through a line
    // discipline, which can echo the input back and rewrite its
    // line endings. So neither `ssh` nor `vagrant ssh` asks for
    // one, and the contents are the command's standard input,
    // never an argument.
    let secret = b"JIRA_TOKEN=s3cr3t-value\r\n";
    let c = refresh_in_guest(&cfg(), GuestHomeFile::Secrets, secret);
    assert!(opts_before_host(&c).is_empty(), "{:?}", c.args);
    assert!(raw_script(&c).contains("'--no-tty'"), "{c}");
    let stdin = c.stdin.as_ref().expect("the file is on stdin");
    assert_eq!(stdin.bytes(), secret);
    assert!(stdin.size_may_be_shown());
    assert!(
        c.args.iter().all(|a| !a.contains("s3cr3t")),
        "the secret reached an argument: {:?}",
        c.args
    );
}

/// A guest home for [`run_hook_script`]: a clone at `proj`, and
/// a `BASH_ENV` file that leaves a marker if any shell reads it.
#[cfg(target_os = "linux")]
fn hook_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("a temp dir");
    std::fs::create_dir(home.path().join("proj")).expect("a clone");
    std::fs::write(
        home.path().join("bash_env.sh"),
        "touch \"$HOME/bash_env_ran\"\n",
    )
    .expect("a BASH_ENV file");
    home
}

/// Writes `body` as the hook at `rel` inside the clone.
#[cfg(target_os = "linux")]
fn write_hook(home: &std::path::Path, rel: &str, body: &str) {
    let path = home.join("proj").join(rel);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the hook's directory");
    }
    std::fs::write(path, body).expect("the hook");
}

/// Runs `REFRESH_THEN_HOOK_SCRIPT` under `sh` as the guest
/// would: `HOME` at `home`, the payload `input` on standard
/// input, the clone `proj`, the hook `hook` and a timeout of
/// `timeout` seconds. Returns the exit status, and standard
/// output followed by standard error.
///
/// The caller's environment carries `BASH_ENV` and a stray
/// variable, the two things the hook must not see.
///
/// Linux only: the guest is Linux, and the script needs GNU
/// `timeout` and `readlink -f`, which macOS does not ship.
#[cfg(target_os = "linux")]
fn run_hook_script(
    home: &std::path::Path,
    input: &[u8],
    hook: &str,
    timeout: &str,
) -> (Option<i32>, String) {
    use std::io::Write as _;
    let mut child = std::process::Command::new("sh")
        .args([
            "-c",
            REFRESH_THEN_HOOK_SCRIPT,
            "sh",
            ".bombyx-env",
            "proj",
            hook,
            timeout,
        ])
        .env("HOME", home)
        .env("BASH_ENV", home.join("bash_env.sh"))
        .env("BOMBYX_LEAK", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("sh starts");
    child
        .stdin
        .take()
        .expect("a piped stdin")
        .write_all(input)
        .expect("the script reads its input");
    let out = child.wait_with_output().expect("sh finishes");
    let mut printed = String::from_utf8_lossy(&out.stdout).into_owned();
    printed.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code(), printed)
}

#[cfg(target_os = "linux")]
#[test]
fn the_hook_runs_in_the_clone_from_a_clean_environment_after_the_write() {
    // What the hook sees is its contract: the clone as its
    // working directory, four named variables and nothing the
    // calling shell carried, input from nowhere, and an
    // ordinary umask -- and the file already rewritten.
    let home = hook_home();
    write_hook(
        home.path(),
        ".bombyx/refresh.sh",
        "{ pwd; umask; [ -c /dev/stdin ] && echo stdin-null; \
             cat \"$BOMBYX_ENV_FILE\"; env; } > \"$HOME/seen\"\n",
    );
    let (code, err) =
        run_hook_script(home.path(), b"NEW=2\n", ".bombyx/refresh.sh", "10");
    assert_eq!(code, Some(0), "{err}");
    let seen = std::fs::read_to_string(home.path().join("seen")).expect("ran");
    let clone =
        std::fs::canonicalize(home.path().join("proj")).expect("the clone");
    let mut lines = seen.lines();
    assert_eq!(lines.next(), Some(clone.to_str().expect("utf-8")));
    assert_eq!(lines.next(), Some("0022"), "{seen}");
    assert_eq!(lines.next(), Some("stdin-null"), "{seen}");
    assert_eq!(lines.next(), Some("NEW=2"), "the hook ran before the write");
    let names: std::collections::BTreeSet<&str> =
        lines.filter_map(|l| l.split('=').next()).collect();
    for want in ["HOME", "PATH", "BOMBYX_PROJECT", "BOMBYX_ENV_FILE"] {
        assert!(names.contains(want), "{want} missing: {seen}");
    }
    for leaked in ["BASH_ENV", "BOMBYX_LEAK"] {
        assert!(!names.contains(leaked), "{leaked} leaked: {seen}");
    }
    assert!(seen.contains("BOMBYX_PROJECT=proj\n"), "{seen}");
    assert!(
        !home.path().join("bash_env_ran").exists(),
        "a shell read BASH_ENV"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_hook_that_fails_is_reported_whatever_status_it_chose() {
    // A hook exiting 1 must not read as a failed write, and one
    // exiting 90 must not read as a refusal: the guest maps
    // every non-zero status to one of its own. 124 and 137 are
    // the exceptions, because `timeout` uses them for a hook it
    // stopped, and `REFRESH_THEN_HOOK_SCRIPT` says so.
    for status in [1, 2, HOOK_REFUSED, HOOK_FAILED, 125] {
        let home = hook_home();
        write_hook(home.path(), "h.sh", &format!("exit {status}\n"));
        let (code, err) =
            run_hook_script(home.path(), b"NEW=2\n", "h.sh", "10");
        assert_eq!(code, Some(HOOK_FAILED), "{status}: {err}");
        assert!(err.contains(&format!("exited {status}")), "{err}");
        let file =
            std::fs::read(home.path().join(".bombyx-env")).expect("the file");
        assert_eq!(file, b"NEW=2\n", "the write must stand");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_hook_the_guest_cannot_find_is_refused_after_the_write() {
    // A missing file, a directory, and no clone at all: each is
    // a refusal, never a silent skip, and the secrets stay
    // written because the hook is the only part that failed.
    type Arrange = fn(&std::path::Path);
    let cases: [(&str, Arrange); 3] = [
        ("nothing there", |_| {}),
        ("a directory", |h| {
            std::fs::create_dir(h.join("proj/h.sh")).expect("a dir");
        }),
        ("no clone", |h| {
            std::fs::remove_dir(h.join("proj")).expect("no clone");
        }),
    ];
    for (case, arrange) in cases {
        let home = hook_home();
        arrange(home.path());
        let (code, err) =
            run_hook_script(home.path(), b"NEW=2\n", "h.sh", "10");
        assert_eq!(code, Some(HOOK_REFUSED), "{case}: {err}");
        assert!(err.contains("did not run") || err.contains("no "), "{err}");
        let file =
            std::fs::read(home.path().join(".bombyx-env")).expect("the file");
        assert_eq!(file, b"NEW=2\n", "{case}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_hook_a_symlink_leads_out_of_the_clone_is_not_run() {
    // The config refuses `..` and an absolute path; a symlink in
    // the repository is what only the guest can see. Both
    // shapes: the file itself a link, and a linked directory
    // above it.
    for shape in ["file", "dir"] {
        let home = hook_home();
        let outside = home.path().join("outside");
        std::fs::create_dir(&outside).expect("outside");
        std::fs::write(outside.join("h.sh"), "touch \"$HOME/outside_ran\"\n")
            .expect("an outside script");
        let (link, target, hook) = match shape {
            "file" => ("proj/h.sh", outside.join("h.sh"), "h.sh"),
            _ => ("proj/sub", outside.clone(), "sub/h.sh"),
        };
        std::os::unix::fs::symlink(target, home.path().join(link))
            .expect("a link");
        let (code, err) = run_hook_script(home.path(), b"NEW=2\n", hook, "10");
        assert_eq!(code, Some(HOOK_REFUSED), "{shape}: {err}");
        assert!(err.contains("outside the clone"), "{err}");
        assert!(
            !home.path().join("outside_ran").exists(),
            "{shape}: the outside script ran"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_write_that_fails_runs_no_hook() {
    // The hook copies `~/.bombyx-env` somewhere else, so running
    // it after a failed write would copy the old secrets and
    // report the refresh as done.
    let home = hook_home();
    write_hook(home.path(), "h.sh", "touch \"$HOME/hook_ran\"\n");
    std::fs::create_dir(home.path().join(".bombyx-env.new"))
        .expect("a directory in the way");
    let (code, err) = run_hook_script(home.path(), b"NEW=2\n", "h.sh", "10");
    assert_eq!(code, Some(1), "{err}");
    assert!(!home.path().join("hook_ran").exists(), "the hook ran");
}

#[cfg(target_os = "linux")]
#[test]
fn a_hook_that_runs_too_long_is_stopped() {
    let home = hook_home();
    write_hook(home.path(), "h.sh", "sleep 30\n");
    let started = std::time::Instant::now();
    let (code, err) = run_hook_script(home.path(), b"NEW=2\n", "h.sh", "1");
    assert_eq!(code, Some(HOOK_TIMED_OUT), "{err}");
    assert!(err.contains("longer than 1 seconds"), "{err}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "the timeout did not stop it"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_process_the_hook_leaves_running_does_not_hold_the_output_open() {
    // A hook may restart a dev server in the background. Its
    // output goes to a file, not to the pipe bombyx reads, so
    // the command ends when the hook does and `shell` opens.
    let home = hook_home();
    write_hook(home.path(), "h.sh", "sleep 8 &\necho started\n");
    let started = std::time::Instant::now();
    let (code, printed) =
        run_hook_script(home.path(), b"NEW=2\n", "h.sh", "10");
    assert_eq!(code, Some(0), "{printed}");
    assert!(printed.contains("started"), "{printed}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the background process held the output open"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_hook_that_ignores_the_stop_signal_is_reported_as_timed_out() {
    // `timeout -k` kills it after the grace period and then
    // exits 137, not 124; that is still a hook that ran too long.
    let home = hook_home();
    write_hook(home.path(), "h.sh", "trap '' TERM\nsleep 30\n");
    let (code, printed) = run_hook_script(home.path(), b"NEW=2\n", "h.sh", "1");
    assert_eq!(code, Some(HOOK_TIMED_OUT), "{printed}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_hook_that_prints_without_end_is_cut() {
    let home = hook_home();
    write_hook(
        home.path(),
        "h.sh",
        "head -c 200000 /dev/zero | tr '\\0' x\n",
    );
    let (code, printed) =
        run_hook_script(home.path(), b"NEW=2\n", "h.sh", "10");
    assert_eq!(code, Some(0), "{}", &printed[printed.len() - 200..]);
    assert!(printed.len() < 70_000, "{} bytes relayed", printed.len());
    assert!(printed.contains("the rest was dropped"), "not said");
}

#[test]
fn the_hook_travels_in_the_same_guest_command_as_the_write() {
    // One `vagrant ssh`, not two, with the four arguments the
    // script reads as `$1` to `$4`, on the same no-terminal
    // route and with the file on standard input.
    let hook = HookPath::parse(".bombyx/refresh-env.sh").expect("a good hook");
    let secrets = Secrets::for_tests(b"K=v\n");
    let c = refresh_secrets_then_hook(&cfg(), &secrets, &hook);
    let guest = format!(
        "if id -u 'agent' >/dev/null 2>&1; \
             then exec sudo -u 'agent' -H -- sh -c {} sh '.bombyx-env' \
             'myproject' '.bombyx/refresh-env.sh' '{HOOK_TIMEOUT_SECS}'; \
             else echo \"bombyx: this guest has no account \
             'agent', so it was never provisioned for it; \
             run provision for this project.\" >&2; exit 1; fi",
        shell_quote(REFRESH_THEN_HOOK_SCRIPT)
    );
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {} vagrant 'ssh' '--no-tty' \
                 '-c' {}",
            vagrant_env(),
            shell_quote(&guest)
        )
    );
    assert!(opts_before_host(&c).is_empty(), "{:?}", c.args);
    let stdin = c.stdin.as_ref().expect("the file is on stdin");
    assert_eq!(stdin.bytes(), b"K=v\n");
}

#[test]
fn the_script_exits_with_the_hook_statuses() {
    // The script cannot name the constants, so this is what keeps
    // `RefreshOutcome::from_code` and the guest in step. Not
    // platform-gated, unlike the tests that run the script.
    for status in [HOOK_REFUSED, HOOK_FAILED, HOOK_TIMED_OUT] {
        assert!(
            REFRESH_THEN_HOOK_SCRIPT.contains(&format!("exit {status}")),
            "the script never exits {status}"
        );
    }
    for code in 1..=255 {
        let spelt = format!("exit {code};");
        let used = REFRESH_THEN_HOOK_SCRIPT.contains(&spelt)
            || REFRESH_THEN_HOOK_SCRIPT.ends_with(&format!("exit {code}"));
        let known = [0, 1, HOOK_REFUSED, HOOK_FAILED, HOOK_TIMED_OUT];
        assert!(!used || known.contains(&code), "unexpected exit {code}");
    }
}

#[test]
fn the_windows_helpers_speak_refresh_outcome() {
    // The helpers are installed, not rendered, so nothing puts
    // these values into them; this keeps them in step with
    // `RefreshOutcome::from_code` and with the Linux script.
    let mut cfg = cfg();
    cfg.vm.guest = crate::config::Guest::Windows;
    let files = crate::vagrantfile::files(&cfg, &cfg.staged_for_tests());
    let text = |name: &str| {
        files
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| t.clone())
            .expect(name)
    };
    let hook = text("hook.ps1");
    let has_line = crate::powershell::has_line;
    for line in [
        format!("$HookRefused = {HOOK_REFUSED}"),
        format!("$HookFailed = {HOOK_FAILED}"),
        format!("$HookTimedOut = {HOOK_TIMED_OUT}"),
        format!("$OutputCap = {HOOK_OUTPUT_CAP}"),
    ] {
        assert!(has_line(&hook, &line), "hook.ps1: {line}");
    }
    let refresh = text("refresh.ps1");
    for line in [
        format!("$HookRefused = {HOOK_REFUSED}"),
        format!("$HookFailed = {HOOK_FAILED}"),
        format!("$HookTimedOut = {HOOK_TIMED_OUT}"),
    ] {
        assert!(has_line(&refresh, &line), "refresh.ps1: {line}");
    }
    assert!(
        REFRESH_THEN_HOOK_SCRIPT
            .contains(&format!("head -c {HOOK_OUTPUT_CAP} ")),
        "the Linux script's cap"
    );
}

#[test]
fn each_exit_status_reads_as_the_part_that_failed() {
    // 1 is what the write, `sudo`, `vagrant` and `ssh` all use,
    // so it and every other unknown status must read as "the
    // file may not be current". Only the three hook statuses
    // may say the secrets are current.
    use RefreshOutcome as O;
    for (code, want) in [
        (Some(0), O::Done),
        (Some(1), O::WriteFailed),
        (Some(255), O::WriteFailed),
        (None, O::WriteFailed),
        (Some(HOOK_REFUSED), O::HookRefused),
        (Some(HOOK_FAILED), O::HookFailed),
        (Some(HOOK_TIMED_OUT), O::HookTimedOut),
    ] {
        assert_eq!(O::from_code(code), want, "{code:?}");
    }
    assert_eq!(O::Done.message(), None);
    for o in [O::HookRefused, O::HookFailed, O::HookTimedOut] {
        let m = o.message().expect("a hook outcome is reported");
        assert!(m.contains("secrets in the guest are current"), "{m}");
    }
    let m = O::WriteFailed.message().expect("reported");
    assert!(!m.contains("current"), "{m}");
}

#[test]
fn a_refreshed_credential_hides_its_size() {
    // The credential is fixed text plus one token, so its
    // length measures the token; `Stdin` says why that stays out
    // of a dry run. The file picks the form, so no caller can
    // send the credential with its count showing.
    let c =
        refresh_in_guest(&cfg(), GuestHomeFile::Credential, b"https://u:t@h\n");
    let stdin = c.stdin.as_ref().expect("the file is on stdin");
    assert_eq!(stdin.bytes(), b"https://u:t@h\n");
    assert!(!stdin.size_may_be_shown());
    assert!(raw_script(&c).contains(".bombyx-git-credentials"), "{c}");
}

#[test]
fn teardown_takes_a_tty_like_every_other_vagrant_call() {
    // `vagrant destroy -f` streams progress, so without a PTY
    // it staircases on the console this parameter exists to
    // fix.
    let with = destroy_vm_if_present(&cfg(), "~/vms/p", Tty::Allocate);
    assert_eq!(opts_before_host(&with), vec!["-t", "-o", "LogLevel=ERROR"]);
    let without = destroy_vm_if_present(&cfg(), "~/vms/p", Tty::NoPty);
    assert!(opts_before_host(&without).is_empty());
    assert_eq!(remote_script(&with), remote_script(&without));
}

#[test]
fn the_stream_rule_needs_both_streams() {
    // stdin, because ssh needs a local terminal to allocate
    // against and merely warns without one; stdout, because the
    // translation only helps output that reaches a terminal.
    assert_eq!(Tty::for_streams(true, true), Tty::Allocate);
    assert_eq!(Tty::for_streams(true, false), Tty::NoPty);
    assert_eq!(Tty::for_streams(false, true), Tty::NoPty);
    assert_eq!(Tty::for_streams(false, false), Tty::NoPty);
}
fn cfg() -> Config {
    Config::for_tests()
}

/// The identity prefix every vagrant script carries.
///
/// `vagrant_carries_the_vm_host_identity` spells it out in
/// full. Everything else references this or [`vagrant_env`],
/// because their subject is the directory and the arguments;
/// repeating the prefix in each of them would push every
/// assertion past the line limit and give it several places
/// to drift.
///
/// Built from the exported constants rather than hardcoding
/// their values. Hardcoded, renaming either constant would
/// leave this module green while bombyx exported a different
/// variable name -- which is the one failure these assertions
/// exist to catch.
fn vm_env() -> String {
    format!("{VM_HOST_ENV}='vmhost' {VM_HOSTNAME_ENV}=$(hostname -s)")
}

/// The whole prefix on every vagrant call: the identity and
/// the provider.
///
/// [`vm_env`] is the identity half alone, which is what the
/// assertions about the guest's two names use.
/// `every_other_project_vagrant_call_names_the_configured_provider`
/// and `every_teardown_destroys_under_the_provider_it_finds_recorded`,
/// both in `plan`, hold the provider half across the actions:
/// the configured provider everywhere but the teardown, and
/// the recorded one there, then one unnamed fallback.
///
/// The provider is read back from the test config rather
/// than spelled out, for the reason [`vm_env`] gives about
/// the variable names: what these assertions are about is
/// that the configured provider reaches the script, not
/// that the word `libvirt` appears in it.
fn vagrant_env() -> String {
    format!("{} {PROVIDER_ENV}='{}'", vm_env(), cfg().vm.provider)
}

#[test]
fn vagrant_names_the_provider_the_config_asks_for() {
    // The generated Vagrantfile *configures* a provider, and
    // configuring one does nothing unless vagrant independently
    // picks it -- so bombyx names it. Without that, a hyperv
    // project on a libvirt-only host boots a libvirt machine at
    // vagrant's defaults, the `:hyperv` settings block never
    // applying and nothing reporting the substitution.
    let mut cfg = cfg();
    cfg.vm.provider = crate::config::Provider::Hyperv;
    let script = remote_script(&vagrant(&cfg, &["up"], Tty::NoPty));
    assert!(
        script.contains(&format!("{PROVIDER_ENV}='hyperv'")),
        "{script}"
    );
}

#[test]
fn vagrant_carries_the_vm_host_identity() {
    // The guest cannot work out which machine it runs on:
    // there is no synced folder, `hostname` inside the VM
    // answers with the guest's own name, and libvirt puts
    // nothing about the host anywhere a non-root process can
    // read. So the two names ride in on the one command that
    // crosses the boundary, and the guest's provisioning
    // writes them down.
    let c = vagrant(&cfg(), &["up"], Tty::NoPty);
    assert_eq!(
        remote_script(&c),
        "cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' \
             BOMBYX_VM_HOSTNAME=$(hostname -s) \
             VAGRANT_DEFAULT_PROVIDER='libvirt' vagrant 'up'"
    );
}

#[test]
fn the_hostname_is_evaluated_on_the_far_side() {
    // `$(...)` in a remote command is the wrong-side
    // expansion trap: expanded here it would report the
    // *workstation's* name, which is plausible enough that
    // nobody would question it. bombyx spawns `ssh` directly
    // rather than through a shell, so the substitution
    // reaches the host verbatim -- and the dry run has to
    // show it escaped, or a pasted line would answer with
    // the wrong machine.
    let c = vagrant(&cfg(), &["up"], Tty::NoPty);
    assert!(
        remote_script(&c).contains("$(hostname -s)"),
        "{}",
        remote_script(&c)
    );
    assert!(c.to_string().contains(r"\$(hostname -s)"), "{c}");
}

#[test]
fn teardown_carries_the_identity_too() {
    // Pins the teardown builder, which builds its command
    // with `vagrant_command_as` rather than `vagrant_script`;
    // `vagrant_command` says why.
    //
    // It matters most here. Teardown still evaluates the
    // project's Vagrantfile, so one reading the variable
    // without a default would raise on `destroy` after working
    // on `up`, and the directory removal that follows would
    // never run.
    //
    // Exhaustiveness across actions is asserted in `plan`,
    // which can enumerate them.
    let script = destroy_vm_if_present(&cfg(), "~/vms/myproject", Tty::NoPty)
        .args[1]
        .clone();
    assert!(script.contains(&vm_env()), "{script}");
}

#[test]
fn the_vm_host_alias_is_quoted_in_the_script() {
    // The alias is interpolated into a remote script, so it
    // goes through `shell_quote` rather than being trusted
    // because `config::host` checked it. A hostile alias
    // cannot be assigned to a `HostName`, so what this
    // asserts is the wiring: delete the `shell_quote` call
    // in `vm_host_env` and the quotes go missing here.
    // `remote::quote` tests what quoting does to a value
    // that needs it.
    let script = remote_script(&vagrant(&cfg(), &["up"], Tty::NoPty));
    assert!(script.contains("BOMBYX_VM_HOST='vmhost'"), "{script}");
}

#[test]
fn builds_a_vagrant_command() {
    let c = vagrant(&cfg(), &["up"], Tty::NoPty);
    let env = vagrant_env();
    assert_eq!(c.program, "ssh");
    assert_eq!(c.args[0], "vmhost");
    assert_eq!(
        remote_script(&c),
        format!("cd ~/'vms/myproject' && {env} vagrant 'up'")
    );
}

#[test]
fn builds_a_vagrant_command_with_several_args() {
    let c = vagrant(
        &cfg(),
        &["snapshot", "restore", "fresh-install"],
        Tty::NoPty,
    );
    let env = vagrant_env();
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {env} vagrant 'snapshot' \
                 'restore' 'fresh-install'"
        )
    );
}

#[test]
fn builds_a_scratch_command() {
    let cfg = cfg();
    let name = ScratchName::parse("pr-1234").unwrap();
    let c =
        vagrant_in(&cfg, &cfg.remote_scratch_dir(&name), &["halt"], Tty::NoPty);
    // `halt` rather than `destroy`: a teardown goes through
    // `destroy_vm_if_present`, never through `vagrant_in`.
    let env = vagrant_env();
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/scratch/myproject/pr-1234' && {env} \
                 vagrant 'halt'"
        )
    );
}

#[test]
fn vagrant_runs_in_the_project_dir() {
    // `vagrant up` reads the Vagrantfile from the directory
    // it runs in, so the command has to cd there first.
    let cfg = cfg();
    let quoted = quote_remote_path(&cfg.remote_project_dir());
    assert!(
        remote_script(&vagrant(&cfg, &["up"], Tty::NoPty))
            .starts_with(&format!("cd {quoted} &&"))
    );
}

#[test]
fn ensure_dir_keeps_the_tilde_expandable() {
    let c = ensure_dir(&cfg(), "~/vms/scratch/pr-1");
    assert_eq!(remote_script(&c), "mkdir -p ~/'vms/scratch/pr-1'");
}

#[test]
fn ensure_dir_quotes_an_absolute_dir() {
    let c = ensure_dir(&cfg(), "/srv/vms/p");
    assert_eq!(remote_script(&c), "mkdir -p '/srv/vms/p'");
}

#[test]
fn remove_dir_quotes_the_path_and_keeps_the_tilde() {
    let c = remove_dir(&cfg(), "~/vms/myproject");
    assert_eq!(c.program, "ssh");
    assert_eq!(c.args[0], "vmhost");
    assert_eq!(remote_script(&c), "rm -rf ~/'vms/myproject'");
}

#[test]
fn remove_dir_removes_an_absolute_path() {
    let c = remove_dir(&cfg(), "/srv/vms/myproject");
    assert_eq!(remote_script(&c), "rm -rf '/srv/vms/myproject'");
}

#[test]
fn remove_dir_quotes_injection_in_the_path() {
    // Config rejects these characters, so this is the
    // second line of defence rather than the first.
    let c = remove_dir(&cfg(), "~/vms/a b; rm /");
    assert_eq!(remote_script(&c), "rm -rf ~/'vms/a b; rm /'");
}

#[test]
fn the_teardown_script_is_spelled_exactly() {
    // Pins the whole teardown script: the Vagrantfile guard,
    // the provider-named branches, the fallback and the
    // refusal. The guard is there because an `up`
    // interrupted before the Vagrantfile write leaves the
    // directory made but empty, a bare `vagrant destroy -f`
    // fails there, and the failure would stop the removal
    // that follows. The `run_teardown` tests exercise what
    // each part does.
    let c = destroy_vm_if_present(&cfg(), "~/vms/myproject", Tty::NoPty);
    let env = vm_env();
    let id = |p| shell_quote(&recorded_machine_id(p));
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && if [ -f Vagrantfile ]; then \
                 if [ -f {} ]; then {env} {PROVIDER_ENV}='libvirt' \
                 vagrant 'destroy' '-f'; \
                 elif [ -f {} ]; then {env} {PROVIDER_ENV}='hyperv' \
                 vagrant 'destroy' '-f'; \
                 elif {ANY_RECORDED_MACHINE}; then {env} \
                 vagrant 'destroy' '-f'; fi; \
                 if {ANY_RECORDED_MACHINE}; then printf 'bombyx: %s \
                 still records a machine under %s/.vagrant/machines, \
                 so the directory stays; destroy that machine by \
                 hand\\n' 'vmhost' \"$PWD\" >&2; exit 1; fi; fi",
            id(Provider::Libvirt),
            id(Provider::Hyperv),
        )
    );
}

/// Runs the teardown script under `sh` in a fresh project
/// directory and returns what a fake `vagrant` was called
/// with, one line per call: the provider it saw, then its
/// arguments. `Ok` when the script exited 0, `Err` when it
/// refused.
///
/// The fake does to the record what vagrant does: it deletes
/// the id of the `default` machine under the provider named,
/// or under any provider when none is named. It never touches
/// another machine name, because vagrant targets only the
/// machines the Vagrantfile defines, and bombyx's defines
/// `default` alone.
///
/// `vagrantfile` decides whether the directory holds a
/// `Vagrantfile`, and `recorded` lists where under
/// `.vagrant/machines` vagrant wrote a machine id, each as
/// `<machine>/<provider>`. The
/// operator's own `VAGRANT_DEFAULT_PROVIDER` is set to a
/// third provider, so a call that saw it proves the `unset`
/// was skipped.
#[cfg(unix)]
fn run_teardown(
    vagrantfile: bool,
    recorded: &[&str],
) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().expect("a temp dir");
    let bin = tmp.path().join("bin");
    let project = tmp.path().join("project");
    let log = tmp.path().join("calls.log");
    std::fs::create_dir_all(&bin).expect("mkdir bin");
    std::fs::create_dir_all(&project).expect("mkdir project");
    let fake = bin.join("vagrant");
    std::fs::write(
        &fake,
        r#"#!/bin/sh
printf '%s %s\n' "${VAGRANT_DEFAULT_PROVIDER-none}" "$*" >> "$LOG"
m=.vagrant/machines/default
if [ -n "${VAGRANT_DEFAULT_PROVIDER-}" ]; then
  rm -f "$m/$VAGRANT_DEFAULT_PROVIDER/id"
else
  rm -f "$m"/*/id
fi
"#,
    )
    .expect("write the fake vagrant");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("chmod the fake vagrant");
    if vagrantfile {
        std::fs::write(project.join("Vagrantfile"), "")
            .expect("write the Vagrantfile");
    }
    for m in recorded {
        // Built by hand rather than with `recorded_machine_id`,
        // because `recorded` can name a machine or a provider
        // bombyx does not use, which no `Provider` spells.
        let id = project.join(format!(".vagrant/machines/{m}/id"));
        std::fs::create_dir_all(id.parent().expect("a parent"))
            .expect("mkdir machine");
        std::fs::write(&id, "some-uuid").expect("write id");
    }

    let dir = project.display().to_string();
    let c = destroy_vm_if_present(&cfg(), &dir, Tty::NoPty);
    let path = std::env::var("PATH").unwrap_or_default();
    let status = std::process::Command::new("sh")
        .args(["-c", &raw_script(&c)])
        .env("PATH", format!("{}:{path}", bin.display()))
        .env("LOG", &log)
        .env(PROVIDER_ENV, "virtualbox")
        .status()
        .expect("sh runs");
    // No log means the fake was never called. Any other read
    // error has to fail, or it would pass the tests that
    // expect no call.
    let calls = match std::fs::read_to_string(&log) {
        Ok(calls) => calls,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => panic!("read the call log: {e}"),
    };
    if status.success() {
        Ok(calls)
    } else {
        Err(calls)
    }
}

#[cfg(unix)]
#[test]
fn the_teardown_names_the_provider_vagrant_recorded() {
    // On a WSL2 host a destroy with no provider named has
    // vagrant probe VirtualBox, which refuses before vagrant
    // reads the machine's record (issue #111). So the
    // teardown names a provider, and names the recorded one
    // rather than the configured one: the test config says
    // libvirt, and a machine recorded under hyperv is still
    // destroyed as hyperv.
    let ok = |calls: &str| Ok(calls.to_owned());
    assert_eq!(
        run_teardown(true, &["default/libvirt"]),
        ok("libvirt destroy -f\n")
    );
    assert_eq!(
        run_teardown(true, &["default/hyperv"]),
        ok("hyperv destroy -f\n")
    );
}

#[cfg(unix)]
#[test]
fn a_default_machine_under_another_provider_gets_the_unnamed_destroy() {
    // A `default` machine vagrant recorded under a provider
    // bombyx does not support is still a machine. The
    // teardown falls back to a destroy naming no provider,
    // and vagrant reads the record.
    //
    // "none" is what the fake prints for an unset variable,
    // so it also proves the operator's exported value was
    // cleared.
    assert_eq!(
        run_teardown(true, &["default/virtualbox"]),
        Ok("none destroy -f\n".to_owned())
    );
}

#[cfg(unix)]
#[test]
fn a_machine_left_recorded_refuses_the_removal() {
    // Vagrant destroys only the machines the Vagrantfile
    // defines, which is `default` alone. A machine recorded
    // under another name survives every destroy, and the
    // removal behind the teardown would then delete its
    // Vagrantfile while it runs. So the script refuses when
    // an id is still recorded afterwards, and `execute` stops
    // before the removal.
    assert_eq!(
        run_teardown(true, &["web/libvirt"]),
        Err("none destroy -f\n".to_owned())
    );
    // The same when `default` goes but a second machine
    // stays.
    assert_eq!(
        run_teardown(true, &["default/libvirt", "web/libvirt"]),
        Err("libvirt destroy -f\n".to_owned())
    );
}

#[cfg(unix)]
#[test]
fn the_teardown_skips_vagrant_when_no_machine_is_recorded() {
    // No machine means nothing for vagrant to destroy, and
    // a vagrant that cannot pick a usable provider would
    // refuse and leave the directory removal behind it
    // unrun.
    assert_eq!(run_teardown(true, &[]), Ok(String::new()));
    // An id with no Vagrantfile beside it is left alone
    // too, because vagrant fails in a directory with no
    // Vagrantfile.
    assert_eq!(run_teardown(false, &["default/libvirt"]), Ok(String::new()));
}

#[test]
fn saving_the_snapshot_replaces_one_that_is_already_there() {
    // `--force` is what makes the command re-takeable. Without
    // it vagrant refuses a name it already holds, exiting 1
    // with `You must include the --force option to replace an
    // existing snapshot.` -- measured on a libvirt host.
    let c = save_snapshot(&cfg(), "~/vms/myproject", Tty::NoPty);
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {} vagrant 'snapshot' 'save' \
                 '-f' 'fresh-install'",
            vagrant_env()
        )
    );
}

#[test]
fn restoring_names_the_snapshot_the_saves_write() {
    // The pairing the three builders exist for, pinned where
    // the shell is spelled. `plan` still has its own test
    // that `reset` is handed this builder and not another.
    let c = restore_snapshot(&cfg(), "~/vms/myproject", Tty::NoPty);
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {} vagrant 'snapshot' \
                 'restore' 'fresh-install'",
            vagrant_env()
        )
    );
}

#[test]
fn the_guarded_save_asks_vagrant_what_it_already_holds() {
    // `up` runs this, and every `up` after the first follows
    // arbitrary use of the machine. Saving only when the name
    // is absent keeps `fresh-install` describing a fresh
    // install.
    //
    // The test is on the listing rather than on vagrant's own
    // refusal because `execute` stops at the first failing
    // step: an unguarded save would make the second `up`
    // report failure.
    let c = save_snapshot_if_absent(&cfg(), "~/vms/myproject", Tty::NoPty);
    let env = vagrant_env();
    assert_eq!(
        remote_script(&c),
        format!(
            "cd ~/'vms/myproject' && {{ names=$({env} vagrant \
                 'snapshot' 'list') && if ! printf '%s\\n' \"$names\" \
                 | grep -qx 'fresh-install'; then {env} vagrant 'snapshot' \
                 'save' 'fresh-install'; fi || printf 'bombyx: could not \
                 save the fresh-install snapshot for %s; re-run this \
                 command with snapshot in place of up\\n' 'myproject' \
                 >&2; }}"
        )
    );
}

#[test]
fn a_listing_that_fails_stops_the_guarded_save() {
    // A shell pipeline reports only its last command's
    // status. Piping the listing straight into `grep` would
    // make a machine vagrant cannot read look exactly like
    // one holding no snapshots. Capturing it and joining with
    // `&&` is what fails the step instead.
    let script =
        remote_script(&save_snapshot_if_absent(&cfg(), "~/vms/p", Tty::NoPty));
    assert!(script.contains("names=$("), "{script}");
    let after_listing = script
        .split_once("'list')")
        .expect("the listing is captured")
        .1;
    assert!(
        after_listing.starts_with(" && "),
        "the listing must gate what follows: {script}"
    );
}

#[test]
fn a_snapshot_that_cannot_be_saved_does_not_fail_up() {
    // `execute` stops at the first failing step and returns
    // its status, and this is the last step of `up`. Without
    // the trailing `||`, a VM that booted and provisioned
    // correctly reports failure because of a snapshot.
    //
    // Two machines reach that on every run, not as an edge
    // case: a provider whose `snapshot list` raises because
    // it has no snapshot support, and one whose listing
    // decorates the name so the guard reads "absent" and the
    // unforced save is then refused.
    let script =
        remote_script(&save_snapshot_if_absent(&cfg(), "~/vms/p", Tty::NoPty));
    assert!(script.contains("|| printf 'bombyx: "), "{script}");
    // The braces keep the `cd` out of the advisory. Every
    // other builder here fails its step on a missing
    // directory, and this one must not differ.
    assert!(script.contains("&& { names=$("), "{script}");
    assert!(script.trim_end().ends_with(">&2; }"), "{script}");
}

#[test]
fn the_guarded_save_does_not_force() {
    // The guard and `-f` answer the same question, and only
    // one of them may. A guarded save carrying `-f` would
    // overwrite the snapshot whenever the listing test was
    // wrong about what is there, which is the failure the
    // guard exists to prevent.
    assert!(
        !remote_script(&save_snapshot_if_absent(&cfg(), "~/vms/p", Tty::NoPty))
            .contains("'-f'")
    );
}

#[test]
fn vagrant_in_runs_in_the_given_dir() {
    let c = vagrant_in(&cfg(), "/srv/x", &["halt"], Tty::NoPty);
    let env = vagrant_env();
    assert_eq!(
        remote_script(&c),
        format!("cd '/srv/x' && {env} vagrant 'halt'")
    );
}

#[test]
fn status_guards_a_never_built_project_and_says_run_up() {
    // The Vagrantfile is tested at its full path, before any
    // `cd`, so a directory that does not exist yet is answered
    // rather than `cd`-ed into; the else branch is a plain
    // message and a zero exit, not vagrant's raw failure.
    let c = status_or_never_built(&cfg(), Tty::NoPty);
    let env = vagrant_env();
    assert_eq!(
        remote_script(&c),
        format!(
            "if [ -f ~/'vms/myproject/Vagrantfile' ]; then \
                 cd ~/'vms/myproject' && {env} vagrant 'status'; \
                 else printf 'bombyx: %s has no VM yet; run `up` to \
                 create it\\n' 'myproject'; fi"
        )
    );
}

#[test]
fn the_removal_runs_whether_vagrant_worked_or_not() {
    // A `&&` here would skip the removal on a failed boot,
    // which is the case where the secrets would otherwise be
    // left on a machine other accounts can log in to.
    let c = vagrant_in_then_remove(
        &cfg(),
        "/srv/x",
        &["up"],
        None,
        Tty::NoPty,
        &["bombyx.env"],
    );
    let env = vagrant_env();
    let file = "'/srv/x/bombyx.env'";
    assert_eq!(
        remote_script(&c),
        format!(
            "cd '/srv/x' && {env} vagrant 'up'; rc=$?; \
                 rm -f {file} || {{ printf 'bombyx: could not remove \
                 %s from the VM host; it may hold secrets for this \
                 project\\n' {file} >&2; \
                 [ \"$rc\" = 0 ] && rc=1; }}; exit $rc"
        )
    );
}

#[test]
fn every_clone_mode_is_listed() {
    // The `match` has no wildcard arm, so a new variant stops
    // this test compiling until it gets an arm, and `ARMS` is
    // raised by hand beside it; `config::vm`'s
    // `every_provider_is_listed` is the same shape.
    const ARMS: usize = 3;
    let slot = |m: CloneUpdate| match m {
        CloneUpdate::Checkout => 0,
        CloneUpdate::Discard => 1,
        CloneUpdate::Keep => 2,
    };
    assert_eq!(CloneUpdate::ALL.len(), ARMS, "a mode is unlisted");
    for (i, m) in CloneUpdate::ALL.into_iter().enumerate() {
        assert_eq!(slot(m), i, "{m:?} is listed out of place");
    }
}

#[test]
fn a_clone_mode_is_named_on_the_vagrant_call_and_only_when_given() {
    // The mode belongs to one run, so it rides on the vagrant
    // invocation; `CLONE_UPDATE_ENV` holds why. A call given no
    // mode must not name one, or it would override the guest's
    // fallback with a value nobody chose.
    let script = |mode| {
        remote_script(&vagrant_in_then_remove(
            &cfg(),
            "/srv/x",
            &["provision"],
            mode,
            Tty::NoPty,
            &["bombyx.env"],
        ))
    };
    let env = vagrant_env();
    for mode in CloneUpdate::ALL {
        let s = script(Some(mode));
        let want = format!(
            "cd '/srv/x' && {env} {CLONE_UPDATE_ENV}='{}' vagrant \
                 'provision';",
            mode.as_str()
        );
        assert!(s.contains(&want), "{mode:?}: {s}");
    }
    assert!(!script(None).contains(CLONE_UPDATE_ENV));
}

#[test]
fn a_removal_that_failed_is_reported_and_fails_the_run() {
    // bombyx tells the operator the VM host keeps no copy.
    // An `rm` that quietly gave up -- a full disk, a
    // directory whose ownership changed -- would leave that
    // claim false with nothing said. The guest half of this
    // design tests every `rm` it runs; so does this half.
    let c = vagrant_in_then_remove(
        &cfg(),
        "/srv/x",
        &["up"],
        None,
        Tty::NoPty,
        &["bombyx.env"],
    );
    let s = remote_script(&c);
    assert!(
        s.contains("bombyx: could not remove"),
        "a failed removal must say so: {s}"
    );
    // And a boot that worked must stop reporting success.
    assert!(
        s.contains("rc=1"),
        "a failed removal must fail the run: {s}"
    );
    // And only when the boot itself did not already fail:
    // vagrant's own status says more than a bare 1.
    assert!(
        s.contains("[ \"$rc\" = 0 ] && rc=1"),
        "a failed boot must keep its own status: {s}"
    );
    // The newline reaches `printf` as the two characters it
    // converts, not as a real one. A raw newline inside the
    // command breaks a printed plan across lines.
    assert!(!s.contains('\n'), "the command must be one line: {s:?}");
}

#[test]
fn the_removal_names_the_file_absolutely() {
    // The `cd` can fail -- a directory removed between the
    // `mkdir` and this step -- and the shell is then in the
    // login directory. A bare `rm -f bombyx.env` would name
    // a file there instead.
    let c = vagrant_in_then_remove(
        &cfg(),
        "/srv/x",
        &["up"],
        None,
        Tty::NoPty,
        &["bombyx.env"],
    );
    let s = remote_script(&c);
    assert!(
        s.contains("rm -f '/srv/x/bombyx.env'"),
        "the removal must carry the whole path: {s}"
    );
}

#[test]
fn one_status_call_carries_every_project_on_the_host() {
    // The whole point of the builder: several projects share
    // a machine, and asking each of them separately would
    // pay for an ssh handshake per project.
    let web = cfg();
    let mut api = cfg();
    api.project = crate::name::ProjectName::parse("api").unwrap();
    let cmd = vagrant_status_many(&web, &[&api]);
    let script = remote_script(&cmd);

    for name in ["myproject", "api"] {
        assert!(
            script.contains(&format!("{LISTING_MARKER}%s\\n' '{name}'")),
            "{name} must be announced: {script}"
        );
    }
    assert_eq!(
        script
            .matches("vagrant 'status' '--machine-readable'")
            .count(),
        2,
        "one status call per project: {script}"
    );
}

#[test]
fn a_listing_command_carries_the_unattended_connection_options() {
    // `list` prints nothing until every host has answered, so
    // an unbounded wait on one is a wait for the whole table,
    // and `docs/usage.md` promises an unreachable machine does
    // not hold up the others. Without `BatchMode` an `ssh`
    // wanting a password waits for input nobody is there to
    // give.
    let cmd = vagrant_status_many(&cfg(), &[]);
    let argv = cmd.args.join(" ");
    for opt in [
        "BatchMode=yes",
        "ConnectTimeout=10",
        "LogLevel=ERROR",
        "ServerAliveInterval=5",
        "ServerAliveCountMax=3",
    ] {
        assert!(argv.contains(opt), "{opt} missing from {argv}");
    }
}

#[test]
fn a_listing_command_never_asks_for_a_remote_terminal() {
    // The reply is parsed. `ssh -t` merges the remote's
    // stderr into stdout, so the fog warning would land
    // inside a project's block, and the remote tty turns
    // every `\n` into `\r\n`, so a state would be read as
    // `running\r`. Neither is the caller's decision to get
    // wrong, so the builder does not take one.
    let cmd = vagrant_status_many(&cfg(), &[]);
    assert!(
        !cmd.args.iter().any(|a| a == "-t"),
        "no PTY may be requested: {:?}",
        cmd.args
    );
}

#[test]
fn a_project_directory_is_entered_in_a_subshell() {
    // `cd` inside one project's fragment must not decide
    // where the next project's fragment runs. The
    // parentheses are what keep it local; without them the
    // second `cd` is relative to the first project's
    // directory and the guard above it has already answered
    // for the wrong path.
    let cmd = vagrant_status_many(&cfg(), &[]);
    let script = remote_script(&cmd);
    assert!(
        script.contains("then ( cd "),
        "the cd must run in a subshell: {script}"
    );
}

#[test]
fn a_project_with_no_vagrantfile_is_never_asked() {
    // vagrant fails outright in a directory holding no
    // Vagrantfile, and its failure would be the whole host's
    // reply. The guard is what keeps one untouched project
    // from hiding the states of the others.
    let cmd = vagrant_status_many(&cfg(), &[]);
    let script = remote_script(&cmd);
    assert!(
        script.contains("if [ -f ~/'vms/myproject/Vagrantfile' ]"),
        "the guard must name the project's Vagrantfile: {script}"
    );
}

#[test]
fn each_project_is_asked_with_its_own_provider() {
    // Two projects on one machine may name different
    // providers, and the reply for each has to come from the
    // one its own table names.
    let web = cfg();
    let mut api = cfg();
    api.project = crate::name::ProjectName::parse("api").unwrap();
    api.vm.provider = crate::config::Provider::Hyperv;
    let script = remote_script(&vagrant_status_many(&web, &[&api]));
    assert!(script.contains("=\'libvirt\'"), "{script}");
    assert!(script.contains("=\'hyperv\'"), "{script}");
}

// Runs the listing script through a real `sh`; the file says why.
#[cfg(unix)]
mod listing_script_tests;
