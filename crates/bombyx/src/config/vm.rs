//! The `[vm]` table of `bombyx.toml`: what machine to build.
//!
//! What the guest clones into that machine is the `[source]`
//! table, in `super::source`. The two tables have one function
//! in common -- [`validate`], which runs the checks that a type
//! cannot express -- and it lives here because `[vm]` has no
//! checked types of its own to carry it.
//!
//! A single config value can end up in three different places,
//! and each one can be attacked differently:
//!
//! - Written into the Vagrantfile, which is a Ruby file.
//! - Passed to `git` on the command line, inside the guest.
//! - Used as a path that gets made executable and run as root,
//!   also inside the guest.
//!
//! So "is this string safe" has no single answer. It depends
//! on which of the three you mean, and a value can be fine for
//! one and dangerous for another. That is why several checks
//! run against the same value.

use std::fmt;

use serde::Deserialize;

use super::error::FieldError;
use super::guards::check_renderable;

/// The virtualization backend the generated Vagrantfile targets.
///
/// An enum rather than a string so an unknown value fails while
/// the config is being read. A string would reach the VM host,
/// render a Vagrantfile no `vagrant` can use, and report it only
/// after the push had already changed state there.
///
/// `#[serde(rename_all = "lowercase")]` is what lets
/// `provider = "libvirt"` in the TOML select the `Libvirt`
/// variant: without it serde matches the Rust spelling, and the
/// operator would have to write `"Libvirt"`.
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
    /// One method produces the name for both readers. If a
    /// separate method produced the Vagrant spelling, the two
    /// could drift apart, and a config value would stop matching
    /// what gets written into the Vagrantfile.
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
///
/// `#[serde(deny_unknown_fields)]` makes a key serde does not
/// recognise an error instead of something quietly ignored. It
/// is what turns `cpu = 2` into a message naming `cpu`, rather
/// than a VM built with whatever `cpus` defaults to.
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

/// Checks the `[vm]` values that types cannot.
///
/// `box` is checked here rather than wrapped in a type, and so
/// are `cpus` and `memory`, whose only rule is a floor. All
/// three are gaps rather than decisions. `Vm` has public
/// fields and `Config::validate` is private, so a hand-built
/// `Vm` never reaches this function and has no way to ask for
/// it. A constructor could not be gone around that way.
///
/// What the weaker guarantee costs is argued once in
/// `docs/architecture.md` under "What config values are
/// checked". Two copies of an argument drift, so it is not
/// repeated here. The work is
/// `newtype-remaining-config-fields` in `docs/todo.md`.
pub(super) fn validate(vm: &Vm) -> Result<(), FieldError> {
    // `box` reaches the generated Vagrantfile, which is a Ruby
    // file, so it gets the Ruby-literal rules. It does not reach
    // a command line: vagrant resolves it, and it never becomes
    // an argument bombyx composes.
    check_renderable("box", &vm.box_name)?;

    // A machine with no CPU or no memory is refused here rather
    // than by vagrant, which would report it on the VM host
    // after the push has already changed state.
    for (field, value) in [("cpus", vm.cpus), ("memory", vm.memory)] {
        if value == 0 {
            return Err(FieldError::Invalid {
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

    #[test]
    fn provider_renders_the_name_config_and_vagrant_both_use() {
        assert_eq!(Provider::Libvirt.to_string(), "libvirt");
        assert_eq!(Provider::Hyperv.to_string(), "hyperv");
    }

    #[test]
    fn a_control_character_in_box_is_reported_as_one() {
        // Separate message from the quote case: a BEL neither
        // ends nor escapes a Ruby literal, and saying it does
        // sends an operator hunting a quoting problem.
        let mut v = vm();
        v.box_name = "generic/ubu\u{7}ntu".to_owned();
        let err = validate(&v).unwrap_err();
        let FieldError::Invalid { reason, .. } = &err else {
            panic!("{err:?}");
        };
        assert!(reason.contains("control character"), "{reason}");
    }

    #[test]
    fn a_machine_with_no_cpu_is_refused() {
        // vagrant would refuse this too, but only on the VM
        // host, after the push has already changed state there.
        let mut v = vm();
        v.cpus = 0;
        let err = validate(&v).unwrap_err();
        assert!(
            matches!(&err, FieldError::Invalid { field: "cpus", .. }),
            "{err:?}"
        );
    }

    #[test]
    fn a_machine_with_no_memory_is_refused() {
        let mut v = vm();
        v.memory = 0;
        let err = validate(&v).unwrap_err();
        assert!(
            matches!(
                &err,
                FieldError::Invalid {
                    field: "memory",
                    ..
                }
            ),
            "{err:?}"
        );
    }
}
