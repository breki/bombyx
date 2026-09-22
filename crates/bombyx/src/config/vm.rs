//! A project entry's `[vm]` table: what machine to build.
//!
//! What the guest clones into that machine is the `[source]`
//! table, in `super::source`.
//!
//! Every value in the table is a type that checks itself:
//! [`Provider`] is an enum, [`BoxName`] is a newtype in the
//! shape `super::source::RepoUrl` describes, `cpus` is a
//! `NonZeroU32`, and `memory` is a [`Memory`] newtype that reads
//! a bare MiB count or a suffixed size like `"6GB"`. So a `Vm`
//! that exists at all is one whose values passed, and there is
//! no separate function to remember to call.
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
use crate::name::ProjectName;
use crate::newtype::{
    checked_str_newtype, checked_str_parse, checked_str_try_from,
};

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
    /// through `positive_cpus`.
    #[serde(deserialize_with = "positive_cpus")]
    pub cpus: NonZeroU32,
    /// Memory the machine gets, held as MiB. Never zero.
    ///
    /// A [`Memory`], so the value is checked while the config is
    /// read: a bare integer is MiB, and a suffixed string like
    /// `"6GB"` or `"512MB"` is converted to MiB first. A zero, a
    /// fraction or an unknown unit is refused there rather than by
    /// vagrant, which would report it on the VM host after bombyx
    /// had already created a directory there.
    pub memory: Memory,

    /// The name the guest answers to, or `None` to derive one.
    ///
    /// Written into the generated Vagrantfile as
    /// `config.vm.hostname`. When the key is absent,
    /// [`Config::vm_hostname`](crate::config::Config::vm_hostname)
    /// derives `<project>-agent` instead, so every guest gets a
    /// name and several agent VMs on one host stay distinguishable
    /// rather than all answering to the box's default.
    ///
    /// A [`Hostname`], so the label rules have run against whatever
    /// is in here. The field is public, which is why the rules
    /// belong to a type rather than to a function a caller has to
    /// remember. This is the guest's own name, unrelated to
    /// `crate::remote::VM_HOSTNAME_ENV`, which carries the VM
    /// host's name into the guest.
    #[serde(default)]
    pub hostname: Option<Hostname>,
}

/// The wording both size fields use to refuse a value below one.
///
/// `cpus` reads it through [`positive_cpus`] and `memory` through
/// [`Memory`], two separate readers. Sharing the one string keeps
/// their wording identical, so a zero of either reads the same.
const AT_LEAST_ONE: &str = "must be at least 1";

/// Reads `cpus`, refusing anything but a positive integer with a
/// message naming the key.
///
/// `NonZeroU32` refuses a zero on its own, and the guarantee
/// rests on the type rather than on this function. What the
/// standard type cannot do is say *which* key was wrong: serde
/// produces `invalid value: integer 0, expected a nonzero u32`
/// for it. bombyx prints `toml`'s `message()` rather than its
/// `Display`, because `Display` quotes the source line into the
/// output, and the key appears only in that quoted line. So
/// `cpus` would have been the one config value whose refusal did
/// not say which key to edit.
///
/// Reading a `u32` and rejecting the zero here is what puts the
/// name back. Taking `u32` and not `NonZeroU32` is the whole
/// trick: serde has to be handed the value that may be wrong,
/// or it refuses the zero itself and this code never runs.
///
/// `memory` does not come through here. It accepts a unit suffix,
/// so it is a [`Memory`] with its own reader; the two share only
/// [`AT_LEAST_ONE`], so a zero of either kind reads the same.
///
/// # Errors
///
/// Returns a deserializer error when the value is zero,
/// negative, larger than `u32::MAX`, or not an integer at all.
/// Every one of those names `cpus`.
fn positive_cpus<'de, D>(d: D) -> Result<NonZeroU32, D::Error>
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
    let raw = u32::deserialize(d).map_err(|e| named("cpus", &e.to_string()))?;
    NonZeroU32::new(raw).ok_or_else(|| named("cpus", AT_LEAST_ONE))
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

