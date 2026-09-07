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
//! - Used as a path that the agent's own user makes
//!   executable and then runs, also inside the guest.
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
use crate::newtype::checked_str_newtype;

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
    /// Virtual CPUs. Never zero.
    ///
    /// `NonZeroU32` is what makes a zero unrepresentable, so a
    /// caller assigning to this public field gets the same rule
    /// the config file got. What the type does *not* do is name
    /// the key when it refuses one, which is why serde reads it
    /// through `positive_cpus` -- named rather than linked,
    /// because it is private and rustdoc refuses a public page
    /// pointing at one.
    #[serde(deserialize_with = "positive_cpus")]
    pub cpus: NonZeroU32,
    /// Memory in MiB. Never zero.
    ///
    /// A machine with no memory is refused while the config is
    /// read rather than by vagrant, which would report it on the
    /// VM host after bombyx had already created a directory
    /// there.
    #[serde(deserialize_with = "positive_memory")]
    pub memory: NonZeroU32,
}

/// Reads `cpus`, refusing a zero with a message naming the key.
///
/// `NonZeroU32` refuses a zero on its own, and the guarantee
/// rests on the type rather than on this function. What the
/// standard type cannot do is say *which* key was wrong: serde
/// produces `invalid value: integer 0, expected a nonzero u32`
/// for it. bombyx prints `toml`'s `message()` rather than its
/// `Display`, because `Display` quotes the source line into the
/// output, and the key appears only in that quoted line. So the
/// two size fields would have been the only config values whose
/// refusal did not say which key to edit.
///
/// Reading a `u32` and rejecting the zero here is what puts the
/// name back. Taking `u32` and not `NonZeroU32` is the whole
/// trick: serde has to be handed the value that may be wrong,
/// or it refuses the zero itself and this code never runs.
///
/// # Errors
///
/// Returns a deserializer error when the value is zero,
/// negative, larger than `u32::MAX`, or not an integer at all.
/// Every one of those names `cpus`; see `at_least_one`.
fn positive_cpus<'de, D>(d: D) -> Result<NonZeroU32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    at_least_one("cpus", d)
}

/// Reads `memory`, refusing a zero with a message naming the
/// key. See [`positive_cpus`].
///
/// # Errors
///
/// Returns a deserializer error for the same values
/// [`positive_cpus`] refuses, naming `memory`.
fn positive_memory<'de, D>(d: D) -> Result<NonZeroU32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    at_least_one("memory", d)
}

/// The rule both size fields share, naming the field that broke
/// it.
///
/// One function rather than the same body twice, so `cpus` and
/// `memory` cannot come to word their refusal differently. The
/// two wrappers above exist only because a serde attribute
/// names a function and cannot pass it an argument.
fn at_least_one<'de, D>(
    field: &'static str,
    d: D,
) -> Result<NonZeroU32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;

    // Every way the value can be wrong goes through one
    // `map_err`, not just the zero. `cpus = -1`, `cpus = "4"`
    // and a value past `u32::MAX` are refused by `u32` itself,
    // and serde's text for those says what is wrong without
    // saying which key carried it. Re-wrapping keeps serde's
    // explanation and puts the field name in front of it.
    let raw = u32::deserialize(d).map_err(|e| named(field, &e.to_string()))?;
    NonZeroU32::new(raw).ok_or_else(|| named(field, "must be at least 1"))
}

/// A deserializer error naming the field, in the wording every
/// other refused config value uses.
///
/// The message is a [`FieldError`], which renders as
/// ``invalid `cpus`: must be at least 1``, and `toml` keeps the
/// position it would have attached anyway.
fn named<E: serde::de::Error>(field: &'static str, reason: &str) -> E {
    serde::de::Error::custom(FieldError::invalid(field, reason))
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
        check_box(raw)?;
        Ok(Self(raw.to_owned()))
    }
}

checked_str_newtype!(BoxName, "The value, as the Vagrantfile sees it.");

impl TryFrom<String> for BoxName {
    type Error = FieldError;

    /// What serde calls. It already owns the `String`, so the
    /// check runs against a borrow and the value moves into the
    /// newtype rather than being copied again.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        check_box(&raw)?;
        Ok(Self(raw))
    }
}

/// Every rule a `box` value must pass, in one place.
///
/// Both [`BoxName::parse`] and [`BoxName::try_from`] call this,
/// so neither can run a different set. `box` reaches the
/// generated Vagrantfile and nothing else, so the Ruby-literal
/// rules are all of them.
fn check_box(value: &str) -> Result<(), FieldError> {
    check_renderable("box", value)
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
    fn every_bad_size_names_the_key_that_carried_it() {
        // A zero is not the only way these two go wrong, and
        // the others are at least as common a typo. Whatever
        // `u32` refuses has to arrive naming `cpus` or `memory`
        // as well, or the operator is told a value is wrong
        // without being told which of the two it was.
        for bad in ["-1", "4294967296", "\"4\"", "2.5"] {
            let src = format!("box = \"b\"\ncpus = {bad}\nmemory = 2048\n");
            let err = toml::from_str::<Vm>(&src).expect_err("must be refused");
            let msg = err.message();
            assert!(msg.starts_with("invalid `cpus`: "), "{bad}: {msg}");
        }
    }

    #[test]
    fn the_refusal_of_a_zero_size_names_the_key_and_the_rule() {
        // The message an operator acts on is the one bombyx
        // prints, which is `FieldError`'s text carried through
        // `toml`. Asserting `toml::de::Error`'s own `Display`
        // instead would pass on a rendering nobody sees: that
        // one echoes the source line, so the key appears in it
        // whether or not the type ever names one.
        for (bad, key) in [
            ("box = \"b\"\ncpus = 0\nmemory = 2048\n", "cpus"),
            ("box = \"b\"\ncpus = 2\nmemory = 0\n", "memory"),
        ] {
            let err = toml::from_str::<Vm>(bad).expect_err("must be refused");
            let msg = err.message();
            assert_eq!(msg, format!("invalid `{key}`: must be at least 1"));
        }
    }
}
