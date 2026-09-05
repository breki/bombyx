//! How bombyx reaches the VM host: over `ssh`, or straight here.
//!
//! `host` names an SSH alias, and the usual case is a second
//! machine. It is also legal for the operator to run bombyx on
//! the machine that runs the VMs, and then an `ssh` hop leaves
//! and re-enters the same computer: it needs a key authorized
//! to the operator's own account, a `Host` block pointing at
//! loopback, and a handshake for every command bombyx emits.
//!
//! bombyx compares `host` against this machine's own name and
//! runs the script through `sh -c` when they match. The script
//! itself is unchanged. Every VM action already builds a POSIX
//! shell *script string* and hands it to the far shell, and
//! `sh -c` supplies the same shell, so the quoting, the
//! heredocs, the `cd` and the `$(hostname -s)` all keep working
//! the way they do over `ssh`.
//!
//! **What this costs.** One `config.toml` now behaves
//! differently depending on which machine reads it, and the
//! configuration it makes reachable without ceremony is the
//! weakest one -- `docs/architecture.md` says what running the
//! guest on your own workstation gives up. `bombyx doctor` and
//! `--dry-run` both state which route is in force, so the
//! difference is visible rather than silent.

/// How bombyx runs the script it built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    /// Over `ssh` to another machine. The default: bombyx
    /// takes the local route only after proving it may.
    #[default]
    Ssh,
    /// Through `sh -c` on this machine, which is the VM host.
    Local,
}

/// The short form of a machine name, lowercased.
///
/// A machine answers to several spellings of one name:
/// `frosti`, `frosti.lan`, `Frosti`. bombyx compares the part
/// before the first dot, in lower case, because that is the
/// spelling an operator writes in `config.toml` and the same
/// one `$(hostname -s)` produces on the far side.
///
/// Returns `None` when nothing is left to compare, which covers
/// the empty string and a name starting with a dot.
fn short_name(name: &str) -> Option<String> {
    let head = name.split('.').next()?;
    (!head.is_empty()).then(|| head.to_ascii_lowercase())
}

/// Decides the route to `host` from `this_machine`.
///
/// Pure, so the rule can be tested without asking the operating
/// system what it is called. [`this_machine`] is the impure half.
///
/// `this_machine` is `None` when the name could not be read, and
/// that answers [`Transport::Ssh`]: bombyx changes route only on
/// a match it can demonstrate.
pub(crate) fn resolve(host: &str, this_machine: Option<&str>) -> Transport {
    let (Some(there), Some(here)) =
        (short_name(host), this_machine.and_then(short_name))
    else {
        return Transport::Ssh;
    };
    if there == here {
        Transport::Local
    } else {
        Transport::Ssh
    }
}

/// This machine's name, as the operating system reports it.
///
/// `None` when the name is not valid UTF-8. bombyx compares it
/// against a `host` that came out of a TOML file, which is UTF-8
/// by definition, so a name that is not cannot match one and the
/// comparison is better skipped than lossily repaired.
pub(crate) fn this_machine() -> Option<String> {
    gethostname::gethostname().into_string().ok()
}

#[cfg(test)]
mod tests {
    use super::{Transport, resolve};

    #[test]
    fn resolve_takes_the_local_route_only_on_a_demonstrated_match() {
        // The whole family the rule claims to cover. `host` is
        // whatever `config.toml` said; the second column is what
        // this machine answers to.
        let cases = [
            ("frosti", Some("frosti"), Transport::Local),
            // Neither spelling is authoritative, so case is
            // ignored in both directions.
            ("Frosti", Some("frosti"), Transport::Local),
            ("frosti", Some("FROSTI"), Transport::Local),
            // A domain on either side, or on both.
            ("frosti.lan", Some("frosti"), Transport::Local),
            ("frosti", Some("frosti.lan"), Transport::Local),
            ("frosti.lan", Some("frosti.home"), Transport::Local),
            // A different machine.
            ("vmhost", Some("frosti"), Transport::Ssh),
            // A prefix is not a match.
            ("frost", Some("frosti"), Transport::Ssh),
            // An alias pointing at loopback is still an alias.
            // The operator asked for `ssh` and gets it.
            ("frosti-local", Some("frosti"), Transport::Ssh),
            // Nothing to compare against.
            ("frosti", None, Transport::Ssh),
            // Degenerate names. `.` and `.lan` have no short
            // form, and two of them must not match each other.
            ("", Some("frosti"), Transport::Ssh),
            ("frosti", Some(""), Transport::Ssh),
            (".", Some("."), Transport::Ssh),
            (".lan", Some(".lan"), Transport::Ssh),
            ("", Some(""), Transport::Ssh),
        ];
        for (host, here, want) in cases {
            assert_eq!(resolve(host, here), want, "{host:?} on {here:?}");
        }
    }

    #[test]
    fn the_default_route_is_the_network_one() {
        assert_eq!(Transport::default(), Transport::Ssh);
    }
}