/// The memory a project's machine gets, held as MiB.
///
/// A *newtype* wrapping one private [`NonZeroU32`], the MiB count
/// vagrant writes into the Vagrantfile. It is built only through
/// [`Memory::parse`], [`Memory::from_mib`] or serde, each of
/// which guarantees the value is at least one.
///
/// The config file may write the value two ways, and both mean
/// MiB in the end:
///
/// - a bare integer, e.g. `memory = 6144`, which is MiB unchanged;
/// - a quoted size with a unit, e.g. `memory = "6GB"` or
///   `memory = "512MB"`.
///
/// The units are powers of two: `MB` and `MiB` both mean one MiB,
/// `GB` and `GiB` both mean 1024 MiB. That is what an operator
/// sizing a machine means by `6GB`, and it is the unit vagrant
/// itself reads, so the round numbers survive the conversion. A
/// fractional size, an unknown unit and a zero are each refused
/// while the config is read.
///
/// A suffix carries the unit in the value itself, so the sample
/// needs no comment to say what a bare number means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory(NonZeroU32);

impl Memory {
    /// A `Memory` from a MiB count already known to be nonzero.
    ///
    /// Total, because [`NonZeroU32`] already carries the one rule
    /// the type has. This is the constructor a caller with a fixed
    /// size uses; a value read from text goes through
    /// [`Memory::parse`] instead.
    #[must_use]
    pub fn from_mib(mib: NonZeroU32) -> Self {
        Self(mib)
    }

    /// The size in MiB, as vagrant writes it into the Vagrantfile.
    #[must_use]
    pub fn mib(&self) -> u32 {
        self.0.get()
    }

    /// Reads a size written as text, converting any unit to MiB.
    ///
    /// Accepts a bare number (`"6144"`), or a number and a unit
    /// with optional whitespace between them (`"6GB"`, `"512 MB"`),
    /// case-insensitively. `MB`/`MiB` are one MiB each and
    /// `GB`/`GiB` are 1024 MiB each.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Invalid`] naming `memory` when the
    /// text has no leading number, carries a fraction, names a
    /// unit other than MB or GB, overflows `u32` MiB, or works out
    /// to zero.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        let text = raw.trim();
        let split = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        let (number, rest) = text.split_at(split);
        if number.is_empty() {
            return Err(FieldError::invalid(
                "memory",
                "must start with a number, e.g. 6144, 512MB or 6GB",
            ));
        }
        let unit = rest.trim_start();
        let per_unit: u64 = match unit.to_ascii_uppercase().as_str() {
            "" | "MB" | "MIB" => 1,
            "GB" | "GIB" => 1024,
            // A fraction reaches here as the `.` left in the unit
            // slot: `"1.5GB"` arrives with `rest` of `.5GB`. Naming
            // it a whole-number rule is clearer than calling that
            // an unknown unit.
            _ if unit.starts_with('.') => {
                return Err(FieldError::invalid(
                    "memory",
                    "must be a whole number",
                ));
            }
            _ => {
                return Err(FieldError::invalid(
                    "memory",
                    format!("unknown unit `{unit}` -- use MB or GB"),
                ));
            }
        };
        // A digit run too long for `u64`, the multiplication, and
        // the narrowing to `u32` are each a way the value can be
        // too large; all three land on the same message.
        let too_large = || FieldError::invalid("memory", "is too large");
        let mib = number
            .parse::<u64>()
            .map_err(|_| too_large())?
            .checked_mul(per_unit)
            .and_then(|m| u32::try_from(m).ok())
            .ok_or_else(too_large)?;
        NonZeroU32::new(mib)
            .map(Self)
            .ok_or_else(|| FieldError::invalid("memory", AT_LEAST_ONE))
    }

    /// A `Memory` from a bare config integer, which is MiB.
    ///
    /// Separate from [`Memory::parse`] because a bare integer
    /// never carries a unit. A value below one is refused with the
    /// same wording a `"0GB"` gets, and one past `u32::MAX` with
    /// the same wording an oversized suffixed value gets.
    fn from_config_int(mib: i64) -> Result<Self, FieldError> {
        u32::try_from(mib)
            .ok()
            .and_then(NonZeroU32::new)
            .map(Self)
            .ok_or_else(|| {
                let reason = if mib > i64::from(u32::MAX) {
                    "is too large"
                } else {
                    AT_LEAST_ONE
                };
                FieldError::invalid("memory", reason)
            })
    }
}

