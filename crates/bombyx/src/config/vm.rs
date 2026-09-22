//! A project entry's `[vm]` table: what machine to build.
//!
//! What the guest clones into that machine is the `[source]`
//! table, in `super::source`.
//!
//! Every value in the table is a type that checks itself:
//! [`Provider`] is an enum, [`BoxName`] is a newtype in the
//! shape `super::source::RepoUrl` describes, `cpus` is a
//! `NonZeroU32`, and `memory` and the optional `disk` are
//! [`Memory`] and [`Disk`] newtypes that each read a bare count or
//! a suffixed size like `"6GB"`. So a `Vm` that exists at all is
//! one whose values passed, and there is no separate function to
//! remember to call.
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
/// `box`, `cpus` and `memory` are required: the base image is the
/// one thing bombyx cannot invent, and a size it chose would be
/// wrong on both a laptop and a workstation. `provider`, `disk`
/// and `hostname` are optional -- `provider` defaults to libvirt,
/// and an absent `disk` or `hostname` leaves the box's own disk
/// size and lets bombyx derive a name.
///
/// Serde reads the table through the private `VmFields`, whose
/// `#[serde(deny_unknown_fields)]` turns a key it does not
/// recognise -- `cpu = 2` for `cpus` -- into a message naming the
/// key rather than a VM built with a default. `VmFields` then
/// converts through [`Vm::try_from`], which enforces the one rule
/// no single field can: a `disk` needs the libvirt provider.
///
/// The fields are public, so a caller can build a `Vm` by hand and
/// skip both the field checks and that conversion. This is the same
/// gap [`Config`](crate::config::Config)'s public fields leave, and
/// the checks are here to run while the config file is read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "VmFields")]
pub struct Vm {
    /// Virtualization backend. Defaults to
    /// [`Provider::Libvirt`] when the key is absent.
    pub provider: Provider,
    /// Vagrant box the VM boots from, e.g.
    /// `generic/ubuntu2204`.
    ///
    /// Named `box_name` because `box` is a Rust keyword.
    pub box_name: BoxName,
    /// Virtual CPUs. Never zero.
    ///
    /// `NonZeroU32` is what makes a zero unrepresentable, so a
    /// caller assigning to this public field gets the same rule
    /// the config file got. What the type does *not* do is name
    /// the key when it refuses one, which is why serde reads it
    /// through `positive_cpus`.
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

    /// The virtual disk size, or `None` to keep the box's own.
    ///
    /// A [`Disk`], held as whole GiB and written into the Vagrantfile
    /// as the libvirt provider's `machine_virtual_size`. When the key
    /// is absent the guest inherits the base box's disk, which ranges
    /// widely between boxes, so a project that outgrows it -- a Rust
    /// target directory fills a small one fast -- sets a size here.
    /// Only the libvirt provider takes one; a `disk` on any other
    /// provider is refused by [`Vm::try_from`] while the config is
    /// read.
    pub disk: Option<Disk>,

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
    pub hostname: Option<Hostname>,
}

/// The `[vm]` table as it parses, before its one cross-field rule.
///
/// [`Vm`] is `#[serde(try_from = "VmFields")]` from this, so serde
/// reads each field through its own type's check and then
/// [`Vm::try_from`] enforces the rule no single field can see: a
/// `disk` needs the libvirt provider. The fields mirror `Vm`'s, and
/// the compiler refuses [`Vm::try_from`] if the two ever drift.
///
/// Every field-level serde attribute lives here rather than on `Vm`,
/// because this is the type serde actually deserializes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VmFields {
    #[serde(default)]
    provider: Provider,
    #[serde(rename = "box")]
    box_name: BoxName,
    #[serde(deserialize_with = "positive_cpus")]
    cpus: NonZeroU32,
    memory: Memory,
    #[serde(default)]
    disk: Option<Disk>,
    #[serde(default)]
    hostname: Option<Hostname>,
}

impl TryFrom<VmFields> for Vm {
    type Error = FieldError;

