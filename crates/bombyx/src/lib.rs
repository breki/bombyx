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
//! decides, and `remote` builds either shape. Nothing from the
//! project's
//! repository reaches that machine, and bombyx reads nothing out
//! of the project's directory either; the guest clones the
//! project itself once running.
//!
//! See `docs/` for the isolation strategy this implements.

pub mod config;
pub mod doctor;
pub mod name;
pub mod plan;
pub mod remote;
pub mod term;
pub mod tool;
pub mod update;
pub mod vagrantfile;
