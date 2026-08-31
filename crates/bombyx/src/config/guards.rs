//! Checks on a single config value, independent of where it
//! came from.
//!
//! A rule that several fields share lives here once, so widening
//! it reaches all of them at the same time. Seven fields use the
//! leading-dash rule, four use the Ruby-literal rule, and both
//! the blank check and the character check have several callers.
//! `check_project_relative` has one caller today and is here
//! because it is the same kind of work.
//!
//! Everything here returns [`FieldError`], not `ConfigError`.
//! These functions check a value and nothing else. The caller
//! decides whether the value came from a file, and reports it
//! that way. See `config::error`.

use std::path::{Component, Path};

use super::error::FieldError;

/// Characters allowed in a path on the VM host.
pub(super) fn is_remote_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '~')
}

/// Requires a value that is not blank.
pub(super) fn check_not_empty(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    if value.trim().is_empty() {
        return Err(FieldError::Empty { field });
    }
    Ok(())
}

/// Refuses a value the named tool would treat as an option.
///
/// Command-line tools tell options from ordinary values by the
/// leading `-`. So a config value starting with `-` that bombyx
/// hands to a program is read as an instruction to that program
/// instead of as data.
///
/// `tool` names the program in the message, because the answer
/// to "which program?" is what tells the operator where to
/// look: `host` reaches `ssh` and `scp`, `ref` reaches `git`,
/// and `vagrant_dir` reaches `tar`.
///
/// **For `ref` this is the second of two guards, not the only
/// one.** The guest runs
/// `git fetch --depth 1 origin -- "$BOMBYX_REF"`, and that `--`
/// already tells `git` that whatever follows it is a value
/// rather than an option.
///
/// The check is kept anyway, because `git` accepts options
/// *after* positional arguments. That is easy to miss, since
/// many tools do not. So a command that forgets the `--` --
/// this one, or a future one bombyx composes -- would read
/// `--upload-pack=/bin/sh` as an instruction naming a program to
/// run on the other end, rather than as a branch name.
///
/// **A new field whose value reaches a command line needs this
/// too.** Seven use it today: `host`, `project`, `vagrant_dir`,
/// `remote_root`, `ref`, `repo` and `script`.
pub(super) fn check_not_an_option(
    field: &'static str,
    value: &str,
    tool: &str,
) -> Result<(), FieldError> {
    if value.starts_with('-') {
        // "would treat" rather than "reads", because `tool` is
        // sometimes two programs at once. A verb agreeing with a
        // single subject turns ungrammatical the moment the
        // caller passes "ssh and scp", and the test below is
        // what holds this wording in place.
        return Err(FieldError::invalid(
            field,
            format!(
                "must not start with `-`, which {tool} would treat \
                 as an option"
            ),
        ));
    }
    Ok(())
}

/// Requires every character of `value` to be one `allowed`
/// accepts, naming `expected` in the message when one is not.
pub(super) fn check_charset(
    field: &'static str,
    value: &str,
    allowed: fn(char) -> bool,
    expected: &str,
) -> Result<(), FieldError> {
    if let Some(bad) = value.chars().find(|c| !allowed(*c)) {
        return Err(FieldError::invalid(
            field,
            format!("character {bad:?} is not allowed; use only {expected}"),
        ));
    }
    Ok(())
}

/// Requires a path that stays inside the project directory.
///
/// The value gets joined onto the working directory, and
/// `Path::join` **discards the left side** when the right one is
/// absolute. So an absolute value does not extend the project
/// path, it replaces it -- and since this config travels inside
/// a repository, that turns a clone into a tool that archives
/// whatever directory the repo specifies.
///
/// The rooted spellings are tested by hand rather than through
/// `Path::is_absolute`, because that answers differently per
/// platform: a Windows drive prefix is not absolute on Unix, and
/// the same config file gets read on both.
pub(super) fn check_project_relative(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(FieldError::invalid(field, "must not name a drive"));
    }
    if value.starts_with('/') || value.starts_with('\\') {
        return Err(FieldError::invalid(
            field,
            "must be relative to the project directory",
        ));
    }
    if value.starts_with('~') {
        return Err(FieldError::invalid(field, "must not start with `~`"));
    }

    // Everything left must be an ordinary segment. This is what
    // rejects `..` and `.`, in any position rather than only at
    // the front.
    for component in Path::new(value).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(FieldError::invalid(
                field,
                "must be a plain relative path, with no `.`, `..` or root",
            ));
        }
    }

    Ok(())
}

