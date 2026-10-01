//! The provisioners that set up a Windows guest, and the four
//! PowerShell scripts they ship.
//!
//! The split mirrors the Linux one. [`ACCOUNT`] runs first, as the
//! account vagrant logs in as, which is an administrator on the
//! box bombyx targets. It creates the agent's account, installs
//! git, and hands [`BOOTSTRAP`] to the agent. [`BOOTSTRAP`] clones
//! the project and runs its script, as the agent. The other two,
//! `REFRESH` and `HOOK`, are helpers [`ACCOUNT`] installs for later:
//! `up` and `shell` call them to refresh a running guest's secrets
//! and run the project's hook.
//!
//! Windows has no `sudo -u`, so the hand-over is an SSH login from
//! the guest to itself, as the agent, with a key [`ACCOUNT`] makes
//! in the guest. From vagrant's SSH session, Windows refuses a
//! scheduled task with an S4U logon ("Access is denied"), and
//! `Start-Process -Credential` returns neither output nor an exit
//! code. The loopback login returns both, so it is the route.
//!
//! Neither script carries a config value. Each arrives as an
//! environment variable, as on Linux, but **base64-encoded**:
//! vagrant's `winssh` shell provisioner writes each value into the
//! script it runs as `$env:NAME="value"` and escapes nothing. A
//! `"` in a value ends the string there, and a `$` is expanded by
//! PowerShell before the script starts. Base64 holds neither.

use std::collections::BTreeMap;

use super::{
    CREDENTIAL_PRESENT_ENV, DEPLOY_KEY_ENV, ENV_FILE_PRESENT_ENV, GIT_HOST_ENV,
    GUEST_USER_ENV, HISTORY_ENV, HOST_KEYS_FORMAT_ENV, HOST_KEYS_URL_ENV,
    PRESERVE_ENV, PROJECT_ENV, REF_ENV, REPO_ENV, SCRIPT_ENV, credential_block,
    deploy_key_block, env_file_block, ruby_string,
};
use crate::config::{Config, EnvName, EnvValue, Staged};
use crate::hostkeys;
use crate::powershell::base64;
use crate::remote::{
    CLONE_UPDATE_ENV, CloneUpdate, VM_HOST_ENV, VM_HOSTNAME_ENV,
};

/// The script that clones the project and runs its script, as the
/// agent, shipped to the host unchanged.
pub(crate) const BOOTSTRAP: &str =
    include_str!("../../templates/bootstrap.ps1");

/// [`BOOTSTRAP`]'s name on the VM host, next to the Vagrantfile.
pub(crate) const BOOTSTRAP_NAME: &str = "bootstrap.ps1";

/// The script that sets up the agent's account and hands
/// [`BOOTSTRAP`] to it, shipped to the host unchanged.
pub(crate) const ACCOUNT: &str = include_str!("../../templates/account.ps1");

/// [`ACCOUNT`]'s name on the VM host, next to the Vagrantfile.
pub(crate) const ACCOUNT_NAME: &str = "account.ps1";

/// The secrets refresh a running guest's `up` and `shell` call,
/// shipped to the host unchanged. [`ACCOUNT`] installs it, because
/// it is too long to send with each call; its header says what it
/// does.
pub(crate) const REFRESH: &str = include_str!("../../templates/refresh.ps1");

/// [`REFRESH`]'s name on the VM host, next to the Vagrantfile.
pub(crate) const REFRESH_NAME: &str = "refresh.ps1";

/// The runner [`REFRESH`] starts, as the agent, for the project's
/// `secrets_refreshed` hook, shipped to the host unchanged.
pub(crate) const HOOK: &str = include_str!("../../templates/hook.ps1");

/// [`HOOK`]'s name on the VM host, next to the Vagrantfile.
pub(crate) const HOOK_NAME: &str = "hook.ps1";

/// Where [`BOOTSTRAP`] is staged, relative to the login home.
///
/// Relative because vagrant expands no `~` on a Windows guest: the
/// file provisioner asks the guest to expand a path only when the
/// guest has the `shell_expand_guest_path` capability, and the
/// Windows guest does not. An SFTP server resolves a relative path
/// against the login home, and vagrant's `winssh` upload creates
/// the one missing directory. [`ACCOUNT`] finds the file under
/// `$env:USERPROFILE`, which is that same home.
const BOOTSTRAP_STAGED_PATH: &str = ".bombyx-staging/bootstrap.ps1";

