//! What `[source.vault]` may be: a KeePassXC database on the
//! workstation, and the entries in it that hold the project's
//! secrets.
//!
//! The vault is the other source of what `super::EnvFilePath`
//! reads. A config names one or the other, and either way bombyx
//! ends up with a `super::Secrets` of `NAME=value` lines, so the
//! file staged for the guest has the same shape whichever source
//! it came from. Only bombyx's own error messages say which.
//!
//! **bombyx never holds the master password.** It starts two
//! processes, `keepassxc-cli open` and a small `sh` script, and
//! joins them with a pipe it never reads. The script reads the
//! password from the terminal and writes it into that pipe first.
//! After that it forwards what bombyx writes, and bombyx writes
//! one `show` command per entry. So the database is unlocked once
//! per run, and the only secrets bombyx sees are the entries the
//! config names. `session` starts the two processes and says why
//! they are two.
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

use super::env_file::Secrets;
use super::error::FieldError;
use super::guards;
use super::workstation_path;
use crate::newtype::{checked_str_newtype, checked_str_try_from};

mod session;

/// The `[source.vault]` table: which database to open, and
/// which entry holds each variable.
///
/// Built through `VaultFields`, so a table with no entries is
/// refused while the config parses.
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
    /// Returns what `super::workstation_path::check_file`
    /// returns, naming `vault.database`.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_database(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// Expands a leading `~/` and returns the path bombyx opens.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::NoHome`] when the value needs a
    /// home directory and the environment names none.
    pub fn resolve<F>(&self, getenv: F) -> Result<PathBuf, VaultError>
    where
        F: Fn(&str) -> Option<String>,
    {
        workstation_path::resolve_home(Self::FIELD, &self.0, getenv)
            .map_err(|e| VaultError::NoHome { value: e.value })
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
    workstation_path::check_file(VaultDatabase::FIELD, value)
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

    /// The database path starts with `~/`, and this machine's
    /// environment names no home directory.
    #[error(
        "`vault.database` is `{value}`, and neither HOME nor \
         USERPROFILE names a directory -- both are unset or empty \
         -- so `~` names nothing"
    )]
    NoHome {
        /// The value, as the operator wrote it.
        value: String,
    },

    /// The path names something that is not a regular file.
    #[error("`vault.database` names {path}, which is not a regular file")]
    NotAFile {
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
    },

    /// The database is a file bombyx may not read.
    ///
    /// Asked before the password prompt, because `keepassxc-cli`
    /// would say so only after starting, while the script reading
    /// the password still waits on the terminal with echo off.
    #[error("`vault.database` names {path}, which could not be read")]
    Unreadable {
        /// The path bombyx tried, after expanding `~`.
        path: PathBuf,
        /// What the operating system said.
        source: io::Error,
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
    #[error("could not unlock {path}; `keepassxc-cli` said why above")]
    Unlock {
        /// The database, after expanding `~`.
        path: PathBuf,
    },

    /// `keepassxc-cli` stopped in the middle of the session.
    #[error("`keepassxc-cli` stopped before bombyx had read every entry")]
    Closed,

    /// `keepassxc-cli` printed something other than the reply to
    /// the command bombyx sent.
    ///
    /// The shell's prompt is the only thing that separates one
    /// reply from the next, so a password holding a line that
    /// looks like the prompt shifts every reply after it. bombyx
    /// stops rather than write a value under the wrong name.
    #[error(
        "`keepassxc-cli`'s output did not match the commands bombyx \
         sent; a password in `vault.entries` may hold text that \
         looks like the shell prompt"
    )]
    Desync,

    /// Reading from or writing to `keepassxc-cli` failed.
    #[error("could not talk to `keepassxc-cli`")]
    Io(#[from] io::Error),

    /// The database holds no entry at the path the config names.
    #[error(
        "`vault.entries` maps `{name}` to \"{entry}\", which is not \
         in the database"
    )]
    Missing {
        /// The variable.
        name: SecretName,
        /// The entry path.
        entry: EntryPath,
    },

    /// The entry's password spans more than one line, which a
    /// `NAME=value` line cannot hold.
    #[error(
        "the password of \"{entry}\" (`{name}`) spans more than one \
         line"
    )]
    Multiline {
        /// The variable.
        name: SecretName,
        /// The entry path.
        entry: EntryPath,
    },

    /// The entry's password holds a `'` and also a `"`, `$`, `\`
    /// or backtick, so neither quote keeps it intact.
    #[error(
        "the password of \"{entry}\" (`{name}`) holds a `'` together \
         with one of `\"`, `$`, `\\` or a backtick, and no quoting \
         of a `NAME=value` line keeps both"
    )]
    Unquotable {
        /// The variable.
        name: SecretName,
        /// The entry path.
        entry: EntryPath,
    },
}

