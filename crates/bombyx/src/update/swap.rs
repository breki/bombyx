//! Finding the installed binary and replacing it. **Touches the
//! filesystem.**
//!
//! This is the only part of the crate's command-building layer that
//! does: [`move_aside`], [`restore`], [`place`] and [`sweep_aside`]
//! rename and *delete* files in the directory holding the installed
//! binary. Called out in its own module rather than left for a
//! reader to discover among the pure version arithmetic next door.
//!
//! Windows is the reason the dance exists. A running executable's
//! image cannot be overwritten -- the attempt fails with
//! `Access is denied (os error 5)` -- but it *can* be renamed, and
//! the running process keeps working from the renamed file. So an
//! update moves the old binary aside, extracts, and sweeps the
//! leftover on the next run that replaces the binary.

use std::path::{Path, PathBuf};

use thiserror::Error;

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
}
