//! `cargo xtask clean-cache` -- drop stale incremental
//! cache contents without forcing a full clean.
//!
//! Long-lived projects accumulate large
//! `target/{debug,release}/incremental/` directories
//! even when incremental compilation is disabled in the
//! current profile (cargo still walks them on every
//! build for fingerprint validation, which on Windows +
//! antivirus is real overhead).
//!
//! This command empties the two incremental
//! subdirectories while preserving the dirs themselves
//! so cargo can refill them on the next build. Manual
//! invocation only -- never wire into builds or
//! deploys; auto-cleanup defeats incremental caching.
//!
//! Robustness notes:
//! - Symlinks and Windows directory junctions are
//!   unlinked, never followed. Without this guard a
//!   junction under `incremental/` could redirect
//!   `remove_dir_all` outside the workspace.
//! - Per-entry failures (locked files from AV scanners
//!   or rust-analyzer handles -- the very motivation
//!   for this tool) are collected and reported at the
//!   end rather than aborting the loop. The remaining
//!   entries and the second `incremental/` directory
//!   are still cleaned.

use std::fs;
use std::io;
use std::path::Path;

use crate::helpers::{
    dir_size, fmt_bytes, is_reparse_or_symlink_meta, workspace_root,
};

/// Incremental cache subdirectories under `target/`.
const INCREMENTAL_DIRS: &[&str] =
    &["target/debug/incremental", "target/release/incremental"];

/// Walk the incremental cache directories, delete their
/// contents, and report bytes freed per directory.
///
/// If any per-entry deletions failed, prints them after
/// the summary and returns `Err` with a count. The
/// successful deletions are still applied -- this is a
/// "best effort, report what you couldn't do" tool.
pub fn clean_cache() -> Result<(), String> {
    let root = workspace_root();

    let mut total_freed: u64 = 0;
    let mut all_errors: Vec<String> = Vec::new();

    for rel in INCREMENTAL_DIRS {
        let path = root.join(rel);
        if !path.exists() {
            println!("{rel:40} not present, skipping");
            continue;
        }
        let (freed, errors) = clear_dir_contents(&path)?;
        total_freed += freed;
        println!("{rel:40} freed {:>10}", fmt_bytes(freed));
        for e in errors {
            all_errors.push(format!("{rel}: {e}"));
        }
    }

    println!("{:40} {:>10}", "Total:", fmt_bytes(total_freed));

    if all_errors.is_empty() {
        Ok(())
    } else {
        eprintln!("\n{} entry/entries could not be deleted:", all_errors.len());
        for e in &all_errors {
            eprintln!("  {e}");
        }
        Err(format!("{} deletion error(s)", all_errors.len()))
    }
}

/// Delete every entry inside `dir`. The directory
/// itself is left intact so cargo can refill it on the
/// next build.
///
/// Returns `(bytes_freed, per_entry_errors)`. A failed
/// `read_dir` on `dir` itself is still a hard `Err` --
/// in that case nothing was cleaned. Once `read_dir`
/// succeeds, per-entry failures are collected and the
/// loop continues; this is what makes the tool useful
/// on Windows where AV or rust-analyzer can transiently
/// lock individual files.
///
/// Symlinks are unlinked (`remove_file` on Unix,
/// `remove_dir` with `remove_file` fallback on Windows
/// for directory junctions) rather than recursed into.
fn clear_dir_contents(dir: &Path) -> Result<(u64, Vec<String>), String> {
    let mut freed = 0u64;
    let mut errors: Vec<String> = Vec::new();

    let entries = fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!(
                    "read_dir entry under {}: {e}",
                    dir.display()
                ));
                continue;
            }
        };
        let path = entry.path();

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                errors.push(format!("file_type {}: {e}", path.display()));
                continue;
            }
        };

        let (size, size_warnings) = dir_size(&path);
        for w in size_warnings {
            // DirSizeWarning carries its own failing
            // path -- push verbatim rather than
            // re-prefixing with `path`, which would
            // misleadingly name a parent when the real
            // culprit was deeper in the walk.
            errors.push(w.to_string());
        }

        let outcome = delete_entry(&path, file_type);

        match outcome {
            Ok(()) => freed += size,
            Err(e) => errors.push(format!("delete {}: {e}", path.display())),
        }
    }

    Ok((freed, errors))
}