impl Vault {
    /// The config key this type reads.
    pub const FIELD: &'static str = "vault";

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
        // message for a missing or unreadable file comes after the
        // password script is already waiting on the terminal.
        if !path.is_file() {
            return Err(VaultError::NotAFile { path });
        }
        if let Err(source) = std::fs::File::open(&path) {
            return Err(VaultError::Unreadable { path, source });
        }
        let values = session::read(&path, &self.entries)?;
        env_text(&values)
    }
}

/// What a session read: each variable, the entry it came from,
/// and that entry's password.
///
/// One list rather than values beside `entries`, so a value can
/// never be written under a name it was not read for.
type Values<'a> = Vec<(&'a SecretName, &'a EntryPath, Vec<u8>)>;

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

/// Reads until the bytes read so far end with `end`, and returns
/// them without it, or `None` when the stream ends first.
fn read_until_end<R: BufRead>(
    out: &mut R,
    end: &[u8],
) -> io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    loop {
        if buf.ends_with(end) {
            buf.truncate(buf.len() - end.len());
            return Ok(Some(buf));
        }
        let mut byte = [0u8; 1];
        if out.read(&mut byte)? == 0 {
            return Ok(None);
        }
        buf.push(byte[0]);
    }
}

/// The value in one reply body, or `None` for an entry the
/// database does not hold.
///
/// A body is what the shell prints after a command's echo, up to
/// the next prompt. For an entry that exists it is the value and
/// a newline, so an empty password leaves a bare newline; a
/// missing entry leaves nothing at all.
///
/// # Errors
///
/// Returns [`VaultError::Desync`] when the body does not end the
/// line, which means the prompt appeared inside a password and
/// cut the body short.
fn body_value(body: &[u8]) -> Result<Option<Vec<u8>>, VaultError> {
    if body.is_empty() {
        return Ok(None);
    }
    let value = body.strip_suffix(b"\n").ok_or(VaultError::Desync)?;
    Ok(Some(value.to_vec()))
}

/// The prompt, as far as `drive` has learned it.
///
/// The shell prompts with the database's stored name, or its file
/// name when the stored name is empty, and then `> `. A stored
/// name may itself hold `> `, so the first `> ` is not always the
/// end. What does end it is the echo of the first command, so the
/// prompt is complete once that echo has been read.
struct Prompt {
    /// The prompt's bytes read so far.
    bytes: Vec<u8>,
    /// Whether an echo has been read after them, so they are the
    /// whole prompt.
    complete: bool,
}

impl Prompt {
    /// Reads the echo of `command`, which the shell prints right
    /// after the prompt.
    ///
    /// The first time, whatever sits before the echo is the rest
    /// of the prompt. After that the echo is read by its length
    /// and compared, rather than read up to a prompt, so an entry
    /// path holding the prompt's text cannot end it early.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Closed`] when the stream ends first,
    /// and [`VaultError::Desync`] when the bytes are not the echo.
    fn read_echo<R: BufRead>(
        &mut self,
        out: &mut R,
        command: &str,
    ) -> Result<(), VaultError> {
        if !self.complete {
            let rest = self.read_rest(out, command)?;
            self.bytes.extend_from_slice(&rest);
            self.complete = true;
            return Ok(());
        }
        let mut echo = vec![0u8; command.len()];
        out.read_exact(&mut echo).map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => VaultError::Closed,
            _ => VaultError::Io(e),
        })?;
        if echo != command.as_bytes() {
            return Err(VaultError::Desync);
        }
        Ok(())
    }
}

