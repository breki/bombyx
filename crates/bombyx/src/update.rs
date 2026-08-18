//! Updating the installed bombyx binary.
//!
//! Split by **effect**, not by topic, because the difference is
//! worth knowing before reading any of it:
//!
//! - `version` is pure. Release tags, version comparison, and the
//!   decision -- including its refusal to downgrade. Reads nothing,
//!   spawns nothing, writes nothing.
//! - `swap` finds the installed binary and **replaces** it.
//!   [`move_aside`], [`restore`], [`place`] and [`sweep_aside`]
//!   rename and *delete* files. This is the only filesystem
//!   mutation in the crate's command-building layer.
//! - [`asset`] builds the download and extract argv, and owns
//!   checksum verification.
//! - This module itself holds the `git` and `cargo` command lines
//!   and re-exports the rest, so `bombyx::update::Version` and
//!   `bombyx::update::place` are unchanged paths.
//!
//! The newest release is found with `git ls-remote --tags`, which
//! needs no HTTP client, no releases-API call and no token. The
//! archive itself is downloaded and checked against the release's
//! `SHA256SUMS` -- see [`asset`], which also explains why
//! verification fails closed.
//!
//! [`install_command`] builds a `cargo install --git --tag` line
//! that is **printed, never run**: it is the manual fallback
//! offered when a release predates the checksum file, since
//! updating from an unverified download is not a trade this makes
//! on the operator's behalf.

pub mod asset;
mod swap;
mod version;

// Re-exported, so `bombyx::update::Version` and
// `bombyx::update::place` keep working: the split is about where the
// code lives, not about the paths callers use.
pub use swap::{
    BINARY, MovedAside, Placed, UpdateError, install_dir, move_aside,
    needs_moving_aside, place, restore, running_dir, sweep_aside,
};
pub use version::{
    Decision, Outcome, Version, decide, newest_release, version_in_tag_line,
};

use crate::remote::RemoteCommand;

/// Where releases come from.
///
/// Taken from the package manifest rather than written out again,
/// so the repository has one spelling. A fork inherits its own.
pub const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");

/// The version this binary was built as.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Builds the command that lists the repository's release tags.
///
/// `git ls-remote` rather than the GitHub releases API: it needs
/// no HTTP client, no JSON parsing and no token, and `git` is
/// already a program this project assumes.
#[must_use]
pub fn list_releases_command() -> RemoteCommand {
    RemoteCommand::new("git", &["ls-remote", "--tags", "--refs", REPO_URL])
}

/// Builds the `cargo install` line for `version`.
///
/// **Printed as a suggestion, not executed.** It is what an
/// operator runs by hand when a release has no `SHA256SUMS` to
/// verify against, which every release cut before that file
/// existed does not.
///
/// `--locked` builds from the lockfile the release was tagged
/// with, so the binary matches the dependency set that passed the
/// release gate -- the same reason the release workflow passes it.
/// `--force` is required because the destination already holds a
/// `bombyx`, and without it `cargo install` refuses rather than
/// replacing. The package is named explicitly because the
/// repository is a workspace, and `xtask` is a member of it.
#[must_use]
pub fn install_command(version: Version) -> RemoteCommand {
    RemoteCommand::new(
        "cargo",
        &[
            "install",
            "--git",
            REPO_URL,
            "--tag",
            &version.tag(),
            "--locked",
            "--force",
            "bombyx",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version, spelled out so the argv assertions read as data.
    fn v(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn lists_releases_over_git_rather_than_http() {
        let c = list_releases_command();
        assert_eq!(c.program, "git");
        assert_eq!(c.args[0], "ls-remote");
        assert!(c.args.iter().any(|a| a == "--tags"));
        assert!(c.args.iter().any(|a| a == "--refs"));
        assert!(c.args.iter().any(|a| a == REPO_URL));
    }

    #[test]
    fn installs_the_named_tag_from_the_lockfile() {
        let c = install_command(v(0, 2, 0));
        assert_eq!(c.program, "cargo");
        assert_eq!(
            c.args,
            vec![
                "install", "--git", REPO_URL, "--tag", "v0.2.0", "--locked",
                "--force", "bombyx",
            ]
        );
    }

    #[test]
    fn the_repo_url_comes_from_the_manifest() {
        // Written out again, a fork would keep updating itself
        // from upstream. This is the assertion that the value is
        // taken from the package rather than pasted.
        assert_eq!(REPO_URL, env!("CARGO_PKG_REPOSITORY"));
        assert!(REPO_URL.starts_with("https://"), "{REPO_URL}");
    }

    #[test]
    fn the_current_version_parses() {
        // `CURRENT` is the crate's own version, so if this ever
        // fails the manifest carries something `--tag` could not
        // name.
        assert!(Version::parse(CURRENT).is_some(), "{CURRENT}");
    }
}
