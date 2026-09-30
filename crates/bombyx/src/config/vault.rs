//! What `[source.vault]` may be: a KeePassXC database on the
//! workstation, and the entries in it that hold the project's
//! secrets.
//!
//! The vault is the other source of what `super::EnvFilePath`
//! reads. A config names one or the other, and either way bombyx
//! ends up with a `super::Secrets` of `NAME=value` lines, so
//! nothing downstream of `super::Config::read_staged` can tell
//! which one it came from.
//!
//! **bombyx never holds the master password.** It starts
//! `keepassxc-cli open` through a small `sh` wrapper, which reads
//! the password from the terminal and writes it to
//! `keepassxc-cli` first. After that the wrapper forwards what
//! bombyx writes, and bombyx writes one `show` command per entry.
//! So the database is unlocked once per run, and the only secrets
//! bombyx sees are the entries the config names.
//!
//! Three parts live here. The value types check the config. The
//! protocol -- [`drive`] and the helpers it calls -- speaks to
//! the interactive shell over any reader and writer, so it is
//! tested without a process. `session` starts the real process
//! and is the one part the tests cannot reach.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use super::env_file::{self, EnvFileError, Secrets};
use super::error::FieldError;
use super::guards;
use crate::newtype::{checked_str_newtype, checked_str_try_from};

mod session;

/// The `[source.vault]` table: which database to open, and
/// which entry holds each variable.
///
/// Built through `VaultFields`, so a table with no entries is
/// refused while the config parses. (Not a rustdoc link, because
/// a public page may not link to a private item.)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "VaultFields")]
pub struct Vault {
    /// The database file, on the workstation.
    pub database: VaultDatabase,
    /// Each variable bombyx writes into the secrets, and the
    /// entry whose `Password` holds its value.
    ///
    /// A `BTreeMap` so the secrets come out in the same order on
    /// every run, whatever order the config file lists them in.
    pub entries: BTreeMap<SecretName, EntryPath>,
}

/// `[source.vault]` as TOML spells it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VaultFields {
    database: VaultDatabase,
    entries: BTreeMap<SecretName, EntryPath>,
}

impl TryFrom<VaultFields> for Vault {
    type Error = FieldError;

    /// Refuses a vault naming no entry. It would unlock the
    /// database, read nothing and send an empty secrets file,
    /// which is the outcome of naming no vault at all, reached
    /// by a password prompt.
    fn try_from(raw: VaultFields) -> Result<Self, Self::Error> {
        if raw.entries.is_empty() {
            return Err(FieldError::invalid(
                EntryPath::FIELD,
                "names no entry; list at least one variable and \
                 the entry that holds it",
            ));
        }
        Ok(Self {
            database: raw.database,
            entries: raw.entries,
        })
    }
}

/// A KeePassXC database file on the workstation.
///
/// The same rules as `super::EnvFilePath`, for the same reason:
/// the file is on the machine bombyx runs on, and bombyx hands
/// the path to a program here rather than to a remote shell.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct VaultDatabase(String);

impl VaultDatabase {
    /// The config key this type reads.
    pub const FIELD: &'static str = "vault.database";

    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns what `super::EnvFilePath::parse` returns, naming
    /// `vault.database`.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_database(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// Expands a leading `~/` and returns the path bombyx opens.
    ///
    /// # Errors
    ///
    /// Returns [`EnvFileError::NoHome`] when the value needs a
    /// home directory and the environment names none.
    pub fn resolve<F>(&self, getenv: F) -> Result<PathBuf, EnvFileError>
    where
        F: Fn(&str) -> Option<String>,
    {
        env_file::resolve_home(Self::FIELD, &self.0, getenv)
    }
}

checked_str_newtype!(VaultDatabase, "The path, as the operator wrote it.");

checked_str_try_from!(
    /// What serde calls while the config parses.
    VaultDatabase,
    FieldError,
    check_database
);

/// Every rule a `vault.database` value must pass.
fn check_database(value: &str) -> Result<(), FieldError> {
    env_file::check_workstation_file(VaultDatabase::FIELD, value)
}