impl Prompt {
    /// Reads up to the first echo of `command` and returns what
    /// came before it: the rest of the prompt.
    ///
    /// Bounded, so a shell that does not echo fails rather than
    /// hangs. Two rules end the read early, and both look only at
    /// the bytes before the point where the echo could have begun,
    /// because an entry path may hold the prompt's text and the
    /// echo then holds it too.
    ///
    /// - **A newline.** The prompt holds none, so a newline there
    ///   means the shell printed something other than the echo.
    ///   Without an echo, `t.kdbx> ` is followed by `ghp_2\n`: a
    ///   value, then a newline, and no command in sight.
    /// - **The prompt again.** `self.bytes` holds the prompt as far
    ///   as the first `> `. Seeing it again means the shell has
    ///   already answered and is asking for the next command. A
    ///   missing entry without an echo prints nothing, so the
    ///   stream reads `t.kdbx> t.kdbx> `.
    ///
    /// A stored name that repeats its own first part fails the
    /// second rule. Named `a> a> b`, the database prompts
    /// `a> a> b> `; bombyx learns `a> ` and then reads `a> ` again,
    /// which looks exactly like the shell asking twice. The run
    /// stops rather than guess.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Closed`] when the stream ends first,
    /// and [`VaultError::Desync`] when either rule above fires.
    fn read_rest<R: BufRead>(
        &self,
        out: &mut R,
        command: &str,
    ) -> Result<Vec<u8>, VaultError> {
        let mut buf = Vec::new();
        loop {
            if buf.ends_with(command.as_bytes()) {
                buf.truncate(buf.len() - command.len());
                return Ok(buf);
            }
            // The earliest point from which the rest of `buf` could
            // still be the start of the echo.
            let echo_from = (0..=buf.len())
                .find(|&i| command.as_bytes().starts_with(&buf[i..]))
                .unwrap_or(buf.len());
            let before = &buf[..echo_from];
            if before.contains(&b'\n')
                || before.windows(self.bytes.len()).any(|w| w == self.bytes)
            {
                return Err(VaultError::Desync);
            }
            let mut byte = [0u8; 1];
            if out.read(&mut byte)? == 0 {
                return Err(VaultError::Closed);
            }
            buf.push(byte[0]);
        }
    }
}

/// Speaks to an unlocked-or-unlocking `keepassxc-cli open`:
/// waits for the first prompt, writes one `show` per entry, and
/// collects each value.
///
/// The wait matters. `keepassxc-cli` buffers its standard input
/// while it reads the password, so a command written before the
/// first prompt is swallowed with it.
///
/// **The prompt is learned, not predicted**, because bombyx
/// cannot know the database's stored name. `Prompt` says how.
///
/// The prompt is the only thing that ends a reply, so a password
/// holding it would cut its own reply short. Three checks catch
/// that: a value must end its line, every echo must be its own
/// command's, and after `exit` nothing may follow its echo. A cut
/// reply fails one of them, and the run stops rather than write a
/// value under the wrong name.
///
/// # Errors
///
/// Returns [`VaultError::Unlock`] when the stream ends before
/// the first prompt, [`VaultError::Closed`] when it ends later,
/// [`VaultError::Missing`] for an entry the database lacks, and
/// [`VaultError::Desync`] when a reply is not the one asked for.
fn drive<'a, R: Read, W: Write>(
    out: R,
    inp: &mut W,
    database: &Path,
    entries: &'a BTreeMap<SecretName, EntryPath>,
) -> Result<Values<'a>, VaultError> {
    let mut out = BufReader::new(out);
    let Some(mut first) = read_until_end(&mut out, b"> ")? else {
        return Err(VaultError::Unlock {
            path: database.to_owned(),
        });
    };
    first.extend_from_slice(b"> ");
    let mut prompt = Prompt {
        bytes: first,
        complete: false,
    };
    let mut values = Vec::with_capacity(entries.len());
    for (name, entry) in entries {
        let command = show_command(entry);
        inp.write_all(command.as_bytes())?;
        inp.flush()?;
        prompt.read_echo(&mut out, &command)?;
        let body = read_until_end(&mut out, &prompt.bytes)?
            .ok_or(VaultError::Closed)?;
        let value = body_value(&body)?.ok_or_else(|| VaultError::Missing {
            name: name.clone(),
            entry: entry.clone(),
        })?;
        values.push((name, entry, value));
    }
    inp.write_all(b"exit\n")?;
    inp.flush()?;
    prompt.read_echo(&mut out, "exit\n")?;
    let mut tail = Vec::new();
    out.read_to_end(&mut tail)?;
    if !tail.is_empty() {
        return Err(VaultError::Desync);
    }
    Ok(values)
}

