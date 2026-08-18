//! Updating the installed bombyx binary.
//!
//! Two halves, and they differ in a way worth knowing before
//! reading further:
//!
//! - **Deciding, and building argv.** Parsing release tags,
//!   comparing versions, refusing a downgrade, and the `git` and
//!   `cargo` command lines. Like the rest of the crate this spawns
//!   nothing; it returns the argv, which is what makes it testable
//!   without a network.
//! - **Moving files.** [`move_aside`], [`restore`], [`place`] and
//!   [`sweep_aside`] rename and *delete* files in the directory
//!   holding the installed binary. This is the only part of the
//!   command-building layer that touches the filesystem, so it is
//!   called out rather than left for a reader to discover.
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

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::remote::RemoteCommand;

/// Where releases come from.
///
/// Taken from the package manifest rather than written out again,
/// so the repository has one spelling. A fork inherits its own.
pub const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");

/// The version this binary was built as.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

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

/// Name of the installed binary on this platform.
pub const BINARY: &str = if cfg!(windows) {
    "bombyx.exe"
} else {
    "bombyx"
};

/// Marks a binary moved aside by an update.
const ASIDE_PREFIX: &str = ".old-";

/// Errors from moving the installed binary around.
#[derive(Debug, Error)]
pub enum UpdateError {
    /// The binary could not be moved aside.
    #[error("moving {} aside: {source}", .path.display())]
    MoveAside {
        /// The binary that could not be moved.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The new binary could not be put in place.
    ///
    /// The old one has been restored by the time this is
    /// returned, so the operator still has a working `bombyx`.
    #[error("installing {}: {source}", .path.display())]
    Place {
        /// Where the new binary was to go.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// A failed update could not put the binary back.
    ///
    /// The worst outcome this module can produce, so it names
    /// both paths: the operator has to finish the rename by hand,
    /// and cannot without knowing where it went.
    #[error(
        "the update failed and {} could not be restored to {}: \
         {source} -- rename it back by hand",
        .aside.display(),
        .original.display()
    )]
    Restore {
        /// Where the binary currently sits.
        aside: PathBuf,
        /// Where it belongs.
        original: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

/// Returns the directory `cargo install` writes binaries into.
///
/// `CARGO_HOME/bin` when that is set, else `~/.cargo/bin`.
/// `None` when the environment names neither, which is a machine
/// this cannot guess about.
#[must_use]
pub fn install_dir() -> Option<PathBuf> {
    install_dir_from(|key| std::env::var(key).ok(), cfg!(windows))
}

/// [`install_dir`] against an arbitrary environment.
///
/// Split out so the precedence is testable without mutating the
/// process environment, which is global and would make these
/// tests race each other -- the same reason
/// `config::user_config_dir` is split this way. `windows` is split
/// out for the same reason and decides the home-directory order.
///
/// A relative value counts as unset, for the reason
/// [`crate::config::is_anchored_dir`] exists: a relative
/// `CARGO_HOME` resolves against the working directory, and this
/// module *renames a file* in the directory it returns. Pointed
/// at a repository, that would move something out of a checkout.
///
/// **On Windows `USERPROFILE` outranks `HOME`, and `HOME` is not
/// consulted at all elsewhere.** Both halves matter, and both
/// mirror what `config::config_dir_from` already does with
/// `APPDATA`:
///
/// - Git Bash sets *both*, with `HOME` in POSIX form
///   (`/c/Users/igor`). MSYS converts that on the way out to a
///   native child, so a Rust binary launched from that shell
///   usually sees the Windows spelling -- but nothing guarantees
///   it, and a `/`-rooted value satisfies `is_anchored_dir` while
///   Windows resolves it against the *current drive*, so
///   `/c/Users/igor` becomes `D:\c\Users\igor` when the working
///   directory is on `D:`. Preferring `USERPROFILE` removes the
///   question.
/// - `USERPROFILE` is routinely exported into non-Windows
///   processes -- under WSL via `WSLENV`, under Wine, in CI images
///   -- so consulting it there would let a Windows home directory
///   outrank `$HOME`, which is the exact trap `APPDATA` set for
///   the config lookup.
fn install_dir_from<F>(var: F, windows: bool) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    let set = |key: &str| {
        var(key)
            .filter(|v| crate::config::is_anchored_dir(v))
            .map(|v| v.trim().to_owned())
    };

    if let Some(home) = set("CARGO_HOME") {
        return Some(Path::new(&home).join("bin"));
    }
    let home = if windows {
        set("USERPROFILE").or_else(|| set("HOME"))?
    } else {
        set("HOME")?
    };
    Some(Path::new(&home).join(".cargo").join("bin"))
}

/// A binary renamed so a running copy can be replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedAside {
    /// Where the binary now sits.
    pub aside: PathBuf,
    /// Where it came from, and where [`restore`] puts it back.
    pub original: PathBuf,
}

