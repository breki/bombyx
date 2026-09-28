//! The `[hooks]` table: scripts from the project's clone that
//! bombyx runs in the guest at a named moment.
//!
//! One moment exists: `secrets_refreshed`, whenever bombyx has
//! written `~/.bombyx-env` -- after the provisioning run of the
//! first `up` and of `provision`, and after `up` or `shell`
//! rewrites the file in a running guest. The project names the
//! script that puts the secrets where it keeps them, usually `.env`
//! in the clone, and that script is its one copy step.
//! `crate::plan::refresh_after_provisioning` and
//! `crate::plan::refresh_secrets` say when each path runs it.
//!
//! The table is named for hooks in general, not for this one,
//! because `provision-lifecycle-hooks` in `docs/todo.md` (#12)
//! plans more of them beside it.
//!
//! **The operator's config names the script, and the clone holds
//! it.** The value is a path relative to the clone, like `script`,
//! so the operator decides whether anything runs and a dry run can
//! print it, while the project decides what the script does.
//! `crate::remote::refresh_secrets_then_hook` builds the command
//! and holds how the guest checks the path and runs the script.

use serde::Deserialize;

use super::error::FieldError;
use super::guards;
use crate::newtype::{
    checked_str_newtype, checked_str_parse, checked_str_try_from,
};

/// The `[hooks]` table of one project.
///
/// Every key is optional, and a project with no table gets
/// [`Hooks::default`], which runs nothing.
///
/// A hook the table names reaches the guest only through the
/// secrets refresh, so a [`super::Config`] built in code with a
/// `secrets_refreshed` hook and no `env_file` runs nothing: there
/// is no refresh for the hook to follow. The registry refuses that
/// pairing while the file is read, so the operator hears about it
/// rather than finding the hook silently idle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    /// The script to run whenever bombyx has written the project's
    /// secrets into the guest.
    #[serde(default)]
    pub secrets_refreshed: Option<HookPath>,
}

/// A path to a hook script, relative to the clone root, on the
/// guest.
///
/// A checked string rather than a `PathBuf` for the reason
/// [`super::ScriptPath`] gives: the guest resolves it, and the
/// guest is always Linux.
///
/// **Its own type rather than a [`super::ScriptPath`]**, although
/// the containment rule is the same one (`guards::check_inside_clone`).
/// A refusal names the key it came from, and a `ScriptPath` names
/// `script`. The character rules differ too: `script` is rendered
/// into the Vagrantfile and carries that file's rules, while this
/// value travels as a quoted shell argument, so it takes a short
/// allowlist whose refusal explains itself.
///
/// It names `secrets_refreshed` in every message, because that is
/// the only key holding one. A second hook key needs either its
/// own type or a message that stops naming the key.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct HookPath(String);

impl HookPath {
    /// The key a refusal names.
    pub const FIELD: &'static str = "secrets_refreshed";
}

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it holds a character outside
    /// letters, digits, `.`, `_`, `-` and `/`, starts with `-`,
    /// is absolute, holds a `..` segment, or cannot name a file.
    HookPath,
    FieldError,
    check_hook
);

checked_str_newtype!(HookPath, "The value, as the guest's shell sees it.");

checked_str_try_from!(
    /// What serde calls; see [`super::RepoUrl::try_from`].
    HookPath,
    FieldError,
    check_hook
);

/// Characters a hook path may hold.
///
/// Narrower than a path needs to be, on purpose. The value travels
/// through three layers of shell quoting on its way to the guest,
/// and a space, a quote or a control character in a script name is
/// far more likely a mistake than a need.
fn is_hook_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')
}

