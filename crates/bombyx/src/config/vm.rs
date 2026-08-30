//! The machine description and where the guest fetches the
//! project: `[vm]` and `[source]` in `bombyx.toml`.
//!
//! Separate from the parent module because these types and
//! their checks are one theme with one entry point, and
//! `config.rs` already held six other types.
//!
//! The checks here guard three different consumers, which is
//! why one rule is not enough. `box` and the three `[source]`
//! fields become Ruby string literals in the generated
//! Vagrantfile; `repo` and `ref` also become `git` arguments
//! inside the guest; and `script` also becomes a path that is
//! made executable and run there, as root.

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
    /// One mapping rather than two. A separate `vagrant_name`
    /// method existed briefly, and two names for one mapping
    /// only invite them to drift.
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
    /// Repository URL, cloned inside the guest.
    pub repo: String,
    /// Branch or tag to clone.
    ///
    /// Named `git_ref` because `ref` is a Rust keyword.
    #[serde(rename = "ref")]
    pub git_ref: String,
    /// Provisioning script to run, relative to the clone root.
    pub script: String,
}

/// Requires a value that survives being rendered into Ruby.
///
/// An empty or whitespace-only value is refused first, as
/// [`ConfigError::Empty`], because none of these fields has a
/// meaning when blank.
///
/// `box`, `repo`, `ref` and `script` all become Ruby string
/// literals in the Vagrantfile bombyx writes. Four shapes break
/// that file, and the whole family is refused rather than only
/// the one that prompted the guard: a double quote ends the
/// literal early, a backslash starts an escape, a control
/// character (a newline included) ends the line, and `#{` is
/// Ruby's interpolation, which is evaluated rather than printed.
///
/// Escaping these instead of refusing them was the alternative.
/// Refusing is better here because none of the four has any
/// business in a box name, a clone URL, a branch or a relative
/// script path, so accepting them would only widen what the
/// renderer has to get right.
fn check_renderable(
    field: &'static str,
    value: &str,
) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::Empty { field });
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

/// Requires a value `git` will not read as an option.
///
/// The same rule `project`, `vagrant_dir` and `remote_root`
/// already carry, applied to the fields that reach `git` argv
/// inside the guest. `git` permutes options after positionals,
/// so `ref = "--upload-pack=..."` is read as an option by
/// `git fetch --depth 1 origin "$BOMBYX_REF"` rather than as a
/// branch.
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

/// Requires a repository URL `git` will fetch rather than
/// execute.
///
/// `git` supports remote helpers spelled `<transport>::<rest>`,
/// and `ext::` runs its argument as a shell command. So a
/// `repo` that looks like a clone URL can be a command, and it
/// runs in the guest before any project code exists. An
/// allowlist of the four spellings a person actually writes is
/// narrower than trying to name every helper.
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

/// Requires a script path that stays inside the clone.
///
/// `script` is joined onto the clone root, made executable and
/// run as root, so the same reasoning as `vagrant_dir` applies:
/// a rooted or traversing value points that at something the
/// project does not own.
fn check_script_path(value: &str) -> Result<(), ConfigError> {
    let bad = if value.starts_with('/') || value.starts_with('\\') {
        Some("must be relative to the clone root")
    } else if value.split('/').any(|s| s == "..") {
        Some("must not contain a `..` segment")
    } else if value.trim() != value {
        Some("must not begin or end with whitespace")
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

/// Checks `box`, `repo`, `ref`, `script`, `cpus` and `memory`.
///
/// The single entry point, so no caller can reach one family of
/// checks without the others.
pub(super) fn validate(vm: &Vm, source: &Source) -> Result<(), ConfigError> {
    // The four string fields. They become Ruby string literals,
    // so they all get the character check.
    for (field, value) in [
        ("box", &vm.box_name),
        ("repo", &source.repo),
        ("ref", &source.git_ref),
        ("script", &source.script),
    ] {
        check_renderable(field, value)?;
    }

    // The three that also reach `git` or the guest filesystem.
    // `box` is not among them: vagrant resolves it, and it never
    // becomes an argument bombyx composes.
    for (field, value) in [
        ("repo", &source.repo),
        ("ref", &source.git_ref),
        ("script", &source.script),
    ] {
        check_not_an_option(field, value)?;
    }
    check_repo_url(&source.repo)?;
    check_script_path(&source.script)?;

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
            repo: "https://example.invalid/p.git".to_owned(),
            git_ref: "main".to_owned(),
            script: "vagrant/provision.sh".to_owned(),
        }
    }

    #[test]
    fn provider_renders_the_name_config_and_vagrant_both_use() {
        assert_eq!(Provider::Libvirt.to_string(), "libvirt");
        assert_eq!(Provider::Hyperv.to_string(), "hyperv");
    }

    #[test]
    fn refuses_values_git_would_read_as_options() {
        // The whole family, because `git` permutes options after
        // positionals: a leading `-` in any of the three reaches
        // an argument position in the guest. `project`,
        // `vagrant_dir` and `remote_root` already carry this
        // rule; these three did not, which is the sibling
        // oversight CLAUDE.md names.
        for bad in ["-x", "--upload-pack=/bin/sh", "--exec=x"] {
            for field in ["repo", "ref", "script"] {
                let mut s = source();
                match field {
                    "repo" => s.repo = bad.to_owned(),
                    "ref" => s.git_ref = bad.to_owned(),
                    _ => s.script = bad.to_owned(),
                }
                let err = validate(&vm(), &s).unwrap_err();
                assert!(
                    matches!(&err, ConfigError::Invalid { field: f, .. }
                        if *f == field),
                    "{field} must refuse {bad:?}, got {err:?}"
                );
            }
        }
    }

    #[test]
    fn refuses_a_repo_that_runs_a_command_instead_of_cloning() {
        // `git` remote helpers are spelled `<transport>::<rest>`
        // and `ext::` runs its argument as a shell command, in
        // the guest, as root, before any project code exists.
        for bad in [
            "ext::sh -c 'id > /pwned'",
            "ext::whoami",
            "fd::7",
            "not-a-url",
            "",
        ] {
            let mut s = source();
            s.repo = bad.to_owned();
            assert!(validate(&vm(), &s).is_err(), "repo must refuse {bad:?}");
        }
    }

    #[test]
    fn accepts_the_url_spellings_people_actually_write() {
        for good in [
            "https://github.com/breki/bombyx",
            "http://example.invalid/p.git",
            "ssh://git@example.invalid/p.git",
            "git://example.invalid/p.git",
            "git@github.com:breki/bombyx.git",
        ] {
            let mut s = source();
            s.repo = good.to_owned();
            assert!(validate(&vm(), &s).is_ok(), "repo must accept {good:?}");
        }
    }

    #[test]
    fn refuses_a_script_path_that_leaves_the_clone() {
        // `script` is made executable and run as root inside the
        // guest, so the same reasoning as `vagrant_dir`: rooted
        // or traversing values point that at something the
        // project does not own.
        for bad in [
            "/usr/bin/env",
            "\\\\windows\\\\x",
            "../../usr/bin/env",
            "a/../../../etc/x",
            " provision.sh",
            "provision.sh ",
        ] {
            let mut s = source();
            s.script = bad.to_owned();
            let err = validate(&vm(), &s).unwrap_err();
            assert!(
                matches!(
                    &err,
                    ConfigError::Invalid {
                        field: "script",
                        ..
                    }
                ),
                "script must refuse {bad:?}, got {err:?}"
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
