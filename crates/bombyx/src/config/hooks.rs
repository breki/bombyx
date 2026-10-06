//! The `[hooks]` table: scripts from the project's clone that
//! bombyx runs in the guest at a named moment.
//!
//! One moment exists: `secrets_refreshed`, whenever bombyx has written
//! `~/.bombyx-env` -- after the provisioning run of the first `up` and of
//! `provision`, and after `up` or `shell --refresh-secrets` rewrites the file
//! in a running guest. The project names the script that puts the secrets
//! where it keeps them, usually `.env` in the clone, and that script is its
//! one copy step. `crate::plan::refresh_after_provisioning` and
//! `crate::plan::refresh_secrets` say when each path runs it.
//!
//! The table is named for hooks in general, not for this one,
//! because issue #12 (`provision-lifecycle-hooks`) plans more of
//! them beside it.
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
/// [`super::ScriptPath`] gives: the guest resolves it, not the
/// machine bombyx runs on.
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

    /// Why a Windows guest cannot run this hook, or `None` when it
    /// can.
    ///
    /// A method rather than a rule in [`HookPath::parse`], because
    /// only a Windows guest has these rules and the path is read
    /// before the config says which guest it is for; the registry's
    /// `parse` asks once `[vm]`'s `guest` is known, as it asks
    /// [`super::GuestUser::windows_refusal`].
    #[must_use]
    pub(crate) fn windows_refusal(
        &self,
        user_len: usize,
        project_len: usize,
    ) -> Option<WindowsHookRefusal> {
        // `C:\Users\`, the two separators around the project, and
        // the names; the rest of Windows' limit is the hook's.
        let clone_folder = "C:\\Users\\".len() + user_len + 1 + project_len + 1;
        let limit = MAX_WINDOWS_HOOK_LEN
            .min(MAX_WINDOWS_PATH_LEN.saturating_sub(clone_folder));
        if !guards::is_powershell_file(&self.0) {
            Some(WindowsHookRefusal::NotPowerShell)
        } else if self.0.len() > limit {
            Some(WindowsHookRefusal::TooLong { limit })
        } else {
            None
        }
    }
}

/// The longest `secrets_refreshed` path a Windows guest takes, whatever
/// the names around it: the refresh call carries the path on the
/// guest's command line, and its length budget, which
/// `the_longest_windows_refresh_command_fits_the_guest_command_line`
/// checks, assumes this cap.
pub(crate) const MAX_WINDOWS_HOOK_LEN: usize = 200;

/// The longest path Windows opens unless long paths are switched on,
/// which the box leaves off: 260 characters, one of them the
/// terminating NUL. The hook's full path in the guest has to fit.
const MAX_WINDOWS_PATH_LEN: usize = 259;

/// Which Windows rule a [`HookPath`] breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsHookRefusal {
    /// Not a `.ps1` file, the only kind `powershell -File` runs.
    NotPowerShell,
    /// Longer than the room left: `MAX_WINDOWS_HOOK_LEN`, or less
    /// when the clone folder's names are long.
    TooLong {
        /// The most characters the hook may hold for this project.
        limit: usize,
    },
}

impl std::fmt::Display for WindowsHookRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPowerShell => f.write_str(
                "a Windows guest runs the hook with PowerShell, which needs \
                 a .ps1 file",
            ),
            Self::TooLong { limit } => write!(
                f,
                "the hook's path in the guest, \
                 C:\\Users\\<guest_user>\\<project>\\<hook>, must fit the 259 \
                 characters Windows allows by default, and the refresh \
                 carries it on a command line of limited length, so for \
                 this project a hook path holds {limit} characters at most"
            ),
        }
    }
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

    #[test]
    fn a_windows_hook_fits_the_path_left_after_the_clone_folder() {
        // The hook sits at C:\Users\<user>\<project>\<hook>, and
        // Windows refuses a path over 259 characters by default, so a
        // long account and project name leave the hook less room than
        // the 200 the command line allows.
        let refusal = |user: &str, project: &str, len: usize| {
            let hook = HookPath::parse(&format!("{}.ps1", "a".repeat(len - 4)))
                .expect("a hook path");
            hook.windows_refusal(user.len(), project.len())
        };
        // Short names: the command-line cap of 200 is the tighter one.
        assert_eq!(refusal("agent", "myproject", 200), None);
        assert!(refusal("agent", "myproject", 201).is_some());
        // The longest names: 259 - 9 ("C:\Users\") - 20 - 1 - 64 - 1.
        let (user, project) = ("u".repeat(20), "p".repeat(64));
        assert_eq!(refusal(&user, &project, 164), None);
        let too_long = refusal(&user, &project, 165).expect("165 is too long");
        assert!(too_long.to_string().contains("164"), "{too_long}");
        // A file that is not .ps1 is refused whatever its length.
        let sh = HookPath::parse("hook.sh").expect("a hook path");
        assert_eq!(
            sh.windows_refusal(5, 9),
            Some(WindowsHookRefusal::NotPowerShell)
        );
    }

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
