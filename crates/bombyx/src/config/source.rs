//! The `[source]` table: where the guest fetches the project
//! from, and the two checked types that hold its values.
//!
//! Both types are *newtypes* -- a struct wrapping one private
//! `String`, buildable only through a function that checks the
//! value first. [`RepoUrl`] explains the pattern in full; read
//! that one first.
//!
//! These two values are the ones that reach `git` and the
//! guest's shell, so they carry the checks that cannot be
//! expressed as "a non-empty string".

use std::fmt;

use serde::Deserialize;

use super::error::FieldError;
use super::guards;

/// Where the guest fetches the project from, as `[source]`.
///
/// The guest clones this itself, so none of it is a path on
/// the workstation or the VM host -- see
/// `docs/trust-boundary.md`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Repository the guest clones.
    pub repo: RepoUrl,
    /// Branch or tag to clone.
    ///
    /// Named `git_ref` because `ref` is a Rust keyword.
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// Provisioning script to run, relative to the clone root.
    pub script: ScriptPath,
}

/// A repository address that `git` will download from, and not
/// run as a command.
///
/// This is a *newtype*: a struct wrapping one `String`, where
/// the `String` inside is private. You cannot build one
/// directly. You have to call [`RepoUrl::parse`], which checks
/// the value first. So if you are holding a `RepoUrl`, it has
/// already been checked, and the compiler is what promises you
/// that.
///
/// Why not just check a plain `String` somewhere? Because the
/// other fields of `Config` are public, so any code can build a
/// `Config` by hand and never call the checking function. A
/// type is harder to go around than a function call.
///
/// The danger it guards against: `git` supports "remote
/// helpers", which are addresses written as `name::rest`. One
/// of them is `ext::`, and it tells `git` to *run* the rest as
/// a shell command. So `ext::sh -c "..."` looks like an address
/// and is really an instruction, and it would run inside the
/// guest VM as root, before any of the project's own code
/// exists. [`RepoUrl::parse`] refuses it.
///
/// You might reach for the `url` crate here. Do not, for two
/// reasons. First, `RepoUrl` accepts `git@github.com:you/repo.git`,
/// the usual way to write an SSH address for `git`, and it is
/// not a valid URL -- a URL parser rejects it. Second, bombyx
/// never looks at the pieces of the address. It passes the
/// whole thing to `git` and writes it into the Vagrantfile, so
/// splitting it into scheme, host and path would buy nothing.
///
/// `#[serde(try_from = "String")]` is what connects the type to
/// the config file. It tells serde to read a plain string and
/// then hand it to [`RepoUrl::try_from`], which may refuse it.
/// Without the attribute serde would build the struct by
/// assigning its private field directly, skipping every check --
/// so the attribute is what makes "holding a `RepoUrl` means it
/// passed" true for a value that came out of TOML.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RepoUrl(String);

impl RepoUrl {
    /// Checks `raw` and wraps it.
    ///
    /// Takes `&str` so a caller holding a borrowed value need
    /// not copy it. The one copy this does make is the value it
    /// keeps. Serde arrives with a `String` it already owns, and
    /// [`RepoUrl::try_from`] hands that straight over rather
    /// than copying it a second time. Both run the same checks --
    /// `check_repo`, which is private, so this is not a link.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, would
    /// be read by `git` as an option, or names a remote helper
    /// rather than a repository.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_repo(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// The value, as `git` and the Vagrantfile see it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for RepoUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoUrl {
    type Error = FieldError;

    /// What serde calls. It already owns the `String`, so the
    /// checks run against a borrow of it and the value moves
    /// into the newtype -- no copy on the path a config load
    /// actually takes. A refused value is dropped here.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_repo(&raw)?;
        Ok(Self(raw))
    }
}

