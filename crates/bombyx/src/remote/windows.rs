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

use crate::config::{Config, HookPath};
use crate::powershell::{code_lines, quote, run_encoded};

/// The call `bombyx up` and `shell` make to refresh a running
/// Windows guest's secrets; its header names the arguments.
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
