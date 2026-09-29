//! The provisioners that set up a Windows guest, and the two
//! PowerShell scripts they ship.
//!
//! The split mirrors the Linux one. [`ACCOUNT`] runs first, as the
//! account vagrant logs in as, which is an administrator on the
//! box bombyx targets. It creates the agent's account, installs
//! git, and hands [`BOOTSTRAP`] to the agent. [`BOOTSTRAP`] clones
//! the project and runs its script, as the agent.
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
    GUEST_USER_ENV, PRESERVE_ENV, PROJECT_ENV, REF_ENV, REPO_ENV, SCRIPT_ENV,
    ruby_string,
};
use crate::config::{Config, EnvName, EnvValue};
use crate::remote::{VM_HOST_ENV, VM_HOSTNAME_ENV};

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

/// The names bombyx sets for a Windows guest, in the order the
/// Vagrantfile lists them.
///
/// Fewer than on Linux: the deploy key, the secrets file, the git
/// credential and the git host's ssh keys are not handed to a
/// Windows guest yet, because its scripts do not place them.
const ENV_NAMES: [&str; 7] = [
    GUEST_USER_ENV,
    REPO_ENV,
    REF_ENV,
    SCRIPT_ENV,
    PROJECT_ENV,
    VM_HOST_ENV,
    VM_HOSTNAME_ENV,
];

/// The provisioners that set up a Windows guest: the upload of
/// [`BOOTSTRAP`], then [`ACCOUNT`].
pub(super) fn provisioning(cfg: &Config) -> String {
    use std::fmt::Write as _;

    let source = &cfg.source;
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
    entry(SCRIPT_ENV, source.script.as_str());
    entry(PROJECT_ENV, cfg.project.as_str());
    for (name, value) in &cfg.env {
        entry(name.as_str(), value.as_str());
    }
    for name in [VM_HOST_ENV, VM_HOSTNAME_ENV] {
        let _ = writeln!(
            env,
            "      {name} => [ENV.fetch({name}, \"unknown\")].pack(\"m0\"),",
            name = ruby_string(name),
        );
    }
    // The last entry's comma is legal Ruby, so none is trimmed.
    format!(
        "  # bootstrap.ps1 travels as a plain upload, because the shell
  # provisioner below runs account.ps1. It lands in the staging
  # directory of the account vagrant logs in as, and account.ps1
  # moves it on. The path is relative because vagrant expands no
  # `~` on a Windows guest.
  config.vm.provision \"file\",
    source: File.expand_path({bootstrap}, __dir__),
    destination: {staged}

  config.vm.provision \"shell\",
    path: {account},
    # As the account vagrant logs in as, an administrator:
    # account.ps1 creates the agent's account and hands
    # bootstrap.ps1 to it over an SSH login to this same guest.
    #
    # vagrant writes each value below into the script as
    # $env:NAME=\"value\" and escapes nothing, so every value is
    # base64, which holds no character PowerShell reads inside
    # double quotes. account.ps1 decodes them. The last two are
    # read from the vagrant process on the VM host, which bombyx
    # sets, and encoded here as vagrant reads this file:
    # `pack(\"m0\")` is Ruby's base64 with no newline.
    env: {{
{env}    }}
",
        bootstrap = ruby_string(BOOTSTRAP_NAME),
        staged = ruby_string(BOOTSTRAP_STAGED_PATH),
        account = ruby_string(ACCOUNT_NAME),
    )
}

/// The value of [`PRESERVE_ENV`] for a Windows guest: every name in
/// [`ENV_NAMES`], then every `[env]` name, comma-separated.
///
/// [`ACCOUNT`] hands the agent these names and no others. A comma
/// cannot split a name, for the reason `super::preserve_list` gives.
pub(super) fn preserve_list(env: &BTreeMap<EnvName, EnvValue>) -> String {
    ENV_NAMES
        .iter()
        .copied()
        .chain(env.keys().map(EnvName::as_str))
        .collect::<Vec<_>>()
        .join(",")
}

/// The standard base64 alphabet, RFC 4648 section 4.
const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in standard base64, padded with `=`, on one line.
///
/// Written out rather than taken from a crate: it is a dozen lines,
/// and a dependency would cost a cooldown and a licence review for
/// them. PowerShell's `[Convert]::FromBase64String` reads exactly
/// this form, and so does Ruby's `unpack("m0")`.
pub(super) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        // Each index is six bits, so it is always inside ALPHABET.
        let sextet =
            |shift: u32| char::from(ALPHABET[(n >> shift & 63) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_test_vectors() {
        // RFC 4648 section 10, which covers every padding case.
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    #[test]
    fn base64_uses_the_two_symbols_of_the_standard_alphabet() {
        // 0xfb 0xff encodes to `+/8=`: the last two alphabet
        // entries, which a URL-safe variant would spell `-_`.
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    /// Passes both scripts through Windows PowerShell's own parser,
    /// so a syntax error fails CI's Windows job rather than a
    /// guest's provisioning. Windows only, because the parser ships
    /// with Windows PowerShell. It checks syntax, not behaviour: a
    /// real `up` against a Windows guest is what shows the scripts
    /// work.
    #[cfg(windows)]
    #[test]
    fn both_scripts_parse_under_windows_powershell() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for (name, text) in
            [(ACCOUNT_NAME, ACCOUNT), (BOOTSTRAP_NAME, BOOTSTRAP)]
        {
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
    fn base64_holds_no_character_powershell_reads_in_double_quotes() {
        let all: Vec<u8> = (0..=255).collect();
        let out = base64(&all);
        assert!(
            out.chars()
                .all(|c| c.is_ascii_alphanumeric()
                    || matches!(c, '+' | '/' | '=')),
            "{out}"
        );
    }
}
