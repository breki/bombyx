//! Which git hosts bombyx can verify before it clones, and
//! where each one publishes its ssh host keys.
//!
//! The guest has never met the git server when it clones, so
//! `ssh` has nothing to compare the offered key against. What
//! closes that is fetching the host's published keys over
//! HTTPS, whose trust comes from a certificate authority rather
//! than from whatever answers on port 22.
//!
//! This module holds the table and nothing else. bombyx makes
//! no HTTPS request of its own -- it has no HTTP client and no
//! JSON parser, and `crate::update` explains why that is
//! deliberate. The fetch happens in the guest, and
//! `templates/bootstrap.sh` is what runs it. So the two values
//! here reach the guest as environment variables in the
//! generated Vagrantfile.
//!
//! A host missing from the table gets no verification, and
//! `crate::vagrantfile` says what the guest falls back to.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostKeys {
    /// The host name, lower-cased.
    ///
    /// This is what goes in front of each key in the guest's
    /// `known_hosts` file, rather than the spelling the
    /// operator used. OpenSSH compares a host name without
    /// regard to case -- measured with `ssh-keygen -F
    /// BitBucket.org` against a lower-cased file -- so the
    /// canonical form matches either way.
    pub host: &'static str,
    /// Where the host publishes its keys. Always `https`, and
    /// a test in this module refuses anything else.
    pub url: &'static str,
    /// What the response looks like.
    pub format: KeyFormat,
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
        assert_eq!(gh.url, "https://api.github.com/meta");
        assert_eq!(gh.format, KeyFormat::Json);

        let bb =
            for_host("bitbucket.org").expect("bitbucket.org is in the table");
        assert_eq!(bb.url, "https://bitbucket.org/site/ssh");
        assert_eq!(bb.format, KeyFormat::Lines);
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
            assert_eq!(found.host, "github.com", "{spelling:?}");
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
                entry.url.starts_with("https://"),
                "{}: {}",
                entry.host,
                entry.url
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
                entry.host,
                entry.host.to_lowercase(),
                "{} is not lower-cased",
                entry.host
            );
        }
    }
}