/// Whether the installed binary must be renamed before an update.
///
/// Windows only, and not out of caution. Windows refuses to
/// overwrite the image of a **running** process, so
/// `cargo install` fails with `Access is denied (os error 5)`
/// while any bombyx runs -- including the `bombyx self-update`
/// doing the updating, which is the same file. Measured on
/// Windows 11: the overwrite is refused and a *rename* of the
/// same running binary is allowed, which is what makes this
/// work at all.
///
/// Unix needs none of it. Replacing a path there unlinks the old
/// inode and leaves running processes on it, so `cargo install`
/// already succeeds and a rename would only litter the directory.
#[must_use]
pub fn needs_moving_aside() -> bool {
    cfg!(windows)
}

/// Renames the installed binary out of the way.
///
/// `Ok(None)` when nothing is installed yet: `cargo install` will
/// simply create it, and there is nothing to move or restore.
///
/// `unique` distinguishes this update from any other, for the
/// same reason [`crate::remote::PushArchive`] takes one: a
/// leftover from an earlier update may still be mapped by a
/// running process and therefore impossible to delete *or*
/// overwrite, so a fixed name would eventually fail.
///
/// # Errors
///
/// [`UpdateError::MoveAside`] when the rename fails.
pub fn move_aside(
    dir: &Path,
    unique: &str,
) -> Result<Option<MovedAside>, UpdateError> {
    let original = dir.join(BINARY);
    if !original.is_file() {
        return Ok(None);
    }
    let aside = dir.join(format!("{BINARY}{ASIDE_PREFIX}{unique}"));
    std::fs::rename(&original, &aside).map_err(|source| {
        UpdateError::MoveAside {
            path: original.clone(),
            source,
        }
    })?;
    Ok(Some(MovedAside { aside, original }))
}

/// Puts a moved-aside binary back, after a failed update.
///
/// # Errors
///
/// [`UpdateError::Restore`], which names both paths: at that
/// point the operator has no working binary and has to finish
/// the rename themselves.
pub fn restore(moved: &MovedAside) -> Result<(), UpdateError> {
    std::fs::rename(&moved.aside, &moved.original).map_err(|source| {
        UpdateError::Restore {
            aside: moved.aside.clone(),
            original: moved.original.clone(),
            source,
        }
    })
}

/// Moves a freshly extracted binary into `dir`, replacing what is
/// there.
///
/// The whole sequence, in one place so its ordering is testable:
/// sweep old leftovers, move the current binary aside where the
/// platform needs it, put the new one in place, and drop the
/// copy that was moved aside. **If the placement fails, the old
/// binary is put back** -- the alternative is leaving nothing on
/// the `PATH`.
///
/// `new_binary` is where extraction left it; `dir` is where the
/// installed one lives.
///
/// A plain rename is tried first and a copy is the fallback,
/// because the two paths can be on different volumes: the archive
/// is unpacked in a temporary directory, and `TMP` on Windows is
/// routinely a different drive from `%USERPROFILE%`, where
/// `std::fs::rename` fails with a cross-device error rather than
/// falling back on its own.
///
/// # Errors
///
/// [`UpdateError::MoveAside`] if the current binary cannot be
/// moved out of the way, [`UpdateError::Place`] if the new one
/// cannot be put in place (after the old one has been restored),
/// and [`UpdateError::Restore`] if restoring it also failed --
/// which is the one case that leaves the operator with work to do
/// by hand, and says so.
pub fn place(
    new_binary: &Path,
    dir: &Path,
    unique: &str,
) -> Result<Placed, UpdateError> {
    let swept = sweep_aside(dir);
    let moved = move_aside(dir, unique)?;
    let target = dir.join(BINARY);

    if let Err(source) = move_file(new_binary, &target) {
        // Put the old binary back before reporting, so the failure
        // costs the operator nothing.
        if let Some(moved) = &moved {
            restore(moved)?;
        }
        return Err(UpdateError::Place {
            path: target,
            source,
        });
    }

    // The old copy is usually still mapped by this very process on
    // Windows, so it cannot be deleted yet. Reported rather than
    // treated as a failure: the update itself succeeded, and the
    // next run's sweep clears it.
    let leftover = moved
        .filter(|m| std::fs::remove_file(&m.aside).is_err())
        .map(|m| m.aside);

    Ok(Placed { swept, leftover })
}