/// Refuses a value that would break the Vagrantfile bombyx
/// writes, or arrive somewhere with whitespace nobody meant.
///
/// bombyx generates a Vagrantfile, which is a Ruby file, and
/// four config values get written into it inside double quotes:
/// `box`, `repo`, `ref` and `script`. Something like
/// `box = "generic/ubuntu2204"` in `bombyx.toml` becomes
/// `config.vm.box = "generic/ubuntu2204"` in the Ruby.
///
/// Four kinds of character break that. All four are refused,
/// not just the ones that seem likely, because "likely" is
/// what the next surprising value will not be:
///
/// - A double quote ends the Ruby string early, so the rest of
///   the line becomes code instead of text.
/// - A backslash starts an escape sequence, so the next
///   character means something other than itself.
/// - A control character, and a newline counts as one, ends the
///   line in the middle of a string.
/// - `#{` is Ruby's way of saying "run this and paste the
///   result here". Ruby would execute it rather than print it.
///
/// Two more refusals are about the value being wrong rather
/// than the Ruby being wrong, and they come first. A blank
/// value means nothing for any of these four fields. And
/// leading or trailing whitespace is almost always a
/// copy-paste artifact, which fails obscurely and far from
/// here -- a trailing space on `repo` comes back from the guest
/// as `repository '...' does not exist`.
///
/// Escaping the four characters would work instead of refusing
/// them. Refusing is better: a box name, a repository address,
/// a branch name and a relative path have no reason to contain
/// any of them, so allowing them would only give the renderer
/// more to get right.
pub(super) fn check_renderable(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    check_not_empty(field, value)?;
    // Surrounding whitespace is almost always a copy-paste
    // artifact, and every one of these fields fails obscurely
    // with it. A trailing space on `repo` reaches the guest and
    // comes back as `repository '...' does not exist`; a
    // leading one makes `git` read the value as a local path
    // and name nothing recognisable. Catching it here means the
    // operator sees it before anything is pushed.
    if value.trim() != value {
        return Err(FieldError::Invalid {
            field,
            reason: "must not begin or end with whitespace".to_owned(),
        });
    }
    if let Some(bad) = value.chars().find(|c| c.is_control()) {
        // Split from the quote case because the mechanism
        // differs: a BEL or a tab neither ends nor escapes a
        // Ruby literal, and telling an operator it does sends
        // them hunting a quoting problem they do not have.
        return Err(FieldError::Invalid {
            field,
            reason: format!(
                "control character {bad:?} is not allowed; use \
                 printable characters only"
            ),
        });
    }
    if let Some(bad) = value.chars().find(|c| *c == '"' || *c == '\\') {
        return Err(FieldError::Invalid {
            field,
            reason: format!(
                "character {bad:?} is not allowed; it would end \
                 or escape the string in the generated Vagrantfile"
            ),
        });
    }
    if value.contains("#{") {
        return Err(FieldError::Invalid {
            field,
            reason: "`#{` is Ruby interpolation and would be \
                     evaluated in the generated Vagrantfile"
                .to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_option_message_names_the_tool_that_would_be_fooled() {
        // One rule, several callers, and the message has to send
        // the operator to the right place. Each pairing here is
        // one the production code really produces: `ref` is
        // handed to `git`, `vagrant_dir` ends up in `tar -C`.
        let err = check_not_an_option("ref", "--upload-pack=x", "git")
            .expect_err("must be refused");
        assert!(err.to_string().contains("git"), "{err}");

        let err = check_not_an_option("vagrant_dir", "-x", "tar")
            .expect_err("must be refused");
        assert!(err.to_string().contains("tar"), "{err}");

        assert!(check_not_an_option("ref", "main", "git").is_ok());
    }

    #[test]
    fn the_option_message_reads_the_same_for_one_tool_or_two() {
        // `host`, `project`, `vagrant_dir` and `remote_root`
        // name two programs at once, so the sentence has to work
        // with a plural subject as well as a singular one. This
        // asserts the whole message rather than a fragment,
        // because a broken verb is exactly what a `contains`
        // check steps over.
        let err = check_not_an_option("host", "-x", "ssh and scp")
            .expect_err("must be refused");
        assert_eq!(
            err.to_string(),
            "invalid `host`: must not start with `-`, which ssh \
             and scp would treat as an option"
        );
    }
}
