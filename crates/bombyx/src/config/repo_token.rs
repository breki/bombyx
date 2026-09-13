//! What `repo_token` and `repo_user` may be, and how bombyx
//! turns the named variable into a credential `git` understands.
//!
//! Three things live here.
//!
//! [`RepoTokenVar`] is the *name* of a variable, not its value.
//! The config states which variable inside `env_file` holds the
//! token, so bombyx need not guess which of them is the git one.
//!
//! [`RepoUser`] is the username that goes with the token. The
//! vendor fixes it and bombyx cannot work it out: a Bitbucket
//! repository access token wants the literal `x-token-auth`,
//! while an Atlassian API token wants the account's email
//! address. Same host, two answers, so the config states it.
//!
//! [`GitCredential`] is the finished file bombyx sends to the
//! guest. `git` reads it through its `store` credential helper,
//! one line of `https://user:token@host`, and `bootstrap.sh`
//! points the clone at it.

use std::fmt;

use serde::Deserialize;
use thiserror::Error;

use super::error::FieldError;
use super::guards;
use crate::newtype::{
    checked_str_newtype, checked_str_parse, checked_str_try_from,
};

/// The name of the variable inside `env_file` holding the git
/// token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RepoTokenVar(String);

impl RepoTokenVar {
    /// The field name, for a message naming it.
    pub const FIELD: &'static str = "repo_token";
}

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it is not a variable name.
    RepoTokenVar,
    FieldError,
    check_token_var
);

checked_str_newtype!(RepoTokenVar, "The variable name, as written.");

checked_str_try_from!(
    /// What serde calls while the config parses.
    RepoTokenVar,
    FieldError,
    check_token_var
);

/// The username `git` sends alongside the token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RepoUser(String);

impl RepoUser {
    /// The field name, for a message naming it.
    pub const FIELD: &'static str = "repo_user";
}

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it carries surrounding
    /// whitespace or a control character.
    RepoUser,
    FieldError,
    check_user
);

checked_str_newtype!(RepoUser, "The username, as written.");

checked_str_try_from!(
    /// What serde calls while the config parses.
    RepoUser,
    FieldError,
    check_user
);

/// How the guest authenticates an https clone: which variable
/// holds the token, and the username it is sent under.
///
/// The two are one value because neither means anything alone.
/// A variable name with no username leaves bombyx guessing the
/// vendor's literal, which is what stating it exists to avoid,
/// and a username with no variable name has no token to go
/// with. Written as two `Option` fields on `super::Source` the
/// split state would be representable, and `Source`'s fields
/// are public -- so every reader of them would have to check
/// the pairing, and the ones that did not would disagree.
///
/// The config file still spells two keys. `super::Source`'s
/// `TryFrom` is what turns them into this, and it is where the
/// message for one without the other lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoToken {
    /// The variable inside `env_file` holding the token.
    pub var: RepoTokenVar,
    /// The username `git` sends the token under.
    pub user: RepoUser,
}

/// The contents of the credential file `git` reads.
#[derive(Clone, PartialEq, Eq)]
pub struct GitCredential(Vec<u8>);

impl GitCredential {
    /// The contents themselves, for whoever writes them into a
    /// pipe.
    #[must_use]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for GitCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GitCredential({} bytes)", self.0.len())
    }
}

