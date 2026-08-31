//! The `[vm]` and `[source]` sections of `bombyx.toml`: what
//! machine to build, and what the guest should clone into it.
//!
//! Its own file because these types and their checks are one
//! subject, and `config.rs` was already holding six other
//! types.
//!
//! There is more than one check per value here, and that is
//! deliberate. A single config value can end up in three
//! different places, each of which can be attacked differently:
//!
//! - Written into the Vagrantfile, which is a Ruby file.
//! - Passed to `git` on the command line, inside the guest.
//! - Used as a path that gets made executable and run as root,
//!   also inside the guest.
//!
//! So "is this string safe" has no single answer. It depends
//! on which of the three you mean, and a value can be fine for
//! one and dangerous for another.

use std::fmt;

use serde::Deserialize;

use super::ConfigError;

/// The virtualization backend the generated Vagrantfile targets.
///
/// An enum rather than a string so an unknown value fails while
/// the config is being read. A string would reach the VM host,
/// render a Vagrantfile no `vagrant` can use, and report it only
/// after the push had already changed state there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// libvirt via `vagrant-libvirt`. The only provider bombyx
    /// has ever booted a machine with.
    Libvirt,
    /// Hyper-V. **Never exercised** -- written from the
    /// provider's documented options, not from a run.
    Hyperv,
}

impl fmt::Display for Provider {
    /// The lowercase name, which is both what serde parses from
    /// `bombyx.toml` and what `Vagrant.configure` expects.
    ///
    /// One place, not two. If a separate method produced the
    /// name for Vagrant, the two spellings could drift apart
    /// and a config value would stop matching what gets
    /// written into the Vagrantfile.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Libvirt => "libvirt",
            Self::Hyperv => "hyperv",
        })
    }
}

/// The machine bombyx builds, as `[vm]` in `bombyx.toml`.
///
/// Every field is required. None of them has a defensible
/// default: the base image is the one thing bombyx cannot
/// invent, and a size bombyx chose would be wrong on both a
/// laptop and a workstation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vm {
    /// Virtualization backend.
    pub provider: Provider,
    /// Vagrant box the VM boots from, e.g.
    /// `generic/ubuntu2204`.
    ///
    /// Named `box_name` because `box` is a Rust keyword.
    #[serde(rename = "box")]
    pub box_name: String,
    /// Virtual CPUs. Must be at least one.
    pub cpus: u32,
    /// Memory in MiB. Must be at least one.
    pub memory: u32,
}

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
/// What we are guarding against: `git` supports "remote
/// helpers", which are addresses written as `name::rest`. One
/// of them is `ext::`, and it tells `git` to *run* the rest as
/// a shell command. So `ext::sh -c "..."` looks like an address
/// and is really an instruction, and it would run inside the
/// guest VM as root, before any of the project's own code
/// exists. [`RepoUrl::parse`] refuses it.
///
/// You might reach for the `url` crate here. Two reasons not
/// to. First, we accept `git@github.com:you/repo.git`, which is
/// the usual way to write an SSH address for `git`, and it is
/// not a valid URL -- a URL parser rejects it. Second, bombyx
/// never looks at the pieces of the address. It passes the
/// whole thing to `git` and writes it into the Vagrantfile, so
/// splitting it into scheme, host and path would buy nothing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct RepoUrl(String);

impl RepoUrl {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Empty`] when `raw` is blank, and
    /// [`ConfigError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, would
    /// be read by `git` as an option, or names a remote helper
    /// rather than a repository.
    pub fn parse(raw: String) -> Result<Self, ConfigError> {
        check_renderable("repo", &raw)?;
        check_not_an_option("repo", &raw)?;
        check_repo_url(&raw)?;
        Ok(Self(raw))
    }

