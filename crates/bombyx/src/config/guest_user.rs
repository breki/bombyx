//! The account the agent works as inside the guest.
//!
//! The guest's account script -- `account.sh` on Linux,
//! `account.ps1` on Windows -- creates this account, and every
//! later step -- the clone, the project's own script,
//! `bombyx shell` -- runs as it. Vagrant keeps logging
//! in as the box's own SSH user, usually `vagrant`, which is an
//! account this one never is.

use serde::Deserialize;

use super::error::FieldError;
use crate::newtype::{
    checked_str_newtype, checked_str_parse, checked_str_try_from,
};

/// The account name when the config sets none.
const DEFAULT_GUEST_USER: &str = "agent";

/// The longest name `useradd` accepts on the boxes bombyx
/// targets.
const MAX_GUEST_USER_LEN: usize = 32;

/// Names the account may not take.
///
/// `root` would hand the agent the machine outright, and the
/// Vagrantfile's privileged provisioner would then run the
/// project's clone as root. `vagrant` is the SSH user Vagrant
/// logs in as on almost every box, and keeping the agent out of
/// that account is what this setting is for.
const REFUSED_GUEST_USERS: [&str; 2] = ["root", "vagrant"];

/// The longest local account name Windows accepts.
const MAX_WINDOWS_GUEST_USER_LEN: usize = 20;

/// The accounts and groups a Windows guest is born with,
/// lower-cased as a [`GuestUser`] must be.
///
/// Taking an account would hand the agent one the box already
/// uses, which is what `REFUSED_GUEST_USERS` keeps it from on
/// every guest. Windows keeps groups and accounts in one
/// namespace, so a group's name cannot be an account either. The
/// groups are the built-in ones whose names a [`GuestUser`] can
/// spell; the others hold a space.
const WINDOWS_BUILT_IN_NAMES: [&str; 8] = [
    "administrator",
    "guest",
    "defaultaccount",
    "wdagutilityaccount",
    "administrators",
    "users",
    "guests",
    "replicator",
];

/// A validated guest account name.
///
/// A *newtype* in the shape `super::source::RepoUrl` describes:
/// holding one is proof that [`GuestUser::parse`] accepted it.
///
/// The rules are the portable subset of what `useradd` takes: a
/// lowercase letter or underscore first, then lowercase letters,
/// digits, underscores and hyphens, at most `MAX_GUEST_USER_LEN`
/// characters. Two guest scripts rely on that set. Both build the
/// account's home as `/home/<name>`, and `bootstrap.sh` puts paths
/// under it into `core.sshCommand` and `credential.helper`,
/// strings `git` hands to a shell. A name with no space, `$`,
/// quote or backtick gives that shell nothing to split or expand.
/// The set also has no `.`, which matters because `sudo` skips a
/// file in `/etc/sudoers.d` whose name contains one, and the
/// account's sudoers file is named after it. On Windows,
/// `account.ps1` puts the name into the command line its SSH
/// hand-over sends to `cmd.exe`, and the set gives `cmd.exe`
/// nothing to read there either.
///
/// `#[serde(try_from = "String")]` is what makes the check run
/// while the config file is being read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct GuestUser(String);

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is empty, and
    /// [`FieldError::Invalid`] when it is too long, starts with
    /// anything but a lowercase letter or underscore, holds a
    /// character outside lowercase letters, digits, `_` and `-`,
    /// or is `root` or `vagrant`.
    GuestUser,
    FieldError,
    check_guest_user
);

checked_str_newtype!(GuestUser, "The account name, as the guest sees it.");

checked_str_try_from!(
    /// What serde calls; see [`GuestUser::parse`].
    GuestUser,
    FieldError,
    check_guest_user
);

/// Which Windows rule a [`GuestUser`] breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsUserRefusal {
    /// Longer than Windows allows a local account name.
    TooLong,
    /// One of the accounts or groups a Windows guest is born with.
    BuiltIn,
    /// A name Windows reserves for a device, which cannot be the
    /// account's profile folder.
    DeviceName,
}

impl std::fmt::Display for WindowsUserRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong => write!(
                f,
                "must be at most {MAX_WINDOWS_GUEST_USER_LEN} characters"
            ),
            Self::BuiltIn => f.write_str(
                "must not name one of the box's built-in accounts or groups",
            ),
            Self::DeviceName => f.write_str(
                "must not be a name Windows reserves for a device, such as \
                 con or com1, because it names the account's profile folder",
            ),
        }
    }
}

