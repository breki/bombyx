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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Over `ssh` to another machine. What this module's
    /// `resolve` answers unless it can demonstrate the other
    /// case.
    Ssh,
    /// Through `sh -c` on this machine, which is the VM host.
    Local,
}

/// Decides the route to `host` from `this_machine`.
///
/// Pure, so the rule can be tested without asking the operating
/// system what it is called. [`this_machine`] is the impure half.
///
/// **The two names must be the same name**, ignoring case and
/// nothing else. `frosti` matches `frosti`; it does not match
/// `frosti.lan`, and `build01.corp.example` does not match
/// `build01.dmz.example`.
///
/// Comparing less than the whole name errs towards the local
/// route, and that is the dangerous direction. A domain is
/// written to say *which* machine, and two machines share a
/// bare label easily -- `ubuntu`, `vagrant`, `build01`.
/// Discarding the half that distinguishes them is how bombyx
/// would boot a guest on the workstation while the operator
/// believed it was elsewhere, and then `rm -rf` the
/// workstation's directory on teardown.
///
/// Exact matching errs the other way: an operator who wants the
/// local route and wrote the name differently gets `ssh`,
/// notices the handshake, and writes what `hostname` prints.
///
/// It is also what makes `you@frosti` force `ssh` from a
/// machine called `frosti`, which `docs/tutorial.md` documents
/// as the escape hatch for an alias that collides with this
/// machine's own name.
///
/// **bombyx never reads `~/.ssh/config`.** So an alias naming
/// this machine but carrying a `User` or a `HostName` that
/// points elsewhere is still taken for this machine. That is
/// the residual hazard, and `you@name` is the answer to it.
///
/// A blank name never matches, whichever side it is on, so an
/// empty `host` cannot make a machine with no readable name
/// look like itself.
///
/// `this_machine` is `None` when the name could not be read, and
/// that answers [`Transport::Ssh`]: bombyx changes route only on
/// a match it can demonstrate.
pub(crate) fn resolve(host: &str, this_machine: Option<&str>) -> Transport {
    resolve_on(host, this_machine, cfg!(windows))
}

/// [`resolve`], told whether it is running on Windows.
///
/// The platform is a parameter so both answers stay testable
/// from either machine. `resolve` supplies the real one.
fn resolve_on(
    host: &str,
    this_machine: Option<&str>,
    windows: bool,
) -> Transport {
    // A Windows machine cannot be its own VM host. The script
    // bombyx builds drives a libvirt guest, and libvirt does
    // not run on Windows; a project configured for Hyper-V does
    // not change that, because the local route would still be
    // running the same script here.
    //
    // Refusing it outright matters because the route would
    // otherwise half-work rather than fail. Git for Windows
    // puts an `sh.exe` on `PATH`, so the writes would land in
    // the MSYS home where the operator will not find them, and
    // `destroy` would remove that directory.
    if windows {
        return Transport::Ssh;
    }
    let Some(here) = this_machine else {
        return Transport::Ssh;
    };
    // `config::host` restricts `host` to ASCII, so a case fold
    // that only knows ASCII cannot answer wrongly: a non-ASCII
    // machine name can only fail to match, which is the safe
    // direction.
    if !host.is_empty() && host.eq_ignore_ascii_case(here) {
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
    use super::{Transport, resolve_on};

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
            ("frosti.lan", Some("FROSTI.lan"), Transport::Local),
            // A domain is part of the name. Dropping it, on
            // either side, is a match bombyx cannot demonstrate.
            ("frosti", Some("frosti.lan"), Transport::Ssh),
            ("frosti.lan", Some("frosti"), Transport::Ssh),
            // Two machines sharing a bare label. This is the
            // case exact matching exists for: they are in
            // different networks and are not the same machine.
            (
                "build01.dmz.example",
                Some("build01.corp.example"),
                Transport::Ssh,
            ),
            // A bare alias colliding with this machine's own
            // name is the residual hazard: bombyx does not read
            // `~/.ssh/config`, so it believes the name.
            ("build01", Some("build01"), Transport::Local),
            // `you@name` is the escape hatch from that, pinned
            // here so a later tidy-up cannot delete it silently.
            ("igor@frosti", Some("frosti"), Transport::Ssh),
            ("igor@frosti.lan", Some("frosti.lan"), Transport::Ssh),
            // A different machine.
            ("vmhost", Some("frosti"), Transport::Ssh),
            // A prefix, a suffix and a substring are not matches.
            ("frost", Some("frosti"), Transport::Ssh),
            ("frostier", Some("frosti"), Transport::Ssh),
            ("frosti-local", Some("frosti"), Transport::Ssh),
            // Nothing to compare against.
            ("frosti", None, Transport::Ssh),
            // Blank on either side, and on both. Two unreadable
            // names must not look like one machine.
            ("", Some("frosti"), Transport::Ssh),
            ("frosti", Some(""), Transport::Ssh),
            ("", Some(""), Transport::Ssh),
            // Degenerate spellings that are equal as strings.
            // `config::host` accepts both, so they can reach
            // here from a file. They match, and that is
            // harmless for one reason only: no operating system
            // reports `.` or a trailing-dot name as its own, so
            // the second half of the comparison never supplies
            // them.
            (".", Some("."), Transport::Local),
            ("frosti.", Some("frosti."), Transport::Local),
            // A trailing dot is a different string, so the
            // absolute and relative spellings do not match.
            ("frosti.lan.", Some("frosti.lan"), Transport::Ssh),
            // Non-ASCII cannot reach `host` at all
            // (`config::host` restricts the charset), and an
            // ASCII-only fold can only fail to match it.
            ("fr\u{f6}sti", Some("FR\u{d6}STI"), Transport::Ssh),
        ];
        for (host, here, want) in cases {
            assert_eq!(
                resolve_on(host, here, false),
                want,
                "{host:?} on {here:?}"
            );
        }
    }

    #[test]
    fn windows_never_takes_the_local_route() {
        // A match that would be `Local` anywhere else. Windows
        // cannot run libvirt, so the local route there is only
        // ever a mistake -- and a quiet one, because Git for
        // Windows supplies the `sh` it would spawn.
        assert_eq!(resolve_on("frosti", Some("frosti"), true), Transport::Ssh);
        assert_eq!(
            resolve_on("frosti", Some("frosti"), false),
            Transport::Local
        );
    }
}
