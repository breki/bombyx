//! Resolving the external programs bombyx runs.
//!
//! bombyx shells out to `ssh` by name, and `self-update` adds
//! `git`, `curl` and `tar`. Left to
//! the operating system, a bare name is looked up through a
//! search order that on Windows includes the **current
//! directory**. bombyx runs from a project repository, and
//! `config.rs` already treats a repo as attacker-controlled:
//! `bombyx.toml` arrives with whatever branch you check out. A
//! `tar.exe` committed beside it is the same trust boundary, and
//! `bombyx doctor` is the command the documentation tells you to
//! run *first* in a fresh clone.
//!
//! The lookup itself is [`which`]'s job. Getting it right means
//! honouring executable permission bits, `PATHEXT` ordering,
//! quoted `PATH` entries, and continuing past a candidate that
//! matches by name but cannot be executed. A hand-written
//! search gets at least one of those wrong.
//!
//! What is left is the one decision that is bombyx's own, and it
//! is a subtraction: **the working directory is never searched.**
//!
//! That subtraction is enforced three separate times, because
//! each barrier on its own has been observed to leak:
//!
//! 1. **A non-plain name is refused outright.** Anything carrying
//!    `/`, `\` or `:` is not a search -- and `:` matters as much
//!    as the slashes, because `C:tar` is the Windows
//!    *drive-relative* spelling of "in the current directory of
//!    drive C". A guard listing only `/` and `\` does not
//!    catch it.
//! 2. **An unsearchable `PATH` stops the lookup.** When no entry
//!    survives the absolute filter, the joined result is an empty
//!    string -- and `std::env::split_paths("")` yields one
//!    **empty** entry rather than none. `which`'s own empty-list
//!    check inspects the split vector, so it does not fire; on
//!    Unix empty entries are deliberately *not* dropped (the real
//!    `which` searches the working directory for them), so the
//!    candidate becomes the relative path `tar`, resolved against
//!    the process working directory. An empty result therefore
//!    has to be refused here rather than handed on.
//! 3. **A relative answer is discarded.** `which_in_global` never
//!    consults the working directory, and anything that is not an
//!    absolute path is dropped regardless of how it arrived.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// `path_var` with every entry that is not absolute removed.
///
/// Both spellings of "search here" have to go. An empty entry is
/// how POSIX writes the working directory, and an explicit `.`,
/// a bare `bin`, or the Windows drive-relative `C:bin` mean the
/// same thing -- `Path::is_absolute` rejects all of them.
fn absolute_entries(path_var: &OsStr) -> OsString {
    let kept: Vec<PathBuf> = std::env::split_paths(path_var)
        .filter(|p| p.is_absolute())
        .collect();
    // `join_paths` fails only on an entry containing the
    // separator, which `split_paths` cannot produce.
    std::env::join_paths(kept).unwrap_or_default()
}

/// Whether `name` is a bare program name rather than a path.
///
/// `:` is rejected alongside the separators. On Windows `C:tar`
/// names a file in the *current directory of drive C*, which is
/// the working directory this module exists to keep out of the
/// search -- and it contains no slash at all.
fn is_bare_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':'])
}

/// Finds `name` on the `PATH`, never in the working directory.
///
/// Returns `None` when it is not there. Callers must report that
/// rather than falling back to a bare-name spawn: the fallback
/// is the OS search this module exists to avoid, and it defeats
/// the whole point at the moment it matters most.
///
/// `None` also covers an absent or entirely relative `PATH`.
/// Reporting that as "not found on PATH" is accurate -- there was
/// nothing bombyx was willing to search -- and the two are
/// deliberately not distinguished, because the remedy is the
/// same.
#[must_use]
pub fn resolve(name: &str) -> Option<PathBuf> {
    if !is_bare_name(name) {
        return None;
    }
    let path = absolute_entries(&std::env::var_os("PATH")?);
    if path.is_empty() {
        return None;
    }
    // `which_in_global` ignores the working directory entirely,
    // unlike `which_in`, which takes one and resolves a
    // separator-bearing name against it.
    which::which_in_global(name, Some(&path))
        .ok()?
        .find(|found| found.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn entries(dirs: &[&str]) -> Vec<PathBuf> {
        let joined = std::env::join_paths(dirs.iter().map(Path::new)).unwrap();
        std::env::split_paths(&absolute_entries(&joined)).collect()
    }

    #[test]
    fn keeps_absolute_entries_in_order() {
        let abs: [&str; 2] = if cfg!(windows) {
            [r"C:\a", r"C:\b"]
        } else {
            ["/a", "/b"]
        };
        assert_eq!(
            entries(&abs),
            vec![PathBuf::from(abs[0]), PathBuf::from(abs[1])]
        );
    }

    #[test]
    fn drops_every_spelling_of_the_working_directory() {
        // The empty entry is how POSIX writes it; `.` and a bare
        // relative name mean the same thing. bombyx runs inside
        // a repo whose contents are untrusted, so none of them
        // may be searched.
        let abs = if cfg!(windows) { r"C:\keep" } else { "/keep" };
        let kept = entries(&["", ".", "..", "bin", "rel/dir", abs]);
        assert_eq!(kept, vec![PathBuf::from(abs)]);
    }

    #[test]
    fn an_all_relative_path_yields_nothing_to_search() {
        // Asserted on the joined value, not on `entries`:
        // `split_paths` reads an empty string as one empty entry,
        // so round-tripping would report a path with something in
        // it when there is nothing to search.
        let joined = std::env::join_paths([".", "bin"].map(Path::new)).unwrap();
        assert!(absolute_entries(&joined).is_empty());
    }

    #[test]
    fn only_a_bare_name_is_a_search() {
        // The whole family, not the three spellings that
        // prompted the guard. `C:tar` is the one that matters
        // most and carries no slash: on Windows it names a file
        // in the current directory of drive C.
        for path_like in [
            "",
            "./tar",
            "../tar",
            "sub/tar",
            r"sub\tar",
            "/usr/bin/tar",
            r"C:\tar",
            "C:tar",
            r"\tar",
            r"\\?\C:\tar",
            r"\\host\share\tar",
        ] {
            assert!(
                !is_bare_name(path_like),
                "{path_like:?} must not be treated as a bare name"
            );
            assert_eq!(resolve(path_like), None, "{path_like:?}");
        }
        assert!(is_bare_name("tar"));
    }

    #[test]
    fn resolve_finds_a_tool_that_is_really_there() {
        // Whatever the platform, the shell bombyx drives is
        // present in CI and on a dev box alike.
        let found = resolve("cargo");
        assert!(found.is_some(), "cargo must be on PATH");
        assert!(found.unwrap().is_absolute());
    }

    #[test]
    fn resolve_reports_absence_rather_than_guessing() {
        assert_eq!(resolve("bombyx-no-such-tool-xyz"), None);
    }

    #[test]
    fn resolve_never_answers_with_a_relative_path() {
        // The property that closes the hole: whatever the `PATH`
        // and whatever the platform, an answer bombyx acts on is
        // absolute, so it can never name something inside the
        // repo bombyx is running in.
        for name in ["cargo", "tar", "ssh", "cmd", "sh"] {
            if let Some(found) = resolve(name) {
                assert!(found.is_absolute(), "{name}: {found:?}");
            }
        }
    }
}