impl GuestUser {
    /// Why a Windows guest cannot hold this account, or `None` when
    /// it can.
    ///
    /// A method rather than a rule in [`GuestUser::parse`], because
    /// only a Windows guest has it and the name is read before the
    /// config says which guest it is for. The registry's `parse`
    /// asks it once `[vm]`'s `guest` is known.
    ///
    /// `account.ps1` keeps its own copy of these rules and of
    /// `REFUSED_GUEST_USERS`, and `bootstrap.ps1` a copy of the name
    /// pattern, which they check in the guest. Nothing ties the
    /// three copies together, so a change to one belongs in all.
    #[must_use]
    pub(crate) fn windows_refusal(&self) -> Option<WindowsUserRefusal> {
        if self.0.len() > MAX_WINDOWS_GUEST_USER_LEN {
            Some(WindowsUserRefusal::TooLong)
        } else if WINDOWS_BUILT_IN_NAMES.contains(&self.0.as_str()) {
            Some(WindowsUserRefusal::BuiltIn)
        } else if super::guards::is_windows_device_name(&self.0) {
            Some(WindowsUserRefusal::DeviceName)
        } else {
            None
        }
    }
}

impl Default for GuestUser {
    /// `agent`, the name bombyx uses when the config sets none.
    fn default() -> Self {
        Self(DEFAULT_GUEST_USER.to_owned())
    }
}

/// Every rule a `guest_user` value must pass, in one place.
fn check_guest_user(value: &str) -> Result<(), FieldError> {
    const FIELD: &str = "guest_user";
    let Some(first) = value.chars().next() else {
        return Err(FieldError::Empty { field: FIELD });
    };
    if !(first.is_ascii_lowercase() || first == '_') {
        return Err(FieldError::invalid(
            FIELD,
            "must start with a lowercase letter or an underscore",
        ));
    }
    if !value.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'
    }) {
        return Err(FieldError::invalid(
            FIELD,
            "must contain only lowercase letters, digits, `_` and `-`",
        ));
    }
    // After the character-set check, so every character is one
    // ASCII byte and the byte count is the character count.
    if value.len() > MAX_GUEST_USER_LEN {
        return Err(FieldError::invalid(
            FIELD,
            format!("must be at most {MAX_GUEST_USER_LEN} characters"),
        ));
    }
    if REFUSED_GUEST_USERS.contains(&value) {
        return Err(FieldError::invalid(
            FIELD,
            format!(
                "must not be `{value}`; the agent needs an account of its own"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guest_user_refuses_the_whole_family_of_bad_names() {
        // Enumerated before the check was written. The name
        // becomes a path segment under /home, a word inside two
        // strings `git` hands to a shell, and a file name in
        // /etc/sudoers.d, so every shape any of those would
        // misread is here.
        let long = "a".repeat(MAX_GUEST_USER_LEN + 1);
        for bad in [
            "",             // empty
            "root",         // the machine itself
            "vagrant",      // the account this setting avoids
            "Agent",        // uppercase
            "9agent",       // leading digit
            "-agent",       // leading hyphen, read as an option
            "ag.ent",       // a dot: sudo skips such a file
            "ag ent",       // whitespace splits the shell word
            "ag$ent",       // a shell would expand it
            "ag/ent",       // a second path segment
            "ag'ent",       // a quote
            "agent$",       // the Samba machine-account form
            "\u{e9}tienne", // non-ASCII
            long.as_str(),  // over the length limit
        ] {
            assert!(GuestUser::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn a_refusal_names_the_rule_the_value_broke() {
        // Twenty non-ASCII characters are forty bytes, so a length
        // check that ran first would call the name too long when
        // the character set is what it broke.
        let wide = "\u{e9}".repeat(20);
        let err = GuestUser::parse(&wide).expect_err("non-ASCII").to_string();
        assert!(err.contains("lowercase"), "{err}");
        let err = GuestUser::parse("vagrant")
            .expect_err("reserved")
            .to_string();
        assert!(err.contains("account of its own"), "{err}");
    }

    #[test]
    fn a_guest_user_accepts_a_portable_name() {
        // The length limit is inclusive.
        let max = "a".repeat(MAX_GUEST_USER_LEN);
        for ok in ["agent", "_svc", "dev-agent", "a1_b-2", max.as_str()] {
            let name = GuestUser::parse(ok)
                .unwrap_or_else(|e| panic!("{ok:?} must pass: {e}"));
            assert_eq!(name.as_str(), ok);
        }
    }

    #[test]
    fn the_default_is_agent_and_passes_its_own_check() {
        let name = GuestUser::default();
        assert_eq!(name.as_str(), "agent");
        assert!(GuestUser::parse(name.as_str()).is_ok());
    }

    #[test]
    fn serde_runs_the_guest_user_check_while_the_value_is_read() {
        // The `try_from` attribute is the only thing that makes
        // the check run during a config load.
        #[derive(Debug, Deserialize)]
        struct Holder {
            #[allow(dead_code)]
            guest_user: GuestUser,
        }
        let err = toml::from_str::<Holder>("guest_user = \"root\"\n")
            .expect_err("root must be refused");
        assert!(err.to_string().contains("guest_user"), "{err}");
    }
}