/// The name of a variable bombyx writes into the secrets.
///
/// The key of a `vault.entries` line. `super::RepoTokenVar`
/// names one of these when the project clones over https.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct SecretName(String);

impl SecretName {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it is not a variable name.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_name(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(SecretName, "The variable name, as written.");

checked_str_try_from!(
    /// What serde calls while the config parses.
    SecretName,
    FieldError,
    check_name
);

/// Every rule a `vault.entries` key must pass.
fn check_name(value: &str) -> Result<(), FieldError> {
    guards::check_variable_name(EntryPath::FIELD, value)
}

/// The path of an entry inside the database, such as
/// `Anthropic/API key`.
///
/// **The value goes into a `keepassxc-cli` command line.** bombyx
/// writes `show -s -a Password -- "<path>"` into the interactive
/// shell, which splits the line into arguments itself. Measured
/// against keepassxc-cli 2.7.6, a `\` there escapes the next
/// character and a `"` ends the quoted argument, so either one
/// lets the path change the command. A newline ends the command
/// and starts another, which could be `rm`. So all three are
/// refused, with every other control character. A leading `-` is
/// safe because of the `--`, and a `'` is an ordinary character
/// to that shell.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct EntryPath(String);

impl EntryPath {
    /// The config key this type reads.
    pub const FIELD: &'static str = "vault.entries";

    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it holds a character the
    /// `keepassxc-cli` shell would read as syntax.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_entry(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(EntryPath, "The entry path, as written.");

checked_str_try_from!(
    /// What serde calls while the config parses.
    EntryPath,
    FieldError,
    check_entry
);

/// Every rule a `vault.entries` value must pass.
fn check_entry(value: &str) -> Result<(), FieldError> {
    guards::check_not_empty(EntryPath::FIELD, value)?;
    guards::check_charset(
        EntryPath::FIELD,
        value,
        |c| !c.is_control() && c != '"' && c != '\\',
        "characters other than `\"`, `\\` and control characters",
    )
}

/// Why bombyx could not read the secrets out of the vault.
///
/// No variant carries a value read from the database. An entry
/// is named by its variable and its path, which are in the
/// config already.
#[derive(Debug, Error)]
pub enum VaultError {
    /// The workstation is Windows, which has no `sh` to run the
    /// wrapper in.
    #[error(
        "`vault` is not supported on a Windows workstation yet; \
         use `env_file` there"
    )]
    Unsupported,

    /// The database path could not be expanded.
    #[error(transparent)]
    Path(#[from] EnvFileError),

    /// The path names something that is not a regular file.
    #[error("`vault.database` names {path}, which is not a regular file")]
    NotAFile {
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
    },

    /// `keepassxc-cli`, or the `sh` that reads the password, was
    /// found and could not be started.
    #[error("could not start `{program}` to open the vault")]
    Spawn {
        /// The program bombyx tried to start.
        program: &'static str,
        /// What the operating system said.
        source: io::Error,
    },

    /// bombyx has no terminal to ask for the master password on,
    /// as when it runs from a script or a scheduler.
    #[error(
        "`vault` asks for the master password on a terminal, and \
         this run has none; run bombyx from a terminal, or use \
         `env_file`"
    )]
    NoTerminal,