/// A path inside the cloned project, on the guest.
///
/// A newtype for the same reason as [`RepoUrl`]: the checks
/// live in [`ScriptPath::parse`], so holding one of these means
/// it has already been checked.
///
/// You would expect `PathBuf` here, and it is the wrong choice.
/// `PathBuf` answers questions about the machine bombyx was
/// compiled for -- on Windows `\` separates directories and
/// `C:` names a drive, on Linux neither does. This path is
/// resolved on the guest, which is always Linux, so a `PathBuf`
/// would answer for the wrong computer, and answer differently
/// depending on who ran `bombyx up`. Since bombyx only checks
/// this value and passes it along, a checked string is the
/// honest representation.
///
/// (`std::os::unix` cannot stand in: those modules exist only
/// when compiling *for* Unix. `vagrant_dir` is the field that
/// really is a local path, and it does become a `PathBuf`,
/// where it is used rather than here.)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ScriptPath(String);

impl ScriptPath {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, would
    /// be read by `git` as an option, or leaves the clone
    /// directory.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_script(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// The value, as the guest's shell sees it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScriptPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ScriptPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ScriptPath {
    type Error = FieldError;

    /// What serde calls; see [`RepoUrl::try_from`].
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_script(&raw)?;
        Ok(Self(raw))
    }
}

/// Every rule a `repo` value must pass, in one place.
///
/// Both [`RepoUrl::parse`] and [`RepoUrl::try_from`] call this,
/// so neither can run a different set.
///
/// The first two rules are shared with other fields: the value
/// is written into a Ruby file, and it reaches `git` on a
/// command line. The third is `repo`'s own, and it stops the
/// `ext::` problem [`RepoUrl`] describes.
///
/// That third rule is an allowlist. Rather than naming every
/// dangerous spelling, it names the safe ones and refuses
/// everything else, so a spelling nobody thought of is refused
/// by default rather than allowed by default.
///
/// Two shapes are allowed. One is a normal URL starting with a
/// recognised scheme. The other is the SSH shorthand
/// `git@github.com:you/repo.git`, which has no `://` at all --
/// it is a host, a colon, then a path. That is what `scp_like`
/// below is looking for, and it refuses any `::`, so
/// `ext::something` cannot slip through as "a host called ext
/// with an empty path".
fn check_repo(value: &str) -> Result<(), FieldError> {
    const ALLOWED: [&str; 4] = ["https://", "http://", "ssh://", "git://"];

    guards::check_renderable("repo", value)?;
    guards::check_not_an_option("repo", value, "git")?;

    let scp_like =
        !value.contains("://") && value.contains(':') && !value.contains("::");
    if ALLOWED.iter().any(|p| value.starts_with(p)) || scp_like {
        return Ok(());
    }
    Err(FieldError::Invalid {
        field: "repo",
        reason: "must be an https, http, ssh or git URL, or \
                 `user@host:path`; a `<transport>::<rest>` \
                 remote helper such as `ext::` runs a command \
                 rather than cloning"
            .to_owned(),
    })
}

/// Every rule a `script` value must pass, in one place.
///
/// Both [`ScriptPath::parse`] and [`ScriptPath::try_from`] call
/// this, so neither can run a different set.
///
/// The first two rules are shared with other fields. The rest
/// are `script`'s own, and they matter because the guest
/// changes into the cloned project, runs `chmod +x` on this
/// path, and executes it as root -- so whatever this names is
/// about to be given the run of the machine.
///
/// A value escapes the clone in two ways, and both are refused:
///
/// - Starting with `/` makes it an absolute path, so it stops
///   being relative to the clone at all. `/usr/bin/env` would
///   make the guest `chmod +x` a system binary.
/// - A `..` segment steps up a directory. Enough of them and
///   you are outside the clone again, by a longer route.
///
/// Two more shapes are refused before this function is reached,
/// which is why it does not test for them. `check_renderable`
/// runs first and rejects any backslash anywhere -- so a
/// Windows-style `\windows\x` never arrives -- and it rejects
/// surrounding whitespace, so ` provision.sh` never arrives
/// either.
fn check_script(value: &str) -> Result<(), FieldError> {
    guards::check_renderable("script", value)?;
    guards::check_not_an_option("script", value, "git")?;

    let bad = if value.starts_with('/') {
        Some("must be relative to the clone root")
    } else if value.split('/').any(|s| s == "..") {
        Some("must not contain a `..` segment")
    } else {
        None
    };
    match bad {
        Some(reason) => Err(FieldError::Invalid {
            field: "script",
            reason: reason.to_owned(),
        }),
        None => Ok(()),
    }
}

