//! A project entry's `[vm]` table: what machine to build.
//!
//! What the guest clones into that machine is the `[source]`
//! table, in `super::source`.
//!
//! Every value in the table is a type that checks itself:
//! [`Provider`] is an enum, [`BoxName`] is a newtype in the
//! shape `super::source::RepoUrl` describes, and `cpus` and
//! `memory` are `NonZeroU32`. So a `Vm` that exists at all is
//! one whose values passed, and there is no separate function
//! to remember to call.
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
use std::num::NonZeroU32;

use serde::Deserialize;

use super::error::FieldError;
use super::guards::check_renderable;

/// The virtualization backend the generated Vagrantfile targets.
///
/// An enum rather than a string so an unknown value fails while
/// the config is being read. A string would reach the VM host,
/// render a Vagrantfile no `vagrant` can use, and report it only
/// after bombyx had already created a directory there.
///
/// `#[serde(rename_all = "lowercase")]` is what lets
/// `provider = "libvirt"` in the TOML select the `Libvirt`
/// variant: without it serde matches the Rust spelling, and the
/// operator would have to write `"Libvirt"`.
///
/// `Libvirt` is the default, so an absent `provider` key means
/// libvirt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// libvirt via `vagrant-libvirt`. The only provider bombyx
    /// has ever booted a machine with.
    #[default]
    Libvirt,
    /// Hyper-V. **Never exercised** -- written from the
    /// provider's documented options, not from a run.
    Hyperv,
}

impl Provider {
    /// The lowercase name, which is what serde parses from the
    /// config file, what `Vagrant.configure` expects, and what
    /// bombyx passes to `vagrant` in the environment.
    ///
    /// One method produces the name for all three readers. If a
    /// second method produced the Vagrant spelling, the two
    /// could drift apart, and a config value would stop matching
    /// what gets written into the Vagrantfile.
    ///
    /// A borrow rather than a `String`, matching `RepoUrl`,
    /// `ScriptPath` and `HostName`. Both words are compile-time
    /// constants, so a caller quoting one for a shell has
    /// nothing to allocate.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Libvirt => "libvirt",
            Self::Hyperv => "hyperv",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The machine bombyx builds, as a project's `[vm]` table.
///
/// Every field but `provider` is required. None of the other
/// three has a defensible default: the base image is the one
/// thing bombyx cannot invent, and a size bombyx chose would be
/// wrong on both a laptop and a workstation.
///
/// `#[serde(deny_unknown_fields)]` makes a key serde does not
/// recognise an error instead of something quietly ignored. It
/// is what turns `cpu = 2` into a message naming `cpu`, rather
/// than a VM built with whatever `cpus` defaults to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vm {
    /// Virtualization backend. Defaults to
    /// [`Provider::Libvirt`] when the key is absent.
    #[serde(default)]
    pub provider: Provider,
    /// Vagrant box the VM boots from, e.g.
    /// `generic/ubuntu2204`.
    ///
    /// Named `box_name` because `box` is a Rust keyword.
    #[serde(rename = "box")]
    pub box_name: BoxName,
    /// Virtual CPUs.
    ///
    /// A `NonZeroU32` rather than a `u32`, so serde refuses a
    /// `0` while the config is being read and names the key
    /// that carried it. See [`Vm::memory`].
    pub cpus: NonZeroU32,
    /// Memory in MiB.
    ///
    /// A `NonZeroU32` for the same reason as [`Vm::cpus`]. A
    /// machine with no memory is refused here rather than by
    /// vagrant, which would report it on the VM host after
    /// bombyx had already created a directory there.
    ///
    /// The standard type is used rather than a newtype of our
    /// own because the only rule either field has is a floor of
    /// one, which is exactly what `NonZeroU32` means.
    pub memory: NonZeroU32,
}

/// A Vagrant box name that will not break the Vagrantfile.
///
/// A *newtype*: a struct wrapping one private `String`,
/// buildable only through [`BoxName::parse`], which checks the
/// value first. `super::source::RepoUrl` explains the pattern
/// in full and is the one to read.
///
/// The value is written into the generated Vagrantfile inside
/// double quotes, so it gets the Ruby-literal rules that
/// `super::guards::check_renderable` holds. It does not reach a
/// command line: vagrant resolves the box itself, and the name
/// never becomes an argument bombyx composes.
///
/// `#[serde(try_from = "String")]` is what makes the check run
/// while the config file is being read. Without it serde
/// assigns the private field directly and [`BoxName::parse`]
/// never runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct BoxName(String);

