//! Builds the two files bombyx writes onto the VM host: the
//! Vagrantfile and the bootstrap script.
//!
//! A Vagrantfile tells Vagrant how to build a VM. Normally a
//! project writes its own and keeps it in its repo. bombyx
//! writes it instead, because Vagrant has to read that file
//! *before* the VM exists, so it cannot come from inside the
//! VM -- and outside the VM is where we are trying not to put
//! the project's files. `docs/trust-boundary.md` explains why.
//!
//! The split between the two files is worth understanding.
//!
//! The Vagrantfile changes per project -- different box,
//! different memory -- so it is built here, with config values
//! pasted into it.
//!
//! [`BOOTSTRAP`] is the same for every project, always. It is
//! shipped exactly as written, and bombyx pastes nothing into
//! it. Anything it needs to know arrives as an environment
//! variable that Vagrant sets. That is the point: pasting
//! config values into a shell script is where quoting bugs and
//! injection holes come from, so we simply never do it.

use crate::config::{Config, Source};

/// The provisioning script, shipped to the host unchanged.
///
/// Held as a file rather than a string literal so it is
/// syntax-highlighted, `shellcheck`-able and diffable.
pub const BOOTSTRAP: &str = include_str!("../templates/bootstrap.sh");

/// The Vagrantfile's name on the VM host.
pub const VAGRANTFILE_NAME: &str = "Vagrantfile";

/// The bootstrap script's name on the VM host.
///
/// The Vagrantfile's `path:` is relative to the directory the
/// Vagrantfile is in, so the two names have to agree and are
/// stated once here.
pub const BOOTSTRAP_NAME: &str = "bootstrap.sh";

/// Where the Vagrantfile's file provisioner drops the deploy
/// key inside the guest.
///
/// Two files have to agree on this path and neither can read
/// the other: [`render`] writes it as the upload's
/// `destination:`, and [`BOOTSTRAP`] has it as a literal. It is
/// stated here so the Rust half has one spelling, and a test
/// asserts the shell half still contains it.
///
/// It is a *constant* rather than another config value on
/// purpose. [`BOOTSTRAP`] is shipped exactly as written and
/// bombyx pastes nothing into it, so the guest-side path has to
/// be something the script can spell for itself.
///
/// The file provisioner runs as the box's SSH user, which for
/// every Vagrant box is `vagrant`, so the destination has to be
/// somewhere that user can write. `bootstrap.sh` moves the key
/// out of there before the clone.
pub const DEPLOY_KEY_GUEST_PATH: &str = "/home/vagrant/.ssh/bombyx-deploy-key";