/// Checks the `[source]` values that types cannot.
///
/// Only `ref`. `repo` and `script` are [`RepoUrl`] and
/// [`ScriptPath`], and those types run their checks when they
/// are built, so one that exists at all is one that passed --
/// there is nothing left here to check.
///
/// `ref` is a plain `String` because its rules are the generic
/// ones any string field would need, so a type wrapping it
/// would promise nothing extra.
pub(super) fn validate(source: &Source) -> Result<(), FieldError> {
    guards::check_renderable("ref", &source.git_ref)?;
    guards::check_not_an_option("ref", &source.git_ref, "git")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Source {
        Source {
            repo: RepoUrl::parse("https://example.invalid/p.git")
                .expect("a valid fixture URL"),
            git_ref: "main".to_owned(),
            script: ScriptPath::parse("vagrant/provision.sh")
                .expect("a valid fixture path"),
        }
    }

    /// Builds either newtype from a string and throws the value
    /// away, so a rule both share can be tested against both.
    type Build = fn(&str) -> Result<(), FieldError>;

    /// The two newtypes, as field name, constructor, and a value
    /// that constructor accepts.
    ///
    /// The accepted value is in the row rather than worked out
    /// from the field name, so a test needing one reads it here.
    /// A third newtype is then one more row, instead of one more
    /// branch inside every test.
    ///
    /// The closures capture nothing, so they become plain
    /// function pointers and the array has one type.
    fn both_newtypes() -> [(&'static str, Build, &'static str); 2] {
        [
            (
                "repo",
                |s| RepoUrl::parse(s).map(|_| ()),
                "https://example.invalid/p.git",
            ),
            (
                "script",
                |s| ScriptPath::parse(s).map(|_| ()),
                "vagrant/provision.sh",
            ),
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
    fn both_newtypes_refuse_a_blank_value() {
        for (field, build, _) in both_newtypes() {
            for bad in ["", "   "] {
                refused_because(build, bad, "must not be empty");
                // The field name is the only part of the error
                // telling an operator which key to edit, and it
                // now travels through a guard, a `FieldError`
                // and a `ConfigError` before it is printed.
                // Swap two of them and only this line notices.
                refused_because(build, bad, field);
            }
        }
    }

    #[test]
    fn both_newtypes_refuse_surrounding_whitespace() {
        // A copy-paste artifact that otherwise fails inside the
        // guest, long after bombyx could have said so.
        for (_, build, good) in both_newtypes() {
            for bad in [format!(" {good}"), format!("{good} ")] {
                refused_because(build, &bad, "whitespace");
            }
        }
    }

    #[test]
    fn both_newtypes_refuse_characters_that_break_the_ruby() {
        // Both characters reach a Ruby string literal in the
        // generated Vagrantfile: a quote ends it early, a
        // backslash escapes whatever follows.
        for (_, build, good) in both_newtypes() {
            for bad in [format!("{good}a\"b"), format!("{good}a\\b")] {
                refused_because(build, &bad, "would end or escape");
            }
        }
    }

    #[test]
    fn a_windows_style_script_path_is_caught_by_the_character_rule() {
        // `check_script_path` says a backslash never reaches it,
        // because `check_renderable` runs first and refuses one
        // anywhere in the value. This is the case holding that
        // claim up: `\windows\x` looks like a path mistake and
        // is refused as a character mistake.
        refused_because(
            |s| ScriptPath::parse(s).map(|_| ()),
            "\\windows\\x",
            "would end or escape",
        );
    }

    #[test]
    fn both_newtypes_refuse_a_value_git_would_treat_as_an_option() {
        // `-oProxyCommand=id:x` is the case that pins this rule
        // for `repo`. One colon, no `://`, so the URL check
        // reads it as the SSH shorthand `host:path` and accepts
        // it outright -- delete the dash rule and that value is
        // not refused at all.
        for (_, build, _) in both_newtypes() {
            for bad in ["-x", "-oProxyCommand=id:x", "--upload-pack=/bin/sh:x"]
            {
                refused_because(build, bad, "git would treat as an option");
            }
        }
    }

    #[test]
    fn a_repo_url_refuses_anything_git_would_not_clone() {
        // `git` remote helpers are written `name::rest`, and
        // `ext::` runs the rest as a shell command rather than
        // cloning anything.
        for bad in ["ext::sh -c 'id > /pwned'", "fd::7", "not-a-url"] {
            refused_because(
                |s| RepoUrl::parse(s).map(|_| ()),
                bad,
                "must be an https",
            );
        }
    }

    #[test]
    fn a_repo_url_keeps_the_spellings_people_write() {
        for good in [
            "https://github.com/breki/bombyx",
            "http://example.invalid/p.git",
            "ssh://git@example.invalid/p.git",
            "git://example.invalid/p.git",
            "git@github.com:breki/bombyx.git",
        ] {
            let parsed = RepoUrl::parse(good)
                .unwrap_or_else(|e| panic!("{good:?}: {e}"));
            assert_eq!(parsed.as_str(), good);
        }
    }

    #[test]
    fn a_script_path_refuses_one_that_leaves_the_clone() {
        // Whatever this names is about to be made executable and
        // run as root in the guest.
        for (bad, reason) in [
            ("/usr/bin/env", "relative to the clone root"),
            ("../../usr/bin/env", "`..` segment"),
            ("a/../../../etc/x", "`..` segment"),
        ] {
            refused_because(|s| ScriptPath::parse(s).map(|_| ()), bad, reason);
        }

        let ok = ScriptPath::parse("vagrant/provision.sh")
            .expect("a plain relative path");
        assert_eq!(ok.as_str(), "vagrant/provision.sh");
    }

    #[test]
    fn the_newtypes_render_as_the_value_they_hold() {
        let repo = RepoUrl::parse("https://example.invalid/p.git").unwrap();
        assert_eq!(repo.to_string(), "https://example.invalid/p.git");
        assert_eq!(repo.as_ref(), "https://example.invalid/p.git");

        let script = ScriptPath::parse("vagrant/provision.sh").unwrap();
        assert_eq!(script.to_string(), "vagrant/provision.sh");
        assert_eq!(script.as_ref(), "vagrant/provision.sh");
    }
    #[test]
    fn refuses_a_ref_git_would_treat_as_an_option() {
        // `ref` is the one `[source]` field `validate` checks.
        // `repo` and `script` are newtypes, so their rules live
        // in their constructors and the tests above cover them.
        //
        // `bootstrap.sh` passes `--` before the ref, so this is
        // the second of two guards rather than the only one. See
        // `check_not_an_option` for why both are kept.
        for bad in ["-x", "--upload-pack=/bin/sh", "--exec=x"] {
            let mut s = source();
            s.git_ref = bad.to_owned();
            let err = validate(&s).unwrap_err();
            assert!(
                matches!(&err, FieldError::Invalid { field: "ref", .. }),
                "ref must refuse {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn a_control_character_in_a_ref_is_reported_as_one() {
        // Separate message from the quote case: a BEL neither
        // ends nor escapes a Ruby literal, and saying it does
        // sends an operator hunting a quoting problem.
        let mut s = source();
        s.git_ref = "ma\u{7}in".to_owned();
        let err = validate(&s).unwrap_err();
        let FieldError::Invalid { reason, .. } = &err else {
            panic!("{err:?}");
        };
        assert!(reason.contains("control character"), "{reason}");
    }
}