    /// `keepassxc-cli` is not on this machine's `PATH`.
    #[error(
        "`vault` needs `keepassxc-cli`, which is not installed or \
         not on PATH"
    )]
    NotInstalled,

    /// `keepassxc-cli` stopped before it opened the database.
    ///
    /// A wrong password is the usual cause, and `keepassxc-cli`
    /// has already said so on the terminal, which is why this
    /// line does not guess.
    #[error("could not unlock {path}; keepassxc-cli said why above")]
    Unlock {
        /// The database, after expanding `~`.
        path: PathBuf,
    },

    /// `keepassxc-cli` stopped in the middle of the session.
    #[error("keepassxc-cli stopped before bombyx had read every entry")]
    Closed,

    /// Reading from or writing to `keepassxc-cli` failed.
    #[error("could not talk to keepassxc-cli")]
    Io(#[from] io::Error),

    /// The database holds no entry at the path the config names.
    #[error(
        "`vault.entries` maps {name} to \"{entry}\", which is not in the database"
    )]
    Missing {
        /// The variable.
        name: String,
        /// The entry path.
        entry: String,
    },

    /// The entry's password spans more than one line, which a
    /// `NAME=value` line cannot hold.
    #[error("the password of \"{entry}\" ({name}) spans more than one line")]
    Multiline {
        /// The variable.
        name: String,
        /// The entry path.
        entry: String,
    },

    /// The entry's password holds a `'` and also a `"`, `$`, `\`
    /// or backtick, so neither quote keeps it intact.
    #[error(
        "the password of \"{entry}\" ({name}) holds a `'` together \
         with one of `\"`, `$`, `\\` or a backtick, and no quoting \
         of a `NAME=value` line keeps both"
    )]
    Unquotable {
        /// The variable.
        name: String,
        /// The entry path.
        entry: String,
    },
}

impl Vault {
    /// Unlocks the database once and reads every entry the
    /// config names, as a secrets file.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Unsupported`] on Windows, and every
    /// other [`VaultError`] for the reason it names.
    pub fn read<F>(&self, getenv: F) -> Result<Secrets, VaultError>
    where
        F: Fn(&str) -> Option<String>,
    {
        if cfg!(windows) {
            return Err(VaultError::Unsupported);
        }
        let path = self.database.resolve(getenv)?;
        // Asked here rather than left to keepassxc-cli, whose
        // message for a missing file comes after the operator
        // has already typed the password.
        if !path.is_file() {
            return Err(VaultError::NotAFile { path });
        }
        let values = session::read(&path, &self.entries)?;
        env_text(&self.entries, &values)
    }
}

/// The attribute bombyx reads from each entry.
const ATTRIBUTE: &str = "Password";

/// The line bombyx writes to read one entry.
///
/// `-s` shows a protected attribute in clear text, which the
/// password is. `--` ends the options, so an entry path that
/// starts with `-` is still a path.
fn show_command(entry: &EntryPath) -> String {
    format!("show -s -a {ATTRIBUTE} -- \"{}\"\n", entry.as_str())
}

/// The prompt the interactive shell prints before each command:
/// the database's file name, then `> `.
fn prompt(database: &Path) -> Vec<u8> {
    let mut p = database
        .file_name()
        .map(|n| n.as_encoded_bytes().to_vec())
        .unwrap_or_default();
    p.extend_from_slice(b"> ");
    p
}

/// Reads up to the next prompt and returns what came before it,
/// or `None` when the stream ends first.
///
/// The prompt is the only framing the shell offers, so a password
/// that holds `<file name>> ` is cut there. The cut reply lacks
/// the newline that closes a value, so `reply_value` reads it as
/// a missing entry and the run stops, naming that entry, rather
/// than sending half a password. The message then misnames the
/// cause; a database name nobody types into a password makes it
/// unlikely.
fn until_prompt<R: BufRead>(
    out: &mut R,
    prompt: &[u8],
) -> io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    loop {
        if buf.ends_with(prompt) {
            buf.truncate(buf.len() - prompt.len());
            return Ok(Some(buf));
        }
        let mut byte = [0u8; 1];
        if out.read(&mut byte)? == 0 {
            return Ok(None);
        }
        buf.push(byte[0]);
    }
}

/// The value in one reply, or `None` for an entry the database
/// does not hold.
///
/// A reply is what the shell prints between two prompts. On
/// Linux it starts with an echo of the command, which is dropped
/// when present. After that a found entry prints its value and a
/// newline, so an empty password is a bare newline, while a
/// missing entry prints nothing on stdout at all.
fn reply_value(reply: &[u8], command: &str) -> Option<Vec<u8>> {
    let rest = reply.strip_prefix(command.as_bytes()).unwrap_or(reply);
    let value = rest.strip_suffix(b"\n")?;
    Some(value.to_vec())
}