    /// Enforces the disk/provider rule, then hands the checked
    /// fields to `Vm` unchanged.
    ///
    /// The rule lives here, not in [`Disk`], because it needs the
    /// `provider` sitting beside the `disk` in the same table, which
    /// a value's own constructor cannot see. `libvirt` is the only
    /// provider whose Vagrantfile carries a disk size, so a `disk`
    /// on any other one would render nothing and silently leave the
    /// box default; refusing it names the field instead.
    fn try_from(fields: VmFields) -> Result<Self, FieldError> {
        if fields.disk.is_some() && fields.provider != Provider::Libvirt {
            return Err(FieldError::invalid(
                "disk",
                "only the libvirt provider supports a disk size",
            ));
        }
        Ok(Self {
            provider: fields.provider,
            box_name: fields.box_name,
            cpus: fields.cpus,
            memory: fields.memory,
            disk: fields.disk,
            hostname: fields.hostname,
        })
    }
}

/// The wording every size field uses to refuse a value below one.
///
/// `cpus` reads it through [`positive_cpus`], and `memory` and
/// `disk` through [`Memory`] and [`Disk`], each a separate reader.
/// Sharing the one string keeps their wording identical, so a zero
/// of any of them reads the same.
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

/// Reads a size written as text into whole base units.
///
/// The shape shared by [`Memory`] and [`Disk`]: a number, then an
/// optional unit with optional whitespace before it,
/// case-insensitively. `unit_multiplier` maps an uppercased unit
/// (`""` for a bare number) to how many base units it is worth, or
/// `None` for a unit this field does not take; `allowed` names the
/// ones it does, for the refusal. `examples` are concrete good
/// values shown when the text has no leading number, so the operator
/// sees the shape rather than being sent to the sample. The base
/// unit is whatever the caller's table treats as `1` -- MiB for
/// memory, GiB for disk.
///
/// # Errors
///
/// Returns [`FieldError::Invalid`] naming `field` when the text has
/// no leading number, carries a fraction, names a unit outside the
/// caller's table, overflows `u32` base units, or works out to zero.
fn parse_size(
    field: &'static str,
    raw: &str,
    unit_multiplier: impl Fn(&str) -> Option<u64>,
    allowed: &str,
    examples: &str,
) -> Result<NonZeroU32, FieldError> {
    let text = raw.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, rest) = text.split_at(split);
    if number.is_empty() {
        return Err(FieldError::invalid(
            field,
            format!("must start with a number, e.g. {examples}"),
        ));
    }
    let unit = rest.trim_start();
    let Some(per_unit) = unit_multiplier(&unit.to_ascii_uppercase()) else {
        // A fraction reaches here as the `.` left in the unit slot:
        // `"1.5GB"` arrives with `rest` of `.5GB`. Naming it a
        // whole-number rule is clearer than calling that an unknown
        // unit.
        if unit.starts_with('.') {
            return Err(FieldError::invalid(field, "must be a whole number"));
        }
        return Err(FieldError::invalid(
            field,
            format!("unknown unit `{unit}` -- use {allowed}"),
        ));
    };
    // A digit run too long for `u64`, the multiplication, and the
    // narrowing to `u32` are each a way the value can be too large;
    // all three land on the same message.
    let too_large = || FieldError::invalid(field, "is too large");
    let value = number
        .parse::<u64>()
        .map_err(|_| too_large())?
        .checked_mul(per_unit)
        .and_then(|m| u32::try_from(m).ok())
        .ok_or_else(too_large)?;
    NonZeroU32::new(value)
        .ok_or_else(|| FieldError::invalid(field, AT_LEAST_ONE))
}

/// Reads a bare config integer as a count of base units.
///
/// A bare integer never carries a unit, so it is one base unit
/// each. A value below one is refused with the same wording a
/// `"0GB"` gets, and one past `u32::MAX` with the same wording an
/// oversized suffixed value gets.
fn size_from_int(
    field: &'static str,
    value: i64,
) -> Result<NonZeroU32, FieldError> {
    u32::try_from(value)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| {
            let reason = if value > i64::from(u32::MAX) {
                "is too large"
            } else {
                AT_LEAST_ONE
            };
            FieldError::invalid(field, reason)
        })
}