    /// The value, as `git` and the Vagrantfile see it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoUrl {
    type Error = ConfigError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// A path inside the cloned project, on the guest.
///
/// A newtype for the same reason as [`RepoUrl`]: the checks
/// live in [`ScriptPath::parse`], so holding one of these means
/// it has already been checked.
///
/// You would expect `PathBuf` here, and it is the wrong choice.
/// `PathBuf` behaves differently depending on which operating
/// system your program was compiled for. On Windows it treats
/// `\` as a folder separator and understands drive letters like
/// `C:`; on Linux it does neither. But this path is never used
/// on the machine running bombyx. It is sent to the guest VM
/// and resolved there, and the guest is always Linux. So a
/// `PathBuf` would answer questions about the wrong computer,
/// and answer them differently depending on who ran
/// `bombyx up`.
///
/// `std::os::unix` is no help either: those modules only exist
/// when you are compiling *for* Unix, so they cannot describe
/// another platform's paths from a Windows build.
///
/// Since bombyx only ever checks this value and passes it
/// along -- it never joins, splits or resolves it -- a plain
/// checked string is the honest representation.
///
/// `vagrant_dir` is the field that really is a local path, and
/// it does become a `PathBuf` -- but only where it is used,
/// joined onto the current directory, not here in the config.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ScriptPath(String);

impl ScriptPath {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Empty`] when `raw` is blank, and
    /// [`ConfigError::Invalid`] when it begins or ends with
    /// whitespace, would break the generated Vagrantfile, would
    /// be read by `git` as an option, or leaves the clone
    /// directory.
    pub fn parse(raw: String) -> Result<Self, ConfigError> {
        check_renderable("script", &raw)?;
        check_not_an_option("script", &raw)?;
        check_script_path(&raw)?;
        Ok(Self(raw))
    }

    /// The value, as the guest's shell sees it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ScriptPath {
    type Error = ConfigError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// Refuses a value that would break the Vagrantfile we write,
/// or arrive somewhere with whitespace nobody meant.
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
/// We could escape the four characters instead of refusing
/// them. Refusing is better: a box name, a repository address,
/// a branch name and a relative path have no reason to contain
/// any of them, so allowing them would only give the renderer
/// more to get right.
fn check_renderable(
    field: &'static str,
    value: &str,
) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::Empty { field });
    }
    // Surrounding whitespace is almost always a copy-paste
    // artifact, and every one of these fields fails obscurely
    // with it. A trailing space on `repo` reaches the guest and
    // comes back as `repository '...' does not exist`; a
    // leading one makes `git` read the value as a local path
    // and name nothing recognisable. Catching it here means the
    // operator sees it before anything is pushed.
    if value.trim() != value {
        return Err(ConfigError::Invalid {
            field,
            reason: "must not begin or end with whitespace".to_owned(),
        });
    }
    if let Some(bad) = value.chars().find(|c| c.is_control()) {
        // Split from the quote case because the mechanism
        // differs: a BEL or a tab neither ends nor escapes a
        // Ruby literal, and telling an operator it does sends
        // them hunting a quoting problem they do not have.
        return Err(ConfigError::Invalid {
            field,
            reason: format!(
                "control character {bad:?} is not allowed; use \
                 printable characters only"
            ),
        });
    }
    if let Some(bad) = value.chars().find(|c| *c == '"' || *c == '\\') {
        return Err(ConfigError::Invalid {
            field,
            reason: format!(
                "character {bad:?} is not allowed; it would end \
                 or escape the string in the generated Vagrantfile"
            ),
        });
    }
    if value.contains("#{") {
        return Err(ConfigError::Invalid {
            field,
            reason: "`#{` is Ruby interpolation and would be \
                     evaluated in the generated Vagrantfile"
                .to_owned(),
        });
    }
    Ok(())
}

/// Refuses a value `git` would treat as an option, not a value.
///
/// Command-line tools tell options apart from ordinary values
/// by the leading `-`. So if a config value starts with `-` and
/// we pass it to `git`, `git` reads it as an instruction to
/// itself.
///
/// This is the second of two guards, not the only one. The
/// guest runs `git fetch --depth 1 origin -- "$BOMBYX_REF"`,
/// and that `--` already tells `git` that whatever follows is a
/// value rather than an option.
///
/// It is kept anyway, because `git` accepts options *after*
/// positional arguments -- which is easy to miss, since many
/// tools do not. So a command that forgets the `--`, here or in
/// a future one bombyx composes, would read
/// `--upload-pack=/bin/sh` as an instruction naming a program
/// to run on the other end rather than as a branch name.
///
/// `project`, `vagrant_dir` and `remote_root` need the same
/// rule, for the same reason. If you add a field whose value
/// reaches a command line, it needs this too.
fn check_not_an_option(
    field: &'static str,
    value: &str,
) -> Result<(), ConfigError> {
    if value.starts_with('-') {
        return Err(ConfigError::Invalid {
            field,
            reason: "must not start with `-`, which git reads as \
                     an option"
                .to_owned(),
        });
    }
    Ok(())
}

