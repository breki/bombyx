//! Which git hosts bombyx can verify before it clones, and
//! where each one publishes its ssh host keys.
//!
//! The guest has never met the git server when it clones, so
//! `ssh` has nothing to compare the offered key against. What
//! closes that is fetching the host's published keys over
//! HTTPS, whose trust comes from a certificate authority rather
//! than from whatever answers on port 22.
//!
//! This module holds the table and nothing else. The request
//! itself happens in the guest, because the guest is the
//! machine that has to trust the answer: keys fetched on the
//! workstation would have to survive two more hand-overs to
//! get there. So the three values here reach the guest as
//! environment variables in the generated Vagrantfile, and
//! `templates/bootstrap.sh` runs the fetch.
//!
//! It also means bombyx needs no HTTP client and no JSON
//! parser, which suits it: `crate::update` reaches for
//! `git ls-remote` over the GitHub releases API for the same
//! kind of reason.
//!
//! A host missing from the table gets no verification. The
//! guest falls back to `StrictHostKeyChecking=accept-new`,
//! which trusts the key it is offered on first sight;
//! `docs/trust-boundary.md` says what that costs.

/// The shape a host serves its keys in.
///
/// The two differ enough that the guest cannot treat them
/// alike, so the table records which is which and
/// `templates/bootstrap.sh` branches on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFormat {
    /// A JSON document with the keys in an `ssh_keys` array,
    /// each entry a bare `<type> <base64>` pair. The host name
    /// has to be put in front of each one to make a
    /// `known_hosts` line, and reading the array needs `jq`.
    Json,
    /// Finished `known_hosts` lines, host name included. The
    /// guest saves the response and needs nothing else.
    Lines,
}

impl KeyFormat {
    /// The spelling that reaches the guest.
    ///
    /// `templates/bootstrap.sh` matches these two words, so
    /// they are part of the agreement between the two files
    /// rather than a display detail.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Lines => "lines",
        }
    }
}

/// One git host bombyx knows how to verify.
///
/// **The three fields are private, and `KNOWN` is the only
/// thing that builds one.** So holding a `HostKeys` is the
/// proof that its values came from that table, and the
/// accessors below can promise what the table's own tests
/// check. Public fields would let any caller -- this module is
/// `pub`, so that includes one outside bombyx -- assemble a
/// `HostKeys` naming an `http` URL, and the promise would then
/// be a comment rather than a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostKeys {
    host: &'static str,
    url: &'static str,
    format: KeyFormat,
}

impl HostKeys {
    /// The host name, lower-cased.
    ///
    /// This is what goes in front of each key in the guest's
    /// `known_hosts` file, rather than the spelling the
    /// operator used. OpenSSH compares a host name without
    /// regard to case -- measured with
    /// `ssh-keygen -F BitBucket.org` against a lower-cased
    /// file -- so the canonical form matches either way.
    #[must_use]
    pub fn host(self) -> &'static str {
        self.host
    }

    /// Where the host publishes its keys.
    ///
    /// Always `https`. `every_entry_is_fetched_over_https` in
    /// this module checks every entry of `KNOWN`, and nothing
    /// else can build a `HostKeys`.
    #[must_use]
    pub fn url(self) -> &'static str {
        self.url
    }

    /// What the response looks like.
    #[must_use]
    pub fn format(self) -> KeyFormat {
        self.format
    }
}

/// Every host in the table.
///
/// Two entries, and adding a third means finding out how that
/// host publishes its keys and whether it needs a new
/// [`KeyFormat`]. Endpoints move, so an entry that stops
/// working shows up as a refused clone in the guest with the
/// URL in the message.
const KNOWN: [HostKeys; 2] = [
    HostKeys {
        host: "github.com",
        url: "https://api.github.com/meta",
        format: KeyFormat::Json,
    },
    HostKeys {
        host: "bitbucket.org",
        url: "https://bitbucket.org/site/ssh",
        format: KeyFormat::Lines,
    },
];

/// The table entry for `host`, when there is one.
///
/// `None` for every host bombyx has not been taught, which
/// includes a self-hosted git server. There is no published
/// key source to reach for in that case.
///
/// **`host` is a `&str` rather than a type of its own**, and
/// the reason is the second of the three `CLAUDE.md` allows: a
/// value built and unwrapped in the same breath with nothing in
/// between. `crate::config::RepoUrl::ssh_host` produces it and
/// this function consumes it, in one expression in
/// `crate::vagrantfile::render`. A second consumer would change
/// that, because a bare `&str` cannot tell a host name from a
/// whole repository URL and the wrong argument here returns
/// `None` -- which is the answer that silently switches
/// verification off.
///
/// The comparison ignores case, because DNS does. ASCII is
/// enough: a host name reaches the wire as ASCII, and an
/// internationalised one arrives already punycoded.
///
/// It is an equality test and never a suffix test.
/// `github.com.example.invalid` is somebody else's domain, and
/// a lookalike is the thing this whole mechanism exists to
/// refuse.
#[must_use]
pub fn for_host(host: &str) -> Option<HostKeys> {
    KNOWN
        .iter()
        .copied()
        .find(|k| k.host.eq_ignore_ascii_case(host))
}

#[cfg(test)]
mod tests {
    use super::{KNOWN, KeyFormat, for_host};

    #[test]
    fn the_two_hosts_in_the_table_are_found() {
        let gh = for_host("github.com").expect("github.com is in the table");
        assert_eq!(gh.url(), "https://api.github.com/meta");
        assert_eq!(gh.format(), KeyFormat::Json);

        let bb =
            for_host("bitbucket.org").expect("bitbucket.org is in the table");
        assert_eq!(bb.url(), "https://bitbucket.org/site/ssh");
        assert_eq!(bb.format(), KeyFormat::Lines);
    }

    #[test]
    fn a_host_written_in_another_case_is_the_same_host() {
        // DNS does not distinguish these, so neither may the
        // table. Withholding verification because somebody
        // capitalised their config would be the worst kind of
        // silent gap: the clone still works.
        for spelling in ["GitHub.com", "GITHUB.COM", "gitHub.Com"] {
            let found = for_host(spelling)
                .unwrap_or_else(|| panic!("{spelling:?} must be found"));
            assert_eq!(found.host(), "github.com", "{spelling:?}");
        }
    }

    #[test]
    fn a_host_the_table_does_not_hold_is_not_guessed_at() {
        for unknown in [
            "gitlab.com",
            "git.example.invalid",
            // A suffix match would let this through, and a
            // lookalike domain is exactly what the check is
            // meant to stop.
            "github.com.example.invalid",
            "notgithub.com",
            "",
        ] {
            assert_eq!(for_host(unknown), None, "{unknown:?}");
        }
    }

    #[test]
    fn every_entry_is_fetched_over_https() {
        // The trust in this whole mechanism is the certificate
        // authority's. An `http` URL in the table would leave
        // the guest verifying one unauthenticated answer
        // against another.
        for entry in KNOWN {
            assert!(
                entry.url().starts_with("https://"),
                "{}: {}",
                entry.host(),
                entry.url()
            );
        }
    }

    #[test]
    fn each_host_name_is_already_canonical() {
        // `for_host` returns this to the guest as the
        // `known_hosts` prefix, so an entry written with a
        // capital would put a name in the file that the
        // canonical form no longer matches.
        for entry in KNOWN {
            assert_eq!(
                entry.host(),
                entry.host().to_lowercase(),
                "{} is not lower-cased",
                entry.host()
            );
        }
    }
}
