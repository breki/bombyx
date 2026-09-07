//! The `[source]` table: where the guest fetches the project
//! from, and the three checked types that hold its values.
//!
//! All three are *newtypes* -- a struct wrapping one private
//! `String`, buildable only through a function that checks the
//! value first. [`RepoUrl`] explains the pattern in full; read
//! that one first.
//!
//! All three values reach `git` and the guest's shell, so they
//! carry the checks that cannot be expressed as "a non-empty
//! string".
//!
//! The table has a fourth key, `deploy_key`, and its type lives
//! in `super::deploy_key` rather than here. It is the one value
//! in the table that is a path on the VM host instead of
//! something the guest hands to `git`, and its rules are its
//! own.

use serde::Deserialize;

use super::deploy_key::DeployKeyPath;
use super::error::FieldError;
use super::guards;
use crate::newtype::checked_str_newtype;

/// Where the guest fetches the project from, as `[source]`.
///
/// The guest clones this itself, so the first three keys are
/// not paths on the workstation or the VM host -- see
/// `docs/trust-boundary.md`. `deploy_key` is the exception,
/// and its own module says why.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Repository the guest clones.
    pub repo: RepoUrl,
    /// Branch or tag to clone.
    ///
    /// Named `git_ref` because `ref` is a Rust keyword.
    #[serde(rename = "ref")]
    pub git_ref: GitRef,
    /// Provisioning script to run, relative to the clone root.
    pub script: ScriptPath,
    /// Private key on the VM host that the guest clones a
    /// private repository with.
    ///
    /// `None` when the config names none, which is what a
    /// public repository wants: `vagrant` then uploads nothing
    /// and the guest clones without a credential.
    #[serde(default)]
    pub deploy_key: Option<DeployKeyPath>,
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
}

checked_str_newtype!(
    RepoUrl,
    "The value, as `git` and the Vagrantfile see it."
);

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
/// when compiling *for* Unix.)
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
}

checked_str_newtype!(ScriptPath, "The value, as the guest's shell sees it.");

impl TryFrom<String> for ScriptPath {
    type Error = FieldError;

    /// What serde calls; see [`RepoUrl::try_from`].
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_script(&raw)?;
        Ok(Self(raw))
    }
}

/// A branch or tag name that `git` will fetch, and not read as
/// an option.
///
/// A newtype for the same reason as [`RepoUrl`]: the rules live
/// in [`GitRef::parse`], so holding one means they have run.
///
/// The value is written into the generated Vagrantfile and then
/// handed to `git` inside the guest, so it gets both the
/// Ruby-literal rules and the leading-dash rule.
///
/// **The dash rule here is the second of two guards.** The
/// guest runs `git fetch --depth 1 origin -- "$BOMBYX_REF"`,
/// and that `--` already tells `git` that whatever follows is a
/// value rather than an option. `super::guards::check_not_an_option`
/// says why the rule is kept anyway.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct GitRef(String);

impl GitRef {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, or
    /// would be read by `git` as an option.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_git_ref(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(GitRef, "The value, as `git` and the Vagrantfile see it.");

impl TryFrom<String> for GitRef {
    type Error = FieldError;

    /// What serde calls; see [`RepoUrl::try_from`].
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_git_ref(&raw)?;
        Ok(Self(raw))
    }
}

/// Every rule a `ref` value must pass, in one place.
///
/// Both [`GitRef::parse`] and [`GitRef::try_from`] call this,
/// so neither can run a different set. Both rules are shared
/// with other fields: the value is written into a Ruby file,
/// and it reaches `git` on a command line.
fn check_git_ref(value: &str) -> Result<(), FieldError> {
    guards::check_renderable("ref", value)?;
    guards::check_not_an_option("ref", value, "git")?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts `bad` is refused with a message mentioning
    /// `reason`.
    ///
    /// The rules these three types share are tested against all
    /// four checked newtypes at once, in `super::guards`'s test
    /// module, where those rules live. What is left here is each
    /// type's own rule, so this helper takes a constructor
    /// rather than a table.
    fn refused_because(
        build: fn(&str) -> Result<(), FieldError>,
        bad: &str,
        reason: &str,
    ) {
        let err = build(bad).expect_err("must be refused").to_string();
        assert!(err.contains(reason), "{bad:?}: want {reason:?}, got {err}");
    }

    #[test]
    fn a_windows_style_script_path_is_caught_by_the_character_rule() {
        // `check_script` says a backslash never reaches it,
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

        let git_ref = GitRef::parse("main").unwrap();
        assert_eq!(git_ref.as_str(), "main");
        assert_eq!(git_ref.to_string(), "main");
        assert_eq!(git_ref.as_ref(), "main");
    }

    #[test]
    fn serde_runs_every_check_while_the_table_is_read() {
        // `try_from` is a second entry point into each type, and
        // the attribute that selects it is easy to drop. Without
        // it serde assigns the private field and no check runs
        // at all, so this asserts against the deserializer
        // rather than against the three `parse` functions.
        let good = "repo = \"https://example.invalid/p.git\"\n\
                    ref = \"main\"\n\
                    script = \"vagrant/provision.sh\"\n";
        toml::from_str::<Source>(good).expect("the fixture must load");

        for (from, to, reason) in [
            (
                "https://example.invalid/p.git",
                "ext::sh -c id",
                "must be an https",
            ),
            (
                "ref = \"main\"",
                "ref = \"--upload-pack=x\"",
                "as an option",
            ),
            (
                "vagrant/provision.sh",
                "/usr/bin/env",
                "relative to the clone",
            ),
        ] {
            let err = toml::from_str::<Source>(&good.replace(from, to))
                .expect_err("must be refused");
            assert!(err.to_string().contains(reason), "{to}: {err}");
        }
    }
}
