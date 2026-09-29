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
//! `the_longest_windows_shell_command_fits_the_guest_command_line`
//! holds [`shell_command`] to a budget.

use crate::config::Config;
use crate::powershell::{code_lines, quote, run_encoded};

/// The script `bombyx shell` runs on a Windows guest; its header says
/// how it reaches the agent's account.
pub(crate) const SHELL: &str = include_str!("../../templates/shell.ps1");

/// The file [`SHELL`] is read from, for a test naming the script.
#[cfg(test)]
pub(crate) const SHELL_NAME: &str = "shell.ps1";

/// The `vagrant ssh -c` text that opens the agent's shell on a
/// Windows guest: [`SHELL`]'s code, after the two lines that give it
/// the account and the clone's folder name.
pub(super) fn shell_command(cfg: &Config) -> String {
    let script = format!(
        "$User = {}\n$Project = {}\n{}",
        quote(cfg.vm.guest_user.as_str()),
        quote(cfg.project.as_str()),
        code_lines(SHELL),
    );
    run_encoded(&script)
}