/// Speaks to an unlocked-or-unlocking `keepassxc-cli open`:
/// waits for the first prompt, writes one `show` per entry, and
/// collects each value.
///
/// The wait matters. `keepassxc-cli` buffers its standard input
/// while it reads the password, so a command written before the
/// first prompt is swallowed with it.
///
/// # Errors
///
/// Returns [`VaultError::Unlock`] when the stream ends before
/// the first prompt, [`VaultError::Closed`] when it ends later,
/// and [`VaultError::Missing`] for an entry the database lacks.
fn drive<R: Read, W: Write>(
    out: R,
    inp: &mut W,
    database: &Path,
    entries: &BTreeMap<SecretName, EntryPath>,
) -> Result<Vec<Vec<u8>>, VaultError> {
    let mut out = BufReader::new(out);
    let prompt = prompt(database);
    if until_prompt(&mut out, &prompt)?.is_none() {
        return Err(VaultError::Unlock {
            path: database.to_owned(),
        });
    }
    let mut values = Vec::with_capacity(entries.len());
    for (name, entry) in entries {
        let command = show_command(entry);
        inp.write_all(command.as_bytes())?;
        inp.flush()?;
        let reply =
            until_prompt(&mut out, &prompt)?.ok_or(VaultError::Closed)?;
        let value = reply_value(&reply, &command).ok_or_else(|| {
            VaultError::Missing {
                name: name.as_str().to_owned(),
                entry: entry.as_str().to_owned(),
            }
        })?;
        values.push(value);
    }
    inp.write_all(b"exit\n")?;
    inp.flush()?;
    Ok(values)
}

