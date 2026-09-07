//! The `[env]` table: variables a project hands to its own
//! provisioning script.
//!
//! Two newtypes, one for a name and one for a value, because
//! the two carry different rules. A name becomes a shell
//! variable in the guest, so it has to be spellable as one. A
//! value is written into the generated Vagrantfile as a Ruby
//! string, which is the rule `box`, `repo`, `ref`, `script` and
//! `deploy_key` already carry.
//!
//! [`super::RepoUrl`] explains the newtype pattern in full;
//! read that one first.

use serde::Deserialize;

use super::error::FieldError;
use super::guards;
use crate::newtype::checked_str_newtype;

/// Field name the errors here quote back to the operator.
///
/// One name for both the key and the value, because
/// [`FieldError`] holds a `&'static str` and a map key is not
/// one. What names the offending entry is the TOML parser,
/// which reports the line the bad key or value sits on.
const FIELD: &str = "env";

/// Prefix bombyx keeps for the variables it sets itself.
///
/// The generated Vagrantfile writes bombyx's own variables and
/// the project's into one Ruby hash literal, and a repeated key
/// in such a literal takes its last value. So a project writing
/// `BOMBYX_SCRIPT` would decide which script bombyx runs.
const RESERVED_PREFIX: &str = "BOMBYX_";

/// The name of a variable the guest's shell will carry.
///
/// This is a *newtype*: a struct wrapping one `String`, where
/// the `String` inside is private, so holding one is the proof
/// that [`EnvName::parse`] accepted it.
///
/// `#[serde(try_from = "String")]` is what makes the check run
/// while the table is parsing. Without it serde assigns the
/// private field directly and the constructor never runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct EnvName(String);

impl EnvName {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it is not spellable as a
    /// shell variable or when it starts with `BOMBYX_`.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_name(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(EnvName, "The name, as the guest's shell sees it.");

impl TryFrom<String> for EnvName {
    type Error = FieldError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_name(&raw)?;
        Ok(Self(raw))
    }
}

/// The value of a variable the guest's shell will carry.
///
/// A newtype for the same reason [`EnvName`] is one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct EnvValue(String);

impl EnvValue {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace or would break the generated Vagrantfile.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_value(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(
    EnvValue,
    "The value, as the Vagrantfile and the guest see it."
);

impl TryFrom<String> for EnvValue {
    type Error = FieldError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_value(&raw)?;
        Ok(Self(raw))
    }
}

/// Accepts a character that may appear after the first one in
/// a shell variable name.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Every rule an `[env]` name has.
///
/// The guest exports the name as a shell variable, and a shell
/// variable is a letter or an underscore followed by letters,
/// digits and underscores. Anything else cannot be assigned:
/// `9LIVES=1` is a syntax error rather than an odd variable,
/// and `WITH-DASH=1` is read as a command to run.
fn check_name(value: &str) -> Result<(), FieldError> {
    guards::check_not_empty(FIELD, value)?;
    if value.starts_with(RESERVED_PREFIX) {
        return Err(FieldError::invalid(
            FIELD,
            format!(
                "`{value}` starts with `{RESERVED_PREFIX}`, \
                 which bombyx keeps for its own variables"
            ),
        ));
    }
    // A digit is the only first character `check_charset`
    // below would otherwise accept, so this is the whole of
    // the first-character rule. Every other bad opener is a
    // character the charset check refuses wherever it sits.
    if value.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(FieldError::invalid(
            FIELD,
            format!(
                "`{value}` must start with a letter or an \
                 underscore to be a shell variable"
            ),
        ));
    }
    guards::check_charset(FIELD, value, is_name_char, "letters, digits and `_`")
}

/// Every rule an `[env]` value has.
///
/// The same rule the other five rendered fields carry, and for
/// the same reason: the value is written into the generated
/// Vagrantfile inside a Ruby string.
fn check_value(value: &str) -> Result<(), FieldError> {
    guards::check_renderable(FIELD, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_names_a_project_actually_uses() {
        for name in [
            "NODE_MAJOR",
            "GIT_USER_NAME",
            "_leading_underscore",
            "lowercase",
            "MIXED_case_9",
            "A",
        ] {
            assert!(
                EnvName::parse(name).is_ok(),
                "should have accepted {name:?}"
            );
        }
    }

    #[test]
    fn refuses_a_name_no_shell_can_spell() {
        // The family, not only the case that prompted the
        // guard: empty, a digit first, and every separator a
        // config author might reach for.
        for name in [
            "",
            "9LIVES",
            "-DASH",
            "WITH-DASH",
            "WITH.DOT",
            "WITH SPACE",
            "WITH$DOLLAR",
            "WITH=EQUALS",
            "WITH\nNEWLINE",
            "WITH/SLASH",
        ] {
            assert!(
                EnvName::parse(name).is_err(),
                "should have refused {name:?}"
            );
        }
    }

    #[test]
    fn refuses_a_name_bombyx_sets_itself() {
        // Ruby's hash literal lets a later key win, so a
        // project writing one of these would choose which
        // script bombyx runs.
        for name in [
            "BOMBYX_SCRIPT",
            "BOMBYX_REPO",
            "BOMBYX_REF",
            "BOMBYX_DEPLOY_KEY",
            "BOMBYX_VM_HOST",
            "BOMBYX_ANYTHING_AT_ALL",
            "BOMBYX_",
        ] {
            assert!(
                EnvName::parse(name).is_err(),
                "should have refused {name:?}"
            );
        }
    }

    #[test]
    fn accepts_a_name_that_only_looks_reserved() {
        // The rule is the `BOMBYX_` prefix, so a name that
        // merely starts with the word is fine.
        for name in ["BOMBYX", "BOMBYXISH", "bombyx_thing"] {
            assert!(
                EnvName::parse(name).is_ok(),
                "should have accepted {name:?}"
            );
        }
    }

    #[test]
    fn accepts_the_values_a_project_actually_uses() {
        for value in [
            "22",
            "Igor Brejc (agent VM)",
            "igor.brejc@gmail.com",
            "Europe/Ljubljana",
        ] {
            assert!(
                EnvValue::parse(value).is_ok(),
                "should have accepted {value:?}"
            );
        }
    }

    #[test]
    fn refuses_a_value_that_would_break_the_vagrantfile() {
        for value in [
            "",
            " leading",
            "trailing ",
            "with \" quote",
            "with \\ backslash",
            "with #{1+1} interpolation",
            "with \n newline",
        ] {
            assert!(
                EnvValue::parse(value).is_err(),
                "should have refused {value:?}"
            );
        }
    }
}
