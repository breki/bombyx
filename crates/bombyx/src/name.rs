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
//! to each call site: neither [`ScratchName`] nor
//! [`ProjectName`] can be constructed from a value that is not
//! a single, benign path segment.
//!
//! [`check_segment`] holds every rule, and both types call it,
//! so the two share their rules by construction rather than by
//! being kept in step. What differs is where the value comes
//! from: a scratch name is typed on the command line, and a
//! project name is a table key in the operator's registry file.
//! They are separate types so a function taking one cannot be
//! handed the other. [`ProjectName`] carries the extra impls a
//! map key needs -- serde, ordering, and `Borrow<str>` for
//! lookups -- and [`ScratchName`] needs none of them.

use std::fmt;

use serde::Deserialize;
use thiserror::Error;

/// Longest accepted name.
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
/// Every rule for such a name is here, length included, and
/// four callers share it: [`ScratchName`], [`ProjectName`], the
/// `project` field of `super::config::Config`, and the registry
/// lookup, which checks the name it was asked for before
/// reporting that no table carries it. Each of those ends up as
/// a single directory name on the host, so a rule one of them
/// applied alone would let the same name through one path and
/// refuse it on another.
///
/// # Errors
///
/// Returns [`NameError`] describing the first rule broken.
pub fn check_segment(value: &str) -> Result<(), NameError> {
    if value.len() > MAX_NAME_LEN {
        return Err(NameError::TooLong(value.len()));
    }
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

/// A validated project name.
///
/// Holding one is proof that the value passed
/// [`check_segment`]: a single path segment, with no traversal,
/// no separator, no leading dash, not empty and not over
/// [`MAX_NAME_LEN`]. It is the key of a `[projects.<name>]`
/// table in the
/// operator's registry, and bombyx joins it onto `remote_root`
/// to build the directory it creates on the VM host with
/// `mkdir` and deletes with `rm -rf`.
///
/// A type rather than a checking function because the value
/// arrives as a map key. Nothing calls a checker on a key while
/// serde is building the map, so a checker would have to run
/// afterwards, in every place a registry is built -- and the
/// place somebody forgets is the one that matters.
///
/// `#[serde(try_from = "String")]` is what makes the check run
/// during parsing. Without it serde assigns the private field
/// directly and [`ProjectName::parse`] never runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct ProjectName(String);

impl ProjectName {
    /// Validates `raw` as a project name.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] when `raw` is empty, longer than
    /// [`MAX_NAME_LEN`], does not start with a letter or digit,
    /// or contains anything outside `[A-Za-z0-9._-]`.
    pub fn parse(raw: &str) -> Result<Self, NameError> {
        check_segment(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// Returns the validated name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProjectName {
    type Error = NameError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl std::borrow::Borrow<str> for ProjectName {
    /// Lets a `BTreeMap<ProjectName, _>` be looked up by `&str`.
    ///
    /// `BTreeMap::get` takes a `&Q` where the key type borrows
    /// as `Q`, so without this a lookup would have to build a
    /// whole `ProjectName` -- and building one runs the check,
    /// which would turn "no such project" into "invalid name"
    /// for anybody who typed one wrong.
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validated name for an ephemeral (`scratch`) VM.
///
/// Holding one is proof that the value passed
/// [`check_segment`]: a single path segment, with no traversal,
/// no separator, no leading dash, not empty and not over
/// [`MAX_NAME_LEN`].
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
    fn a_project_name_is_one_path_segment_or_nothing() {
        assert_eq!(
            ProjectName::parse("myproject").unwrap().as_str(),
            "myproject"
        );
        assert_eq!(
            ProjectName::parse("my.proj_1-x").unwrap().to_string(),
            "my.proj_1-x"
        );
        for bad in ["", ".", "..", "-x", "a/b", "a\\b", "a b"] {
            assert!(ProjectName::parse(bad).is_err(), "{bad:?} was accepted");
        }
        assert!(matches!(
            ProjectName::parse(&"a".repeat(MAX_NAME_LEN + 1)),
            Err(NameError::TooLong(_))
        ));
    }

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