impl<'de> serde::Deserialize<'de> for Memory {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// Reads the value however TOML typed it: an integer is a
        /// MiB count, a string carries a unit, and a float is a
        /// fraction bombyx refuses.
        struct MemoryVisitor;

        impl<'de> serde::de::Visitor<'de> for MemoryVisitor {
            type Value = Memory;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a memory size in MiB, or a string like \"6GB\"")
            }

            fn visit_i64<E: serde::de::Error>(
                self,
                v: i64,
            ) -> Result<Memory, E> {
                Memory::from_config_int(v).map_err(E::custom)
            }

            // A TOML integer arrives through `visit_i64`; this
            // catches a value past `i64::MAX` from any other
            // deserializer before it wraps negative.
            fn visit_u64<E: serde::de::Error>(
                self,
                v: u64,
            ) -> Result<Memory, E> {
                let mib = i64::try_from(v).map_err(|_| {
                    E::custom(FieldError::invalid("memory", "is too large"))
                })?;
                Memory::from_config_int(mib).map_err(E::custom)
            }

            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> Result<Memory, E> {
                Memory::parse(v).map_err(E::custom)
            }

            fn visit_f64<E: serde::de::Error>(
                self,
                _v: f64,
            ) -> Result<Memory, E> {
                Err(E::custom(FieldError::invalid(
                    "memory",
                    "must be a whole number",
                )))
            }

            // A boolean, an array, a table or a datetime is the
            // wrong TOML type entirely. serde's own message for
            // these names no key, and `cpus` names its key for the
            // same mistakes, so these arms keep memory's refusal in
            // the house style rather than leaving the one field
            // whose error does not say what to edit. A datetime
            // reaches `visit_map`.
            fn visit_bool<E: serde::de::Error>(
                self,
                _v: bool,
            ) -> Result<Memory, E> {
                Err(E::custom(Self::wrong_type()))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                _seq: A,
            ) -> Result<Memory, A::Error> {
                Err(serde::de::Error::custom(Self::wrong_type()))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                _map: A,
            ) -> Result<Memory, A::Error> {
                Err(serde::de::Error::custom(Self::wrong_type()))
            }
        }

        impl MemoryVisitor {
            /// The refusal for a value of the wrong TOML type,
            /// naming `memory` and the two shapes it does accept.
            fn wrong_type() -> FieldError {
                FieldError::invalid(
                    "memory",
                    "must be a number of MiB, or a string like \"6GB\"",
                )
            }
        }

        d.deserialize_any(MemoryVisitor)
    }
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

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it begins or ends with
    /// whitespace or would break the generated Vagrantfile.
    BoxName,
    FieldError,
    check_box
);

checked_str_newtype!(BoxName, "The value, as the Vagrantfile sees it.");

checked_str_try_from!(
    /// What serde calls. It already owns the `String`, so the
    /// check runs against a borrow and the value moves into the
    /// newtype rather than being copied again.
    BoxName,
    FieldError,
    check_box
);

/// Every rule a `box` value must pass, in one place.
///
/// Both [`BoxName::parse`] and [`BoxName::try_from`] call this,
/// so neither can run a different set. `box` reaches the
/// generated Vagrantfile and nothing else, so the Ruby-literal
/// rules are all of them.
fn check_box(value: &str) -> Result<(), FieldError> {
    check_renderable("box", value)
}

/// Longest accepted hostname.
///
/// One DNS label, so the RFC 1123 limit of 63 characters applies.
/// bombyx writes a single label rather than a dotted name, which
/// is what `jutro-agent` -- the value this feature was found
/// converting -- is.
const MAX_HOSTNAME_LEN: usize = 63;

/// The suffix a derived hostname carries, and the width it costs.
///
/// [`Hostname::derived_from`] appends it, so the derivation
/// truncates the project-derived part to leave room for it inside
/// `MAX_HOSTNAME_LEN`.
const HOSTNAME_SUFFIX: &str = "-agent";

