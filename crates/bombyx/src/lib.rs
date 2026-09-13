//! bombyx -- drive isolated AI-agent VMs on a libvirt host.
//!
//! The control plane is deliberately thin: bombyx runs
//! `vagrant` on the VM host and streams the output back. It
//! generates the Vagrantfile and a bootstrap script from the
//! operator's own `config.toml` and writes those onto the VM
//! host too.
//!
//! The VM host is usually a second machine, reached over SSH.
//! Where `host` names the machine bombyx is running on, the
//! same script goes to `sh -c` instead -- `config::transport`
//! decides, and `remote` builds either shape. bombyx sends that
//! machine no file from the project's repository, and it opens
//! no file in the project's directory either; the guest clones
//! the project itself once running.
//!
//! `docs/trust-boundary.md` states the isolation strategy this
//! implements, and the two qualifications on the sentence
//! above: the guest's disk image, and the two arguments that
//! point the config loader at a file of the caller's choosing.

pub mod config;
pub mod doctor;
pub mod hostkeys;
pub mod listing;
pub mod name;
mod newtype;
pub mod plan;
pub mod remote;
pub mod run;
pub mod term;
pub mod tool;
pub mod update;
pub mod vagrantfile;
