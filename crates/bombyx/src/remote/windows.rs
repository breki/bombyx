//! The guest commands for a Windows guest.
//!
//! A Linux guest gets a POSIX script through `vagrant ssh -c`, which
//! vagrant wraps in `bash -l -c`. A Windows guest's box sets
//! `config.winssh.shell = "powershell"`, so vagrant hands the text
//! to Windows PowerShell 5.1 instead, and it rewrites every `'` in
//! it as `'\''` first -- vagrant 2.4.9's `ssh_run.rb` does, for
//! every shell. That is `sh`'s way to put a `'` inside a quoted
//! string; PowerShell has no backslash escape, so it reads a closed
//! string, a stray `\` and a new one, and the script breaks. So each
//! command here is its script in base64, run by `Invoke-Expression`
//! ([`run_encoded`]), which holds no `'` at all, and the script
//! inside is free to use them.
//!
//! [`shell_script`] is the exception: it is a VM-host script that
//! logs in to the guest with plain `ssh` rather than `vagrant ssh
//! -c`, so its PowerShell travels as `-EncodedCommand` instead. Its
//! PowerShell is one `Set-Location`, so the length limit below never
//! comes near it.
//!
//! **The command line has a length limit.** vagrant encodes the
//! text again, as `-encodedCommand`'s UTF-16, and the guest's sshd
//! hands the result to `cmd.exe`, whose documented limit is 8191
//! characters. So a script loses its comments before it is encoded
//! ([`code_lines`]); the template keeps them for the reader.
//! `docs/windows-guest-box.md` records the measured lengths, and
//! `the_longest_windows_refresh_command_fits_the_guest_command_line`
//! holds `refresh_command` to a budget of 7800.
//! The refresh helpers themselves would not fit: sent this way, the
//! code of `refresh.ps1` and `hook.ps1` measured about 33700
//! characters, so `account.ps1` installs them and the call names them.

use super::{
    WINDOWS_SHELL_ADVICE, quote_remote_path, shell_quote, vagrant_command,
};
use crate::config::{Config, HookPath};
use crate::powershell::{code_lines, encoded_command, quote, run_encoded};

/// The call `bombyx up` and `shell --refresh-secrets` make to refresh a
/// running Windows guest's secrets; its header names the arguments.
pub(crate) const REFRESH_CALL: &str =
    include_str!("../../templates/refresh-call.ps1");

/// The version of the call [`REFRESH_CALL`] makes to the installed
/// `refresh.ps1`, which refuses any other. Raise it whenever the
/// helper's arguments change, so a guest provisioned by another
/// bombyx says to provision it again rather than misread them.
pub(crate) const HELPER_CALL: u32 = 1;

/// The `vagrant ssh -c` text that writes `file`, arriving on
/// standard input, over the agent's copy, then, when `hook` names
/// one, runs it with that many seconds to finish. The hook stays a
/// checked [`HookPath`], whose Windows length cap the call's length
/// budget rests on.
pub(super) fn refresh_command(
    cfg: &Config,
    file: &str,
    hook: Option<(&HookPath, u32)>,
) -> String {
    let (path, timeout) = hook.map_or(("", 0), |(p, t)| (p.as_str(), t));
    let prefix = format!(
        "$Interface = {HELPER_CALL}\n$User = {}\n$File = {}\n\
         $Project = {}\n$Hook = {}\n$Timeout = {timeout}\n",
        quote(cfg.vm.guest_user.as_str()),
        quote(file),
        quote(cfg.project.as_str()),
        quote(path),
    );
    guest_command(&prefix, REFRESH_CALL)
}

/// `template`'s code after `prefix`, the lines that set its inputs,
/// as the text `vagrant ssh -c` carries: comments dropped for the
/// command line's length, and base64 for vagrant's quote rewriting,
/// as the module doc says.
fn guest_command(prefix: &str, template: &str) -> String {
    run_encoded(&format!("{prefix}{}", code_lines(template)))
}