/// A validated guest hostname.
///
/// This is the guest's own name, and a different type from
/// `super::host::HostName`, which is the VM host bombyx connects
/// to. The two are one letter's case apart on purpose: `HostName`
/// is the machine that runs the VMs, `Hostname` is what one of
/// those VMs answers to.
///
/// A *newtype* in the shape `super::source::RepoUrl` describes: a
/// struct wrapping one private `String`, buildable only through
/// [`Hostname::parse`], which checks the value first. Holding one
/// is proof it is a single DNS label -- letters, digits and
/// hyphens, no leading or trailing hyphen, not empty and not over
/// `MAX_HOSTNAME_LEN`.
///
/// The value is written into the generated Vagrantfile inside
/// double quotes as `config.vm.hostname`. Its own character set is
/// a subset of what `super::guards::check_renderable` allows -- no
/// quote, backslash or `#` can occur -- so the label rules are the
/// stronger guard and the Ruby-literal rules need no separate
/// check. It reaches no command line: vagrant applies the hostname
/// itself, and the value never becomes an argument bombyx composes.
///
/// `#[serde(try_from = "String")]` is what makes the check run
/// while the config file is being read. Without it serde assigns
/// the private field directly and [`Hostname::parse`] never runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Hostname(String);

checked_str_parse!(
    /// Checks `raw` and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Empty`] when `raw` is blank, and
    /// [`FieldError::Invalid`] when it is longer than
    /// `MAX_HOSTNAME_LEN`, holds a character other than a
    /// letter, digit or hyphen, or begins or ends with a hyphen.
    Hostname,
    FieldError,
    check_hostname
);

checked_str_newtype!(Hostname, "The value, as the Vagrantfile sees it.");

checked_str_try_from!(
    /// What serde calls; see [`BoxName::try_from`].
    Hostname,
    FieldError,
    check_hostname
);

impl Hostname {
    /// A hostname derived from a project name, always valid.
    ///
    /// bombyx uses this when a project sets no `hostname` of its
    /// own: a guest with no name at all answers to the box's
    /// default, so several agent VMs on one host become
    /// indistinguishable.
    ///
    /// The parameter is a [`ProjectName`] rather than a `&str`
    /// because the derivation is total only for what that type
    /// guarantees: a non-empty value whose first character is a
    /// letter or digit. A project name may still hold characters
    /// and lengths a hostname may not -- `.`, `_`, and up to 64
    /// characters -- so it is sanitized rather than used raw. Each
    /// character that is not a letter or digit becomes a hyphen,
    /// letters are lowercased, the result is truncated to leave
    /// room for `-agent` within `MAX_HOSTNAME_LEN`, and any hyphen
    /// left at either end is trimmed before the suffix is joined
    /// on. The first character maps to a letter or digit, so it
    /// survives trimming and the base is never empty, which is why
    /// the final [`Hostname::parse`] cannot fail.
    ///
    /// The mapping is not injective: two project names differing
    /// only in case or in which non-alphanumeric character they
    /// use derive the same hostname. bombyx keys nothing on the
    /// hostname -- directories and ssh use the project name -- so
    /// the only cost is a human seeing two guests with one name,
    /// which an explicit `hostname` on one of them resolves.
    #[must_use]
    pub fn derived_from(project: &ProjectName) -> Self {
        let mut base: String = project
            .as_str()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        // Every mapped character is one ASCII byte, so a byte
        // truncation cannot split a character here.
        base.truncate(MAX_HOSTNAME_LEN - HOSTNAME_SUFFIX.len());
        // A `ProjectName`'s first character is a letter or digit,
        // which maps to itself, so it is never a hyphen and `trim`
        // cannot empty the base.
        let base = base.trim_matches('-');

        let name = format!("{base}{HOSTNAME_SUFFIX}");
        Self::parse(&name)
            .expect("a hostname derived from a project name is a valid label")
    }
}

