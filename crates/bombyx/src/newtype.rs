//! The impls every checked string newtype in bombyx repeats.
//!
//! These types wrap one private `String` that a constructor has
//! already checked: `config::RepoUrl`, `config::ScriptPath`,
//! `config::GitRef`, `config::BoxName`, `config::RemoteRoot`,
//! `config::HostName`, `config::DeployKeyPath`,
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
//! [`checked_str_newtype`] writes those three. What it
//! deliberately does not write is `parse` or `TryFrom`: those
//! bodies really do differ. `RemoteRoot` drops a trailing slash
//! before storing the value, and `ProjectName` fails with
//! `name::NameError` rather than `config::FieldError`. A macro
//! covering them would have to grow an arm per exception, and
//! the exceptions are the parts worth reading.

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
/// and reachable as `self.0`. That is the shape all nine have,
/// and a type without it fails to compile here rather than
/// silently getting the wrong impls.
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

// A `macro_rules!` macro is not an item like a function or a
// struct. It is visible only to code that appears *after* it in
// the crate's source order, whatever module that code is in, so
// a module declared before this one could not see it at all.
// Re-exporting it turns it into something a sibling imports by
// path, which is why every call site writes
// `use crate::newtype::checked_str_newtype;` and why the order
// of the `mod` lines in `lib.rs` then stops mattering.
pub(crate) use checked_str_newtype;
