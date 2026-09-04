//! What `remote_root` may be, and why the rules are strict.
//!
//! `remote_root` names a directory on the VM host. bombyx joins
//! the project name onto it and *deletes* the result on
//! teardown, so a mistake here is a mistake with `rm -rf` behind
//! it. `bombyx.toml` travels inside a repo, which makes this
//! attacker-controlled input rather than a typo risk.
//!
//! Every rule is enforced in one function, called from
//! `Config::validate` for a `bombyx.toml` and from
//! `Project::validate` for a registry entry, so a `remote_root`
//! from either file gets the same rules and every command
//! agrees on which roots are usable. Gating only the removal
//! would leave `up` free to write into `/etc` while teardown
//! refused to touch it.

use super::error::FieldError;
use super::guards;

/// Fewest real segments `remote_root` must contain.
///
/// The value is one. `remote_root` must name a directory
/// *below* `/` or `~`, so the directory bombyx creates and
/// deletes -- `<remote_root>/<project>` -- is always at least
/// two deep.
///
/// That is the floor below which a configuration mistake stops
/// being recoverable. With a root of `~` or `/` accepted, the
/// directory bombyx deletes would be a top-level or home-adjacent
/// one.
const MIN_ROOT_SEGMENTS: usize = 1;

/// The meaningful segments of a remote path.
///
/// Drops the leading `~` root marker and any empty segment left
/// by a doubled or trailing slash, so counting the result
/// measures real depth rather than characters. A `.` segment is
/// deliberately *kept*: [`check`] rejects it, and
/// filtering it here would let `~/.` pass as depth one.
pub(crate) fn path_segments(path: &str) -> Vec<&str> {
    path.strip_prefix('~')
        .unwrap_or(path)
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// Builds this module's one error shape, which always names
/// the same field.
fn invalid(reason: impl Into<String>) -> FieldError {
    FieldError::Invalid {
        field: "remote_root",
        reason: reason.into(),
    }
}

/// Checks a `remote_root` value against every rule here.
///
/// # Errors
///
/// Returns [`FieldError::Empty`] when the value is blank, and
/// [`FieldError::Invalid`] naming `remote_root` when the value
/// starts with `-` and so would read as an option to `ssh`,
/// holds a character outside the allowed set, does not start
/// with `~` or `/`, contains a `.` or `..` segment, is not deep
/// enough, or spells `~` anywhere but first.
pub(super) fn check(value: &str) -> Result<(), FieldError> {
    // The blank and leading-dash rules are here rather than at
    // the call site, so this module really does own every rule
    // the field has and a second caller cannot get half of them.
    guards::check_not_empty("remote_root", value)?;
    guards::check_not_an_option("remote_root", value, "ssh")?;
    guards::check_charset(
        "remote_root",
        value,
        guards::is_remote_path_char,
        "letters, digits, `.`, `_`, `-`, `/` or `~`",
    )?;

    // The root must be anchored. An unrooted value resolves
    // against whatever directory the SSH login lands in, which
    // makes the depth rule below meaningless -- bombyx cannot
    // count the segments of a path whose start it does not know.
    //
    // A `~` has to be followed by `/`, or be the whole value.
    // `~name` is the shell's way of writing that user's home
    // directory, and `quote_remote_path` does not expand it:
    // only `~` and `~/` are left outside the quotes, so `~vms`
    // is emitted as a fully quoted string and the remote shell
    // reads it as an ordinary relative name. Accepting `~vms`
    // here would pass it through the anchoring rule and then
    // resolve it against the login directory anyway, which is
    // the outcome this rule exists to stop.
    let anchored =
        value.starts_with('/') || value == "~" || value.starts_with("~/");
    if !anchored {
        return Err(invalid(
            "must start with `/` or `~/`; a relative path resolves \
             against the login directory",
        ));
    }

    // `..` escapes the root outright. `.` is subtler: it adds a
    // segment without adding depth, so `remote_root = "/."` with
    // `project = "etc"` counts as two segments deep while
    // resolving to `/etc`.
    let segments = path_segments(value);
    if let Some(bad) = segments.iter().find(|s| **s == "." || **s == "..") {
        return Err(invalid(format!(
            "must not contain a `{bad}` segment; it changes where \
             the path resolves without changing how deep it looks"
        )));
    }

    if segments.len() < MIN_ROOT_SEGMENTS {
        return Err(invalid(format!(
            "must name at least {MIN_ROOT_SEGMENTS} directory below \
             `/` or `~`, so the project directory bombyx creates and \
             deletes is not a top-level one"
        )));
    }

    // A remote shell expands `~` only in leading position.
    // Anywhere else it is a literal character in a directory
    // name, and almost certainly a mistake.
    if value.char_indices().any(|(i, c)| c == '~' && i > 0) {
        return Err(invalid("`~` is only allowed as the first character"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts `bad` is refused, with a message mentioning
    /// `reason`.
    fn refused(bad: &str, reason: &str) {
        let err = check(bad).expect_err("must be refused").to_string();
        assert!(err.contains(reason), "{bad:?}: want {reason:?}, got {err}");
    }

    #[test]
    fn the_whole_family_of_bad_roots_is_refused() {
        // Written as a table rather than one case at a time,
        // because a guard on a path is only as good as the
        // shapes nobody thought to try. `/.` is the one that
        // matters most: five characters, and it defeats a depth
        // count that treats every segment as a directory.
        refused("", "must not be empty");
        refused("   ", "must not be empty");
        refused("-x", "would treat as an option");
        refused("vms", "must start with");
        // `~name` is another user's home directory to a shell,
        // and `quote_remote_path` leaves only `~` and `~/`
        // unquoted -- so this one reads as anchored and resolves
        // relatively. See `check`.
        refused("~vms", "must start with");
        refused("~", "at least 1 directory");
        refused("/", "at least 1 directory");
        refused("//", "at least 1 directory");
        refused("~/", "at least 1 directory");
        refused("/.", "`.` segment");
        refused("~/.", "`.` segment");
        refused("/vms/..", "`..` segment");
        refused("~/../..", "`..` segment");
        refused("/vms/~/x", "first character");
        refused("/vms;rm -rf /", "is not allowed");
    }

    #[test]
    fn the_roots_people_actually_write_are_kept() {
        for good in ["~/vms", "/srv/vms", "~/a/b/c", "/srv/vms/"] {
            check(good).unwrap_or_else(|e| panic!("{good:?}: {e}"));
        }
    }

    #[test]
    fn the_depth_message_says_the_same_thing_as_the_constant() {
        // The rule is stated in three places: this constant,
        // the message an operator reads, and
        // `docs/architecture.md`. Asserting the whole sentence
        // here is what keeps the three from drifting.
        let err = check("/").expect_err("must be refused").to_string();
        assert_eq!(
            err,
            "invalid `remote_root`: must name at least 1 directory \
             below `/` or `~`, so the project directory bombyx \
             creates and deletes is not a top-level one"
        );
    }

    #[test]
    fn path_segments_counts_directories_and_not_characters() {
        // A doubled or trailing slash adds no directory, and the
        // leading `~` is a root marker rather than one.
        assert_eq!(path_segments("~/vms"), ["vms"]);
        assert_eq!(path_segments("/srv//vms/"), ["srv", "vms"]);
        assert_eq!(path_segments("~"), Vec::<&str>::new());
        // `.` is kept, so `check` can refuse it. Filtering it
        // here would let `~/.` pass as depth one.
        assert_eq!(path_segments("~/."), ["."]);
    }
}
