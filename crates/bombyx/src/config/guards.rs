//! Checks on a single config value, independent of where it
//! came from.
//!
//! A rule that several fields share lives here once, so widening
//! it reaches all of them at the same time. Every field reaching a
//! command line uses the leading-dash rule. Six fields carry the
//! Ruby-literal rule, and `[env]` is one of them -- every entry's
//! value goes through it, so one field there is many values. The
//! blank check, the character check and the inside-the-clone
//! check have several callers each.
//!
//! Each `check_*` function returns [`FieldError`], not
//! `ConfigError`; the `is_*` predicates return a `bool`.
//! These functions check a value and nothing else. The caller
//! decides whether the value came from a file, and reports it
//! that way. See `config::error`.

use super::error::FieldError;

/// Characters allowed in a path on the VM host.
pub(super) fn is_remote_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '~')
}

/// Whether `path`, a path on the guest, names a `.ps1` file, the only
/// kind `powershell -File` runs.
///
/// Windows matches an extension without regard to case, so
/// `Setup.PS1` counts. A file named `.ps1` alone does not: it has no
/// name in front of the extension.
pub(super) fn is_powershell_file(path: &str) -> bool {
    let file = path.rsplit(['/', '\\']).next().unwrap_or_default();
    file.len() > ".ps1".len() && file.to_ascii_lowercase().ends_with(".ps1")
}

/// Whether Windows reserves `name` for a device: `con`, `prn`,
/// `aux`, `nul`, `com0` to `com9` or `lpt0` to `lpt9`, in upper or
/// lower case and with any extension. Microsoft's naming rules include `com0`
/// and `lpt0`, which Windows may accept in practice. They are refused
/// anyway: a refused name costs a rename, while a name Windows then
/// refuses fails the guest after the boot.
///
/// Windows cannot create a file or folder with such a name, so the
/// config refuses a value that names a folder in a Windows guest,
/// before the guest boots rather than in it.
pub(super) fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default();
    let stem = stem.to_ascii_lowercase();
    match stem.as_bytes() {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', d] | [b'l', b'p', b't', d] => d.is_ascii_digit(),
        _ => false,
    }
}