/// Why bombyx could not build a credential from the file.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RepoTokenError {
    /// The file holds no such variable.
    #[error(
        "`repo_token` names `{var}`, and {path} holds no such \
         variable"
    )]
    NotInFile {
        /// The variable the config named.
        var: String,
        /// The file, as the operator wrote it.
        path: String,
    },

    /// The variable is there and its value is empty.
    #[error("`{var}` in {path} is empty, so there is no token")]
    EmptyValue {
        /// The variable the config named.
        var: String,
        /// The file, as the operator wrote it.
        path: String,
    },

    /// The variable is there and a comment is all that follows
    /// the `=`.
    ///
    /// Separate from [`RepoTokenError::EmptyValue`] because the
    /// operator's file does not look empty. Telling them the
    /// variable is empty sends them looking for a blank value
    /// that is not there, when what they have is a value bombyx
    /// read as a comment -- and the cure, quoting, is not
    /// something that message would suggest.
    #[error(
        "`{var}` in {path} is a comment: a `#` with nothing but \
         whitespace in front of it ends a value, so bombyx found \
         no token there. Quote the value to keep a `#` inside it"
    )]
    CommentedOutValue {
        /// The variable the config named.
        var: String,
        /// The file, as the operator wrote it.
        path: String,
    },

    /// The config names a `repo_token` and no `env_file`, so
    /// there is no file to read the variable out of.
    ///
    /// Unreachable from a config serde parsed, like
    /// [`RepoTokenError::NoHttpsHost`] below and for the same
    /// reason: `super::Source`'s `TryFrom` refuses the pairing
    /// while the file is read, and `super::Config`'s fields are
    /// public.
    #[error(
        "`repo_token` names `{var}` and no `env_file` is set, so \
         there is no file to read it out of"
    )]
    NoEnvFile {
        /// The variable the config named.
        var: String,
    },

    /// `repo` names no https host, so the credential would have
    /// nowhere to go.
    ///
    /// A config that serde parsed never reaches this: the rule
    /// in `super::Source`'s `TryFrom` refuses the pairing while
    /// the file is being read. `Config`'s fields are public, so
    /// a config assembled in code can still arrive here.
    #[error(
        "`repo` is `{repo}`, which names no https host, so \
         `repo_token` has nothing to authenticate against"
    )]
    NoHttpsHost {
        /// The repository address, as the operator wrote it.
        repo: String,
    },
}

/// Finds `var` in a `.env` file and returns its value.
///
/// The whole format bombyx accepts is in this one function, and
/// every rule of it was chosen rather than inherited, because a
/// value read slightly wrong becomes a token the server refuses
/// with a message about credentials rather than about parsing.
///
/// A line carrying no `=` is skipped, and that one rule covers
/// a blank line as well. A comment is skipped too, without a
/// rule of its own: `# BITBUCKET_TOKEN=x` has `# BITBUCKET_TOKEN`
/// in front of its `=`, which is not the name anything asks
/// for. Otherwise the name is what precedes the first `=` and
/// the value is everything after it, so a token containing `=`
/// needs no escaping. An `export` in front of the name is
/// accepted, because a file people also `source` usually has
/// one. Whitespace around the name and around the value is
/// dropped.
///
/// [`value_of`] then decides where the value ends: a quoted one
/// ends at its closing quote, and an unquoted one ends at a `#`
/// that has whitespace in front of it. That covers the note
/// somebody writes beside a secret.
///
/// The last assignment wins, which is what a shell reading the
/// same file ends up with.
///
/// Bytes rather than text throughout. `super::Secrets` accepts
/// a file that is not UTF-8, and a token that came back as a
/// replacement character would fail authentication with nothing
/// pointing at the encoding.
pub(crate) fn lookup(secrets: &[u8], var: &str) -> Option<Vec<u8>> {
    scan(secrets, var, value_of)
}

/// [`lookup`] with neither rule that decides where a value
/// ends.
///
/// Hands back the text after the first `=`, trimmed, with the
/// quote rule and the comment rule both skipped.
///
/// One caller: [`credential`], choosing between two messages for
/// a value that came back empty. Empty here too means the line
/// really is a bare `TOKEN=`. Non-empty here means something was
/// written and one of the two rules consumed it, and `credential`
/// runs the comment rule against this text to find out which --
/// so `TOKEN=""` is reported as empty rather than as a comment.
fn lookup_raw(secrets: &[u8], var: &str) -> Option<Vec<u8>> {
    scan(secrets, var, |v| v)
}

/// The line walk both lookups share, with `value` deciding
/// where a value ends.
fn scan(
    secrets: &[u8],
    var: &str,
    value: fn(&[u8]) -> &[u8],
) -> Option<Vec<u8>> {
    let mut found = None;
    for line in secrets.split(|b| *b == b'\n') {
        let line = trim(line);
        let line = match strip_export(line) {
            Some(rest) => trim(rest),
            None => line,
        };
        let Some(eq) = line.iter().position(|b| *b == b'=') else {
            continue;
        };
        if trim(&line[..eq]) != var.as_bytes() {
            continue;
        }
        found = Some(value(trim(&line[eq + 1..])).to_vec());
    }
    found
}