/// Writes the values out as `NAME=value` lines, in the order of
/// `entries`.
///
/// A value is quoted when it holds a byte that
/// `super::repo_token`'s reader, or a shell, would read as
/// syntax: whitespace, `#`, a quote, `$`, `\` or a backtick. It
/// takes `'`, inside which both read every byte literally. A
/// value holding a `'` takes `"` instead, which is literal to
/// both as long as the value holds no `"`, `$`, `\` or backtick.
///
/// # Errors
///
/// Returns [`VaultError::Multiline`] for a value holding a line
/// break, and [`VaultError::Unquotable`] for one that fits
/// neither quote.
fn env_text(
    entries: &BTreeMap<SecretName, EntryPath>,
    values: &[Vec<u8>],
) -> Result<Secrets, VaultError> {
    let mut text = Vec::new();
    for ((name, entry), value) in entries.iter().zip(values) {
        let refused = |multiline: bool| {
            let name = name.as_str().to_owned();
            let entry = entry.as_str().to_owned();
            if multiline {
                VaultError::Multiline { name, entry }
            } else {
                VaultError::Unquotable { name, entry }
            }
        };
        if value.iter().any(|b| matches!(b, b'\n' | b'\r')) {
            return Err(refused(true));
        }
        let needs_quotes = value.iter().any(|b| {
            b.is_ascii_whitespace()
                || matches!(b, b'#' | b'"' | b'\'' | b'$' | b'\\' | b'`')
        });
        let quote: &[u8] = if !needs_quotes {
            b""
        } else if !value.contains(&b'\'') {
            b"'"
        } else if value
            .iter()
            .any(|b| matches!(b, b'"' | b'$' | b'\\' | b'`'))
        {
            return Err(refused(false));
        } else {
            b"\""
        };
        text.extend_from_slice(name.as_str().as_bytes());
        text.push(b'=');
        text.extend_from_slice(quote);
        text.extend_from_slice(value);
        text.extend_from_slice(quote);
        text.push(b'\n');
    }
    Ok(Secrets::from_vault(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::repo_token;

    fn entries(pairs: &[(&str, &str)]) -> BTreeMap<SecretName, EntryPath> {
        pairs
            .iter()
            .map(|(n, e)| {
                (
                    SecretName::parse(n).expect("name"),
                    EntryPath::parse(e).expect("entry"),
                )
            })
            .collect()
    }

    /// What `keepassxc-cli open t.kdbx` printed on stdout for
    /// the commands `drive` writes, measured against 2.7.6: the
    /// prompt, then per command its echo and the value.
    fn transcript(replies: &[&str]) -> Vec<u8> {
        let mut t = b"t.kdbx> ".to_vec();
        for r in replies {
            t.extend_from_slice(r.as_bytes());
            t.extend_from_slice(b"t.kdbx> ");
        }
        t
    }

    #[test]
    fn an_entry_path_refuses_what_the_cli_shell_reads_as_syntax() {
        for bad in ["a\"b", "a\\b", "a\nb", "a\rb", "a\tb", "a\u{7f}b"] {
            assert!(EntryPath::parse(bad).is_err(), "{bad:?}");
        }
        assert!(EntryPath::parse(" ").is_err(), "blank");
        for good in ["Anthropic/API key", "Bob's key", "-dash", "a/b/c"] {
            assert!(EntryPath::parse(good).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn a_secret_name_is_a_variable_name() {
        for bad in ["", "1A", "A-B", "A B", "A=B"] {
            assert!(SecretName::parse(bad).is_err(), "{bad:?}");
        }
        assert!(SecretName::parse("GIT_TOKEN").is_ok());
    }

    #[test]
    fn a_database_path_follows_the_env_file_rules() {
        assert!(VaultDatabase::parse("~/s.kdbx").is_ok());
        for bad in ["s.kdbx", "~", "~/", "~/d/.."] {
            let err = VaultDatabase::parse(bad).expect_err(bad);
            assert!(err.to_string().contains("vault.database"), "{err}");
        }
    }

    #[test]
    fn the_show_command_quotes_the_path_after_a_double_dash() {
        let e = EntryPath::parse("-a b").expect("entry");
        assert_eq!(show_command(&e), "show -s -a Password -- \"-a b\"\n");
    }

    #[test]
    fn the_prompt_is_the_database_file_name() {
        assert_eq!(prompt(Path::new("/h/i/t.kdbx")), b"t.kdbx> ");
    }

    #[test]
    fn a_reply_drops_the_echo_and_the_newline() {
        let cmd = "show -s -a Password -- \"G\"\n";
        let echoed = format!("{cmd}ghp_abc\n");
        assert_eq!(
            reply_value(echoed.as_bytes(), cmd),
            Some(b"ghp_abc".to_vec())
        );
        // Without the echo, as a platform whose line reader does
        // not echo would print it.
        assert_eq!(reply_value(b"ghp_abc\n", cmd), Some(b"ghp_abc".to_vec()));
    }

    #[test]
    fn an_empty_password_and_a_missing_entry_differ() {
        let cmd = "show -s -a Password -- \"G\"\n";
        let empty = format!("{cmd}\n");
        assert_eq!(reply_value(empty.as_bytes(), cmd), Some(Vec::new()));
        assert_eq!(reply_value(cmd.as_bytes(), cmd), None);
        assert_eq!(reply_value(b"", cmd), None);
    }

    #[test]
    fn a_multi_line_password_comes_back_whole() {
        let cmd = "show -s -a Password -- \"G\"\n";
        let reply = format!("{cmd}a\nb\n");
        assert_eq!(reply_value(reply.as_bytes(), cmd), Some(b"a\nb".to_vec()));
    }

    #[test]
    fn drive_writes_nothing_before_the_first_prompt_and_reads_each_entry() {
        let e = entries(&[("A_KEY", "Anthropic/API key"), ("G", "Git token")]);
        let out = transcript(&[
            "show -s -a Password -- \"Anthropic/API key\"\nsk-1\n",
            "show -s -a Password -- \"Git token\"\nghp_2\n",
            "exit\n",
        ]);
        let mut inp = Vec::new();
        let values = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect("both entries are there");
        assert_eq!(values, vec![b"sk-1".to_vec(), b"ghp_2".to_vec()]);
        assert_eq!(
            String::from_utf8(inp).expect("utf8"),
            "show -s -a Password -- \"Anthropic/API key\"\n\
             show -s -a Password -- \"Git token\"\n\
             exit\n"
        );
    }

    #[test]
    fn drive_reports_a_stream_that_ends_before_the_prompt_as_unlock() {
        let e = entries(&[("G", "Git token")]);
        let mut inp = Vec::new();
        let err = drive(&b""[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect_err("no prompt");
        assert!(matches!(err, VaultError::Unlock { .. }), "{err:?}");
        assert!(inp.is_empty(), "nothing is written before the prompt");
    }

    #[test]
    fn drive_reports_a_stream_that_ends_mid_session_as_closed() {
        let e = entries(&[("G", "Git token")]);
        let mut inp = Vec::new();
        let err = drive(&b"t.kdbx> "[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect_err("stops after the prompt");
        assert!(matches!(err, VaultError::Closed), "{err:?}");
    }

    #[test]
    fn a_password_holding_the_prompt_stops_the_run_rather_than_being_cut() {
        let e = entries(&[("G", "Git token")]);
        let out = transcript(&[
            "show -s -a Password -- \"Git token\"\nab t.kdbx> cd\n",
            "exit\n",
        ]);
        let mut inp = Vec::new();
        let err = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect_err("the cut reply has no closing newline");
        assert!(matches!(err, VaultError::Missing { .. }), "{err:?}");
    }

    #[test]
    fn drive_names_a_missing_entry() {
        let e = entries(&[("G", "Nope")]);
        let out = transcript(&["show -s -a Password -- \"Nope\"\n"]);
        let mut inp = Vec::new();
        let err = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect_err("missing");
        let msg = err.to_string();
        assert!(msg.contains('G') && msg.contains("Nope"), "{msg}");
    }

    #[test]
    fn env_text_writes_lines_the_repo_token_reader_reads_back() {
        let values: [&[u8]; 5] =
            [b"plain", b"has space", b"a #b", b"\"q\"", b"$x`y\\z"];
        let e = entries(&[
            ("A", "a"),
            ("B", "b"),
            ("C", "c"),
            ("D", "d"),
            ("E", "e"),
        ]);
        let v: Vec<Vec<u8>> = values.iter().map(|v| v.to_vec()).collect();
        let secrets = env_text(&e, &v).expect("all quotable");
        for (name, want) in ["A", "B", "C", "D", "E"].iter().zip(values) {
            assert_eq!(
                repo_token::lookup(secrets.as_bytes(), name).as_deref(),
                Some(want),
                "{name}"
            );
        }
        assert!(
            secrets.as_bytes().starts_with(b"A=plain\nB='has space'\n"),
            "{:?}",
            String::from_utf8_lossy(secrets.as_bytes())
        );
    }

    #[test]
    fn env_text_refuses_a_line_break_and_an_unquotable_value() {
        let e = entries(&[("A", "a")]);
        for (value, multiline) in
            [(&b"a\nb"[..], true), (b"a\rb", true), (b"it's $x", false)]
        {
            let err = env_text(&e, &[value.to_vec()]).expect_err("refused");
            assert_eq!(
                matches!(err, VaultError::Multiline { .. }),
                multiline,
                "{err:?}"
            );
            assert!(!err.to_string().contains("it's"), "no value in: {err}");
        }
    }

    #[test]
    fn env_text_puts_a_value_holding_an_apostrophe_in_double_quotes() {
        let e = entries(&[("A", "a")]);
        let secrets = env_text(&e, &[b"it's x".to_vec()]).expect("quotable");
        assert_eq!(secrets.as_bytes(), b"A=\"it's x\"\n");
        assert_eq!(
            repo_token::lookup(secrets.as_bytes(), "A").as_deref(),
            Some(&b"it's x"[..])
        );
    }

    #[test]
    fn a_vault_with_no_entries_is_refused() {
        let err =
            toml::from_str::<Vault>("database = \"~/s.kdbx\"\n[entries]\n")
                .expect_err("no entries");
        assert!(err.to_string().contains("vault.entries"), "{err}");
    }
}