/// Wraps `value` in double quotes, ready to drop into Ruby.
///
/// Three characters would otherwise change what the Ruby means
/// rather than appearing in it:
///
/// - `"` would end the string early.
/// - `\` would start an escape sequence.
/// - `#` starts Ruby's `#{...}`, which runs code and pastes the
///   result in.
///
/// Each gets a `\` in front of it, which tells Ruby to treat it
/// as an ordinary character.
///
/// We escape *every* `#`, not only the ones followed by `{`.
/// `\#` is just `#` to Ruby, so escaping the harmless ones costs
/// nothing -- and a rule with no exceptions cannot be got around
/// by some spelling nobody thought of.
///
/// You may notice the config checks already refuse all three of
/// these, which makes this look redundant. It is not. Those
/// checks only run when the value was loaded from a file, and
/// `Config`'s fields are public, so code using bombyx as a
/// library can build one by hand and call straight in here. A
/// function should not need a check in a different file to be
/// correct.
fn ruby_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '\\' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Builds the text of the Vagrantfile for `cfg`.
///
/// Returns a `String`. Nothing is written to disk here, and
/// nothing is sent anywhere -- `remote::write_file` does that.
///
/// **The block below configures a provider; it does not select
/// one.** Vagrant applies it only to the provider it has
/// chosen, and it chooses from what the host offers. bombyx
/// makes that choice elsewhere, by setting
/// [`crate::remote::PROVIDER_ENV`] on `vagrant up`.
///
/// A provider is the thing that actually runs the VM, libvirt
/// or Hyper-V. Both of them happen to spell their settings the
/// same way (`cpus`, `memory`), so the only difference between
/// the two outputs is the provider's name.
///
/// Be careful with that convenience: the Hyper-V version was
/// written from Hyper-V's documentation and **has never started
/// a real machine**. If you are the first person to try it,
/// expect to fix something. See
/// [`Provider`](crate::config::Provider).
#[must_use]
pub fn render(cfg: &Config) -> String {
    let vm = &cfg.vm;
    let source = &cfg.source;
    format!(
        "# Generated by bombyx {version}. Do not edit.
#
# bombyx rewrites this file on every `up`, `provision` and
# `scratch`. Change your own config.toml instead.

Vagrant.configure(\"2\") do |config|
  config.vm.box = {box_name}

  # The default share would mount the workstation's copy of the
  # project at /vagrant, which is the copy this design exists to
  # keep out of the guest. It also hangs on a host whose
  # firewall refuses NFS from the guest bridge.
  config.vm.synced_folder \".\", \"/vagrant\", disabled: true

  config.vm.provider :{provider} do |v|
    v.cpus = {cpus}
    v.memory = {memory}
  end

{deploy_key}  config.vm.provision \"shell\",
    path: {bootstrap},
    env: {{
      \"BOMBYX_REPO\" => {repo},
      \"BOMBYX_REF\" => {git_ref},
      \"BOMBYX_SCRIPT\" => {script},
      # Read from the vagrant process on the VM host, which
      # bombyx sets. Vagrant does not export its own
      # environment into a guest, so this hand-over is what
      # makes the two readable inside the VM.
      \"{host_env}\" => ENV.fetch(\"{host_env}\", \"unknown\"),
      \"{hostname_env}\" => ENV.fetch(\"{hostname_env}\", \"unknown\")
    }}
end
",
        version = env!("CARGO_PKG_VERSION"),
        deploy_key = deploy_key_block(source),
        box_name = ruby_string(vm.box_name.as_str()),
        provider = vm.provider,
        cpus = vm.cpus,
        memory = vm.memory,
        bootstrap = ruby_string(BOOTSTRAP_NAME),
        repo = ruby_string(source.repo.as_str()),
        git_ref = ruby_string(source.git_ref.as_str()),
        script = ruby_string(source.script.as_str()),
        host_env = crate::remote::VM_HOST_ENV,
        hostname_env = crate::remote::VM_HOSTNAME_ENV,
    )
}

/// The Ruby that uploads the deploy key, or nothing at all.
///
/// An empty string when the config names no key, so a public
/// repository's Vagrantfile carries no upload block.
///
/// [`render`] places this ahead of the shell provisioner.
/// Vagrant runs provisioners in the order the file declares
/// them, and [`BOOTSTRAP`] looks for the key as soon as it
/// starts.
///
/// The upload is conditional, and that is not the same as
/// tolerating a missing key. `crate::remote::require_file`
/// refuses the run before this file is even written, so a boot
/// never reaches an absent key.
///
/// What the condition protects is every *other* verb.
/// `vagrant destroy` loads this file too, so a `raise` here
/// would leave a directory that no bombyx command could tear
/// down: teardown stops at the failing destroy and never
/// reaches the removal that follows it.
/// `crate::remote::destroy_vm_if_present` holds that argument.
fn deploy_key_block(source: &Source) -> String {
    let Some(key) = source.deploy_key.as_ref() else {
        return String::new();
    };
    format!(
        "  # The credential the guest clones a private repository
  # with. bombyx never opens the file: vagrant reads it here on
  # the VM host and uploads it, so the workstation never holds
  # it. docs/trust-boundary.md says what keeping it inside the
  # guest costs.
  #
  # The upload is conditional so that `vagrant destroy` can
  # still load this file after the key has gone. bombyx refuses
  # a boot with no key of its own accord, before writing this
  # file at all.
  bombyx_deploy_key = File.expand_path({key})
  if File.exist?(bombyx_deploy_key)
    config.vm.provision \"file\",
      source: bombyx_deploy_key,
      destination: {dest}
  end

",
        key = ruby_string(key.as_str()),
        dest = ruby_string(DEPLOY_KEY_GUEST_PATH),
    )
}

/// Every file bombyx writes into the project directory on the
/// VM host, as `(name, contents)` pairs.
///
/// The list exists once, here, and everything else reads it:
/// `plan` to build the write commands, and the tests to check
/// each file is safe to send.
///
/// That is the whole reason for the function. If the list were
/// written out separately in each of those places, adding a
/// third file would mean remembering all of them -- and the one
/// people forget is the test, so the new file would be written
/// to the host without ever being checked.
#[must_use]
pub fn files(cfg: &Config) -> [(&'static str, String); 2] {
    [
        (VAGRANTFILE_NAME, render(cfg)),
        (BOOTSTRAP_NAME, BOOTSTRAP.to_owned()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    use crate::config::{
        BoxName, DeployKeyPath, GitRef, Provider, RepoUrl, ScriptPath, Source,
        Vm,
    };

    /// A `deploy_key` value every rule accepts, written once so
    /// the tests below and the expected Ruby agree.
    const KEY: &str = "~/.secrets/myproject-deploy-key";

    fn cfg_with(provider: Provider) -> Config {
        let mut cfg = Config::for_tests();
        cfg.vm = Vm {
            provider,
            box_name: BoxName::parse("generic/ubuntu2204")
                .expect("a valid fixture box name"),
            cpus: NonZeroU32::new(4).expect("a positive fixture count"),
            memory: NonZeroU32::new(8192).expect("a positive fixture size"),
        };
        cfg.source = Source {
            repo: RepoUrl::parse("https://example.invalid/p.git")
                .expect("a valid fixture URL"),
            git_ref: GitRef::parse("main").expect("a valid fixture ref"),
            script: ScriptPath::parse("vagrant/provision.sh")
                .expect("a valid fixture path"),
            deploy_key: None,
        };
        cfg
    }

    /// [`cfg_with`] on libvirt, carrying a `deploy_key`.
    fn cfg_with_key() -> Config {
        let mut cfg = cfg_with(Provider::Libvirt);
        cfg.source.deploy_key =
            Some(DeployKeyPath::parse(KEY).expect("a valid fixture path"));
        cfg
    }

    #[test]
    fn carries_every_configured_value() {
        // Each needle is a whole rendered line, not just the
        // value. Searching for `"4"` alone would pass no matter
        // what: the box name `generic/ubuntu2204` and the
        // version `0.4.1` both contain a 4, so the test would
        // stay green even with `v.cpus` deleted from the
        // template. A test that cannot fail is worse than none,
        // because it looks like cover.
        let out = render(&cfg_with(Provider::Libvirt));
        for needle in [
            "config.vm.box = \"generic/ubuntu2204\"",
            "v.cpus = 4",
            "v.memory = 8192",
            "\"BOMBYX_REPO\" => \"https://example.invalid/p.git\"",
            "\"BOMBYX_REF\" => \"main\"",
            "\"BOMBYX_SCRIPT\" => \"vagrant/provision.sh\"",
        ] {
            assert!(out.contains(needle), "{needle} missing from:\n{out}");
        }
    }

    #[test]
    fn disables_the_default_synced_folder() {
        // Vagrant mounts the Vagrantfile's directory at /vagrant
        // unless told not to. That directory is on the VM host
        // and holds only what bombyx generated, so the mount
        // leaks nothing -- but it hangs on a host whose firewall
        // refuses NFS from the guest bridge, which
        // docs/vm-host-setup.md warns
        // about.
        for provider in [Provider::Libvirt, Provider::Hyperv] {
            let out = render(&cfg_with(provider));
            assert!(
                out.contains(
                    "config.vm.synced_folder \".\", \"/vagrant\", \
                     disabled: true"
                ),
                "{out}"
            );
        }
    }

    #[test]
    fn forwards_the_vm_host_identity_into_the_guest() {
        // A guest has no way to work out which machine is
        // running it. bombyx sets these two variables on the
        // vagrant process on the VM host, but Vagrant does not
        // pass its own environment into a VM, so the
        // Vagrantfile has to hand them over deliberately. Since
        // bombyx writes that file, this is where it happens.
        let out = render(&cfg_with(Provider::Libvirt));
        for var in [crate::remote::VM_HOST_ENV, crate::remote::VM_HOSTNAME_ENV]
        {
            assert!(
                out.contains(&format!(
                    "\"{var}\" => ENV.fetch(\"{var}\", \"unknown\")"
                )),
                "{var} not forwarded:\n{out}"
            );
        }
    }

    #[test]
    fn names_the_provider_it_was_given() {
        assert!(render(&cfg_with(Provider::Libvirt)).contains(":libvirt"));
        assert!(render(&cfg_with(Provider::Hyperv)).contains(":hyperv"));
    }

    #[test]
    fn points_the_provisioner_at_the_bootstrap_script() {
        // The two names must agree or vagrant reports a missing
        // path after bombyx has already created a directory
        // on the host.
        let out = render(&cfg_with(Provider::Libvirt));
        assert!(out.contains(BOOTSTRAP_NAME), "{out}");
        assert!(out.contains("config.vm.provision"), "{out}");
    }

    #[test]
    fn a_ruby_string_escapes_what_would_change_what_the_file_means() {
        // Every value `render` passes through this function is
        // a checked type that refuses all three characters, so
        // no config can reach the escaping. It is tested
        // directly because the escaping is what makes the
        // refusal a precaution rather than the only thing
        // standing between a config value and Ruby code.
        //
        // A quote ends the literal, a backslash escapes what
        // follows it, and `#` begins the interpolation `#{`.
        assert_eq!(ruby_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(ruby_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(ruby_string("a#{x}"), "\"a\\#{x}\"");
        assert_eq!(ruby_string("plain"), "\"plain\"");
    }

    #[test]
    fn a_configured_deploy_key_is_uploaded_before_the_bootstrap_runs() {
        // Vagrant runs provisioners in the order the
        // Vagrantfile declares them, so the upload has to come
        // first or the bootstrap looks for a key that is not
        // there yet.
        let out = cfg_with_key();
        let out = render(&out);
        let upload = out
            .find("config.vm.provision \"file\"")
            .expect("the file provisioner must be rendered");
        let shell = out
            .find("config.vm.provision \"shell\"")
            .expect("the shell provisioner must be rendered");
        assert!(upload < shell, "the upload must come first:\n{out}");
        assert!(
            out.contains(&format!("File.expand_path(\"{KEY}\")")),
            "{out}"
        );
        assert!(
            out.contains(&format!("destination: \"{DEPLOY_KEY_GUEST_PATH}\"")),
            "{out}"
        );
    }

    #[test]
    fn the_upload_is_conditional_and_never_raises() {
        // `vagrant destroy` loads this file too. A `raise` here
        // stops the teardown at its first step and leaves a
        // directory no bombyx command can clear -- which is a
        // failure this repo has had before, and
        // `remote::destroy_vm_if_present` records.
        //
        // The loud failure lives in the plan instead:
        // `plan::tests::a_deploy_key_is_checked_before_anything_is_created`.
        let out = render(&cfg_with_key());
        assert!(out.contains("if File.exist?"), "{out}");
        assert!(!out.contains("raise"), "a raise breaks destroy:\n{out}");
    }

    #[test]
    fn no_deploy_key_renders_no_upload_at_all() {
        // A public repository needs no credential, and an
        // upload block with an empty path would fail every
        // `up`.
        let out = render(&cfg_with(Provider::Libvirt));
        for absent in [
            "config.vm.provision \"file\"",
            "File.expand_path",
            DEPLOY_KEY_GUEST_PATH,
        ] {
            assert!(!out.contains(absent), "{absent} rendered:\n{out}");
        }
    }

    #[test]
    fn the_bootstrap_script_reads_the_path_the_vagrantfile_writes_to() {
        // Two files have to agree on one path and neither can
        // see the other: the Vagrantfile uploads to it and
        // `bootstrap.sh` has it as a literal. This is what
        // catches a rename in one of them.
        assert!(
            BOOTSTRAP.contains(DEPLOY_KEY_GUEST_PATH),
            "{DEPLOY_KEY_GUEST_PATH} is not in the bootstrap script"
        );
    }

    #[test]
    fn the_bootstrap_script_points_ssh_at_the_key_and_nothing_else() {
        // `IdentitiesOnly=yes` alone is not enough: it does not
        // exclude identities named in an `ssh_config`, so
        // `-F /dev/null` is what makes "only this key" true.
        for needle in ["GIT_SSH_COMMAND", "IdentitiesOnly=yes", "-F /dev/null"]
        {
            assert!(BOOTSTRAP.contains(needle), "{needle} missing");
        }
    }

    #[test]
    fn the_bootstrap_script_deletes_a_key_no_upload_replaced() {
        // Removing `deploy_key` from the config has to remove
        // the credential from the guest, not leave one behind
        // that nothing points at.
        assert!(
            BOOTSTRAP.contains("rm -f \"$DEPLOY_KEY\""),
            "the stale-key removal is gone"
        );
    }

    #[test]
    fn the_bootstrap_script_guards_every_variable_it_needs() {
        // Without the `:?` guards an unset variable clones into
        // an empty path as root.
        for guard in ["BOMBYX_REPO:?", "BOMBYX_REF:?", "BOMBYX_SCRIPT:?"] {
            assert!(BOOTSTRAP.contains(guard), "{guard} missing");
        }
        assert!(BOOTSTRAP.contains("set -euo pipefail"));
    }
}