/// Deserializes a size newtype's inner value from any TOML shape.
///
/// [`Memory`] and [`Disk`] both read a bare integer, a suffixed
/// string, or the wrong type entirely, and all three answers name
/// the key rather than falling back on serde's keyless `invalid
/// type` message. `from_text` is the field's own text reader, and
/// `shape` describes what it accepts -- used both for serde's
/// `expecting` and for the refusal of a boolean, array, table or
/// datetime.
fn deserialize_size<'de, D>(
    d: D,
    field: &'static str,
    from_text: fn(&str) -> Result<NonZeroU32, FieldError>,
    shape: &'static str,
) -> Result<NonZeroU32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct SizeVisitor {
        field: &'static str,
        from_text: fn(&str) -> Result<NonZeroU32, FieldError>,
        shape: &'static str,
    }

    impl SizeVisitor {
        /// The refusal for a value of the wrong TOML type, naming
        /// the field and the shapes it does take.
        fn wrong_type(&self) -> FieldError {
            FieldError::invalid(self.field, format!("must be {}", self.shape))
        }
    }

    impl<'de> serde::de::Visitor<'de> for SizeVisitor {
        type Value = NonZeroU32;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.shape)
        }

        fn visit_i64<E: serde::de::Error>(
            self,
            v: i64,
        ) -> Result<NonZeroU32, E> {
            size_from_int(self.field, v).map_err(E::custom)
        }

        // A TOML integer arrives through `visit_i64`; this catches a
        // value past `i64::MAX` from any other deserializer before
        // it wraps negative.
        fn visit_u64<E: serde::de::Error>(
            self,
            v: u64,
        ) -> Result<NonZeroU32, E> {
            let v = i64::try_from(v).map_err(|_| {
                E::custom(FieldError::invalid(self.field, "is too large"))
            })?;
            size_from_int(self.field, v).map_err(E::custom)
        }

        fn visit_str<E: serde::de::Error>(
            self,
            v: &str,
        ) -> Result<NonZeroU32, E> {
            (self.from_text)(v).map_err(E::custom)
        }

        fn visit_f64<E: serde::de::Error>(
            self,
            _v: f64,
        ) -> Result<NonZeroU32, E> {
            Err(E::custom(FieldError::invalid(
                self.field,
                "must be a whole number",
            )))
        }

        // A boolean, an array, a table or a datetime is the wrong
        // TOML type entirely. serde's own message for these names no
        // key, and `cpus` names its key for the same mistakes, so
        // these arms keep the refusal in the house style. A datetime
        // reaches `visit_map`.
        fn visit_bool<E: serde::de::Error>(
            self,
            _v: bool,
        ) -> Result<NonZeroU32, E> {
            Err(E::custom(self.wrong_type()))
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            _seq: A,
        ) -> Result<NonZeroU32, A::Error> {
            Err(serde::de::Error::custom(self.wrong_type()))
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            _map: A,
        ) -> Result<NonZeroU32, A::Error> {
            Err(serde::de::Error::custom(self.wrong_type()))
        }
    }

    d.deserialize_any(SizeVisitor {
        field,
        from_text,
        shape,
    })
}

/// What [`Memory`] accepts, for `expecting` and the wrong-type
/// refusal.
const MEMORY_SHAPE: &str = "a number of MiB, or a string like \"6GB\"";

/// What [`Disk`] accepts, for `expecting` and the wrong-type
/// refusal.
const DISK_SHAPE: &str = "a number of GiB, or a string like \"40GB\"";

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
/// A suffixed value describes itself, which is why the sample can
/// lead with `memory = "8GB"` rather than a bare number and a
/// comment.
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
        memory_mib(raw).map(Self)
    }
}

/// Every rule a `memory` value's text must pass, converting to MiB.
///
/// One function so [`Memory::parse`] and its serde reader run the
/// identical unit table; `GB`/`GiB` are 1024 MiB, `MB`/`MiB` one.
fn memory_mib(raw: &str) -> Result<NonZeroU32, FieldError> {
    parse_size(
        "memory",
        raw,
        |unit| match unit {
            "" | "MB" | "MIB" => Some(1),
            "GB" | "GIB" => Some(1024),
            _ => None,
        },
        "MB or GB",
        "6144, 512MB or 6GB",
    )
}