/// Every rule a `hostname` value must pass, in one place.
///
/// Both [`Hostname::parse`] and [`Hostname::try_from`] call this,
/// so neither can run a different set. The rules are one DNS
/// label's: RFC 1123 allows a leading digit, so only a leading or
/// trailing hyphen is refused at the ends.
fn check_hostname(value: &str) -> Result<(), FieldError> {
    if value.is_empty() {
        return Err(FieldError::Empty { field: "hostname" });
    }
    if value.len() > MAX_HOSTNAME_LEN {
        return Err(FieldError::invalid(
            "hostname",
            format!("must be at most {MAX_HOSTNAME_LEN} characters"),
        ));
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(FieldError::invalid(
            "hostname",
            "must contain only letters, digits and hyphens",
        ));
    }
    if value.starts_with('-') || value.ends_with('-') {
        return Err(FieldError::invalid(
            "hostname",
            "must not begin or end with a hyphen",
        ));
    }
    Ok(())
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
    fn a_hostname_refuses_the_whole_family_of_bad_labels() {
        // Enumerated before the check was written: a hostname is
        // one DNS label, so every shape a label forbids is here.
        let long = "a".repeat(MAX_HOSTNAME_LEN + 1);
        for bad in [
            "",               // empty
            "-lead",          // leading hyphen
            "trail-",         // trailing hyphen
            "has_underscore", // underscore is not a label character
            "has.dot",        // a dot would make it two labels
            "has space",      // whitespace
            "bang!",          // any other punctuation
            long.as_str(),    // over the length limit
        ] {
            assert!(Hostname::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn a_hostname_accepts_a_plain_label() {
        // RFC 1123 allows a leading digit and mixed case, and the
        // length limit is inclusive.
        let max = "a".repeat(MAX_HOSTNAME_LEN);
        for ok in ["jutro-agent", "a", "9lead", "Mixed-Case", max.as_str()] {
            let name = Hostname::parse(ok)
                .unwrap_or_else(|e| panic!("{ok:?} must pass: {e}"));
            assert_eq!(name.as_str(), ok);
        }
    }

    #[test]
    fn a_derived_hostname_is_a_valid_label_for_any_project_name() {
        // The left value is what a `[projects.<name>]` key could
        // hold; the right is the hostname it should derive to.
        for (project, want) in [
            ("jutro", "jutro-agent"),
            ("web_app", "web-app-agent"),
            ("a.b", "a-b-agent"),
            ("MyProj", "myproj-agent"),
            ("9x", "9x-agent"),
            // A trailing non-label character would leave a hyphen
            // against the suffix; it is trimmed first.
            ("foo.", "foo-agent"),
        ] {
            let name = ProjectName::parse(project)
                .unwrap_or_else(|e| panic!("{project:?} is a valid name: {e}"));
            assert_eq!(
                Hostname::derived_from(&name).as_str(),
                want,
                "derived from {project:?}"
            );
        }
    }

    #[test]
    fn a_derived_hostname_stays_within_the_length_limit() {
        // A project name may be up to 64 characters, longer than a
        // hostname label, so the derivation truncates rather than
        // producing a value its own type would refuse.
        let long = ProjectName::parse(&"a".repeat(64))
            .expect("64 characters is the maximum a project name allows");
        let derived = Hostname::derived_from(&long);
        assert!(
            derived.as_str().len() <= MAX_HOSTNAME_LEN,
            "{} is over the limit",
            derived.as_str()
        );
        assert!(derived.as_str().ends_with("-agent"));
    }

    #[test]
    fn serde_runs_the_hostname_check_while_the_table_is_read() {
        // As with `box`, the `try_from` attribute is the only
        // thing that makes the check run during a config load.
        let err = toml::from_str::<Vm>(
            "box = \"b\"\ncpus = 2\nmemory = 2048\n\
             hostname = \"bad_name\"\n",
        )
        .expect_err("an underscore must be refused");
        assert!(
            err.to_string().contains("letters, digits and hyphens"),
            "{err}"
        );
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
            // A zero reads the same whether it arrives bare or with
            // a unit; `AT_LEAST_ONE` is what keeps the wording one.
            ("box = \"b\"\ncpus = 2\nmemory = \"0GB\"\n", "memory"),
        ] {
            let err = toml::from_str::<Vm>(bad).expect_err("must be refused");
            let msg = err.message();
            assert_eq!(msg, format!("invalid `{key}`: must be at least 1"));
        }
    }

    #[test]
    fn memory_reads_a_bare_integer_as_mib() {
        // The historical form: a bare integer is MiB, unchanged, so
        // no config written before this feature shifts meaning.
        let vm = toml::from_str::<Vm>("box = \"b\"\ncpus = 2\nmemory = 6144\n")
            .expect("a bare integer is a valid size");
        assert_eq!(vm.memory.mib(), 6144);
    }

    #[test]
    fn memory_reads_a_suffixed_size_in_powers_of_two() {
        // The left value is what the config file may write; the
        // right is the MiB it converts to. MB and MiB are one MiB,
        // GB and GiB are 1024, case and a space before the unit do
        // not matter, and a bare numeric string is MiB.
        for (written, mib) in [
            ("\"512MB\"", 512),
            ("\"6GB\"", 6144),
            ("\"6GiB\"", 6144),
            ("\"6MiB\"", 6),
            ("\"6 GB\"", 6144),
            ("\"6gb\"", 6144),
            ("\"2048\"", 2048),
        ] {
            let src = format!("box = \"b\"\ncpus = 2\nmemory = {written}\n");
            let vm = toml::from_str::<Vm>(&src)
                .unwrap_or_else(|e| panic!("{written} must pass: {e}"));
            assert_eq!(vm.memory.mib(), mib, "from {written}");
        }
    }

    #[test]
    fn memory_refuses_the_whole_family_of_bad_values() {
        // Enumerated before the reader was written: every shape a
        // size can be wrong, each refused while the config parses
        // and each naming `memory`.
        for bad in [
            "\"\"",          // empty string
            "0",             // bare zero
            "\"0GB\"",       // zero with a unit
            "-5",            // negative
            "\"1.5GB\"",     // a suffixed fraction
            "1.5",           // a bare fraction
            "\"6TB\"",       // a unit outside MB/GB
            "\"6KB\"",       // KB is not accepted either
            "\"GB\"",        // a unit with no number
            "\"lots\"",      // not a number at all
            "999999999999",  // past u32 MiB on its own
            "\"9999999GB\"", // overflows once multiplied out
        ] {
            let src = format!("box = \"b\"\ncpus = 2\nmemory = {bad}\n");
            let err = toml::from_str::<Vm>(&src)
                .err()
                .unwrap_or_else(|| panic!("{bad} must be refused"));
            assert!(
                err.message().starts_with("invalid `memory`: "),
                "{bad}: {}",
                err.message()
            );
        }
    }

    #[test]
    fn a_bad_memory_size_gives_the_reason_that_fits_it() {
        // The family test proves each is refused; this pins the
        // wording, so a fraction is not called an unknown unit and
        // an overflow is not called a zero.
        for (bad, reason) in [
            ("memory = \"1.5GB\"", "must be a whole number"),
            ("memory = 1.5", "must be a whole number"),
            ("memory = \"6TB\"", "unknown unit `TB` -- use MB or GB"),
            ("memory = \"9999999GB\"", "is too large"),
        ] {
            let src = format!("box = \"b\"\ncpus = 2\n{bad}\n");
            let err = toml::from_str::<Vm>(&src).expect_err("must be refused");
            assert_eq!(err.message(), format!("invalid `memory`: {reason}"));
        }
    }

    #[test]
    fn memory_from_mib_and_parse_reach_the_same_value() {
        let built = Memory::from_mib(
            NonZeroU32::new(8192).expect("a positive fixture size"),
        );
        assert_eq!(built.mib(), 8192);
        // 8 GiB is 8192 MiB, so the two constructors agree.
        assert_eq!(Memory::parse("8GB").expect("8GB is a valid size"), built);
    }

    #[test]
    fn a_wrong_typed_memory_still_names_the_key() {
        // `memory` given the wrong TOML type -- a boolean, an
        // array, a table, a datetime -- must name the key like
        // every other refused value, the way `cpus` does. The
        // visitor handles integers, strings and floats; without an
        // arm for these, serde's own `invalid type` message names
        // no key.
        for bad in [
            "memory = true",
            "memory = [8192]",
            "memory = {a = 1}",
            "memory = 2020-01-01",
        ] {
            let src = format!("box = \"b\"\ncpus = 2\n{bad}\n");
            let err = toml::from_str::<Vm>(&src).expect_err("must be refused");
            assert!(
                err.message().starts_with("invalid `memory`: "),
                "{bad}: {}",
                err.message()
            );
        }
    }
}
