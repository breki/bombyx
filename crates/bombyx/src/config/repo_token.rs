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
use crate::newtype::{checked_str_newtype, checked_str_try_from};

/// The name of the variable inside `env_file` holding the git
/// token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RepoTokenVar(String);

impl RepoTokenVar {
    /// The field name, for a message naming it.
    pub const FIELD: &'static str = "repo_token";

    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it is not a variable name.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_token_var(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

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

    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it carries surrounding
    /// whitespace or a control character.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_user(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(RepoUser, "The username, as written.");

checked_str_try_from!(
    /// What serde calls while the config parses.
    RepoUser,
    FieldError,
    check_user
);

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
/// needs no escaping. An `export ` in front of the name is
/// accepted, because a file people also `source` usually has
/// one. Whitespace around the name and around the value is
/// dropped. One matching pair of surrounding quotes is dropped
/// too -- and only one, so a value that is meant to keep its
/// inner quotes does.
///
/// The last assignment wins, which is what a shell reading the
/// same file ends up with.
///
/// Bytes rather than text throughout. `super::Secrets` accepts
/// a file that is not UTF-8, and a token that came back as a
/// replacement character would fail authentication with nothing
/// pointing at the encoding.
pub(crate) fn lookup(secrets: &[u8], var: &str) -> Option<Vec<u8>> {
    let mut found = None;
    for line in secrets.split(|b| *b == b'\n') {
        let line = trim(line);
        let line = match strip_prefix_bytes(line, b"export ") {
            Some(rest) => trim(rest),
            None => line,
        };
        let Some(eq) = line.iter().position(|b| *b == b'=') else {
            continue;
        };
        if trim(&line[..eq]) != var.as_bytes() {
            continue;
        }
        found = Some(unquote(trim(&line[eq + 1..])).to_vec());
    }
    found
}

/// Drops `prefix` from the front of `line`, or answers `None`.
fn strip_prefix_bytes<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    line.starts_with(prefix).then(|| &line[prefix.len()..])
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

/// Drops one matching pair of surrounding quotes.
fn unquote(value: &[u8]) -> &[u8] {
    let (Some(first), Some(last)) = (value.first(), value.last()) else {
        return value;
    };
    if value.len() >= 2 && first == last && (*first == b'"' || *first == b'\'')
    {
        return &value[1..value.len() - 1];
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
    user: &RepoUser,
    secrets: &[u8],
    var: &RepoTokenVar,
    path: &str,
) -> Result<GitCredential, RepoTokenError> {
    let token = lookup(secrets, var.as_str()).ok_or_else(|| {
        RepoTokenError::NotInFile {
            var: var.as_str().to_owned(),
            path: path.to_owned(),
        }
    })?;
    if token.is_empty() {
        return Err(RepoTokenError::EmptyValue {
            var: var.as_str().to_owned(),
            path: path.to_owned(),
        });
    }
    // The trailing newline is part of the format: `git`'s
    // `store` helper reads the file a line at a time.
    let line = format!(
        "https://{user}:{token}@{host}\n",
        user = percent_encode(user.as_str().as_bytes()),
        token = percent_encode(&token),
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
            "must not start with a digit, which no shell would              accept as a variable name",
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
                "control character {bad:?} is not allowed; use                  printable characters only"
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

    fn user() -> RepoUser {
        RepoUser::parse("x-token-auth").expect("a plain username")
    }

    fn var() -> RepoTokenVar {
        RepoTokenVar::parse("BITBUCKET_TOKEN").expect("a plain name")
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
            &user(),
            secrets("BITBUCKET_TOKEN=abc\n"),
            &var(),
            "~/secrets/x.env",
        )
        .expect("the variable is in the file");
        assert_eq!(line(&cred), "https://x-token-auth:abc@bitbucket.org\n");
    }

    #[test]
    fn a_token_with_url_characters_is_encoded_into_the_line() {
        let cred = credential(
            "bitbucket.org",
            &user(),
            secrets("BITBUCKET_TOKEN=a/b@c\n"),
            &var(),
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
            &user(),
            secrets("OTHER=abc\n"),
            &var(),
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
            &user(),
            secrets("BITBUCKET_TOKEN=\n"),
            &var(),
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