impl<'de> serde::Deserialize<'de> for Memory {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserialize_size(d, "memory", memory_mib, MEMORY_SHAPE).map(Self)
    }
}

/// The virtual disk a project's machine gets, held as whole GiB.
///
/// A *newtype* wrapping one private [`NonZeroU32`], the GiB count
/// bombyx writes into the Vagrantfile as the libvirt provider's
/// `machine_virtual_size`, which sizes the disk in whole GiB. It is
/// built only through [`Disk::parse`], [`Disk::from_gib`] or serde,
/// each of which guarantees the value is at least one.
///
/// Like [`Memory`], the config may write it as a bare integer
/// (`disk = 40`) or a suffixed string (`disk = "40GB"`). The unit
/// is whole GiB, so only `GB` and `GiB` are accepted -- both mean
/// one GiB -- and a sub-GiB unit like `MB` is refused, because the
/// provider setting cannot express a fraction of a GiB.
///
/// The field is optional. When a project sets no `disk`, the guest
/// keeps the base box's own disk size, which is what
/// `config.toml.sample` says. Only the libvirt provider takes a
/// disk size; [`Vm`] refuses a `disk` on any other provider while
/// the config is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disk(NonZeroU32);

impl Disk {
    /// A `Disk` from a GiB count already known to be nonzero.
    ///
    /// Total, because [`NonZeroU32`] already carries the one rule
    /// the type has, matching [`Memory::from_mib`].
    #[must_use]
    pub fn from_gib(gib: NonZeroU32) -> Self {
        Self(gib)
    }

    /// The size in GiB, as bombyx writes it into the Vagrantfile.
    #[must_use]
    pub fn gib(&self) -> u32 {
        self.0.get()
    }

    /// Reads a size written as text, in whole GiB.
    ///
    /// Accepts a bare number (`"40"`), or a number and a `GB`/`GiB`
    /// unit with optional whitespace between them (`"40GB"`,
    /// `"40 GiB"`), case-insensitively. Both units mean one GiB.
    ///
    /// # Errors
    ///
    /// Returns [`FieldError::Invalid`] naming `disk` when the text
    /// has no leading number, carries a fraction, names a unit other
    /// than GB (a sub-GiB unit included), overflows `u32` GiB, or
    /// works out to zero.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        disk_gib(raw).map(Self)
    }
}

/// Every rule a `disk` value's text must pass, in whole GiB.
///
/// One function so [`Disk::parse`] and its serde reader run the
/// identical unit table; `GB` and `GiB` are one GiB, and nothing
/// smaller is accepted.
fn disk_gib(raw: &str) -> Result<NonZeroU32, FieldError> {
    parse_size(
        "disk",
        raw,
        |unit| match unit {
            "" | "GB" | "GIB" => Some(1),
            _ => None,
        },
        "GB or GiB",
        "40 or \"40GB\"",
    )
}

impl<'de> serde::Deserialize<'de> for Disk {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserialize_size(d, "disk", disk_gib, DISK_SHAPE).map(Self)
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
            // A value with no leading number shows concrete good
            // ones, so the operator sees the shape here rather than
            // being sent to the sample.
            (
                "memory = \"GB\"",
                "must start with a number, e.g. 6144, 512MB or 6GB",
            ),
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

    /// A `[vm]` table with a valid `disk` line, parsed to a `Vm`.
    fn vm_with_disk(disk: &str) -> Result<Vm, toml::de::Error> {
        let src =
            format!("box = \"b\"\ncpus = 2\nmemory = 2048\ndisk = {disk}\n");
        toml::from_str::<Vm>(&src)
    }

    #[test]
    fn disk_defaults_to_none_when_the_key_is_absent() {
        // No `disk` means the guest keeps the box's own size, so the
        // field is `None` rather than a number bombyx invented.
        let vm = toml::from_str::<Vm>("box = \"b\"\ncpus = 2\nmemory = 2048\n")
            .expect("a table without disk is valid");
        assert_eq!(vm.disk, None);
    }