/// Each script that travels as a plain upload, with where it is
/// staged, relative to the login home for the reason
/// [`BOOTSTRAP_STAGED_PATH`] gives. [`ACCOUNT`] moves each into the
/// bombyx folder under Program Files.
const UPLOADS: [(&str, &str); 3] = [
    (BOOTSTRAP_NAME, BOOTSTRAP_STAGED_PATH),
    (REFRESH_NAME, ".bombyx-staging/refresh.ps1"),
    (HOOK_NAME, ".bombyx-staging/hook.ps1"),
];

/// Where the deploy key is staged, relative to the login home for
/// the reason [`BOOTSTRAP_STAGED_PATH`] gives. [`ACCOUNT`] places it
/// as the agent's `.ssh\bombyx-deploy-key`.
pub(super) const DEPLOY_KEY_STAGED_PATH: &str = ".bombyx-staging/deploy-key";

/// Where the secrets file is staged. [`ACCOUNT`] places it as the
/// agent's `.bombyx-env`.
pub(super) const ENV_FILE_STAGED_PATH: &str = ".bombyx-staging/env";

/// Where the git credential is staged. [`ACCOUNT`] places it as the
/// agent's `.bombyx-git-credentials`.
pub(super) const CREDENTIAL_STAGED_PATH: &str =
    ".bombyx-staging/git-credentials";

/// The provisioners that set up a Windows guest: the uploads of
/// [`BOOTSTRAP`] and the two helpers, then [`ACCOUNT`].
pub(super) fn provisioning(cfg: &Config, staged: &Staged) -> String {
    use std::fmt::Write as _;

    let source = &cfg.source;
    // `None` when `repo` reaches the server by something other than
    // ssh, and when it names a host bombyx has no key source for.
    let host_keys = source.repo.ssh_host().and_then(hostkeys::for_host);
    let flag = |present: bool| if present { "1" } else { "0" };
    let mut env = String::new();
    // Writing to a `String` cannot fail, so the results are dropped.
    let mut entry = |name: &str, value: &str| {
        let _ = writeln!(
            env,
            "      {} => {},",
            ruby_string(name),
            ruby_string(&base64(value.as_bytes()))
        );
    };
    entry(GUEST_USER_ENV, cfg.vm.guest_user.as_str());
    entry(PRESERVE_ENV, &preserve_list(&cfg.env));
    entry(REPO_ENV, source.repo.as_str());
    entry(REF_ENV, source.git_ref.as_str());
    entry(HISTORY_ENV, source.history.as_str());
    entry(SCRIPT_ENV, source.script.as_str());
    entry(PROJECT_ENV, cfg.project.as_str());
    // Each "0" too, so the guest removes a copy an earlier provision
    // left rather than keeping a credential the config dropped.
    entry(DEPLOY_KEY_ENV, flag(staged.deploy_key().is_some()));
    entry(ENV_FILE_PRESENT_ENV, flag(staged.secrets().is_some()));
    entry(CREDENTIAL_PRESENT_ENV, flag(staged.credential().is_some()));
    // Empty for an https clone, which opens no ssh connection, and
    // for an ssh host bombyx publishes no keys for. For that host,
    // bootstrap.ps1 accepts the key it is offered on first connection
    // when a deploy key is configured. Without one, it adds nothing:
    // such a clone has no credential for that host, so it could not
    // authenticate whatever the host-key setting said.
    entry(GIT_HOST_ENV, host_keys.map_or("", |k| k.host()));
    entry(HOST_KEYS_URL_ENV, host_keys.map_or("", |k| k.url()));
    entry(
        HOST_KEYS_FORMAT_ENV,
        host_keys.map_or("", |k| k.format().as_str()),
    );
    for (name, value) in &cfg.env {
        entry(name.as_str(), value.as_str());
    }
    // Read from the vagrant process, as the Linux Vagrantfile's are;
    // the clone mode falls back to the one that refuses to overwrite
    // the agent's work.
    for (name, fallback) in [
        (VM_HOST_ENV, "unknown"),
        (VM_HOSTNAME_ENV, "unknown"),
        (CLONE_UPDATE_ENV, CloneUpdate::default().as_str()),
    ] {
        let _ = writeln!(
            env,
            "      {name} => [ENV.fetch({name}, {fallback})].pack(\"m0\"),",
            name = ruby_string(name),
            fallback = ruby_string(fallback),
        );
    }
    let mut uploads = String::new();
    for (name, staged) in UPLOADS {
        let _ = write!(
            uploads,
            "  config.vm.provision \"file\",
    source: File.expand_path({name}, __dir__),
    destination: {staged}
",
            name = ruby_string(name),
            staged = ruby_string(staged),
        );
    }
    // The last entry's comma is legal Ruby, so none is trimmed.
    format!(
        "  # bootstrap.ps1, refresh.ps1 and hook.ps1 travel as plain
  # uploads, because the shell provisioner below runs account.ps1.
  # They land in the staging directory of the account vagrant logs
  # in as, and account.ps1 moves them on. The paths are relative
  # because vagrant expands no `~` on a Windows guest.
{uploads}
{deploy_key}{env_file}{credential}  config.vm.provision \"shell\",
    path: {account},
    # As the account vagrant logs in as, an administrator:
    # account.ps1 creates the agent's account and hands
    # bootstrap.ps1 to it over an SSH login to this same guest.
    #
    # vagrant writes each value below into the script as
    # $env:NAME=\"value\" and escapes nothing, so every value is
    # base64, which holds no character PowerShell reads inside
    # double quotes. account.ps1 decodes them. BOMBYX_VM_HOST,
    # BOMBYX_VM_HOSTNAME and BOMBYX_CLONE_UPDATE are read from the
    # vagrant process on the VM host, which bombyx sets, and
    # encoded here as vagrant reads this file: `pack(\"m0\")` is
    # Ruby's base64 with no newline.
    env: {{
{env}    }}
",
        account = ruby_string(ACCOUNT_NAME),
        deploy_key = deploy_key_block(
            staged.deploy_key().is_some(),
            DEPLOY_KEY_STAGED_PATH
        ),
        env_file =
            env_file_block(staged.secrets().is_some(), ENV_FILE_STAGED_PATH),
        credential = credential_block(
            staged.credential().is_some(),
            CREDENTIAL_STAGED_PATH
        ),
    )
}

