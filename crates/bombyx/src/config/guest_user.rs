//! The account the agent works as inside the guest.
//!
//! The generated Vagrantfile's privileged provisioner creates
//! this account, and every later step -- the clone, the project's
//! own script, `bombyx shell` -- runs as it. Vagrant keeps logging
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
/// account's sudoers file is named after it.
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
    if value.len() > MAX_GUEST_USER_LEN {
        return Err(FieldError::invalid(
            FIELD,
            format!("must be at most {MAX_GUEST_USER_LEN} characters"),
        ));
    }
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
    if REFUSED_GUEST_USERS.contains(&value) {
        return Err(FieldError::invalid(
            FIELD,
            format!("must not be `{value}`"),
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
