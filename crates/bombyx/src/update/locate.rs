//! Which directory holds the binary to be replaced.
//!
//! Read-only. Nothing here renames or deletes anything -- that is
//! [`super::swap`], and the two were one module until a review
//! pointed out that grouping a pure environment lookup with the
//! only deleting code in the crate defeats the "which half of this
//! can hurt me" question the effect split exists to answer.
//!
//! The distinction that matters between the two functions:
//! [`running_dir`] answers with the directory of the *invoked*
//! executable, and [`install_dir`] guesses from the environment.
//! They differ more often than they look -- `cargo install --root`,
//! a copy into `~/bin`, a Scoop or winget shim, or simply running
//! `target/release/bombyx`. Deriving the target from `CARGO_HOME`
//! alone wrote a fresh binary into `~/.cargo/bin`, printed
//! `updated`, and left the binary the operator actually invokes
//! untouched. So `install_dir` is the fallback, never the first
//! answer.

use std::path::{Path, PathBuf};

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

/// The directory holding the running executable.
///
/// This, not a directory derived from the environment, is where
/// the binary being replaced lives -- see `target_dir` in the
/// binary for why the two differ more often than they look.
/// `None` only where `current_exe` fails, which is rare enough
/// that [`install_dir`] exists as the fallback.
#[must_use]
pub fn running_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