/// Drops a leading `export` keyword, or answers `None`.
///
/// The word has to be followed by whitespace. Without that
/// check `exportED=1` would be read as an export of `ED`, when
/// it is a variable called `exportED`: nothing separates the
/// word from the name, so there is no keyword there at all.
///
/// Any ASCII whitespace separates them, a tab included. A tab is
/// the one worth naming, because getting it wrong does not
/// mangle the value -- it makes the name `export\tNAME`, so
/// bombyx reports a file holding no such variable while the
/// variable is on the line in front of it.
fn strip_export(line: &[u8]) -> Option<&[u8]> {
    let rest = line.strip_prefix(b"export".as_slice())?;
    rest.first()
        .is_some_and(u8::is_ascii_whitespace)
        .then_some(rest)
}

/// Drops ASCII whitespace from both ends of `line`.
///
/// A carriage return counts as whitespace, so this is also what
/// takes the `\r` off a line of a file written on Windows --
/// both from the end of the name and from the end of the value.
fn trim(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &line[start..end]
}

/// Takes the value out of what follows the first `=`.
///
/// Two shapes, and which one applies is decided by the first
/// character.
///
/// A value opening with a quote ends at the next quote of the
/// same kind, and whatever follows that is discarded. So
/// `"abc" # note` is `abc`, and quoting is how an operator
/// keeps a value that really does contain a `#` after a space.
/// A quote that never closes is not a quoted value at all, and
/// the second shape handles it.
///
/// Every other value runs to the end of the line, except that a
/// `#` with whitespace in front of it ends it. That is the
/// trailing comment people write beside a secret. A `#` without
/// whitespace in front stays, because a token may contain one.
fn value_of(value: &[u8]) -> &[u8] {
    if let Some(&quote) = value.first()
        && (quote == b'"' || quote == b'\'')
        && let Some(end) = value[1..].iter().position(|b| *b == quote)
    {
        return &value[1..=end];
    }
    strip_inline_comment(value)
}

/// Cuts `value` at a `#` that nothing but whitespace precedes.
///
/// The start of the value counts as whitespace, so a value that
/// is only a comment comes back empty. That is the unfilled
/// slot in a `.env` template -- `TOKEN= # paste yours here` --
/// and coming back empty is what makes
/// [`RepoTokenError::EmptyValue`] name the variable and the
/// file, instead of the comment text travelling to the server
/// as a token.
///
/// A `#` with a non-blank character in front stays, because a
/// token may contain one, and a quoted value never reaches here
/// at all.
fn strip_inline_comment(value: &[u8]) -> &[u8] {
    for i in 0..value.len() {
        let blank_in_front = i == 0 || value[i - 1].is_ascii_whitespace();
        if value[i] == b'#' && blank_in_front {
            return trim(&value[..i]);
        }
    }
    value
}