/// The value of [`PRESERVE_ENV`] for a Windows guest: every name
/// bombyx sets, which is the Linux list `BOMBYX_ENV_NAMES`, then every
/// `[env]` name, comma-separated.
///
/// [`ACCOUNT`] hands the agent these names and no others. A comma
/// cannot split a name, for the reason `super::preserve_list` gives.
pub(super) fn preserve_list(env: &BTreeMap<EnvName, EnvValue>) -> String {
    super::BOMBYX_ENV_NAMES
        .iter()
        .copied()
        .chain(env.keys().map(EnvName::as_str))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_shell_uses_the_paths_the_provisioning_scripts_write() {
        // Three files that cannot see each other agree on three paths:
        // account.ps1 keeps the hand-over key and localhost's host key
        // in the login account's `.ssh`, bootstrap.ps1 clones into
        // `$env:USERPROFILE\<project>`, and shell.ps1 reads the first
        // two and enters the third. A change to one would open no
        // shell, or one outside the clone, and fail nothing else.
        use crate::remote::windows::{SHELL, SHELL_NAME};
        for text in [
            "$LoginSsh = Join-Path $env:USERPROFILE '.ssh'",
            "Join-Path $LoginSsh 'bombyx-handover'",
            "Join-Path $LoginSsh 'bombyx-localhost-known-hosts'",
        ] {
            assert!(ACCOUNT.contains(text), "account.ps1: {text}");
        }
        for text in [
            "$loginSsh = Join-Path $env:USERPROFILE '.ssh'",
            "Join-Path $loginSsh 'bombyx-handover'",
            "Join-Path $loginSsh 'bombyx-localhost-known-hosts'",
            "(Join-Path `$env:USERPROFILE $projectLiteral)",
        ] {
            assert!(SHELL.contains(text), "{SHELL_NAME}: {text}");
        }
        for text in [
            "$AgentHome = $env:USERPROFILE",
            "$CloneDir = Join-Path $AgentHome $Project",
        ] {
            assert!(BOOTSTRAP.contains(text), "bootstrap.ps1: {text}");
        }
    }

    #[test]
    fn bootstrap_ps1_updates_the_clone_as_the_mode_says() {
        // The Windows half of `bootstrap_tests`' clone-mode tests:
        // the same fallback, the same three words, a forced checkout
        // only under `discard`, and no deletion of a clone holding
        // work without it. Two needles span a line break, and the
        // file checks out with CRLF endings, so they are matched
        // against LF text.
        let script = BOOTSTRAP.replace("\r\n", "\n");
        let fallback =
            format!("$CloneUpdate = '{}'", CloneUpdate::default().as_str());
        let words = CloneUpdate::ALL
            .map(|m| format!("'{}'", m.as_str()))
            .join(", ");
        let check = format!("if ($CloneUpdate -cnotin @({words})) {{");
        for text in [
            "$CloneUpdate = $env:BOMBYX_CLONE_UPDATE",
            fallback.as_str(),
            check.as_str(),
            "if ($CloneUpdate -ne 'discard' -and\n                \
             (Test-CloneHoldsWork $Git $CloneDir)) {",
            "if ($CloneUpdate -eq 'keep') {",
            "$head = 'an unreadable HEAD'",
            "if ($CloneUpdate -eq 'discard') {",
            "Invoke-Native $Git -C $CloneDir -c core.fileMode=false `\n                    \
             checkout FETCH_HEAD",
        ] {
            assert!(script.contains(text), "bootstrap.ps1: {text}");
        }
        assert_eq!(script.matches("checkout --force").count(), 1);
    }

    #[test]
    fn bootstrap_ps1_clones_the_configured_history() {
        // The Windows half of `bootstrap_tests`' history tests: the
        // same fallback and words, every branch under `full`, and
        // `--depth 1` only when the setting is `shallow` and the
        // clone is shallow already, or on a first clone under
        // `shallow`. The first clone passes its depth as the array
        // `@('--depth', '1')`, so the literal `--depth 1` appears
        // once, in the fetch, where the shell script has it twice.
        use crate::config::History;
        let script = BOOTSTRAP.replace("\r\n", "\n");
        let fallback = format!("$History = '{}'", History::default().as_str());
        let words =
            History::ALL.map(|h| format!("'{}'", h.as_str())).join(", ");
        let check = format!("if ($History -cnotin @({words})) {{");
        for text in [
            "$History = $env:BOMBYX_HISTORY",
            fallback.as_str(),
            check.as_str(),
            "Invoke-Native $Git -C $CloneDir remote set-branches origin '*'",
            "Invoke-Native $Git @gitNet -C $CloneDir fetch `\n                        \
             --unshallow origin",
            "if ($History -eq 'shallow' -and\n                \
             (Test-CloneIsShallow $Git $CloneDir)) {",
            "$depth = @('--depth', '1')",
            "clone @depth --branch $Ref '--' $Repo",
        ] {
            assert!(script.contains(text), "bootstrap.ps1: {text}");
        }
        // Counted over code lines, so a comment naming the flag
        // does not count.
        let depths = script
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter(|l| l.contains("--depth 1"))
            .count();
        assert_eq!(depths, 1, "{script}");
    }

    #[test]
    fn account_ps1_installs_the_helpers_where_the_refresh_calls_them() {
        // account.ps1 moves each helper from the staging directory into
        // the bombyx folder under Program Files; refresh-call.ps1 names
        // refresh.ps1 there, and refresh.ps1 finds hook.ps1 beside
        // itself. A name that drifts in one file leaves the refresh
        // calling a script that is not there.
        use crate::remote::windows::{HELPER_CALL, REFRESH_CALL};
        for text in [
            "$InstallDir = Join-Path $env:ProgramFiles 'bombyx'",
            "Join-Path $Staging 'refresh.ps1'",
            "Join-Path $Staging 'hook.ps1'",
            "Join-Path $InstallDir 'refresh.ps1'",
            "Join-Path $InstallDir 'hook.ps1'",
        ] {
            assert!(ACCOUNT.contains(text), "{ACCOUNT_NAME}: {text}");
        }
        assert!(
            REFRESH_CALL
                .contains("Join-Path $env:ProgramFiles 'bombyx\\refresh.ps1'"),
            "refresh-call.ps1"
        );
        assert!(
            REFRESH.contains("Join-Path $PSScriptRoot 'hook.ps1'"),
            "{REFRESH_NAME}"
        );
        // The call's version and the helper's must agree, or every
        // refresh would be refused as coming from another bombyx.
        assert!(
            crate::powershell::has_line(
                REFRESH,
                &format!("$Supported = {HELPER_CALL}")
            ),
            "{REFRESH_NAME}: $Supported"
        );
    }

    /// Passes every Windows guest script -- the provisioning
    /// scripts, the refresh helpers, and the calls `bombyx shell` and
    /// the refresh send -- through Windows PowerShell's own parser, so
    /// a syntax error fails CI's Windows job rather than a guest's
    /// provisioning. Windows only, because the parser ships with
    /// Windows PowerShell. It checks syntax, not behaviour: a real
    /// `up` or `shell` against a Windows guest is what shows the
    /// scripts work.
    #[cfg(windows)]
    #[test]
    fn every_guest_script_parses_under_windows_powershell() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for (name, text) in [
            (ACCOUNT_NAME, ACCOUNT),
            (BOOTSTRAP_NAME, BOOTSTRAP),
            (
                crate::remote::windows::SHELL_NAME,
                crate::remote::windows::SHELL,
            ),
            (REFRESH_NAME, REFRESH),
            (HOOK_NAME, HOOK),
            ("refresh-call.ps1", crate::remote::windows::REFRESH_CALL),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, text).expect("the script is written");
            // Single-quoted, with any `'` doubled, so the path
            // reaches the parser as written.
            let quoted = path.display().to_string().replace('\'', "''");
            let check = format!(
                "$errors = $null; \
                 [void][System.Management.Automation.Language.Parser]::\
                 ParseFile('{quoted}', [ref]$null, [ref]$errors); \
                 foreach ($e in $errors) {{ \
                 [Console]::Out.WriteLine(\
                 [string]$e.Extent.StartLineNumber + ': ' + $e.Message) }}; \
                 exit $errors.Count"
            );
            let out = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &check])
                .output()
                .expect("Windows PowerShell runs");
            assert!(
                out.status.success(),
                "{name} does not parse:\n{}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }

    #[test]
    fn the_scripts_agree_on_where_each_secret_is_staged_and_placed() {
        // The Vagrantfile uploads under a name, account.ps1 moves the
        // file from that name to a path, and bootstrap.ps1 reads it
        // there. A rename in one of the three strands the secret.
        for (staged, placed) in [
            (DEPLOY_KEY_STAGED_PATH, r"'.ssh\bombyx-deploy-key'"),
            (ENV_FILE_STAGED_PATH, "'.bombyx-env'"),
            (CREDENTIAL_STAGED_PATH, "'.bombyx-git-credentials'"),
        ] {
            let (dir, name) = staged.split_once('/').expect("a staged path");
            assert!(ACCOUNT.contains(&format!("'{dir}'")), "{dir}");
            assert!(ACCOUNT.contains(&format!("'{name}'")), "{name}");
            assert!(ACCOUNT.contains(placed), "account.ps1: {placed}");
            assert!(BOOTSTRAP.contains(placed), "bootstrap.ps1: {placed}");
        }
    }

    #[test]
    fn a_refreshed_file_is_made_the_agents_as_account_ps1_makes_it() {
        // The login account writes a refreshed file, and Windows'
        // ssh refuses a private key another account owns, so the
        // refresh hands the file to the agent as `Protect` in
        // account.ps1 does at provisioning. It does so on the
        // temporary, before the secret is written and before the
        // rename, so a failure exits 1 while the old copy stands --
        // the status refresh.ps1's header promises.
        let lines: Vec<&str> = REFRESH.lines().map(str::trim_end).collect();
        let at = |line: &str| {
            lines
                .iter()
                .position(|l| *l == line)
                .unwrap_or_else(|| panic!("refresh.ps1 lacks {line:?}"))
        };
        let owner = at("    $out = & icacls.exe $new /setowner \"*$sid\" 2>&1");
        let write = at("    [IO.File]::WriteAllBytes($new, $buffer.ToArray())");
        let rename = at("        [IO.File]::Move($new, $target)");
        assert!(owner < write && write < rename, "{owner} {write} {rename}");
    }

    #[test]
    fn a_refresh_creates_the_folder_a_key_needs() {
        // The key lives in `.ssh`, which a guest provisioned before
        // its config named a key does not have.
        for line in [
            "    $dir = Split-Path -Parent $target",
            "    New-Item -ItemType Directory -Force -Path $dir | Out-Null",
        ] {
            assert!(crate::powershell::has_line(REFRESH, line), "{line}");
        }
    }
}
