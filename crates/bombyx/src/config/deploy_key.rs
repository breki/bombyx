//! What `deploy_key` may be: the path, on the VM host, of the
//! private key the guest clones a private repository with.
//!
//! The field is optional. A public repository needs no
//! credential, so most `[source]` tables leave it out.
//!
//! bombyx never opens the file. The path is written into the
//! generated Vagrantfile, and `vagrant` -- running on the VM
//! host -- expands it and uploads the file into the guest. So
//! the key stays on the VM host until the guest exists, and the
//! workstation never holds it.
//!
//! What refuses a path the VM host does not have is
//! `crate::remote::require_file`, before bombyx writes the
//! Vagrantfile at all. The upload in the Vagrantfile is
//! conditional and carries no `raise` of its own.
//! `crate::vagrantfile` records why, and `docs/architecture.md`
//! under **What config values are checked** holds the argument.
//!
//! Every rule is enforced in one function, the private `check`
//! below, and [`DeployKeyPath`] is the only thing that calls
//! it.

use serde::Deserialize;

use super::error::FieldError;
use super::guards;
use super::path_segments;
use crate::newtype::checked_str_newtype;

/// A private key file on the VM host, ready to be written into
/// the generated Vagrantfile.
///
/// This is a *newtype*: a struct wrapping one `String`, where
/// the `String` inside is private. You cannot build one
/// directly. You have to call [`DeployKeyPath::parse`], which
/// runs the private `check` first. So holding one is the proof
/// that every rule in this module ran, and the compiler is what
/// promises that.
///
/// A `PathBuf` would be the wrong representation, for the same
/// reason `super::ScriptPath` is not one: this path is resolved
/// on the VM host, and `PathBuf` answers for the machine bombyx
/// was compiled for. A Windows workstation driving a Linux VM
/// host would have `PathBuf` reading `~/.secrets/k` with
/// Windows' separator rules.
///
/// `#[serde(try_from = "String")]` is what connects the type to
/// the config file. Without it serde would assign the private
/// field directly and skip every rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct DeployKeyPath(String);

impl DeployKeyPath {
    /// The config key this type reads, and the name every one
    /// of its errors reports against.
    ///
    /// Public because it is also what tells an operator which
    /// line to edit, and `crate::plan` needs it for the
    /// message `crate::remote::require_file` prints. Written
    /// on the type rather than as a bare literal in each
    /// place, so renaming the TOML key cannot leave one caller
    /// naming a key the config no longer has.
    pub const FIELD: &'static str = "deploy_key";

    /// Checks `raw` against every rule here and wraps it.
    ///
    /// Takes `&str` so a caller holding a borrowed value need
    /// not copy it. Serde arrives owning a `String` and hands
    /// that to [`DeployKeyPath::try_from`] instead, which runs
    /// the same private `check`.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] naming `deploy_key` when it
    /// breaks any other rule `check` holds.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(
    DeployKeyPath,
    "The value, as the generated Vagrantfile and the VM host see it."
);

impl TryFrom<String> for DeployKeyPath {
    type Error = FieldError;

    /// What serde calls. It already owns the `String`, so the
    /// rules run against a borrow of it rather than a copy.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check(&raw)?;
        Ok(Self(raw))
    }
}

