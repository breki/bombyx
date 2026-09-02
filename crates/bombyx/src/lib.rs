//! bombyx -- drive isolated AI-agent VMs on a remote
//! libvirt host over SSH.
//!
//! The control plane is deliberately thin: bombyx runs
//! `vagrant` on the VM host over SSH and streams the output
//! back. The project repo holds the Vagrantfile and is the
//! source of truth; the host keeps a pushed copy.
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