/// Refuses a repository address `git` would run as a command.
///
/// See [`RepoUrl`] for the `ext::` problem this exists to stop.
///
/// The approach is an allowlist: instead of trying to name
/// every dangerous spelling, we name the safe ones and refuse
/// everything else. That is the safer direction, because a
/// spelling nobody thought of is refused by default rather than
/// allowed by default.
///
/// Two shapes are allowed. One is a normal URL starting with a
/// scheme we recognise. The other is the SSH shorthand
/// `git@github.com:you/repo.git`, which has no `://` at all --
/// it is a host, a colon, then a path. That is what `scp_like`
/// below is looking for, and it refuses any `::`, so
/// `ext::something` cannot slip through as "a host called ext
/// with an empty path".
fn check_repo_url(value: &str) -> Result<(), ConfigError> {
    const ALLOWED: [&str; 4] = ["https://", "http://", "ssh://", "git://"];
    let scp_like =
        !value.contains("://") && value.contains(':') && !value.contains("::");
    if ALLOWED.iter().any(|p| value.starts_with(p)) || scp_like {
        return Ok(());
    }
    Err(ConfigError::Invalid {
        field: "repo",
        reason: "must be an https, http, ssh or git URL, or \
                 `user@host:path`; a `<transport>::<rest>` \
                 remote helper such as `ext::` runs a command \
                 rather than cloning"
            .to_owned(),
    })
}

/// Refuses a script path that points outside the clone.
///
/// The guest changes into the cloned project, runs `chmod +x`
/// on this path, and then executes it as root. So whatever this
/// names is about to be given the run of the machine.
///
/// Two ways a value escapes the clone, and both are refused:
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
fn check_script_path(value: &str) -> Result<(), ConfigError> {
    let bad = if value.starts_with('/') {
        Some("must be relative to the clone root")
    } else if value.split('/').any(|s| s == "..") {
        Some("must not contain a `..` segment")
    } else {
        None
    };
    match bad {
        Some(reason) => Err(ConfigError::Invalid {
            field: "script",
            reason: reason.to_owned(),
        }),
        None => Ok(()),
    }
}