/// Percent-encodes every byte a URL does not leave alone.
///
/// The credential line is a URL, so a `/` or an `@` inside the
/// token would end the password early and leave `git` reading
/// the remainder as a host. Encoding is what stops that.
///
/// The rule is an allowlist: the unreserved set from RFC 3986
/// survives and every other byte is written as `%XX`. Naming
/// the safe bytes rather than the dangerous ones means a
/// spelling nobody thought of is encoded rather than passed
/// through. Encoding a byte that needed no encoding costs
/// nothing, since `git` decodes the whole field either way.
pub(crate) fn percent_encode(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(*b));
        } else {
            use std::fmt::Write as _;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Builds the credential file from the token in `secrets`.
///
/// `host` is the authority `git` will contact, port included
/// when the repository names one, and it has to match what
/// `git` asks the helper about or the helper answers nothing.
/// `token` names the variable to read and the username to send.
/// `path` is the `env_file` value as the operator wrote it, and
/// it appears in both errors so a refusal names the file to go
/// and edit.
///
/// # Errors
///
/// Returns [`RepoTokenError`] when the file holds no such
/// variable, or holds it with an empty value.
pub(crate) fn credential(
    host: &str,
    token: &RepoToken,
    secrets: &[u8],
    path: &str,
) -> Result<GitCredential, RepoTokenError> {
    let var = &token.var;
    let user = &token.user;
    let value = lookup(secrets, var.as_str()).ok_or_else(|| {
        RepoTokenError::NotInFile {
            var: var.as_str().to_owned(),
            path: path.to_owned(),
        }
    })?;
    if value.is_empty() {
        // Three ways to arrive here, and one of them deserves
        // a different message. `lookup` hands back an empty
        // value for a bare `TOKEN=`, for `TOKEN=""`, and for
        // `TOKEN= # paste yours here` -- and an operator looking
        // at the last one can see a value on the line, so being
        // told the variable is empty sends them hunting.
        //
        // `lookup_raw` reads the same line with neither value
        // rule applied. Running the comment rule against what it
        // returns is what picks out the third case: only there
        // does a non-empty raw value become empty.
        let commented = lookup_raw(secrets, var.as_str()).is_some_and(|raw| {
            !raw.is_empty() && strip_inline_comment(&raw).is_empty()
        });
        let (v, p) = (var.as_str().to_owned(), path.to_owned());
        return Err(if commented {
            RepoTokenError::CommentedOutValue { var: v, path: p }
        } else {
            RepoTokenError::EmptyValue { var: v, path: p }
        });
    }
    // The trailing newline is part of the format: `git`'s
    // `store` helper reads the file a line at a time.
    let line = format!(
        "https://{user}:{token}@{host}\n",
        user = percent_encode(user.as_str().as_bytes()),
        token = percent_encode(&value),
    );
    Ok(GitCredential(line.into_bytes()))
}

/// Every rule a `repo_token` value must pass, in one place.
///
/// The value names an environment variable, so it gets the
/// shape a shell gives one: a letter or an underscore, then
/// letters, digits and underscores. A name outside that shape
/// could never have been set by the file's own `export` line,
/// so accepting it would mean looking for something that cannot
/// be there.
fn check_token_var(value: &str) -> Result<(), FieldError> {
    guards::check_not_empty(RepoTokenVar::FIELD, value)?;
    guards::check_charset(
        RepoTokenVar::FIELD,
        value,
        |c| c.is_ascii_alphanumeric() || c == '_',
        "letters, digits and underscores",
    )?;
    if value.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(FieldError::invalid(
            RepoTokenVar::FIELD,
            "must not start with a digit, which no shell \
             would accept as a variable name",
        ));
    }
    Ok(())
}

