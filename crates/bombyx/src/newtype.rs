//! The impls every checked string newtype in bombyx repeats.
//!
//! These types wrap one private `String` that a constructor has
//! already checked: `config::RepoUrl`, `config::ScriptPath`,
//! `config::GitRef`, `config::BoxName`, `config::RemoteRoot`,
//! `config::HostName`, `config::DeployKeyPath`,
//! `config::EnvFilePath`, `config::EnvName`, `config::EnvValue`,
//! `name::ProjectName` and `name::ScratchName`. Each explains
//! its own rules, and those explanations are the reason the
//! types are worth reading.
//!
//! What none of them explains is how to hand the wrapped value
//! back, because they all do it identically: `as_str` borrows
//! the field, `Display` writes it, and `AsRef<str>` borrows it
//! again. Written by hand that is three near-identical impl
//! blocks per type, and `cargo xtask dupes` counts every one of
//! them against a 6% budget.
//!
//! [`checked_str_newtype`] writes those three.
//!
//! Two more macros write the constructors, and they are separate
//! because a type may want one, both or neither.
//! [`checked_str_parse`] writes `parse`, and
//! [`checked_str_try_from`] writes `TryFrom<String>`, which is
//! what connects a type to serde. Both take the check function
//! and the error type as parameters, so a type failing with
//! `name::NameError` uses the same macro as one failing with
//! `config::FieldError`.
//!
//! `config::RemoteRoot` uses neither. It drops a trailing slash
//! before storing the value, so its bodies are not the shared
//! shape, and that difference is the part worth reading.

/// Writes `as_str`, `Display` and `AsRef<str>` for a newtype
/// wrapping one private `String`.
///
/// `$ty` is the type and `$as_str_doc` is the doc comment for
/// its `as_str`. The doc is a parameter because it is the one
/// part that differs: `HostName`'s says "as `ssh` sees it" and
/// `RemoteRoot`'s says "ready to have a `/` and a name joined
/// onto it", and those sentences say who reads the value.
///
/// The type must have exactly one field, and it must be private
/// and reachable as `self.0`. Every type listed above has that
/// shape, and a type without it fails to compile here rather
/// than silently getting the wrong impls.
macro_rules! checked_str_newtype {
    ($ty:ident, $as_str_doc:literal) => {
        impl $ty {
            #[doc = $as_str_doc]
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::std::fmt::Display for $ty {
            fn fmt(
                &self,
                f: &mut ::std::fmt::Formatter<'_>,
            ) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl ::std::convert::AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

/// Writes `parse` for a newtype wrapping one private `String`.
///
/// `$ty` is the type, `$err` the error its check returns, and
/// `$check` the function holding every rule the value has. The
/// doc comment is written at the call site, ahead of the type
/// name, and passed through: each one names that field's own
/// rules, and clippy requires the `# Errors` section.
///
/// The body is the same for every caller -- run the check, wrap
/// a copy -- which is why it is here rather than written out
/// once per type.
macro_rules! checked_str_parse {
    (
        $(#[$doc:meta])*
        $ty:ident, $err:ty, $check:path
    ) => {
        impl $ty {
            $(#[$doc])*
            pub fn parse(raw: &str) -> Result<Self, $err> {
                $check(raw)?;
                Ok(Self(raw.to_owned()))
            }
        }
    };
}

/// Writes `TryFrom<String>` for a newtype wrapping one private
/// `String`.
///
/// The parameters are [`checked_str_parse`]'s. The body differs
/// from `parse`'s in one way that matters: serde arrives owning
/// a `String`, so the check runs against a borrow and the value
/// moves into the newtype, with no second copy on the path a
/// config load actually takes.
///
/// **This is what makes a type's rules run while the config
/// parses.** Without it serde assigns the private field
/// directly and every check is skipped.
macro_rules! checked_str_try_from {
    (
        $(#[$doc:meta])*
        $ty:ident, $err:ty, $check:path
    ) => {
        impl ::std::convert::TryFrom<String> for $ty {
            type Error = $err;

            $(#[$doc])*
            fn try_from(raw: String) -> Result<Self, Self::Error> {
                $check(&raw)?;
                Ok(Self(raw))
            }
        }
    };
}

// A `macro_rules!` macro is not an item like a function or a
// struct. It is visible only to code that appears *after* it in
// the crate's source order, whatever module that code is in, so
// a module declared before this one could not see it at all.
// Re-exporting it turns it into something a sibling imports by
// path, which is why every call site writes
// `use crate::newtype::checked_str_newtype;` and why the order
// of the `mod` lines in `lib.rs` then stops mattering.
pub(crate) use {checked_str_newtype, checked_str_parse, checked_str_try_from};
