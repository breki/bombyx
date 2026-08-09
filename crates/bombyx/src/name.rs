//! Validation for user-supplied names that become
//! directories on the VM host.
//!
//! A scratch name arrives from the command line and, in
//! automation, often from a branch name or a PR title. It is
//! then interpolated into a path on the VM host. Quoting stops
//! it from being read as shell *syntax*, but quoting does
//! nothing about traversal: a correctly quoted
//! `'../../../../etc'` is still `/etc`.
//!
//! So the safe shape is enforced by parsing rather than left
//! to each call site: [`ScratchName`] cannot be constructed
//! from a value that is not a single, benign path segment.

use std::fmt;

use thiserror::Error;

/// Longest accepted scratch name.
pub const MAX_NAME_LEN: usize = 64;

/// Why a user-supplied name was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameError {
    /// The name was empty.
    #[error("must not be empty")]
    Empty,

    /// The name was longer than [`MAX_NAME_LEN`].
    #[error("must be at most {MAX_NAME_LEN} characters, got {0}")]
    TooLong(usize),

    /// The name did not start with a letter or digit.
    ///
    /// This is the check that rejects `.`, `..` and anything
    /// starting with `-`.
    #[error("must start with a letter or digit, got {0:?}")]
    BadStart(String),

    /// The name held a character outside the allowed set.
    ///
    /// This is the check that rejects `/` and `\`, so a name
    /// can never span more than one directory level.
    #[error(
        "must contain only letters, digits, `.`, `_` or `-`, \
         got {0:?}"
    )]
    BadChar(String),
}

/// Characters allowed after the first in a path segment.
fn is_segment_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// Returns `Ok` when `value` is safe to use as exactly one
/// path segment on the VM host.
///
/// Shared by [`ScratchName`] and the `project` config field,
/// which both end up as a single directory name on the host.
///
/// # Errors
///
/// Returns [`NameError`] describing the first rule broken.
pub fn check_segment(value: &str) -> Result<(), NameError> {
    let Some(first) = value.chars().next() else {
        return Err(NameError::Empty);
    };
    if !first.is_ascii_alphanumeric() {
        return Err(NameError::BadStart(value.to_owned()));
    }
    if !value.chars().all(is_segment_char) {
        return Err(NameError::BadChar(value.to_owned()));
    }
    Ok(())
}

/// A validated name for an ephemeral (`scratch`) VM.
///
/// Holding one is proof that the value is a single path
/// segment with no traversal and no shell metacharacters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchName(String);

impl ScratchName {
    /// Validates `raw` as a scratch VM name.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] when `raw` is empty, longer than
    /// [`MAX_NAME_LEN`], does not start with a letter or
    /// digit, or contains anything outside
    /// `[A-Za-z0-9._-]`.
    pub fn parse(raw: &str) -> Result<Self, NameError> {
        if raw.len() > MAX_NAME_LEN {
            return Err(NameError::TooLong(raw.len()));
        }
        check_segment(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// Returns the validated name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScratchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_typical_name() {
        let n = ScratchName::parse("pr-1234").unwrap();
        assert_eq!(n.as_str(), "pr-1234");
        assert_eq!(n.to_string(), "pr-1234");
    }

    #[test]
    fn accepts_dots_and_underscores() {
        assert!(ScratchName::parse("v1.2_beta").is_ok());
    }

    #[test]
    fn rejects_an_empty_name() {
        assert_eq!(ScratchName::parse(""), Err(NameError::Empty));
    }

    #[test]
    fn rejects_parent_directory_traversal() {
        // The attack this type exists for: `..` quoted is
        // still `..`, so it must never reach a path.
        let err = ScratchName::parse("..").unwrap_err();
        assert_eq!(err, NameError::BadStart("..".to_owned()));
    }

    #[test]
    fn rejects_a_deep_traversal() {
        let err = ScratchName::parse("../../../../etc").unwrap_err();
        assert_eq!(err, NameError::BadStart("../../../../etc".to_owned()));
    }

    #[test]
    fn rejects_a_nested_path() {
        let err = ScratchName::parse("a/b").unwrap_err();
        assert_eq!(err, NameError::BadChar("a/b".to_owned()));
    }

    #[test]
    fn rejects_a_windows_nested_path() {
        let err = ScratchName::parse(r"a\b").unwrap_err();
        assert_eq!(err, NameError::BadChar(r"a\b".to_owned()));
    }

    #[test]
    fn rejects_shell_metacharacters() {
        for raw in ["a;id", "a$(id)", "a`id`", "a|b", "a b"] {
            assert!(ScratchName::parse(raw).is_err(), "{raw} must be rejected");
        }
    }

    #[test]
    fn rejects_a_leading_dash() {
        let err = ScratchName::parse("-rf").unwrap_err();
        assert_eq!(err, NameError::BadStart("-rf".to_owned()));
    }

    #[test]
    fn rejects_a_leading_dot() {
        assert!(ScratchName::parse(".hidden").is_err());
    }

    #[test]
    fn rejects_an_overlong_name() {
        let raw = "a".repeat(MAX_NAME_LEN + 1);
        assert_eq!(
            ScratchName::parse(&raw),
            Err(NameError::TooLong(MAX_NAME_LEN + 1))
        );
    }

    #[test]
    fn accepts_a_name_at_the_length_limit() {
        let raw = "a".repeat(MAX_NAME_LEN);
        assert!(ScratchName::parse(&raw).is_ok());
    }

    #[test]
    fn check_segment_accepts_a_plain_segment() {
        assert!(check_segment("phren").is_ok());
    }

    #[test]
    fn error_messages_name_the_rule() {
        assert!(NameError::Empty.to_string().contains("empty"));
        assert!(NameError::TooLong(99).to_string().contains("64"));
    }
}