/// Every rule a `repo_user` value must pass, in one place.
///
/// Deliberately not `guards::check_renderable`. That guard
/// refuses a quote and a backslash because the value is written
/// into the generated Vagrantfile, and this one never is: it
/// goes into the credential file percent-encoded, where no
/// character has a meaning left. Borrowing the guard would have
/// refused values for a reason that does not apply and said so
/// in the message.
///
/// Two rules remain, and both are about the value being wrong
/// rather than about escaping. Surrounding whitespace is a
/// copy-paste artifact that would reach the server as part of
/// the username. A control character is in no username the two
/// vendors specify.
fn check_user(value: &str) -> Result<(), FieldError> {
    guards::check_not_empty(RepoUser::FIELD, value)?;
    if value.trim() != value {
        return Err(FieldError::invalid(
            RepoUser::FIELD,
            "must not begin or end with whitespace",
        ));
    }
    if let Some(bad) = value.chars().find(|c| c.is_control()) {
        return Err(FieldError::invalid(
            RepoUser::FIELD,
            format!(
                "control character {bad:?} is not allowed; use \
                 printable characters only"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file's bytes, as `credential` takes them.
    fn secrets(text: &str) -> &[u8] {
        text.as_bytes()
    }

    fn token() -> RepoToken {
        RepoToken {
            var: RepoTokenVar::parse("BITBUCKET_TOKEN").expect("a plain name"),
            user: RepoUser::parse("x-token-auth").expect("a plain username"),
        }
    }

    /// The rendered credential line, for the tests that read it.
    fn line(cred: &GitCredential) -> String {
        String::from_utf8(cred.as_bytes().to_vec())
            .expect("the tests all use ASCII tokens")
    }

    #[test]
    fn a_plain_assignment_is_found() {
        let s = b"A=1\nBITBUCKET_TOKEN=abc\nB=2\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"abc".to_vec()));
    }

    #[test]
    fn a_missing_variable_is_not_found() {
        let s = b"A=1\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), None);
    }

    #[test]
    fn a_commented_out_assignment_does_not_answer_for_the_variable() {
        // Written last, so the "last assignment wins" rule would
        // hand back `wrong` if the comment counted as one.
        //
        // No rule of its own does this. The name in front of the
        // `=` is `# BITBUCKET_TOKEN`, which is not what the
        // config asked for, and the blank line carries no `=` at
        // all.
        let s = b"BITBUCKET_TOKEN=right\n\n# BITBUCKET_TOKEN=wrong\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"right".to_vec()));
    }

    #[test]
    fn an_export_prefix_is_accepted() {
        let s = b"export BITBUCKET_TOKEN=abc\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"abc".to_vec()));
    }

    #[test]
    fn everything_after_the_first_equals_is_the_value() {
        let s = b"BITBUCKET_TOKEN=a=b=c\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"a=b=c".to_vec()));
    }

    #[test]
    fn one_matching_pair_of_quotes_is_stripped() {
        for (raw, want) in [
            ("BITBUCKET_TOKEN=\"abc\"\n", "abc"),
            ("BITBUCKET_TOKEN='abc'\n", "abc"),
            // Not a matching pair, so both stay.
            ("BITBUCKET_TOKEN=\"abc'\n", "\"abc'"),
            // Only the outermost pair goes.
            ("BITBUCKET_TOKEN=\"'abc'\"\n", "'abc'"),
        ] {
            assert_eq!(
                lookup(raw.as_bytes(), "BITBUCKET_TOKEN"),
                Some(want.as_bytes().to_vec()),
                "reading {raw:?}"
            );
        }
    }

    #[test]
    fn a_trailing_comment_is_not_part_of_the_value() {
        // A `#` after whitespace ends the value. Left in, the
        // token reaches the server with a sentence glued to it
        // and the refusal talks about credentials rather than
        // about the note somebody wrote beside one.
        for (raw, want) in [
            ("BITBUCKET_TOKEN=abc   # rotate in June\n", "abc"),
            ("BITBUCKET_TOKEN=\"abc\" # x\n", "abc"),
            // No whitespace in front, so it is part of the
            // token. A `#` can appear in one.
            ("BITBUCKET_TOKEN=ab#c\n", "ab#c"),
            // Quoting is the escape hatch for the other case.
            ("BITBUCKET_TOKEN=\"ab #c\"\n", "ab #c"),
        ] {
            assert_eq!(
                lookup(raw.as_bytes(), "BITBUCKET_TOKEN"),
                Some(want.as_bytes().to_vec()),
                "reading {raw:?}"
            );
        }
    }

    #[test]
    fn the_examples_config_toml_sample_states_are_the_ones_we_read() {
        // The sample is the only operator-facing statement of
        // this format, and it is written from intent, so it
        // drifts silently. These six rows are copied from it by
        // hand -- `CLAUDE.md` under **Documentation style** asks
        // for exactly that, and warns off a test that goes and
        // finds the examples in the document at run time.
        for (line, want) in [
            ("TOKEN=abc=d", "abc=d"),
            ("TOKEN=abc # rotate soon", "abc"),
            ("TOKEN=ab#c", "ab#c"),
            ("TOKEN= # paste yours", ""),
            ("TOKEN=\"abc # d\"", "abc # d"),
            ("TOKEN=\"abc\"xyz", "abc"),
        ] {
            assert_eq!(
                lookup(line.as_bytes(), "TOKEN"),
                Some(want.as_bytes().to_vec()),
                "config.toml.sample says {line:?} yields {want:?}"
            );
        }
    }

    #[test]
    fn a_value_that_is_only_a_comment_is_empty() {
        // The unfilled slot in a `.env` template. Left as it
        // is, the comment text becomes the token and the guest
        // fails against the server with a 401, which is the
        // failure this module exists to turn into a message
        // about the file.
        for raw in [
            "BITBUCKET_TOKEN= # paste yours here\n",
            "BITBUCKET_TOKEN=#note\n",
            "BITBUCKET_TOKEN=  #note\n",
        ] {
            assert_eq!(
                lookup(raw.as_bytes(), "BITBUCKET_TOKEN"),
                Some(Vec::new()),
                "reading {raw:?}"
            );
        }
        // And quoting is still the way to keep one.
        assert_eq!(
            lookup(b"BITBUCKET_TOKEN=\"#real\"\n", "BITBUCKET_TOKEN"),
            Some(b"#real".to_vec())
        );
    }

    #[test]
    fn a_commented_out_value_says_so_rather_than_saying_empty() {
        // The operator's file plainly holds a value on that
        // line. "The variable is empty" sends them looking for
        // a blank one, and never mentions quoting -- which is
        // the only way to keep a value starting with `#`.
        let err = credential(
            "bitbucket.org",
            &token(),
            secrets("BITBUCKET_TOKEN=#s3cret\n"),
            "~/secrets/x.env",
        )
        .expect_err("the comment rule consumed the value");
        assert!(
            matches!(err, RepoTokenError::CommentedOutValue { .. }),
            "{err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("Quote the value"), "{msg}");

        // An empty QUOTED value is empty, not a comment. The
        // operator has already done the thing the comment
        // message tells them to do, so sending them to do it
        // again is the worst answer available.
        let quoted = credential(
            "bitbucket.org",
            &token(),
            secrets("BITBUCKET_TOKEN=\"\"\n"),
            "~/secrets/x.env",
        )
        .expect_err("an empty quoted value is no token");
        assert!(
            matches!(quoted, RepoTokenError::EmptyValue { .. }),
            "{quoted:?}"
        );

        // And a genuinely blank one still says empty, so the
        // two messages stay distinguishable.
        let bare = credential(
            "bitbucket.org",
            &token(),
            secrets("BITBUCKET_TOKEN=\n"),
            "~/secrets/x.env",
        )
        .expect_err("an empty token is no token");
        assert!(
            matches!(bare, RepoTokenError::EmptyValue { .. }),
            "{bare:?}"
        );
    }

    #[test]
    fn a_comment_only_value_is_reported_as_an_empty_token() {
        // The whole point of the row above: the run stops with
        // a message naming the variable and the file, rather
        // than reaching Bitbucket with a sentence as a token.
        let err = credential(
            "bitbucket.org",
            &token(),
            secrets("BITBUCKET_TOKEN= # paste yours here\n"),
            "~/secrets/x.env",
        )
        .expect_err("a comment is not a token");
        assert!(
            matches!(err, RepoTokenError::CommentedOutValue { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn export_is_accepted_ahead_of_any_whitespace() {
        // A tab is the one that bites: without this the name is
        // read as `export\tBITBUCKET_TOKEN`, so bombyx reports a
        // file holding no such variable while it is right there.
        for raw in [
            "export BITBUCKET_TOKEN=abc\n",
            "export\tBITBUCKET_TOKEN=abc\n",
            "export  BITBUCKET_TOKEN=abc\n",
        ] {
            assert_eq!(
                lookup(raw.as_bytes(), "BITBUCKET_TOKEN"),
                Some(b"abc".to_vec()),
                "reading {raw:?}"
            );
        }
    }

    #[test]
    fn a_name_beginning_with_export_is_not_an_export_line() {
        // `exportED=1` is a variable called `exportED`, because
        // nothing separates the word from the name.
        let s = b"exportED=wrong\nED=right\n";
        assert_eq!(lookup(s, "ED"), Some(b"right".to_vec()));
        assert_eq!(lookup(s, "exportED"), Some(b"wrong".to_vec()));
    }

    #[test]
    fn a_trailing_carriage_return_is_dropped() {
        // A file written on Windows ends each line `\r\n`, and a
        // `\r` left on the token reaches the server as part of
        // it. `trim` is what removes it, because a carriage
        // return is ASCII whitespace.
        let s = b"BITBUCKET_TOKEN=abc\r\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"abc".to_vec()));
    }

    #[test]
    fn surrounding_whitespace_around_the_name_is_ignored() {
        let s = b"   BITBUCKET_TOKEN = abc \n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"abc".to_vec()));
    }

    #[test]
    fn a_name_that_only_shares_a_prefix_is_not_a_match() {
        let s = b"BITBUCKET_TOKEN_OLD=wrong\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), None);
    }

    #[test]
    fn the_last_assignment_wins() {
        // What a shell sourcing the file would end up with.
        let s = b"BITBUCKET_TOKEN=first\nBITBUCKET_TOKEN=second\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"second".to_vec()));
    }

    #[test]
    fn a_line_without_an_equals_is_skipped() {
        let s = b"BITBUCKET_TOKEN\nBITBUCKET_TOKEN=abc\n";
        assert_eq!(lookup(s, "BITBUCKET_TOKEN"), Some(b"abc".to_vec()));
    }

    #[test]
    fn unreserved_bytes_survive_encoding() {
        assert_eq!(percent_encode(b"aZ0-._~"), "aZ0-._~");
    }

    #[test]
    fn every_character_with_a_meaning_in_a_url_is_encoded() {
        // Each of these would end the user, the password or the
        // host if it reached the line as itself.
        assert_eq!(percent_encode(b"/"), "%2F");
        assert_eq!(percent_encode(b"@"), "%40");
        assert_eq!(percent_encode(b":"), "%3A");
        assert_eq!(percent_encode(b"#"), "%23");
        assert_eq!(percent_encode(b"?"), "%3F");
        assert_eq!(percent_encode(b"%"), "%25");
    }

    #[test]
    fn a_non_utf8_byte_is_encoded_rather_than_refused() {
        // `Secrets` accepts any bytes, so the token may be
        // latin-1 and must still reach the file whole.
        assert_eq!(percent_encode(&[0xff]), "%FF");
    }

    #[test]
    fn the_credential_line_names_the_origin_the_user_and_token() {
        let cred = credential(
            "bitbucket.org",
            &token(),
            secrets("BITBUCKET_TOKEN=abc\n"),
            "~/secrets/x.env",
        )
        .expect("the variable is in the file");
        assert_eq!(line(&cred), "https://x-token-auth:abc@bitbucket.org\n");
    }

    #[test]
    fn a_token_with_url_characters_is_encoded_into_the_line() {
        let cred = credential(
            "bitbucket.org",
            &token(),
            secrets("BITBUCKET_TOKEN=a/b@c\n"),
            "~/secrets/x.env",
        )
        .expect("the variable is in the file");
        assert_eq!(
            line(&cred),
            "https://x-token-auth:a%2Fb%40c@bitbucket.org\n"
        );
    }

    #[test]
    fn a_missing_variable_names_both_the_variable_and_the_file() {
        let err = credential(
            "bitbucket.org",
            &token(),
            secrets("OTHER=abc\n"),
            "~/secrets/x.env",
        )
        .expect_err("the variable is not in the file");
        let msg = err.to_string();
        assert!(msg.contains("BITBUCKET_TOKEN"), "{msg}");
        assert!(msg.contains("~/secrets/x.env"), "{msg}");
    }

    #[test]
    fn an_empty_value_is_refused() {
        let err = credential(
            "bitbucket.org",
            &token(),
            secrets("BITBUCKET_TOKEN=\n"),
            "~/secrets/x.env",
        )
        .expect_err("an empty token is no token");
        assert!(matches!(err, RepoTokenError::EmptyValue { .. }));
    }

    #[test]
    fn a_variable_name_must_look_like_one() {
        for bad in ["", " ", "1ABC", "A-B", "A B", "A=B"] {
            assert!(
                RepoTokenVar::parse(bad).is_err(),
                "{bad:?} must be refused"
            );
        }
        for good in ["A", "_A", "BITBUCKET_TOKEN", "a9_"] {
            assert!(
                RepoTokenVar::parse(good).is_ok(),
                "{good:?} must be accepted"
            );
        }
    }

    #[test]
    fn a_username_must_be_printable_and_untrimmed() {
        for bad in ["", "  ", " x-token-auth", "x-token-auth ", "a\nb"] {
            assert!(RepoUser::parse(bad).is_err(), "{bad:?} must be refused");
        }
        for good in ["x-token-auth", "me@example.com", "x-access-token"] {
            assert!(RepoUser::parse(good).is_ok(), "{good:?} must be accepted");
        }
    }
}