/// Checks a `deploy_key` value against every rule here.
///
/// # Errors
///
/// Returns [`FieldError::Empty`] when the value is blank, and
/// [`FieldError::Invalid`] naming `deploy_key` when the value
/// would break the generated Vagrantfile, holds a character
/// outside the allowed set, does not start with `/` or `~/`,
/// names no file below that anchor, contains `//` or a `.` or
/// `..` segment, ends in `/`, or spells `~` anywhere but
/// first.
fn check(value: &str) -> Result<(), FieldError> {
    // The path is written into the generated Vagrantfile
    // inside a double-quoted Ruby string, so it carries the
    // same rule the four other rendered fields do.
    //
    // Both calls are needed because the charset rule says
    // nothing about a blank value: `check_charset` looks for a
    // character it disallows, and an empty string has none to
    // find. `check_renderable` is what refuses one.
    guards::check_renderable(DeployKeyPath::FIELD, value)?;
    guards::check_charset(
        DeployKeyPath::FIELD,
        value,
        guards::is_remote_path_char,
        "letters, digits, `.`, `_`, `-`, `/` or `~`",
    )?;

    // The path must be anchored. `vagrant` runs in the
    // project's directory on the VM host, so a relative value
    // would send `File.expand_path` looking under
    // `<remote_root>/<project>` -- a directory bombyx creates,
    // writes and deletes, and no place to keep a key.
    //
    // A bare `~` is let through here and refused by the rule
    // below, which names the real problem: it is a directory,
    // not a key file.
    let anchored =
        value.starts_with('/') || value == "~" || value.starts_with("~/");
    if !anchored {
        return Err(invalid(
            "must start with `/` or `~/`; a relative path resolves \
             against the directory vagrant runs in on the VM host",
        ));
    }

    let segments = path_segments(value);
    if segments.is_empty() {
        return Err(invalid(
            "must name a file below `/` or `~`; the value as written \
             is a directory",
        ));
    }

    // `path_segments` filters empty segments away, so without
    // this rule `~//k` counts as depth one and reaches the VM
    // host as `~/'/keys/k'` -- `quote_remote_path` keeps the
    // `~/` outside the quotes. A path starting with exactly two
    // slashes is implementation-defined in POSIX and need not
    // name the same file as one slash, which is why this is a
    // refusal rather than something to tidy up.
    if value.contains("//") {
        return Err(invalid(
            "must not contain `//`; the path is expanded on the VM \
             host, where you cannot see what it resolved to",
        ));
    }

    // Reported separately from the rule above, because the
    // value does name a file and only its spelling says
    // otherwise.
    if value.ends_with('/') {
        return Err(invalid(
            "must not end with `/`; a private key is a file, not a \
             directory",
        ));
    }

    // This rule is not a containment guarantee, and it does
    // not need to be. `vagrant` copies this file's contents
    // into the guest rather than deleting anything, so a
    // config naming the wrong path leaks a file instead of
    // destroying one -- and no rule here restricts which file
    // it names. `docs/architecture.md` under **What config
    // values are checked** holds that.
    //
    // What the rule buys is that the path says plainly which
    // directory it reads, because the VM host expands it and
    // the operator never sees the result.
    if let Some(bad) = segments.iter().find(|s| **s == "." || **s == "..") {
        return Err(invalid(format!(
            "must not contain a `{bad}` segment; the path is expanded \
             on the VM host, where you cannot see what it resolved to"
        )));
    }

    // Both a remote shell and Ruby's `File.expand_path` expand
    // `~` only in leading position. Anywhere else it is a
    // literal character in a file name.
    if value.char_indices().any(|(i, c)| c == '~' && i > 0) {
        return Err(invalid("`~` is only allowed as the first character"));
    }

    Ok(())
}