/// Remove a single entry without following symlinks
/// or Windows directory junctions.
///
/// `file_type` is captured from the `DirEntry` and
/// covers the common case (regular file / regular
/// dir / true symlink) with no extra syscall. On
/// Windows, when `file_type.is_symlink()` is false we
/// pay one extra `symlink_metadata` to inspect
/// `FILE_ATTRIBUTE_REPARSE_POINT` -- this is required
/// because `is_symlink()` is only set for
/// `IO_REPARSE_TAG_SYMLINK`, not directory junctions
/// (`mklink /J`), and without the check a junction
/// would fall into the `is_dir()` branch and
/// `remove_dir_all` would traverse it and delete the
/// *target* tree. On Unix the helper short-circuits
/// on `is_symlink()` alone, so no extra syscall.
///
/// Dispatch: reparse point / symlink -> unlink the
/// link itself; regular dir -> `remove_dir_all`;
/// otherwise -> `remove_file`.
///
/// On Windows, directory reparse points cannot be
/// removed via `remove_file`, so we try `remove_dir`
/// first and fall back to `remove_file` for file-style
/// links. On Unix, all symlinks unlink with
/// `remove_file`.
fn delete_entry(
    path: &Path,
    file_type: std::fs::FileType,
) -> Result<(), io::Error> {
    if is_reparse_or_symlink_path(path, file_type)? {
        #[cfg(windows)]
        {
            fs::remove_dir(path).or_else(|_| fs::remove_file(path))
        }
        #[cfg(not(windows))]
        {
            fs::remove_file(path)
        }
    } else if file_type.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Path-based wrapper over `is_reparse_or_symlink_meta`
/// used by deletion. Avoids the extra
/// `symlink_metadata` call on Unix by short-circuiting
/// on `file_type.is_symlink()` directly.
fn is_reparse_or_symlink_path(
    path: &Path,
    file_type: std::fs::FileType,
) -> Result<bool, io::Error> {
    if file_type.is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        let meta = fs::symlink_metadata(path)?;
        Ok(is_reparse_or_symlink_meta(&meta))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;

    use super::*;
    use crate::helpers::temp_scratch;

    #[test]
    fn clear_dir_contents_removes_files_and_subdirs_but_keeps_root() {
        let temp = temp_scratch("clean-cache-clear");
        let inc = temp.join("incremental");
        let nested = inc.join("crate-abc-123");
        fs::create_dir_all(&nested).unwrap();
        let payload = vec![0u8; 4096];
        File::create(nested.join("data.bin"))
            .unwrap()
            .write_all(&payload)
            .unwrap();
        File::create(inc.join("toplevel.bin"))
            .unwrap()
            .write_all(&payload)
            .unwrap();

        let (freed, errors) = clear_dir_contents(&inc).unwrap();

        assert!(
            errors.is_empty(),
            "no per-entry errors expected: {errors:?}"
        );
        assert!(
            freed >= 8192,
            "freed bytes ({freed}) should account for both 4 KiB files"
        );
        assert!(inc.exists(), "incremental/ itself must remain");
        assert_eq!(
            fs::read_dir(&inc).unwrap().count(),
            0,
            "incremental/ must be empty after clear"
        );

        let _ = fs::remove_dir_all(temp);
    }

    #[test]
    fn clear_dir_contents_leaves_siblings_alone() {
        let temp = temp_scratch("clean-cache-siblings");
        let inc = temp.join("incremental");
        fs::create_dir_all(&inc).unwrap();
        File::create(inc.join("inside.bin")).unwrap();
        File::create(temp.join("outside.bin")).unwrap();
        let other = temp.join("other-dir");
        fs::create_dir_all(&other).unwrap();
        File::create(other.join("keep.bin")).unwrap();

        let (_freed, errors) = clear_dir_contents(&inc).unwrap();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");

        assert!(!inc.join("inside.bin").exists());
        assert!(
            temp.join("outside.bin").exists(),
            "files outside the target dir must not be touched"
        );
        assert!(
            other.join("keep.bin").exists(),
            "sibling directories must not be touched"
        );

        let _ = fs::remove_dir_all(temp);
    }

    /// If a symlink inside `incremental/` ever pointed at
    /// something outside the workspace, following it would
    /// be catastrophic. This test plants a sibling
    /// "outside" tree, symlinks to it from inside the
    /// scratch incremental dir, runs the cleaner, and
    /// asserts the symlink target's contents survive.
    ///
    /// Symlink creation on Windows requires elevated
    /// privileges or developer mode; the test best-efforts
    /// the link creation and skips silently if the OS
    /// refuses. The Unix path always runs.
    #[test]
    fn clear_dir_contents_does_not_follow_symlinks() {
        let temp = temp_scratch("clean-cache-symlink");
        let inc = temp.join("incremental");
        fs::create_dir_all(&inc).unwrap();

        let outside = temp.join("outside-tree");
        fs::create_dir_all(&outside).unwrap();
        File::create(outside.join("must-survive.bin"))
            .unwrap()
            .write_all(&[0u8; 64])
            .unwrap();

        let link_path = inc.join("link-to-outside");
        let link_ok = create_dir_symlink(&outside, &link_path);
        if !link_ok {
            // Best-effort: symlinks unavailable on this OS
            // or under this account. The non-symlink
            // behaviour is covered by the other tests.
            let _ = fs::remove_dir_all(&temp);
            return;
        }

        let (_freed, errors) = clear_dir_contents(&inc).unwrap();
        assert!(
            errors.is_empty(),
            "symlink unlink should succeed cleanly: {errors:?}"
        );
        assert!(
            outside.join("must-survive.bin").exists(),
            "files behind the symlink must NOT be deleted"
        );
        assert!(
            !link_path.exists(),
            "the symlink entry itself should be gone"
        );

        let _ = fs::remove_dir_all(temp);
    }

    #[cfg(unix)]
    fn create_dir_symlink(src: &Path, dst: &Path) -> bool {
        std::os::unix::fs::symlink(src, dst).is_ok()
    }

    #[cfg(windows)]
    fn create_dir_symlink(src: &Path, dst: &Path) -> bool {
        std::os::windows::fs::symlink_dir(src, dst).is_ok()
    }

    #[cfg(not(any(unix, windows)))]
    fn create_dir_symlink(_src: &Path, _dst: &Path) -> bool {
        false
    }

    /// Windows-only regression test: a directory
    /// junction (`mklink /J`) is a reparse point but
    /// not a true symlink, so `FileType::is_symlink()`
    /// returns `false` for it. Without the
    /// `is_reparse_or_symlink` guard the cleaner would
    /// fall into the `is_dir()` branch and
    /// `remove_dir_all` would traverse the junction
    /// and delete the target tree.
    ///
    /// Junctions, unlike symlinks, do not require
    /// elevation or developer mode to create on
    /// Windows, so this test runs unconditionally on
    /// Windows CI.
    #[cfg(windows)]
    #[test]
    fn clear_dir_contents_does_not_follow_junctions() {
        use std::process::Command;

        let temp = temp_scratch("clean-cache-junction");
        let inc = temp.join("incremental");
        fs::create_dir_all(&inc).unwrap();

        let outside = temp.join("outside-tree");
        fs::create_dir_all(&outside).unwrap();
        // Use a payload bigger than the junction
        // entry's plausible metadata size so the
        // `freed` upper-bound assertion below can
        // catch a regression that walks the target.
        File::create(outside.join("must-survive.bin"))
            .unwrap()
            .write_all(&vec![0u8; 65536])
            .unwrap();

        let junction = inc.join("junction-to-outside");

        // Guard against BatBadBut (CVE-2024-24576): the
        // `cmd /c` re-parser is sensitive to a handful
        // of metacharacters in args. `temp_dir()` is
        // user-controllable via `TMP`/`TEMP`, so refuse
        // to invoke `mklink` if either path embeds one.
        let unsafe_chars = ['&', '|', '<', '>', '^', '"', '%', '\n', '\r'];
        let safe = |p: &Path| {
            p.to_str()
                .is_some_and(|s| !s.chars().any(|c| unsafe_chars.contains(&c)))
        };
        if !(safe(&junction) && safe(&outside)) {
            let _ = fs::remove_dir_all(&temp);
            return;
        }

        let status = Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                junction.to_str().unwrap(),
                outside.to_str().unwrap(),
            ])
            .status();
        let junction_ok = matches!(status, Ok(s) if s.success());
        if !junction_ok {
            // mklink unavailable in this environment;
            // bail without failing.
            let _ = fs::remove_dir_all(&temp);
            return;
        }

        let (freed, errors) = clear_dir_contents(&inc).unwrap();
        assert!(
            errors.is_empty(),
            "junction unlink should succeed cleanly: {errors:?}"
        );
        assert!(
            outside.join("must-survive.bin").exists(),
            "files behind the junction must NOT be deleted"
        );
        // `Path::exists` follows reparse points, so an
        // existing-but-broken link would also report
        // false. `symlink_metadata` reports the link
        // entry directly: `Err` here means the entry
        // itself is gone.
        assert!(
            fs::symlink_metadata(&junction).is_err(),
            "the junction entry itself should be gone"
        );
        // If `dir_size` regressed and traversed the
        // junction, `freed` would include the 64 KiB
        // payload behind the target. Bound it well
        // below that.
        assert!(
            freed < 4096,
            "junction sizing must not walk the target \
             tree (freed = {freed})"
        );

        let _ = fs::remove_dir_all(temp);
    }
}
