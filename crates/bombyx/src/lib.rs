//! bombyx -- drive isolated AI-agent VMs on a remote
//! libvirt host over SSH.
//!
//! The control plane is deliberately thin: bombyx runs
//! `vagrant` on the VM host over SSH and streams the output
//! back. bombyx generates the Vagrantfile and a bootstrap
//! script from `bombyx.toml` and writes them onto the VM host
//! over SSH. Nothing from the project's repository reaches that
//! machine; the guest clones the project itself once running.
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