/// What [`place`] did, for the caller to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Superseded binaries deleted before this update.
    pub swept: usize,
    /// The replaced binary, when it could not be deleted yet.
    pub leftover: Option<PathBuf>,
}

/// Renames `from` to `to`, copying when they are on different
/// volumes.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    // A failure to remove the source leaves a file in a temporary
    // directory that is about to be deleted anyway, so it is not
    // worth failing the update over.
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Deletes binaries left behind by earlier updates, and returns
/// how many went away.
///
/// Best effort on purpose. A leftover is still mapped by whatever
/// process was running when it was moved, and Windows refuses to
/// delete a mapped image -- so a failure here means "that copy is
/// still in use", which is not a reason to fail an update. The
/// next run tries again.
///
/// Only names this module creates are considered, so nothing else
/// in `~/.cargo/bin` can be caught by it.
pub fn sweep_aside(dir: &Path) -> usize {
    let prefix = format!("{BINARY}{ASIDE_PREFIX}");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(&prefix))
        })
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
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

    /// A directory holding an installed binary.
    fn installed() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(BINARY);
        std::fs::write(&bin, b"old binary").unwrap();
        (dir, bin)
    }

    #[test]
    fn moves_the_binary_aside_and_back() {
        let (dir, bin) = installed();

        let moved = move_aside(dir.path(), "42").unwrap().expect("was there");
        assert!(!bin.exists(), "the original must be out of the way");
        assert!(moved.aside.is_file());
        assert_eq!(moved.original, bin);
        // The bytes have to survive: this is the copy a failed
        // update puts back.
        assert_eq!(std::fs::read(&moved.aside).unwrap(), b"old binary");

        restore(&moved).unwrap();
        assert!(bin.is_file(), "restore must put it back");
        assert!(!moved.aside.exists());
        assert_eq!(std::fs::read(&bin).unwrap(), b"old binary");
    }

    #[test]
    fn moving_aside_nothing_is_not_an_error() {
        // A first install has no binary to move. Treating that as
        // an error would make `self-update` fail on exactly the
        // machine where it has least to undo.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(move_aside(dir.path(), "42").unwrap(), None);
    }

    #[test]
    fn each_update_moves_aside_under_its_own_name() {
        // A fixed name would collide with a leftover that is still
        // mapped by a running process, and therefore can be
        // neither deleted nor overwritten.
        let (dir, bin) = installed();
        let first = move_aside(dir.path(), "1").unwrap().unwrap();
        std::fs::write(&bin, b"newer").unwrap();
        let second = move_aside(dir.path(), "2").unwrap().unwrap();

        assert_ne!(first.aside, second.aside);
        assert!(first.aside.is_file(), "the first must survive");
        assert!(second.aside.is_file());
    }

    #[test]
    fn sweeps_only_the_leftovers_it_made() {
        let (dir, bin) = installed();
        let moved = move_aside(dir.path(), "7").unwrap().unwrap();
        std::fs::write(&bin, b"new binary").unwrap();

        // Bystanders that must be left alone: the live binary, an
        // unrelated tool, and a name that merely looks similar.
        let other = dir.path().join("ripgrep.exe");
        std::fs::write(&other, b"x").unwrap();
        let lookalike = dir.path().join("bombyx-helper.old-7");
        std::fs::write(&lookalike, b"x").unwrap();

        assert_eq!(sweep_aside(dir.path()), 1);
        assert!(!moved.aside.exists(), "the leftover must be gone");
        assert!(bin.is_file(), "the live binary must survive");
        assert!(other.is_file(), "an unrelated tool must survive");
        assert!(lookalike.is_file(), "a lookalike must survive");
    }

    #[test]
    fn places_the_new_binary_and_clears_the_old_one() {
        let (dir, bin) = installed();
        let work = tempfile::tempdir().unwrap();
        let fresh = work.path().join(BINARY);
        std::fs::write(&fresh, b"new binary").unwrap();

        let placed = place(&fresh, dir.path(), "1").unwrap();
        assert_eq!(std::fs::read(&bin).unwrap(), b"new binary");
        assert!(!fresh.exists(), "the staged copy must be consumed");
        assert_eq!(placed.swept, 0);
        // Nothing holds the old copy in a test, so it goes now.
        assert_eq!(placed.leftover, None);
    }

    #[test]
    fn places_into_a_directory_with_no_binary_yet() {
        // A first install has nothing to move aside.
        let dir = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let fresh = work.path().join(BINARY);
        std::fs::write(&fresh, b"new binary").unwrap();

        place(&fresh, dir.path(), "1").unwrap();
        assert_eq!(
            std::fs::read(dir.path().join(BINARY)).unwrap(),
            b"new binary"
        );
    }

    #[test]
    fn a_failed_placement_puts_the_old_binary_back() {
        // The invariant the whole dance exists for: after a failed
        // update the operator still has a working binary. The
        // failure is induced by naming a source that is not there.
        let (dir, bin) = installed();
        let missing = dir.path().join("not-extracted");

        let err = place(&missing, dir.path(), "1").unwrap_err();
        assert!(matches!(err, UpdateError::Place { .. }), "{err}");
        assert!(bin.is_file(), "the old binary must be restored");
        assert_eq!(std::fs::read(&bin).unwrap(), b"old binary");
        // And no leftover is left lying around.
        assert_eq!(sweep_aside(dir.path()), 0);
    }

    #[test]
    fn placing_reports_swept_leftovers() {
        let (dir, _bin) = installed();
        // A leftover from an earlier update, which this run clears.
        std::fs::write(
            dir.path().join(format!("{BINARY}{ASIDE_PREFIX}old")),
            b"stale",
        )
        .unwrap();
        let work = tempfile::tempdir().unwrap();
        let fresh = work.path().join(BINARY);
        std::fs::write(&fresh, b"new").unwrap();

        assert_eq!(place(&fresh, dir.path(), "2").unwrap().swept, 1);
    }

    #[test]
    fn sweeping_an_empty_or_missing_dir_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(sweep_aside(dir.path()), 0);
        assert_eq!(sweep_aside(&dir.path().join("nope")), 0);
    }

    #[test]
    fn moving_aside_is_a_windows_only_need() {
        // Tied to the platform rather than configurable: on Unix
        // replacing a path unlinks the old inode and running
        // processes keep it, so the rename would only litter
        // ~/.cargo/bin.
        assert_eq!(needs_moving_aside(), cfg!(windows));
    }

    /// `<home>/.cargo/bin`, joined the way the code joins it.
    ///
    /// Not a literal like `"/home/igor/.cargo"`: a backslash is not
    /// a separator on Unix, so a single `r"C:\Users\igor\.cargo"`
    /// string is one component there and two on Windows, and an
    /// expectation written that way passes on one platform only.
    fn cargo_bin(home: &str) -> PathBuf {
        Path::new(home).join(".cargo").join("bin")
    }

    /// An environment of `(key, value)` pairs, for `install_dir_from`.
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// Both platform orders, so a test names the one it means.
    const WINDOWS: bool = true;
    const UNIX: bool = false;

    #[test]
    fn install_dir_prefers_cargo_home() {
        // Ahead of both home variables, on either platform.
        for windows in [WINDOWS, UNIX] {
            let dir = install_dir_from(
                env(&[
                    ("CARGO_HOME", "/opt/cargo"),
                    ("HOME", "/home/igor"),
                    ("USERPROFILE", r"C:\Users\igor"),
                ]),
                windows,
            );
            // Only `bin` is appended: CARGO_HOME *is* the cargo
            // directory, so a `.cargo` here would be doubled.
            assert_eq!(dir, Some(Path::new("/opt/cargo").join("bin")));
        }
    }

    #[test]
    fn install_dir_falls_back_to_the_home_cargo_dir() {
        let dir = install_dir_from(env(&[("HOME", "/home/igor")]), UNIX);
        assert_eq!(dir, Some(cargo_bin("/home/igor")));
    }

    #[test]
    fn install_dir_accepts_userprofile_on_a_bare_windows_shell() {
        let home = r"C:\Users\igor";
        let dir = install_dir_from(env(&[("USERPROFILE", home)]), WINDOWS);
        assert_eq!(dir, Some(cargo_bin(home)));
    }

    #[test]
    fn windows_prefers_userprofile_over_home() {
        // The case no test set before: Git Bash sets *both*, with
        // `HOME` in POSIX form. A `/`-rooted value passes the
        // anchored check and Windows then resolves it against the
        // current drive, so `/c/Users/igor` becomes `D:\c\Users\...`
        // when the working directory is on `D:`. Measured in this
        // repo's own shell: HOME=/c/Users/igor, USERPROFILE=C:\Users\igor.
        //
        // Reversing the two would have kept every other test in this
        // group green, which is why this one exists.
        let dir = install_dir_from(
            env(&[
                ("HOME", "/c/Users/igor"),
                ("USERPROFILE", r"C:\Users\igor"),
            ]),
            WINDOWS,
        );
        assert_eq!(dir, Some(cargo_bin(r"C:\Users\igor")));
    }

    #[test]
    fn unix_never_consults_userprofile() {
        // Exported into non-Windows processes by WSLENV, Wine and
        // some CI images, so honouring it there would let a Windows
        // home directory outrank `$HOME` -- the same trap `APPDATA`
        // set for the config lookup.
        let only_userprofile =
            install_dir_from(env(&[("USERPROFILE", r"C:\Users\igor")]), UNIX);
        assert_eq!(only_userprofile, None);

        let both = install_dir_from(
            env(&[("HOME", "/home/igor"), ("USERPROFILE", r"C:\Users\igor")]),
            UNIX,
        );
        assert_eq!(both, Some(cargo_bin("/home/igor")));
    }

    #[test]
    fn install_dir_ignores_a_relative_or_blank_value() {
        // A relative value resolves against the working directory,
        // and this module renames a file in whatever directory it
        // returns -- pointed at a repo that would move something out
        // of a checkout. Fed to *every* variable, not just
        // CARGO_HOME: each one reaches the same primitive.
        for bad in ["", "   ", ".", "..", "cargo", "sub/dir", "C:cargo"] {
            let via_home = install_dir_from(
                env(&[("CARGO_HOME", bad), ("HOME", "/home/igor")]),
                UNIX,
            );
            assert_eq!(
                via_home,
                Some(cargo_bin("/home/igor")),
                "CARGO_HOME={bad:?} must be ignored"
            );

            // A bad HOME must fall through to USERPROFILE on
            // Windows rather than being used.
            let via_userprofile = install_dir_from(
                env(&[("HOME", bad), ("USERPROFILE", r"C:\Users\igor")]),
                WINDOWS,
            );
            assert_eq!(
                via_userprofile,
                Some(cargo_bin(r"C:\Users\igor")),
                "HOME={bad:?} must be ignored"
            );

            // And a bad value everywhere leaves nothing to guess.
            let nothing = install_dir_from(
                env(&[
                    ("CARGO_HOME", bad),
                    ("HOME", bad),
                    ("USERPROFILE", bad),
                ]),
                WINDOWS,
            );
            assert_eq!(nothing, None, "all of them {bad:?} must yield None");
        }
    }

    #[test]
    fn install_dir_is_none_when_the_environment_names_no_home() {
        // On either platform: a machine naming no home directory is
        // one this cannot guess about.
        assert_eq!(install_dir_from(|_| None, WINDOWS), None);
        assert_eq!(install_dir_from(|_| None, UNIX), None);
    }

    #[test]
    fn the_binary_name_matches_the_platform() {
        assert_eq!(
            BINARY,
            if cfg!(windows) {
                "bombyx.exe"
            } else {
                "bombyx"
            }
        );
    }

    #[test]
    fn the_current_version_parses() {
        // `CURRENT` is the crate's own version, so if this ever
        // fails the manifest carries something `--tag` could not
        // name.
        assert!(Version::parse(CURRENT).is_some(), "{CURRENT}");
    }
}