    #[test]
    fn disk_reads_a_bare_integer_and_a_suffix_as_gib() {
        // The left value is what the config may write; the right is
        // the GiB it holds. A bare integer is GiB, GB and GiB both
        // mean one GiB, and case and a space before the unit do not
        // matter.
        for (written, gib) in [
            ("40", 40),
            ("\"40GB\"", 40),
            ("\"40GiB\"", 40),
            ("\"40 GB\"", 40),
            ("\"40gb\"", 40),
            ("\"128GB\"", 128),
        ] {
            let vm = vm_with_disk(written)
                .unwrap_or_else(|e| panic!("{written} must pass: {e}"));
            assert_eq!(
                vm.disk.expect("disk was set").gib(),
                gib,
                "from {written}"
            );
        }
    }

    #[test]
    fn disk_refuses_the_whole_family_of_bad_values() {
        // The same family `memory` refuses, plus a sub-GiB unit,
        // which `disk` cannot express. Each names `disk`.
        for bad in [
            "\"\"",             // empty string
            "0",                // bare zero
            "\"0GB\"",          // zero with a unit
            "-5",               // negative
            "\"1.5GB\"",        // a suffixed fraction
            "1.5",              // a bare fraction
            "\"512MB\"",        // sub-GiB: not expressible in whole GiB
            "\"40TB\"",         // a unit outside GB/GiB
            "\"GB\"",           // a unit with no number
            "\"lots\"",         // not a number at all
            "true",             // the wrong TOML type
            "99999999999",      // past u32 GiB on its own
            "\"9999999999GB\"", // past u32 GiB once read as a size
        ] {
            let err = vm_with_disk(bad)
                .err()
                .unwrap_or_else(|| panic!("{bad} must be refused"));
            assert!(
                err.message().starts_with("invalid `disk`: "),
                "{bad}: {}",
                err.message()
            );
        }
    }

    #[test]
    fn a_sub_gib_disk_unit_says_what_to_use() {
        // `MB` is a real unit, just not one a whole-GiB disk takes,
        // so the message points at the units that work.
        let err = vm_with_disk("\"512MB\"").expect_err("must be refused");
        assert_eq!(
            err.message(),
            "invalid `disk`: unknown unit `MB` -- use GB or GiB"
        );
    }

    #[test]
    fn a_disk_on_a_non_libvirt_provider_is_refused() {
        // Only libvirt's Vagrantfile carries a disk size. A `disk`
        // on any other provider would render nothing and silently
        // leave the box default, so the config is refused instead,
        // naming the field.
        let src = "box = \"b\"\ncpus = 2\nmemory = 2048\n\
                   provider = \"hyperv\"\ndisk = \"40GB\"\n";
        let err = toml::from_str::<Vm>(src).expect_err("must be refused");
        assert_eq!(
            err.message(),
            "invalid `disk`: only the libvirt provider supports a disk size"
        );
    }

    #[test]
    fn a_disk_is_accepted_on_libvirt_named_or_defaulted() {
        // libvirt is the default provider, so a `disk` is fine both
        // when the key is absent and when it names libvirt.
        for provider in ["", "provider = \"libvirt\"\n"] {
            let src = format!(
                "box = \"b\"\ncpus = 2\nmemory = 2048\n{provider}\
                 disk = \"40GB\"\n"
            );
            let vm = toml::from_str::<Vm>(&src).unwrap_or_else(|e| {
                panic!("{provider:?} + disk must pass: {e}")
            });
            assert_eq!(vm.disk.expect("disk was set").gib(), 40);
            assert_eq!(vm.provider, Provider::Libvirt);
        }
    }

    #[test]
    fn hyperv_without_a_disk_is_still_accepted() {
        // The rule refuses a disk on hyperv, not hyperv itself.
        let src =
            "box = \"b\"\ncpus = 2\nmemory = 2048\nprovider = \"hyperv\"\n";
        let vm =
            toml::from_str::<Vm>(src).expect("hyperv without disk is fine");
        assert_eq!(vm.provider, Provider::Hyperv);
        assert_eq!(vm.disk, None);
    }

    #[test]
    fn disk_from_gib_and_parse_reach_the_same_value() {
        let built =
            Disk::from_gib(NonZeroU32::new(40).expect("a positive fixture"));
        assert_eq!(built.gib(), 40);
        assert_eq!(Disk::parse("40GiB").expect("40GiB is valid"), built);
    }
}