/// Requires a name a shell would accept as a variable: letters,
/// digits and underscores, not starting with a digit.
///
/// Two fields name a variable inside the secrets bombyx sends:
/// `repo_token`, and each key of `vault.entries`. The guest's
/// project reads that file as `NAME=value` lines, so a name
/// outside this set is one it could not look up.
pub(super) fn check_variable_name(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    check_not_empty(field, value)?;
    check_charset(
        field,
        value,
        |c| c.is_ascii_alphanumeric() || c == '_',
        "letters, digits and underscores",
    )?;
    if value.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(FieldError::invalid(
            field,
            "must not start with a digit, which no shell \
             would accept as a variable name",
        ));
    }
    Ok(())
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
/// look: `host` and `remote_root` reach `ssh`, and `ref`,
/// `repo` and `script` reach `git`.
///
/// **For `ref` this is the second of two guards, not the only
/// one.** The guest runs `git fetch origin -- "$BOMBYX_REF"`,
/// with `--depth 1` for a shallow clone, and that `--` already
/// tells `git` that whatever follows it is a value rather than
/// an option.
///
/// The check is kept anyway, because `git` accepts options
/// *after* positional arguments. That is easy to miss, since
/// many tools do not. So a command that forgets the `--` --
/// this one, or a future one bombyx composes -- would read
/// `--upload-pack=/bin/sh` as an instruction naming a program to
/// run on the other end, rather than as a branch name.
///
/// **Add a field whose value reaches a command line, and it
/// requires this check too.** It has one caller per such field:
/// `host`, `remote_root`, `ref`, `repo`, `script` and the
/// `secrets_refreshed` hook. `project` needs
/// no separate call: it is a `crate::name::ProjectName`, whose
/// rule refuses any first character that is not a letter or a
/// digit, and that covers a leading dash.
pub(super) fn check_not_an_option(
    field: &'static str,
    value: &str,
    tool: &str,
) -> Result<(), FieldError> {
    if value.starts_with('-') {
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

/// Refuses a path that leaves the clone before the guest has
/// resolved it: an absolute one, or one holding a `..` segment.
///
/// Two fields name a file inside the clone -- `script` and the
/// `secrets_refreshed` hook -- and both reach a command on the
/// guest that runs whatever the path names. This is the half of
/// the containment rule the config can check. The other half runs
/// on the guest, which resolves the path with `readlink -f` and
/// refuses one that a symlink in the repository leads out of,
/// because only the guest can see what the repository put there.
pub(super) fn check_inside_clone(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    let bad = if value.starts_with('/') {
        Some("must be relative to the clone root")
    } else if value.split('/').any(|s| s == "..") {
        Some("must not contain a `..` segment")
    } else {
        None
    };
    match bad {
        Some(reason) => Err(FieldError::invalid(field, reason)),
        None => Ok(()),
    }
}

/// Refuses a path inside the clone that cannot name a file: a
/// final `.` segment, which is the directory holding it, and a
/// trailing `/`.
///
/// The same two fields as [`check_inside_clone`] call it, and for
/// a reason the guest cannot give. `readlink -f` resolves `.` to
/// the clone itself, which the guest's containment check then
/// refuses as leading outside the clone -- a false reason, repeated
/// on every run, for a value that is simply not a file. A `.` or a
/// doubled `/` elsewhere in the path is left alone, because
/// `./a.sh`, `a/./b.sh` and `a//b.sh` all name a file.
pub(super) fn check_names_a_file(
    field: &'static str,
    value: &str,
) -> Result<(), FieldError> {
    let names_a_file =
        !value.ends_with('/') && value.rsplit('/').next() != Some(".");
    if names_a_file {
        return Ok(());
    }
    Err(FieldError::invalid(
        field,
        "must name a file: no final `.` and no trailing `/`",
    ))
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

/// Refuses a value that would break the Vagrantfile bombyx
/// writes, or arrive somewhere with whitespace nobody meant.
///
/// bombyx generates a Vagrantfile, which is a Ruby file, and
/// config values get written into it inside double quotes:
/// `box`, `repo`, `ref`, `script`, and every value in the
/// `[env]` table. Something like
/// `box = "generic/ubuntu2204"` in the config becomes
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
/// value means nothing for any of these fields. And
/// leading or trailing whitespace is almost always a
/// copy-paste artifact, which fails obscurely and far from
/// here -- a trailing space on `repo` comes back from the guest
/// as `repository '...' does not exist`.
///
/// Escaping the four characters would work instead of refusing
/// them. Refusing is better: a box name, a repository address,
/// a branch name and a path have no reason to contain any of
/// them, so allowing them would only give the renderer more to
/// get right.
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
    // operator sees it before bombyx reaches the VM host.
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
    use crate::config::{BoxName, EnvValue, GitRef, RepoUrl, ScriptPath};

    /// Builds one of the checked newtypes from a string and
    /// throws the value away, so a rule several of them share
    /// can be tested against every one of them.
    type Build = fn(&str) -> Result<(), FieldError>;

    /// The five newtypes whose rules are [`check_renderable`]
    /// and, for three of them, [`check_not_an_option`].
    ///
    /// Each row is a field name, its constructor, a value that
    /// constructor accepts, and whether the value reaches a
    /// command line. The accepted value is written in the row
    /// rather than worked out from the field name, so a test
    /// needing one reads it here. The last column decides which
    /// rows [`check_not_an_option`] applies to. Two rows do not
    /// carry it: `box` is resolved by vagrant and never becomes
    /// an argument bombyx composes, and an `[env]` value
    /// becomes the right-hand side of a shell assignment, where
    /// a leading `-` is an ordinary character.
    ///
    /// The table lives here rather than beside any one type,
    /// because the rules live here. So a new field sharing that
    /// rule set is one more row, wherever the type itself
    /// lives.
    ///
    /// **Other newtypes use rules from this module and are
    /// deliberately not rows.** `RemoteRoot` and `HostName` are
    /// built on `check_not_empty`, `check_not_an_option` and
    /// `check_charset`, never on `check_renderable`, and each
    /// carries anchoring or charset rules of its own that no
    /// column here could express. `super::root` and
    /// `super::host` test them, the dash rule included.
    ///
    /// So this table is not the answer to "which types use this
    /// module"; it is the answer to "which types share one rule
    /// set".
    ///
    /// The closures capture nothing, so they become plain
    /// function pointers and the array has one type.
    fn renderable_newtypes() -> [(&'static str, Build, &'static str, bool); 5] {
        [
            (
                "repo",
                |s| RepoUrl::parse(s).map(|_| ()),
                "https://example.invalid/p.git",
                true,
            ),
            (
                "script",
                |s| ScriptPath::parse(s).map(|_| ()),
                "vagrant/provision.sh",
                true,
            ),
            ("ref", |s| GitRef::parse(s).map(|_| ()), "main", true),
            (
                "box",
                |s| BoxName::parse(s).map(|_| ()),
                "generic/ubuntu2204",
                false,
            ),
            ("env", |s| EnvValue::parse(s).map(|_| ()), "22", false),
        ]
    }

    /// Asserts `bad` is refused with a message mentioning
    /// `reason`.
    ///
    /// Pinning the reason, not just the failure, is what makes
    /// these tests notice a deleted rule. A value refused by
    /// some *other* check would still fail `is_err()`, so a
    /// weaker assertion goes green while the rule it covered
    /// is gone.
    fn refused_because(build: Build, bad: &str, reason: &str) {
        let err = build(bad).expect_err("must be refused").to_string();
        assert!(err.contains(reason), "{bad:?}: want {reason:?}, got {err}");
    }

    #[test]
    fn every_renderable_newtype_refuses_a_blank_value() {
        for (field, build, _, _) in renderable_newtypes() {
            for bad in ["", "   "] {
                refused_because(build, bad, "must not be empty");
                // The field name is the only part of the error
                // telling an operator which key to edit, and it
                // travels through a guard, a `FieldError` and a
                // `ConfigError` before it is printed. Swap two
                // of them and only this line notices.
                refused_because(build, bad, field);
            }
        }
    }

    #[test]
    fn every_renderable_newtype_refuses_surrounding_whitespace() {
        // A copy-paste artifact that otherwise fails inside the
        // guest, long after bombyx could have said so.
        for (_, build, good, _) in renderable_newtypes() {
            for bad in [format!(" {good}"), format!("{good} ")] {
                refused_because(build, &bad, "whitespace");
            }
        }
    }

    #[test]
    fn every_renderable_newtype_refuses_characters_that_break_the_ruby() {
        // Both characters reach a Ruby string literal in the
        // generated Vagrantfile: a quote ends it early, a
        // backslash escapes whatever follows.
        for (_, build, good, _) in renderable_newtypes() {
            for bad in [format!("{good}a\"b"), format!("{good}a\\b")] {
                refused_because(build, &bad, "would end or escape");
            }
        }
    }

    #[test]
    fn every_renderable_newtype_reports_a_control_character_as_one() {
        // Separate message from the quote case: a BEL neither
        // ends nor escapes a Ruby literal, and saying it does
        // sends an operator hunting a quoting problem they do
        // not have.
        for (_, build, good, _) in renderable_newtypes() {
            let bad = format!("{good}a\u{7}b");
            refused_because(build, &bad, "control character");
        }
    }

    #[test]
    fn a_renderable_newtype_reaching_a_command_line_refuses_an_option() {
        // `-oProxyCommand=id:x` is the case that pins this rule
        // for `repo`. One colon, no `://`, so the URL check
        // reads it as the SSH shorthand `host:path` and accepts
        // it outright -- delete the dash rule and that value is
        // not refused at all.
        for (field, build, good, reaches_a_command_line) in
            renderable_newtypes()
        {
            // The row's last column is read once, here, rather
            // than inside the loop over bad values, so the
            // branch reads as the per-type decision it is.
            if !reaches_a_command_line {
                // `box` is the row without the rule, and it is
                // asserted rather than skipped: otherwise this
                // test would go green on a table where every row
                // had lost its last column.
                let dashed = format!("-{good}");
                assert!(
                    build(&dashed).is_ok(),
                    "{field} carries no option rule, so {dashed:?} \
                     must be accepted"
                );
                continue;
            }
            for bad in ["-x", "-oProxyCommand=id:x", "--upload-pack=/bin/sh:x"]
            {
                // The tool name is asserted, not just the rule,
                // because each constructor chooses which program
                // to name. All three of these hand their value
                // to `git`, and a constructor changed to name
                // something else is what this line catches.
                refused_because(build, bad, "git would treat as an option");
            }
        }
    }

    #[test]
    fn the_option_message_names_the_field_the_tool_and_the_rule() {
        // Several callers share this rule, so the message has
        // to send the operator to the right place. Both pairings
        // below are ones the production code really produces:
        // `ref` is handed to `git`, and `host` becomes `ssh`'s
        // first positional argument.
        //
        // The whole message per row rather than a fragment,
        // because a `contains` check steps over a sentence that
        // has come apart around the pieces it looks for.
        let cases = [
            (
                "ref",
                "--upload-pack=x",
                "git",
                "invalid `ref`: must not start with `-`, which git \
                 would treat as an option",
            ),
            (
                "host",
                "-x",
                "ssh",
                "invalid `host`: must not start with `-`, which ssh \
                 would treat as an option",
            ),
        ];
        for (field, value, tool, expected) in cases {
            let err = check_not_an_option(field, value, tool)
                .expect_err("must be refused");
            assert_eq!(err.to_string(), expected);
        }

        assert!(check_not_an_option("ref", "main", "git").is_ok());
    }
}