/// The error for a program that could not be started.
///
/// `keepassxc-cli` not found is the operator's to fix by
/// installing it, so it gets its own message; anything else, and
/// any failure to start `sh`, is reported as the spawn failure it
/// is.
fn spawn_error(program: &'static str, source: io::Error) -> VaultError {
    if program == "keepassxc-cli" && source.kind() == io::ErrorKind::NotFound {
        return VaultError::NotInstalled;
    }
    VaultError::Spawn { program, source }
}

/// What a session returns once both children have been waited on.
///
/// `drive`'s own error comes first, because it says what went
/// wrong; a failed wait only says the cleanup failed too.
fn finish(
    driven: Result<Values<'_>, VaultError>,
    waited: [io::Result<()>; 2],
) -> Result<Values<'_>, VaultError> {
    let values = driven?;
    for wait in waited {
        wait?;
    }
    Ok(values)
}

/// Writes the values out as `NAME=value` lines, in the order
/// the session read them.
///
/// A value stays bare only when every byte is a letter, a digit
/// or one of `_ . / : @ % + , -`. Those are the bytes neither
/// `super::repo_token`'s reader nor a shell sourcing the file
/// reads as syntax. `~` and `=` are not among them. At the start
/// of an assignment's value, and after an unquoted `:` in it, a
/// shell replaces `~` with a home directory, and zsh replaces
/// `=cmd` with the path of the command `cmd`. An allowlist rather
/// than a list of the dangerous bytes, because the shell's list is
/// long and a byte missing from it runs as a command.
///
/// Every other value is quoted. It takes `'`, inside which both
/// readers take every byte literally. A value holding a `'` takes
/// `"` instead, which is literal to both as long as the value
/// holds no `"`, `$`, `\` or backtick.
///
/// # Errors
///
/// Returns [`VaultError::Multiline`] for a value holding a line
/// break, and [`VaultError::Unquotable`] for one that fits
/// neither quote.
fn env_text(values: &Values<'_>) -> Result<Secrets, VaultError> {
    let mut text = Vec::new();
    for &(name, entry, ref value) in values {
        let refused = |multiline: bool| {
            let name = name.clone();
            let entry = entry.clone();
            if multiline {
                VaultError::Multiline { name, entry }
            } else {
                VaultError::Unquotable { name, entry }
            }
        };
        if value.iter().any(|b| matches!(b, b'\n' | b'\r')) {
            return Err(refused(true));
        }
        let needs_quotes = !value.iter().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'_' | b'.'
                        | b'/'
                        | b':'
                        | b'@'
                        | b'%'
                        | b'+'
                        | b','
                        | b'-'
                )
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

    /// `entries`, each paired with the value at the same position
    /// in `values`, as a session hands them to `env_text`.
    fn paired<'a>(
        entries: &'a BTreeMap<SecretName, EntryPath>,
        values: &[&[u8]],
    ) -> Values<'a> {
        assert_eq!(entries.len(), values.len(), "one value per entry");
        entries
            .iter()
            .zip(values)
            .map(|((n, e), v)| (n, e, v.to_vec()))
            .collect()
    }

    /// Each variable a session read, with its value.
    fn named(values: &Values<'_>) -> Vec<(String, Vec<u8>)> {
        values
            .iter()
            .map(|(n, _, v)| (n.as_str().to_owned(), v.clone()))
            .collect()
    }

    /// What `keepassxc-cli open t.kdbx` printed on stdout for
    /// the commands `drive` writes, measured against 2.7.6: the
    /// prompt, then per command its echo and the value, and a
    /// prompt after each reply but the one to `exit`, after which
    /// the shell ends.
    fn transcript(replies: &[&str]) -> Vec<u8> {
        let mut t = b"t.kdbx> ".to_vec();
        for r in replies {
            t.extend_from_slice(r.as_bytes());
            if *r != "exit\n" {
                t.extend_from_slice(b"t.kdbx> ");
            }
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
    fn a_body_drops_the_newline() {
        assert_eq!(
            body_value(b"ghp_abc\n").expect("framed"),
            Some(b"ghp_abc".to_vec())
        );
    }

    #[test]
    fn a_body_that_does_not_end_its_line_is_a_desync() {
        assert!(matches!(body_value(b"ghp"), Err(VaultError::Desync)));
    }

    #[test]
    fn an_empty_password_and_a_missing_entry_differ() {
        assert_eq!(body_value(b"\n").expect("framed"), Some(Vec::new()));
        assert_eq!(body_value(b"").expect("framed"), None);
    }

    #[test]
    fn a_multi_line_password_comes_back_whole() {
        assert_eq!(
            body_value(b"a\nb\n").expect("framed"),
            Some(b"a\nb".to_vec())
        );
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
        assert_eq!(
            named(&values),
            [
                ("A_KEY".to_owned(), b"sk-1".to_vec()),
                ("G".to_owned(), b"ghp_2".to_vec())
            ]
        );
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
        assert!(matches!(err, VaultError::Desync), "{err:?}");
    }

    #[test]
    fn drive_reads_a_database_whose_prompt_is_its_stored_name() {
        // keepassxc-cli prompts with the database's stored name when
        // it has one, whatever the file is called.
        let e = entries(&[("G", "Git token")]);
        let out = b"My Vault> show -s -a Password -- \"Git token\"\n\
                    ghp_2\nMy Vault> exit\n";
        let mut inp = Vec::new();
        let values = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect("the prompt is learned, not predicted");
        assert_eq!(named(&values), [("G".to_owned(), b"ghp_2".to_vec())]);
    }

    #[test]
    fn drive_reads_a_database_whose_stored_name_holds_the_prompt_mark() {
        // The prompt is `Work> Keys> `, so the first `> ` is not
        // its end.
        let e = entries(&[("G", "Git token")]);
        let out = b"Work> Keys> show -s -a Password -- \"Git token\"\n\
                    ghp_2\nWork> Keys> exit\n";
        let mut inp = Vec::new();
        let values = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect("the rest of the prompt is learned from the echo");
        assert_eq!(named(&values), [("G".to_owned(), b"ghp_2".to_vec())]);
    }

    #[test]
    fn drive_stops_when_the_first_command_is_not_echoed() {
        // A build that does not echo prints the value, or nothing
        // for a missing entry, and then its prompt again. Waiting
        // for an echo that never comes would hang.
        let e = entries(&[("G", "Git token")]);
        for out in [&b"t.kdbx> ghp_2\nt.kdbx> "[..], b"t.kdbx> t.kdbx> "] {
            let mut inp = Vec::new();
            let err = drive(out, &mut inp, Path::new("/d/t.kdbx"), &e)
                .expect_err("no echo");
            assert!(matches!(err, VaultError::Desync), "{err:?}");
        }
    }

    #[test]
    fn drive_reads_an_entry_whose_path_holds_the_prompt() {
        let e = entries(&[("G", "t.kdbx> api")]);
        let out = transcript(&[
            "show -s -a Password -- \"t.kdbx> api\"\nghp_2\n",
            "exit\n",
        ]);
        let mut inp = Vec::new();
        let values = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect("the echo is read by its length, not up to a prompt");
        assert_eq!(named(&values), [("G".to_owned(), b"ghp_2".to_vec())]);
    }

    #[test]
    fn drive_refuses_a_reply_that_is_not_the_echo_of_its_command() {
        // A's password holds a line starting with the prompt, which
        // cuts A's reply short and shifts the rest onto B's.
        let e = entries(&[("A", "a"), ("B", "b")]);
        let out = transcript(&[
            "show -s -a Password -- \"a\"\nx\nt.kdbx> y\n",
            "show -s -a Password -- \"b\"\nv\n",
            "exit\n",
        ]);
        let mut inp = Vec::new();
        let err = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect_err("B's reply does not start with B's command");
        assert!(matches!(err, VaultError::Desync), "{err:?}");
    }

    #[test]
    fn drive_refuses_output_left_over_after_the_last_entry() {
        let e = entries(&[("A", "a")]);
        let out = transcript(&[
            "show -s -a Password -- \"a\"\nx\nt.kdbx> y\n",
            "exit\n",
        ]);
        let mut inp = Vec::new();
        let err = drive(&out[..], &mut inp, Path::new("/d/t.kdbx"), &e)
            .expect_err("the rest of A's password is still in the stream");
        assert!(matches!(err, VaultError::Desync), "{err:?}");
    }

    #[test]
    fn env_text_quotes_every_byte_a_shell_reads_as_syntax() {
        // A shell expands `~` at the start of an assignment's value
        // and after an unquoted `:` in it, so every `~` is quoted.
        // zsh expands a leading `=`, and one after a `:`, into a
        // command's path, so every `=` is quoted too.
        let values: [&[u8]; 8] = [
            b"k9;Zq&w(",
            b"a|b<c>d)",
            b"~x",
            b"x:~/y",
            b"a:~root",
            b"=R2d2",
            b"a:=b",
            b"abc==",
        ];
        let names = ["A", "B", "C", "D", "E", "F", "G", "H"];
        let pairs: Vec<_> = names.iter().map(|n| (*n, "e")).collect();
        let e = entries(&pairs);
        let secrets = env_text(&paired(&e, &values)).expect("quotable");
        assert_eq!(
            secrets.as_bytes(),
            b"A='k9;Zq&w('\nB='a|b<c>d)'\nC='~x'\nD='x:~/y'\nE='a:~root'\n\
              F='=R2d2'\nG='a:=b'\nH='abc=='\n"
        );
        for (name, want) in names.iter().zip(values) {
            assert_eq!(
                repo_token::lookup(secrets.as_bytes(), name).as_deref(),
                Some(want),
                "{name}"
            );
        }
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
        let secrets = env_text(&paired(&e, &values)).expect("all quotable");
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
            let err = env_text(&paired(&e, &[value])).expect_err("refused");
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
        let secrets = env_text(&paired(&e, &[b"it's x"])).expect("quotable");
        assert_eq!(secrets.as_bytes(), b"A=\"it's x\"\n");
        assert_eq!(
            repo_token::lookup(secrets.as_bytes(), "A").as_deref(),
            Some(&b"it's x"[..])
        );
    }

    #[test]
    fn a_missing_keepassxc_cli_is_reported_as_not_installed() {
        let not_found = || io::Error::from(io::ErrorKind::NotFound);
        assert!(matches!(
            spawn_error("keepassxc-cli", not_found()),
            VaultError::NotInstalled
        ));
        assert!(matches!(
            spawn_error("sh", not_found()),
            VaultError::Spawn { program: "sh", .. }
        ));
        assert!(matches!(
            spawn_error(
                "keepassxc-cli",
                io::Error::from(io::ErrorKind::PermissionDenied)
            ),
            VaultError::Spawn {
                program: "keepassxc-cli",
                ..
            }
        ));
    }

    #[test]
    fn a_session_reports_what_drive_said_before_a_failed_wait() {
        let e = entries(&[("A", "a")]);
        let failed = || Err(io::Error::other("wait failed"));
        let err = finish(Err(VaultError::Desync), [failed(), failed()])
            .expect_err("drive failed");
        assert!(matches!(err, VaultError::Desync), "{err:?}");
        let err = finish(Ok(paired(&e, &[b"v"])), [Ok(()), failed()])
            .expect_err("a wait failed");
        assert!(matches!(err, VaultError::Io(_)), "{err:?}");
        let ok = finish(Ok(paired(&e, &[b"v"])), [Ok(()), Ok(())])
            .expect("everything worked");
        assert_eq!(named(&ok), [("A".to_owned(), b"v".to_vec())]);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_database_is_refused_before_anything_starts() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("t.kdbx");
        std::fs::write(&db, b"x").expect("write");
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o000))
            .expect("chmod");
        if std::fs::File::open(&db).is_ok() {
            // Running as root, which reads any file.
            return;
        }
        let vault: Vault = toml::from_str(&format!(
            "database = {:?}\n[entries]\nA = \"a\"\n",
            db.display().to_string()
        ))
        .expect("a vault table");
        let err = vault.read(|_| None).expect_err("mode 000");
        assert!(matches!(err, VaultError::Unreadable { .. }), "{err:?}");
    }

    #[test]
    fn a_vault_with_no_entries_is_refused() {
        let err =
            toml::from_str::<Vault>("database = \"~/s.kdbx\"\n[entries]\n")
                .expect_err("no entries");
        assert!(err.to_string().contains("vault.entries"), "{err}");
    }
}
