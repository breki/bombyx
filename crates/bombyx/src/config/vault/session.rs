//! Starts `keepassxc-cli open` and the password script, joins
//! them, and hands the ends bombyx holds to `super::drive`.
//!
//! The one part of the vault the unit tests cannot reach: it needs
//! `keepassxc-cli` and a terminal. So it holds no decision of its
//! own, and the coverage gate names this file as an exception.
//! The ignored test at the end runs it for real on a machine that
//! has `keepassxc-cli`, with a file standing in for the terminal.
//!
//! Two processes, joined by an OS pipe that bombyx creates and
//! never reads:
//!
//! ```text
//! bombyx --stdin--> sh (password, then cat) --pipe--> keepassxc-cli
//! bombyx <---------------------stdout---------------- keepassxc-cli
//! ```
//!
//! A single `sh -c 'a | keepassxc-cli'` would be shorter and
//! deadlocks: the parent shell holds bombyx's stdout open while it
//! waits for `cat`, so a `keepassxc-cli` that exits on a wrong
//! password never reaches bombyx as the end of the stream.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{EntryPath, SecretName, VaultError, drive};

/// The script that reads the master password from the terminal,
/// so bombyx never holds it. The script says how.
const PASSWORD: &str = include_str!("../../../templates/vault-password.sh");

/// Opens `database`, reading its password from the terminal, and
/// returns each entry's value in the order of `entries`.
pub(super) fn read(
    database: &Path,
    entries: &BTreeMap<SecretName, EntryPath>,
) -> Result<Vec<Vec<u8>>, VaultError> {
    read_with_tty(database, entries, Path::new("/dev/tty"))
}

/// [`read`], with the file the password is read from as a
/// parameter so the ignored test can supply one.
fn read_with_tty(
    database: &Path,
    entries: &BTreeMap<SecretName, EntryPath>,
    tty: &Path,
) -> Result<Vec<Vec<u8>>, VaultError> {
    // Opened here, and closed at once, only to learn whether there
    // is a terminal. Without one the script would send an empty
    // password, and keepassxc-cli would report it as a wrong one.
    // Opening the terminal reads nothing from it.
    if std::fs::File::open(tty).is_err() {
        return Err(VaultError::NoTerminal);
    }
    let (pipe_out, pipe_in) = io::pipe()?;

    // keepassxc-cli first: when it is missing, nothing else has
    // started, so no script is left waiting on the terminal.
    // stderr is the operator's terminal, so its password prompt
    // and its errors reach them unchanged.
    let mut cli = Command::new("keepassxc-cli")
        .arg("open")
        .arg("--")
        .arg(database)
        .stdin(pipe_out)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => VaultError::NotInstalled,
            _ => VaultError::Spawn {
                program: "keepassxc-cli",
                source: e,
            },
        })?;
    let mut script = match Command::new("sh")
        .arg("-c")
        .arg(PASSWORD)
        .arg("bombyx-vault")
        .arg(tty)
        .stdin(Stdio::piped())
        .stdout(pipe_in)
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            // Its stdin was the pipe, whose write end went with
            // the failed spawn, so keepassxc-cli reads an end of
            // file and exits.
            let _ = cli.wait();
            return Err(VaultError::Spawn {
                program: "sh",
                source: e,
            });
        }
    };
    let (Some(mut inp), Some(out)) = (script.stdin.take(), cli.stdout.take())
    else {
        unreachable!("both pipes were asked for above");
    };
    let result = drive(out, &mut inp, database, entries);
    // Closing stdin ends `cat`, and with it keepassxc-cli's
    // input, so both waits return whichever way `drive` finished.
    drop(inp);
    script.wait()?;
    cli.wait()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the whole session against a real database.
    ///
    /// Needs `keepassxc-cli` on `PATH`, so it is ignored by
    /// default: `cargo xtask test --ignored`. A file holding the
    /// password stands in for the terminal, which is the one part
    /// this does not exercise.
    #[test]
    #[ignore = "needs keepassxc-cli"]
    fn reads_every_entry_after_one_unlock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("t.kdbx");
        let cli = |args: &[&str], stdin: &str| {
            let mut c = Command::new("keepassxc-cli")
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("keepassxc-cli");
            std::io::Write::write_all(
                c.stdin.as_mut().expect("stdin"),
                stdin.as_bytes(),
            )
            .expect("write");
            assert!(c.wait().expect("wait").success(), "{args:?}");
        };
        let db_s = db.to_str().expect("utf8");
        cli(&["db-create", "-p", db_s], "pw\npw\n");
        cli(&["mkdir", db_s, "Anthropic"], "pw\n");
        cli(&["add", "-p", db_s, "Anthropic/API key"], "pw\nsk-1\n");
        cli(&["add", "-p", db_s, "Bob's key"], "pw\nit's x\n");

        let tty = dir.path().join("tty");
        std::fs::write(&tty, "pw\n").expect("tty");
        let entries: BTreeMap<_, _> =
            [("A", "Anthropic/API key"), ("B", "Bob's key")]
                .iter()
                .map(|(n, e)| {
                    (
                        SecretName::parse(n).expect("n"),
                        EntryPath::parse(e).expect("e"),
                    )
                })
                .collect();
        let values = read_with_tty(&db, &entries, &tty).expect("unlocks");
        assert_eq!(values, vec![b"sk-1".to_vec(), b"it's x".to_vec()]);

        std::fs::write(&tty, "wrong\n").expect("tty");
        let err = read_with_tty(&db, &entries, &tty).expect_err("wrong");
        assert!(matches!(err, VaultError::Unlock { .. }), "{err:?}");
    }
}