/// Checks the `[vm]` and `[source]` values that types cannot.
///
/// One function, called from one place, so nobody can run half
/// the checks by accident.
pub(super) fn validate(vm: &Vm, source: &Source) -> Result<(), ConfigError> {
    // `repo` and `script` are not checked here, and do not need
    // to be. They are `RepoUrl` and `ScriptPath`, and those
    // types run their checks when they are built, so one that
    // exists at all is one that passed. See `RepoUrl` for how
    // that works.
    //
    // `box` and `ref` are checked here instead, because their
    // rules are generic ones any string field would need, so a
    // type wrapping them would promise nothing extra.
    //
    // Why that line falls where it does, and what the weaker
    // guarantee costs, is argued once in
    // `docs/architecture.md`, under "What config values are
    // checked". Not repeated here: two copies of an argument
    // drift.
    for (field, value) in [("box", &vm.box_name), ("ref", &source.git_ref)] {
        check_renderable(field, value)?;
    }

    // Only `ref` reaches a command line. `box` does not:
    // vagrant resolves it, and it never becomes an argument
    // bombyx composes.
    check_not_an_option("ref", &source.git_ref)?;

    // A machine with no CPU or no memory is refused here rather
    // than by vagrant, which would report it on the VM host
    // after the push has already changed state.
    for (field, value) in [("cpus", vm.cpus), ("memory", vm.memory)] {
        if value == 0 {
            return Err(ConfigError::Invalid {
                field,
                reason: "must be at least 1".to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm() -> Vm {
        Vm {
            provider: Provider::Libvirt,
            box_name: "generic/ubuntu2204".to_owned(),
            cpus: 2,
            memory: 2048,
        }
    }

    fn source() -> Source {
        Source {
            repo: RepoUrl::parse("https://example.invalid/p.git".to_owned())
                .expect("a valid fixture URL"),
            git_ref: "main".to_owned(),
            script: ScriptPath::parse("vagrant/provision.sh".to_owned())
                .expect("a valid fixture path"),
        }
    }

    #[test]
    fn a_repo_url_refuses_what_git_would_not_clone() {
        // The constructor holds every rule, so an invalid
        // `RepoUrl` cannot be built at all -- not by a config
        // file and not by a library caller.
        //
        // Each case names the rule it must trip, so a case that
        // starts failing for a different reason shows up as a
        // failure instead of passing quietly.
        //
        // Naming the rule is what makes this suite notice if
        // the leading-dash check is deleted. Without it, `-x`
        // would still be refused -- by the URL check, which
        // catches it for a different reason -- and a test
        // asserting only "this is refused" would stay green
        // while the rule it covered was gone.
        //
        // `-oProxyCommand=id:x` is stronger still. One colon,
        // no `://`, so the URL check reads it as the SSH
        // shorthand `host:path` and accepts it outright. Delete
        // the dash check and that value is not refused at all.
        for (bad, reason) in [
            ("ext::sh -c 'id > /pwned'", "must be an https"),
            ("fd::7", "must be an https"),
            ("not-a-url", "must be an https"),
            ("", "must not be empty"),
            ("   ", "must not be empty"),
            (" https://example.invalid/p.git", "whitespace"),
            ("https://example.invalid/p.git ", "whitespace"),
            ("-x", "git reads as an option"),
            ("-oProxyCommand=id:x", "git reads as an option"),
            ("--upload-pack=/bin/sh:x", "git reads as an option"),
            ("https://example.invalid/a\"b", "would end or escape"),
        ] {
            let err = RepoUrl::parse(bad.to_owned())
                .expect_err("must be refused")
                .to_string();
            assert!(
                err.contains(reason),
                "{bad:?}: expected {reason:?}, got {err}"
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
            let parsed = RepoUrl::parse(good.to_owned())
                .unwrap_or_else(|e| panic!("{good:?}: {e}"));
            assert_eq!(parsed.as_str(), good);
        }
    }

    #[test]
    fn a_script_path_must_stay_inside_the_clone() {
        // Whatever this names is about to be made executable and
        // run as root in the guest.
        //
        // Each case names the rule it must trip, not just "this
        // is refused". Asserting only `is_err()` hides which
        // check fired, and the two are easy to confuse:
        // `\windows\x` looks like a path rule and is actually
        // caught by the character rule, because a backslash
        // would escape the next character in the generated Ruby.
        for (bad, reason) in [
            ("/usr/bin/env", "relative to the clone root"),
            ("../../usr/bin/env", "`..` segment"),
            ("a/../../../etc/x", "`..` segment"),
            (" provision.sh", "whitespace"),
            ("provision.sh ", "whitespace"),
            ("-x", "git reads as an option"),
            ("", "must not be empty"),
            ("a\"b", "would end or escape"),
            ("a\\b", "would end or escape"),
        ] {
            let err = ScriptPath::parse(bad.to_owned())
                .expect_err("must be refused")
                .to_string();
            assert!(
                err.contains(reason),
                "{bad:?}: expected {reason:?}, got {err}"
            );
        }

        let ok = ScriptPath::parse("vagrant/provision.sh".to_owned())
            .expect("a plain relative path");
        assert_eq!(ok.as_str(), "vagrant/provision.sh");
    }

    #[test]
    fn provider_renders_the_name_config_and_vagrant_both_use() {
        assert_eq!(Provider::Libvirt.to_string(), "libvirt");
        assert_eq!(Provider::Hyperv.to_string(), "hyperv");
    }

    #[test]
    fn refuses_a_ref_git_would_read_as_an_option() {
        // `ref` is the one `[source]` field `validate` checks.
        // `repo` and `script` are newtypes, so their rules live
        // in their constructors and the tests above cover them.
        //
        // `bootstrap.sh` passes `--` before the ref, so this is
        // the second of two guards rather than the only one.
        // See `check_not_an_option` for why both are kept.
        for bad in ["-x", "--upload-pack=/bin/sh", "--exec=x"] {
            let mut s = source();
            s.git_ref = bad.to_owned();
            let err = validate(&vm(), &s).unwrap_err();
            assert!(
                matches!(&err, ConfigError::Invalid { field: "ref", .. }),
                "ref must refuse {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn a_control_character_is_reported_as_one() {
        // Separate message from the quote case: a BEL neither
        // ends nor escapes a Ruby literal, and saying it does
        // sends an operator hunting a quoting problem.
        let mut s = source();
        s.git_ref = "ma\u{7}in".to_owned();
        let err = validate(&vm(), &s).unwrap_err();
        let ConfigError::Invalid { reason, .. } = &err else {
            panic!("{err:?}");
        };
        assert!(reason.contains("control character"), "{reason}");
    }
}