/// The VM-host script that opens the agent's shell on a Windows
/// guest, in the clone, in one SSH login.
///
/// **One login, because a second one breaks the terminal.** Windows
/// has no `sudo -u` that keeps the terminal, so the login account
/// could only reach the agent by a second SSH login from the guest
/// to itself, and Windows' `ssh.exe` on that hop splits an arrow
/// key's `ESC [ A` into Escape and the text `[A` (#182). So the VM
/// host logs in as the agent itself, with vagrant's machine key:
/// the key pair vagrant makes for a machine at its first boot,
/// keeping the private half under the project's `.vagrant/machines/`
/// on the VM host and adding the public half to the login account's
/// `authorized_keys`. `account.ps1` authorizes that key for the agent
/// too, and `vagrant ssh-config`'s `IdentityFile` line names it.
///
/// **Plain `ssh` with vagrant's config, because `vagrant ssh` keeps
/// its own user.** `vagrant ssh -- -l agent` still logs in as the
/// login account, measured with vagrant 2.4.9. `vagrant ssh-config`
/// writes the address, port and key vagrant would use, and a
/// command-line `-l` overrides the `User` in that file. The file
/// lives in a `mktemp` name and names the key's path, not the key,
/// so it holds no secret and its place in `/tmp` needs no guarding;
/// the `EXIT` trap only tidies it away. The signal traps end the
/// script, so a dropped connection starts nothing after it. vagrant's update
/// check is off for that call, because its notices print on stdout
/// and would land in the file `ssh` parses.
///
/// **The clone path is spelled in the guest.** The agent's
/// PowerShell `Set-Location`s into `$env:USERPROFILE\<project>`,
/// where `bootstrap.ps1` clones. The command travels as
/// `-EncodedCommand`, which holds no character the VM host's `sh`
/// or the guest's shell reads. A missing clone prints the error and
/// leaves the shell in the profile, to look into why.
///
/// **A probe checks the login before the session starts.** A
/// non-interactive `ssh -n ... exit` as the agent runs first; `-n`
/// keeps it off the terminal, so what the operator types while
/// vagrant starts reaches the session. 0 opens the agent's shell.
/// Anything else prints [`WINDOWS_SHELL_ADVICE`] and ends with the
/// probe's status. The script does not guess the cause: `ssh` exits
/// 255 both when the guest's sshd does not answer and when the agent
/// refuses the key, and only a provision fixes the second, so the
/// advice names both and no other account's shell opens in the
/// agent's place. The session's exit status becomes the script's,
/// and nothing branches on it: Windows' sshd reports 0 for a session
/// with a terminal, and `ssh` exits 255 when a working session's
/// connection drops.
pub(super) fn shell_script(cfg: &Config) -> String {
    let enter = encoded_command(&shell_entry(cfg.project.as_str()));
    format!(
        "cd {dir} && c=$(mktemp) && \
         trap 'rm -f \"$c\"' EXIT && trap 'exit 129' HUP && \
         trap 'exit 130' INT && trap 'exit 143' TERM && {{ \
         VAGRANT_CHECKPOINT_DISABLE=1 {config} > \"$c\" || exit; \
         ssh -n -F \"$c\" -o BatchMode=yes -l {user} {SHELL_HOST} exit; \
         rc=$?; if [ \"$rc\" = 0 ]; then \
         ssh -F \"$c\" -t -l {user} {SHELL_HOST} powershell.exe \
         -NoLogo -NoExit -EncodedCommand {enter}; \
         else \
         printf 'bombyx: could not log in to the guest as %s: %s\\n' \
         {user} {advice} >&2; exit \"$rc\"; fi; }}",
        dir = quote_remote_path(&cfg.remote_project_dir()),
        config =
            vagrant_command(cfg, &["ssh-config", "--host", SHELL_HOST], None),
        user = shell_quote(cfg.vm.guest_user.as_str()),
        advice = shell_quote(WINDOWS_SHELL_ADVICE),
    )
}

/// The PowerShell the agent's shell on a Windows guest runs first:
/// it enters `$env:USERPROFILE\<project>`, the folder `bootstrap.ps1`
/// clones into. A test in `vagrantfile::windows` holds the two
/// spellings together.
pub(crate) fn shell_entry(project: &str) -> String {
    format!(
        "Set-Location -LiteralPath (Join-Path $env:USERPROFILE {})",
        quote(project)
    )
}

/// The name [`shell_script`] gives the guest in the config
/// `vagrant ssh-config` writes, and then logs in to. A fixed word,
/// so it needs no quoting where the script uses it bare.
const SHELL_HOST: &str = "guest";
