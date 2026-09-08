//! The `[env]` table: variables a project hands to its own
//! provisioning script.
//!
//! They reach bombyx's own `bootstrap.sh` as well, because
//! Vagrant puts the whole table in the provisioner's
//! environment. So two sets of names are refused: the ones
//! bombyx sets itself, and the ones that change what its script
//! does. `HOME` changes what the script does and is accepted
//! anyway, because honouring it moves the clone, which is a
//! decision rather than an accident.
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
pub(crate) const RESERVED_PREFIX: &str = "BOMBYX_";

/// Names that change what `bootstrap.sh` does, so a project may
/// not set them.
///
/// Not "names the script reads": most of these appear nowhere in
/// it. `LD_PRELOAD` and `LD_LIBRARY_PATH` are read by the
/// dynamic loader, and the `GIT_*` names by `git`. What they
/// share is that the script behaves differently when they are
/// set.
///
/// Vagrant renders the provisioner's `env:` as an assignment
/// prefix on the command it runs, so an `[env]` name is in
/// `bootstrap.sh`'s own environment and not only the project
/// script's. Measured against a real VM host: an `[env]` entry
/// setting `PATH` to a directory with no `bash` in it fails the
/// provision at the `#!/usr/bin/env bash` line.
///
/// Six of these were measured to disarm one of bombyx's own
/// guarantees, on bash 5.2.21 and git 2.43.0:
///
/// - `PATH` decides which `git`, `readlink`, `rm` and `chmod`
///   the script finds, and `readlink -f` is the whole of the
///   check that the project's script resolves inside the clone.
/// - `SHELLOPTS=noexec` makes `bash` parse the script and exit
///   0, so Vagrant reports a provision in which nothing was
///   cloned and the uploaded key was never tightened or
///   removed.
/// - `BASH_ENV` names a file the shell sources before the
///   script.
/// - `GIT_CONFIG_COUNT` is treated as `git -c`, which outranks
///   every config file -- so it beats the `core.sshCommand` the
///   script writes on the clone, which `GIT_CONFIG_GLOBAL`
///   cannot do.
/// - `GIT_DIR` and `GIT_WORK_TREE` outrank `git -C`, so every
///   `git -C "$CLONE_DIR"` here would act on a repository the
///   `[env]` table names while the clone itself was left alone.
///
/// The rest are refused in advance rather than on a
/// measurement. `IFS` and `ENV` were measured to be inert:
/// bash resets `IFS` to its default at startup, and `ENV` is
/// read only by an interactive shell or by `sh`, neither of
/// which this script is. They stay on the list because
/// refusing them costs nothing and one `sh` line here would
/// make both live. `BASHOPTS`, `LD_PRELOAD`,
/// `LD_LIBRARY_PATH`, `GIT_SSH_COMMAND`, `GIT_CONFIG_GLOBAL`
/// and `GIT_CONFIG_SYSTEM` are the same case: each is a name
/// this script's behaviour could turn on, and none is a name a
/// project needs bombyx to hand onward.
///
/// `HOME` is deliberately absent: `bootstrap.sh` derives the
/// clone directory from it, so writing it here moves the clone,
/// and that is a recorded decision rather than an accident.
/// `docs/architecture.md` under **Who runs the project's
/// script** holds the argument.
///
/// Keeping a list is a maintenance cost, and `docs/todo.md`
/// holds the alternative to it as `bootstrap-sets-own-path`.
const NAMES_THAT_CHANGE_WHAT_BOOTSTRAP_DOES: [&str; 14] = [
    "PATH",
    "IFS",
    "BASH_ENV",
    "ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "GIT_SSH_COMMAND",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_DIR",
    "GIT_WORK_TREE",
];

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
///
/// Two sets of names are then refused rather than spelled
/// wrongly: [`RESERVED_PREFIX`] for the ones bombyx sets, and
/// [`NAMES_THAT_CHANGE_WHAT_BOOTSTRAP_DOES`] for the ones its script reads.
fn check_name(value: &str) -> Result<(), FieldError> {
    guards::check_not_empty(FIELD, value)?;
    if NAMES_THAT_CHANGE_WHAT_BOOTSTRAP_DOES.contains(&value) {
        return Err(FieldError::invalid(
            FIELD,
            format!(
                "`{value}` changes what bombyx's own \
                 provisioning script does in the guest, rather \
                 than what your script does. Set it inside your \
                 own provisioning script instead."
            ),
        ));
    }
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
        // Every name bombyx sets itself. `RESERVED_PREFIX`
        // above holds why taking one over would matter.
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
    fn refuses_a_name_the_bootstrap_script_itself_reads() {
        // Vagrant renders `env:` as an assignment prefix on the
        // command it runs, so an `[env]` name is in
        // `bootstrap.sh`'s own environment and not only the
        // project script's. Measured against a real VM host:
        // `PATH = "/nonexistent-bombyx-pathtest"` fails the
        // provision at `#!/usr/bin/env bash`.
        //
        // The family is "a name that changes what bombyx's own
        // fixed script does", and each one below disarms a
        // guarantee rather than merely inconveniencing the
        // project: `SHELLOPTS=noexec` makes bash parse the
        // script and exit 0, so Vagrant reports a provision in
        // which nothing was cloned and the uploaded key was
        // never tightened or removed.
        for name in [
            "PATH",
            "IFS",
            "BASH_ENV",
            "ENV",
            "SHELLOPTS",
            "BASHOPTS",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "GIT_SSH_COMMAND",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            // The three that actually beat what the script
            // writes, measured on git 2.43.0.
            // `GIT_CONFIG_GLOBAL` above cannot override the
            // `core.sshCommand` the clone carries, because a
            // repository's own config wins over a global one.
            // These do:
            //
            //   GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=... \
            //     GIT_CONFIG_VALUE_0=...
            //
            // is treated as `git -c`, which outranks every
            // config file, and `GIT_DIR` outranks `git -C`, so
            // every `git -C "$CLONE_DIR"` in the script would
            // act on a repository the `[env]` table names while
            // the clone itself was left alone.
            //
            // Refusing `GIT_CONFIG_COUNT` disarms the whole
            // mechanism, because git ignores `GIT_CONFIG_KEY_n`
            // and `GIT_CONFIG_VALUE_n` without the count. So
            // this stays a list of names and needs no prefix
            // rule.
            "GIT_CONFIG_COUNT",
            "GIT_DIR",
            "GIT_WORK_TREE",
        ] {
            assert!(
                EnvName::parse(name).is_err(),
                "should have refused {name:?}"
            );
        }
    }

    #[test]
    fn accepts_home_because_moving_the_clone_is_a_decision() {
        // `HOME` changes what the script does and is accepted
        // anyway, which is the one exception to the list above.
        // `bootstrap.sh` derives the clone directory from
        // `$HOME`, so writing it here moves the clone -- the
        // operator's own line in their own config, and the
        // script checks the value before using it.
        // `docs/architecture.md` under **Who runs the project's
        // script** holds the argument.
        assert!(EnvName::parse("HOME").is_ok());
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

    // No table of bad values here. `EnvValue` shares
    // `check_renderable` with `box`, `repo`, `ref`, `script` and
    // `deploy_key`, and `guards::tests::renderable_newtypes`
    // exercises that rule set against every type it has a row
    // for, this one included. A copy here would drift the first
    // time a rule is added there.
}
