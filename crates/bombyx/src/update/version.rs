//! Release versions and the update decision. **Pure.**
//!
//! Nothing here reads the environment, spawns a process or touches a
//! file. Split out on that line rather than by topic: the sibling
//! [`super::swap`] renames and deletes the installed binary, and the
//! two being in one module made "which half of this can hurt me"
//! a question a reader had to answer by scrolling.

use crate::update::REPO_URL;
/// A released version, `MAJOR.MINOR.PATCH`.
///
/// Ordering is derived, and the field order is what makes that
/// correct: `Ord` on a struct compares field by field, so
/// `major` decides first and `patch` last. Reordering the fields
/// would silently make `0.9.0` newer than `1.0.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Incompatible-change component.
    pub major: u64,
    /// Backwards-compatible feature component.
    pub minor: u64,
    /// Fix component.
    pub patch: u64,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Version {
    /// Parses a bare `MAJOR.MINOR.PATCH`, with no `v` prefix.
    ///
    /// Deliberately strict, and the strictness is the feature:
    /// whatever this accepts becomes an argument to
    /// `cargo install --tag`, and a version bombyx invents is a
    /// tag that does not exist. It therefore refuses anything
    /// that is not exactly three runs of ASCII digits --
    /// including a **pre-release suffix**, which is the case that
    /// matters. The release workflow publishes `v1.0.0-rc1` as a
    /// GitHub pre-release precisely so it is not what people
    /// download; an update that jumped onto one would defeat
    /// that.
    ///
    /// A leading zero is refused too, so a version has one
    /// spelling and `01.2.3` cannot compare equal to `1.2.3`
    /// while sorting elsewhere.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let major = component(parts.next()?)?;
        let minor = component(parts.next()?)?;
        let patch = component(parts.next()?)?;
        // A fourth component means this is not a version bombyx
        // knows how to install.
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Renders this version as its git tag, e.g. `v0.2.0`.
    ///
    /// The `v` prefix is what the release workflow's `v*.*.*`
    /// trigger matches, so it is part of the tag's identity
    /// rather than decoration.
    #[must_use]
    pub fn tag(&self) -> String {
        format!("v{self}")
    }
}

/// Parses one dotted component: ASCII digits, no leading zero.
fn component(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    // Refuses a component too large to hold, rather than
    // wrapping into a plausible-looking small number.
    text.parse().ok()
}

/// Extracts the version from one `git ls-remote --tags` line.
///
/// The output is `<sha>\t<ref>`, and only `refs/tags/vX.Y.Z` is a
/// bombyx release. Anything else -- a branch, a tag without the
/// `v`, a pre-release suffix -- yields `None` and is skipped.
///
/// `--refs` suppresses the `^{}` dereference lines that annotated
/// tags otherwise add, but they are rejected here anyway, because
/// this must not depend on a flag being passed to stay correct.
#[must_use]
pub fn version_in_tag_line(line: &str) -> Option<Version> {
    let reference = line.split('\t').nth(1)?.trim();
    let tag = reference.strip_prefix("refs/tags/")?;
    Version::parse(tag.strip_prefix('v')?)
}

/// The newest release in `git ls-remote --tags` output.
///
/// `None` when the repository publishes no parsable release tag,
/// which is a state to report rather than to treat as
/// "up to date".
#[must_use]
pub fn newest_release(ls_remote_stdout: &str) -> Option<Version> {
    ls_remote_stdout
        .lines()
        .filter_map(version_in_tag_line)
        .max()
}

/// What an update check concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The installed version is the newest release.
    UpToDate(Version),
    /// A newer release exists.
    Available {
        /// What is installed now.
        current: Version,
        /// What would be installed.
        latest: Version,
    },
    /// This build is newer than any release.
    ///
    /// Reported rather than acted on. A locally built binary from
    /// a bumped `Cargo.toml` lands here, and installing the
    /// newest *release* over it would be a silent downgrade --
    /// which is the one outcome an update command must never
    /// produce.
    Ahead {
        /// What is installed now.
        current: Version,
        /// The newest release, older than `current`.
        latest: Version,
    },
    /// The repository has no release tags at all.
    NoReleases {
        /// What is installed now.
        current: Version,
    },
}

/// What the caller should do about a [`Decision`], with the words
/// to say about it.
///
/// The wording lives here rather than in the binary because the
/// binary is outside the coverage gate, and these three sentences
/// are the whole operator-facing surface of the decision. Getting
/// `Ahead`'s two versions the wrong way round, for instance, tells
/// a developer their fresh build is out of date; written in the
/// binary, no test asserted which version each sentence named.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Install this version.
    Install(Version),
    /// Nothing to do; print this and exit successfully.
    Nothing(String),
    /// Refuse, with this reason.
    Refuse(String),
}