/// Every rule a [`HookPath`] has.
fn check_hook(value: &str) -> Result<(), FieldError> {
    let field = HookPath::FIELD;
    guards::check_not_empty(field, value)?;
    guards::check_charset(
        field,
        value,
        is_hook_path_char,
        "letters, digits, `.`, `_`, `-` and `/`",
    )?;
    // Defence in depth rather than a live risk: the guest joins the
    // value onto the clone's path before any command sees it, so no
    // command receives a leading `-`. The rule stays for the day a
    // command is handed the value as it stands, as it does for
    // `ref`, whose `git fetch` also puts it after `--`.
    guards::check_not_an_option(field, value, "a command given it unjoined")?;
    guards::check_inside_clone(field, value)?;
    guards::check_names_a_file(field, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `toml` as a `[hooks]` table.
    fn hooks(toml: &str) -> Result<Hooks, toml::de::Error> {
        toml::from_str(toml)
    }

    #[test]
    fn a_path_inside_the_clone_is_accepted() {
        for good in [
            ".bombyx/refresh-env.sh",
            "refresh.sh",
            "scripts/a_b/c-d.1.sh",
            "./tools/refresh",
        ] {
            let path = HookPath::parse(good).expect(good);
            assert_eq!(path.as_str(), good);
        }
    }

    #[test]
    fn every_path_that_could_leave_the_clone_is_refused() {
        // The whole family the guard claims, not only the case
        // that prompted it. `readlink -f` on the guest still runs
        // afterwards, for a symlink these cannot see.
        for (bad, reason) in [
            ("/etc/passwd", "relative to the clone root"),
            ("/", "relative to the clone root"),
            ("..", "`..` segment"),
            ("../x.sh", "`..` segment"),
            ("a/../../x.sh", "`..` segment"),
            ("a/..", "`..` segment"),
        ] {
            let err = HookPath::parse(bad).expect_err(bad).to_string();
            assert!(err.contains(reason), "{bad:?}: {err}");
            assert!(err.contains("secrets_refreshed"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn a_path_that_cannot_name_a_file_is_refused() {
        // The rest of the family: the clone itself and a directory.
        // The guest would refuse each on every run, and for `.` with
        // the false reason "leads outside the clone". A doubled `/`
        // still names a file, so it is accepted.
        for bad in [".", "./", "a/.", "a/", "a/./"] {
            let err = HookPath::parse(bad).expect_err(bad).to_string();
            assert!(err.contains("must name a file"), "{bad:?}: {err}");
        }
        HookPath::parse("a//b.sh").expect("a doubled slash names a file");
    }

    #[test]
    fn a_blank_or_option_like_or_oddly_spelt_path_is_refused() {
        for (bad, reason) in [
            ("", "must not be empty"),
            ("   ", "must not be empty"),
            ("-x.sh", "must not start with `-`"),
            ("a b.sh", "is not allowed"),
            (" x.sh", "is not allowed"),
            ("x.sh\n", "is not allowed"),
            ("x'.sh", "is not allowed"),
            ("x\".sh", "is not allowed"),
            ("$(id).sh", "is not allowed"),
            ("~/x.sh", "is not allowed"),
            ("a\\b.sh", "is not allowed"),
        ] {
            let err = HookPath::parse(bad).expect_err(bad).to_string();
            assert!(err.contains(reason), "{bad:?}: {err}");
        }
    }

    #[test]
    fn serde_runs_the_same_rules() {
        // Without `try_from`, serde would assign the private field
        // and skip every check above.
        let err = hooks("secrets_refreshed = \"../x.sh\"\n")
            .expect_err("a `..` path must be refused while parsing")
            .to_string();
        assert!(err.contains("`..` segment"), "{err}");
    }

    #[test]
    fn an_empty_table_runs_nothing_and_an_unknown_key_is_refused() {
        assert_eq!(hooks("").expect("an empty table"), Hooks::default());
        let err = hooks("secret_refreshed = \"x.sh\"\n")
            .expect_err("a misspelt key must be refused")
            .to_string();
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn the_hook_key_reads_into_the_table() {
        let got = hooks("secrets_refreshed = \".bombyx/refresh-env.sh\"\n")
            .expect("a good hook");
        assert_eq!(
            got.secrets_refreshed.as_ref().map(HookPath::as_str),
            Some(".bombyx/refresh-env.sh")
        );
    }
}