impl BoxName {
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace or would break the generated Vagrantfile.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        check_renderable("box", raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// The value, as the Vagrantfile sees it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BoxName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for BoxName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BoxName {
    type Error = FieldError;

    /// What serde calls. It already owns the `String`, so the
    /// check runs against a borrow and the value moves into the
    /// newtype rather than being copied again.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_renderable("box", &raw)?;
        Ok(Self(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_renders_the_name_config_and_vagrant_both_use() {
        assert_eq!(Provider::Libvirt.as_str(), "libvirt");
        assert_eq!(Provider::Hyperv.as_str(), "hyperv");
        // `Display` must not grow a spelling of its own, so it
        // is asserted against the same two words rather than
        // trusted to delegate.
        assert_eq!(Provider::Libvirt.to_string(), "libvirt");
        assert_eq!(Provider::Hyperv.to_string(), "hyperv");
    }

    #[test]
    fn a_box_name_refuses_a_value_that_would_break_the_ruby() {
        // The name is written into the Vagrantfile inside
        // double quotes, so a quote ends the string early and a
        // backslash escapes whatever follows.
        for bad in ["generic/ubu\"ntu", "generic\\ubuntu"] {
            let err = BoxName::parse(bad).expect_err("must be refused");
            assert!(
                err.to_string().contains("would end or escape"),
                "{bad:?}: {err}"
            );
        }
    }

    #[test]
    fn a_control_character_in_a_box_name_is_reported_as_one() {
        // Separate message from the quote case: a BEL neither
        // ends nor escapes a Ruby literal, and saying it does
        // sends an operator hunting a quoting problem.
        let err =
            BoxName::parse("generic/ubu\u{7}ntu").expect_err("must be refused");
        let FieldError::Invalid { reason, .. } = &err else {
            panic!("{err:?}");
        };
        assert!(reason.contains("control character"), "{reason}");
    }

    #[test]
    fn a_box_name_refuses_a_blank_value() {
        for bad in ["", "   "] {
            let err = BoxName::parse(bad).expect_err("must be refused");
            // The field name is what tells an operator which key
            // to edit, so it is asserted alongside the rule.
            assert!(err.to_string().contains("box"), "{err}");
            assert!(err.to_string().contains("must not be empty"), "{err}");
        }
    }

    #[test]
    fn a_box_name_keeps_the_value_it_was_given() {
        let name =
            BoxName::parse("generic/ubuntu2204").expect("a plain box name");
        assert_eq!(name.as_str(), "generic/ubuntu2204");
        assert_eq!(name.as_ref(), "generic/ubuntu2204");
        assert_eq!(name.to_string(), "generic/ubuntu2204");
    }

    #[test]
    fn serde_runs_the_box_name_check_while_the_table_is_read() {
        // `try_from` is a second entry point into the type, and
        // an attribute is easy to drop. Without it serde would
        // assign the private field and no check would run at
        // all, so this asserts against the deserializer rather
        // than against `parse`.
        let err = toml::from_str::<Vm>(
            "box = \"gen\\\"eric\"\ncpus = 2\nmemory = 2048\n",
        )
        .expect_err("must be refused");
        assert!(err.to_string().contains("would end or escape"), "{err}");
    }

    #[test]
    fn serde_refuses_a_machine_with_no_cpu_or_no_memory() {
        // vagrant would refuse these too, but only on the VM
        // host, after bombyx has already created a directory
        // there. `NonZeroU32` is what moves the refusal to the
        // moment the operator's file is read.
        for (bad, key) in [
            ("box = \"b\"\ncpus = 0\nmemory = 2048\n", "cpus"),
            ("box = \"b\"\ncpus = 2\nmemory = 0\n", "memory"),
        ] {
            let err = toml::from_str::<Vm>(bad).expect_err("must be refused");
            let msg = err.to_string();
            assert!(msg.contains(key), "{msg}");
            assert!(msg.contains("nonzero"), "{msg}");
        }
    }
}