/// Builds this module's one error shape, which always names the
/// same field.
fn invalid(reason: impl Into<String>) -> FieldError {
    FieldError::Invalid {
        field: DeployKeyPath::FIELD,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value every rule here accepts, so a test needing a
    /// good path does not invent its own.
    const GOOD: &str = "~/.secrets/myproject-deploy-key";

    /// Asserts `bad` is refused, with a message mentioning
    /// `reason`.
    ///
    /// Pinning the reason rather than only the failure is what
    /// makes these notice a deleted rule: a value refused by
    /// some other check would still fail `is_err()`.
    fn refused(bad: &str, reason: &str) {
        let err = check(bad).expect_err("must be refused").to_string();
        assert!(err.contains(reason), "{bad:?}: want {reason:?}, got {err}");
    }

    #[test]
    fn a_key_path_under_either_anchor_is_accepted() {
        for good in [
            GOOD,
            "/etc/bombyx/deploy-key",
            "~/deploy-key",
            "/srv/keys/a_b-c.pem",
        ] {
            assert!(check(good).is_ok(), "{good:?} must be accepted");
        }
    }

    #[test]
    fn the_stored_value_is_the_value_given() {
        // No normalizing here, unlike `super::root::RemoteRoot`,
        // which drops a trailing slash. Nothing joins anything
        // onto this path, so there is no second spelling to
        // reconcile -- and a trailing slash is refused outright
        // rather than repaired.
        let key = DeployKeyPath::parse(GOOD).expect("a valid fixture path");
        assert_eq!(key.as_str(), GOOD);
    }

    #[test]
    fn a_blank_value_is_refused() {
        for bad in ["", "   "] {
            refused(bad, "must not be empty");
            // The field name is the only part of the message
            // telling an operator which key to edit, and it
            // travels through a guard and two error types
            // before it is printed.
            refused(bad, DeployKeyPath::FIELD);
        }
    }

    #[test]
    fn surrounding_whitespace_is_refused_as_whitespace() {
        // A copy-paste artifact. Reported as whitespace rather
        // than as a disallowed character, because "character
        // ' ' is not allowed" sends an operator looking for a
        // space in the middle of the path.
        for bad in [format!(" {GOOD}"), format!("{GOOD} ")] {
            refused(&bad, "whitespace");
        }
    }

    #[test]
    fn characters_that_would_break_the_generated_ruby_are_refused() {
        // The path is written into the Vagrantfile inside a
        // double-quoted Ruby string. A quote ends it early and
        // a backslash escapes what follows.
        for bad in [format!("{GOOD}a\"b"), format!("{GOOD}a\\b")] {
            refused(&bad, "would end or escape");
        }
        refused(&format!("{GOOD}#{{x}}"), "Ruby interpolation");
    }

    #[test]
    fn a_control_character_is_reported_as_one() {
        refused(&format!("{GOOD}a\u{7}b"), "control character");
    }

    #[test]
    fn a_character_outside_the_path_set_is_refused() {
        // Narrower than the Ruby rule above: a space or a `$`
        // breaks no Ruby string, and neither belongs in a key
        // path the operator wrote.
        for bad in ["~/my keys/k", "~/keys/$k", "~/keys/k;rm"] {
            refused(bad, "is not allowed");
        }
    }

    #[test]
    fn an_unanchored_path_is_refused() {
        // Relative to what? `vagrant` runs in the project's
        // directory on the VM host, so a relative path would
        // look for the key under `~/vms/<project>` -- which is
        // a directory bombyx creates, writes and deletes.
        for bad in ["deploy-key", ".secrets/k", "keys/k"] {
            refused(bad, "must start with `/` or `~/`");
        }
    }

    #[test]
    fn an_anchor_with_no_file_below_it_is_refused() {
        // `~` and `/` are directories. Naming one as the key
        // file would reach the VM host as a path `File.exist?`
        // answers yes for, and `ssh` would then refuse a
        // directory as an identity file inside the guest,
        // which is a long way from the config line at fault.
        for bad in ["/", "~", "~/"] {
            refused(bad, "must name a file below");
        }
    }

    #[test]
    fn a_trailing_slash_is_refused() {
        // It says the value is a directory, and a private key
        // is a file.
        refused(&format!("{GOOD}/"), "must not end with `/`");
    }

    #[test]
    fn a_doubled_slash_is_refused() {
        // `path_segments` filters empty segments away, so
        // without this rule `~//k` counts as depth one and
        // passes every other check. It also survives into the
        // script bombyx sends: `quote_remote_path` keeps the
        // `~/` outside the quotes, so `~//keys/k` is emitted
        // as `~/'/keys/k'`.
        //
        // A leading `//` is the case worth refusing rather than
        // tidying: POSIX leaves a path beginning with exactly
        // two slashes implementation-defined, so it need not
        // resolve to the same file as one slash.
        for bad in ["~//k", "/srv//keys/k", "//etc/keys/k"] {
            refused(bad, "must not contain `//`");
        }
    }

    #[test]
    fn a_dot_or_dot_dot_segment_is_refused() {
        // The rule is that the path says plainly which
        // directory it reads, because it is expanded on
        // another machine and the operator never sees the
        // result. `check` says what the field really does with
        // the file, which is not read it.
        for bad in ["~/./k", "~/.secrets/../k", "/srv/keys/.."] {
            refused(bad, "segment");
        }
    }

    #[test]
    fn a_tilde_past_the_first_character_is_refused() {
        // A remote shell and Ruby's `File.expand_path` both
        // expand `~` only in leading position. Anywhere else
        // it is a literal character in a name, and almost
        // certainly a mistake.
        refused("~/keys/~k", "`~` is only allowed as the first");
    }

    #[test]
    fn a_leading_dash_cannot_arise() {
        // `super::guards::check_not_an_option` is deliberately
        // not called here. The value reaches a Ruby literal
        // and, through `crate::remote::require_file`, a shell
        // assignment that `quote_remote_path` quotes. Neither
        // is an argv position, and the anchoring rule already
        // refuses every value that could read as an option.
        refused("-x", "must start with `/` or `~/`");
    }

    #[test]
    fn serde_runs_the_same_rules() {
        // The `try_from` attribute is what makes "holding one
        // means it passed" true for a value out of TOML.
        // Without it serde assigns the private field directly.
        let ok: Result<DeployKeyPath, _> =
            DeployKeyPath::try_from(GOOD.to_owned());
        assert_eq!(ok.expect("a valid fixture path").as_str(), GOOD);

        let err = DeployKeyPath::try_from("keys/k".to_owned())
            .expect_err("must be refused");
        assert!(err.to_string().contains("must start with"), "{err}");
    }
}