impl Decision {
    /// The action and the sentence that goes with it.
    #[must_use]
    pub fn outcome(self) -> Outcome {
        match self {
            Decision::UpToDate(version) => Outcome::Nothing(format!(
                "bombyx {version} is the newest release"
            )),
            // Not an error: a locally built binary ahead of the last
            // tag is the normal state while developing. Reported
            // rather than silently downgraded -- a downgrade is the
            // one outcome an update command must never produce.
            Decision::Ahead { current, latest } => Outcome::Nothing(format!(
                "bombyx {current} is newer than the newest release \
                 {latest}; nothing to do"
            )),
            Decision::NoReleases { current } => Outcome::Refuse(format!(
                "{REPO_URL} publishes no release tags, so there is \
                 nothing to update {current} to"
            )),
            Decision::Available { latest, .. } => Outcome::Install(latest),
        }
    }
}

/// Compares the running version with the newest release.
#[must_use]
pub fn decide(current: Version, latest: Option<Version>) -> Decision {
    let Some(latest) = latest else {
        return Decision::NoReleases { current };
    };
    match latest.cmp(&current) {
        std::cmp::Ordering::Greater => Decision::Available { current, latest },
        std::cmp::Ordering::Equal => Decision::UpToDate(current),
        std::cmp::Ordering::Less => Decision::Ahead { current, latest },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn parses_a_plain_version() {
        assert_eq!(Version::parse("0.2.0"), Some(v(0, 2, 0)));
        assert_eq!(Version::parse("1.20.300"), Some(v(1, 20, 300)));
        assert_eq!(Version::parse("0.0.0"), Some(v(0, 0, 0)));
    }

    #[test]
    fn rejects_the_whole_family_of_non_versions() {
        // The family enumerated before the guard, not after: what
        // this accepts becomes a `--tag` argument, so anything it
        // waves through is an install of a tag that does not
        // exist.
        for bad in [
            "",            // nothing at all
            "1",           // too few components
            "1.2",         // still too few
            "1.2.3.4",     // too many
            "a.b.c",       // not numeric
            "1.2.x",       // partly numeric
            "v1.2.3",      // the `v` is stripped by the caller
            "1.2.3-rc1",   // pre-release: must not be installed
            "1.2.3+build", // build metadata
            " 1.2.3",      // leading space
            "1.2.3 ",      // trailing space
            "1..3",        // empty component
            ".1.2",        // empty leading component
            "1.2.",        // empty trailing component
            "01.2.3",      // leading zero, a second spelling
            "1.02.3",      // leading zero elsewhere
            "-1.2.3",      // negative
            "1.2.-3",      // negative component
            // Too large for u64: must be refused rather than
            // wrapped into a small, plausible number.
            "18446744073709551616.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?} must be refused");
        }
    }

    #[test]
    fn zero_is_a_valid_component_on_its_own() {
        // The leading-zero rule must not reject a bare `0`, which
        // every 0.x version has.
        assert_eq!(Version::parse("0.0.1"), Some(v(0, 0, 1)));
    }

    #[test]
    fn orders_by_major_then_minor_then_patch() {
        // The field order in the struct is what makes derived
        // `Ord` correct here, so it is asserted rather than
        // trusted.
        assert!(v(1, 0, 0) > v(0, 9, 9));
        assert!(v(0, 2, 0) > v(0, 1, 99));
        assert!(v(0, 2, 1) > v(0, 2, 0));
        assert!(v(0, 10, 0) > v(0, 9, 0));
    }

    #[test]
    fn renders_the_git_tag_with_its_v() {
        assert_eq!(v(0, 2, 0).tag(), "v0.2.0");
        assert_eq!(v(1, 20, 3).to_string(), "1.20.3");
    }

    #[test]
    fn reads_a_version_out_of_an_ls_remote_line() {
        let line = "75d9e309383254953329c965430fd398a2c8b301\t\
                    refs/tags/v0.2.0";
        assert_eq!(version_in_tag_line(line), Some(v(0, 2, 0)));
    }

    #[test]
    fn skips_ls_remote_lines_that_are_not_releases() {
        for line in [
            "",
            "no-tab-at-all",
            "sha\trefs/heads/main",      // a branch
            "sha\trefs/tags/0.2.0",      // no `v`
            "sha\trefs/tags/nightly",    // not a version
            "sha\trefs/tags/v0.2.0-rc1", // pre-release
            "sha\trefs/tags/v0.2.0^{}",  // deref line
            "sha\trefs/pull/12/head",    // a PR ref
        ] {
            assert_eq!(version_in_tag_line(line), None, "{line:?}");
        }
    }

    /// Real `git ls-remote --tags --refs` output for this repo.
    fn ls_remote() -> &'static str {
        "f494495109766ccdddac40d7f45ac1e85cb13431\trefs/tags/v0.1.0\n\
         75d9e309383254953329c965430fd398a2c8b301\trefs/tags/v0.2.0\n"
    }

    #[test]
    fn picks_the_newest_release() {
        assert_eq!(newest_release(ls_remote()), Some(v(0, 2, 0)));
    }

    #[test]
    fn picks_the_newest_by_version_not_by_line_order() {
        // Tag order in the output is lexicographic, so `v0.10.0`
        // sorts *before* `v0.9.0` there. Taking the last line
        // would install the older one.
        let out = "sha\trefs/tags/v0.10.0\nsha\trefs/tags/v0.9.0\n";
        assert_eq!(newest_release(out), Some(v(0, 10, 0)));
    }

    #[test]
    fn no_parsable_tag_means_no_release() {
        assert_eq!(newest_release(""), None);
        assert_eq!(newest_release("sha\trefs/heads/main\n"), None);
        // A repository whose only tags are pre-releases has
        // nothing this command may install.
        assert_eq!(newest_release("sha\trefs/tags/v1.0.0-rc1\n"), None);
    }

    #[test]
    fn reports_an_available_update() {
        assert_eq!(
            decide(v(0, 1, 0), Some(v(0, 2, 0))),
            Decision::Available {
                current: v(0, 1, 0),
                latest: v(0, 2, 0),
            }
        );
    }

    #[test]
    fn reports_up_to_date_on_an_exact_match() {
        assert_eq!(
            decide(v(0, 2, 0), Some(v(0, 2, 0))),
            Decision::UpToDate(v(0, 2, 0))
        );
    }

    #[test]
    fn refuses_to_treat_a_newer_local_build_as_an_update() {
        // The case that would otherwise be a silent downgrade: a
        // binary built from a bumped Cargo.toml is newer than any
        // tag, and installing the newest release over it would
        // throw away exactly the code being tested.
        assert_eq!(
            decide(v(0, 3, 0), Some(v(0, 2, 0))),
            Decision::Ahead {
                current: v(0, 3, 0),
                latest: v(0, 2, 0),
            }
        );
    }

    #[test]
    fn reports_a_repository_with_no_releases() {
        assert_eq!(
            decide(v(0, 1, 0), None),
            Decision::NoReleases {
                current: v(0, 1, 0),
            }
        );
    }

    #[test]
    fn an_available_release_is_the_one_to_install() {
        assert_eq!(
            decide(v(0, 2, 0), Some(v(0, 3, 0))).outcome(),
            Outcome::Install(v(0, 3, 0))
        );
    }

    #[test]
    fn being_up_to_date_says_so_and_is_not_an_error() {
        let Outcome::Nothing(why) =
            decide(v(0, 3, 0), Some(v(0, 3, 0))).outcome()
        else {
            panic!("up to date must be a no-op, not a refusal");
        };
        assert!(why.contains("0.3.0"), "{why}");
        assert!(why.contains("newest release"), "{why}");
    }

    #[test]
    fn being_ahead_names_both_versions_and_refuses_to_downgrade() {
        // The sentence has to name the *local* version as the newer
        // one. Getting the two the wrong way round would tell a
        // developer their fresh build is out of date, and it is the
        // downgrade direction that must never be taken silently.
        let Outcome::Nothing(why) =
            decide(v(0, 4, 0), Some(v(0, 3, 0))).outcome()
        else {
            panic!("a local build ahead of the tags is not an error");
        };
        assert!(why.starts_with("bombyx 0.4.0 is newer"), "{why}");
        assert!(why.contains("0.3.0"), "{why}");
    }

    #[test]
    fn no_releases_at_all_is_a_refusal_naming_the_repository() {
        let Outcome::Refuse(why) = decide(v(0, 1, 0), None).outcome() else {
            panic!("with nothing to update to, there is nothing to do");
        };
        assert!(why.contains(REPO_URL), "{why}");
        assert!(why.contains("0.1.0"), "{why}");
    }
}
