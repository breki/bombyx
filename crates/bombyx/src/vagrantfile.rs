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

use std::collections::BTreeMap;

use crate::config::{Config, DeployKeyPath, EnvName, EnvValue};

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

/// Where the deploy key lives inside the guest.
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
/// Private, unlike [`VAGRANTFILE_NAME`] and [`BOOTSTRAP_NAME`],
/// which the integration suite uses. Where the key lands in the
/// guest is this module's business, and making it public would
/// invite a caller to depend on it.
///
/// The Vagrantfile's file provisioner drops it here and
/// `bootstrap.sh` leaves it here, at `0600` and owned by the
/// box's SSH user, for the life of the VM. It is not moved
/// anywhere more private on purpose: the agent works as that
/// user and has to push with this key, so a placement it could
/// not read would be a key that cannot do its job.
/// `docs/trust-boundary.md` under **What this costs** holds
/// what that exposes.
///
/// The provisioner runs as the box's SSH user, so the
/// destination has to be somewhere that user can write. This
/// path assumes that user is `vagrant`, which is Vagrant's
/// default for `config.ssh.username` rather than a rule -- a
/// box is free to set another, and some do. On a box that sets
/// a different user the upload fails inside Vagrant, a long way
/// from `box` in the config.
///
/// This path is a constant because Vagrant evaluates the
/// upload's `destination:` before the guest exists, so nothing
/// in the guest can be consulted for it. `bootstrap.sh` derives
/// the clone directory from `$HOME` instead, which is why the
/// two do not have to agree about the account's name.
const DEPLOY_KEY_GUEST_PATH: &str = "/home/vagrant/.ssh/bombyx-deploy-key";

/// Environment variable telling the guest that the operator's
/// config named a `deploy_key`.
///
/// [`render`] sets it in the shell provisioner's `env:` block
/// on every render. [`BOOTSTRAP`] branches on it.
///
/// **Why the guest is told rather than left to look.** The
/// upload lands at [`DEPLOY_KEY_GUEST_PATH`], in a directory the
/// box's SSH user owns -- the user the agent works as. So a
/// leftover from an interrupted provision, or one `touch` by
/// code running in the VM, would answer "was a key configured?"
/// on the operator's behalf.
///
/// **It is set on every render, `"1"` or `"0"`, and that is the
/// half that matters.** Vagrant runs a shell provisioner
/// through `config.ssh.shell`, whose default is `bash -l` -- a
/// login shell, which sources `/etc/profile` and
/// `/etc/profile.d/*.sh` before the script. The `env:` block is
/// the only thing that overrides what those files set. So a
/// name bombyx does not render is left to them, and an export
/// placed there reaches `bootstrap.sh` unopposed. Rendering the
/// entry only for a configured key
/// would leave the *no-key* case forgeable in exactly the
/// direction that matters: the guest could claim a key was
/// configured and keep a stale credential alive. Naming it
/// always means the config answers either way.
const DEPLOY_KEY_ENV: &str = "BOMBYX_DEPLOY_KEY";

/// Repository the guest clones, as the guest's shell sees it.
const REPO_ENV: &str = "BOMBYX_REPO";

/// Branch or tag the guest checks out.
const REF_ENV: &str = "BOMBYX_REF";

/// Provisioning script the guest runs out of the clone.
const SCRIPT_ENV: &str = "BOMBYX_SCRIPT";

/// Every variable bombyx sets in the provisioner itself.
///
/// Test-only, because nothing in the rendering reads it: the
/// six names are written into the template one by one, in
/// shapes that differ. What this array buys is a list to walk,
/// and the test below is the only walker.
///
/// The `[env]` table refuses a name carrying the prefix
/// `RESERVED_PREFIX` names, and `config::env` holds why. This
/// array is what makes that reservation checkable: a test walks
/// it and asserts each name is one the reservation covers.
/// Without the list, the guard protects whichever names happen
/// to start with the prefix, and a bombyx variable added
/// without it would fall outside the reservation with every
/// test still green.
#[cfg(test)]
const BOMBYX_ENV_NAMES: [&str; 6] = [
    REPO_ENV,
    REF_ENV,
    SCRIPT_ENV,
    DEPLOY_KEY_ENV,
    crate::remote::VM_HOST_ENV,
    crate::remote::VM_HOSTNAME_ENV,
];

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

/// Renders the project's own variables as the tail of the
/// provisioner's `env:` hash.
///
/// Returns the empty string when the project names none, so the
/// entry above it stays the last one and the literal closes
/// without a stray comma.
///
/// Each entry is written on its own line, prefixed with the
/// comma that separates it from whatever came before. Building
/// it that way rather than joining and appending means the
/// no-variables case needs no special handling at the call
/// site. Two variables land like this, under the last entry
/// bombyx writes itself:
///
/// ```text
///       "BOMBYX_VM_HOSTNAME" => ENV.fetch(...),
///       "GIT_USER_NAME" => "Igor Brejc",
///       "NODE_MAJOR" => "22"
///     }
/// ```
///
/// A `BTreeMap` iterates in key order, so the output is sorted
/// by name and a re-run with an unchanged config produces a
/// byte-identical file.
fn project_env_block(env: &BTreeMap<EnvName, EnvValue>) -> String {
    let mut out = String::new();
    for (name, value) in env {
        out.push_str(",\n      ");
        out.push_str(&ruby_string(name.as_str()));
        out.push_str(" => ");
        out.push_str(&ruby_string(value.as_str()));
    }
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
    # Vagrant runs a shell provisioner as root without this.
    # `bootstrap.sh` acts only on the agent's own home, and a
    # project that has to install something calls `sudo` from
    # its own script.
    privileged: false,
    env: {{
      \"{repo_env}\" => {repo},
      \"{ref_env}\" => {git_ref},
      \"{script_env}\" => {script},
      \"{deploy_key_env_name}\" => \"{deploy_key_env}\",
      # Read from the vagrant process on the VM host, which
      # bombyx sets. Vagrant does not export its own
      # environment into a guest, so this hand-over is what
      # makes the two readable inside the VM.
      \"{host_env}\" => ENV.fetch(\"{host_env}\", \"unknown\"),
      \"{hostname_env}\" => ENV.fetch(\"{hostname_env}\", \"unknown\"){project_env}
    }}
end
",
        version = env!("CARGO_PKG_VERSION"),
        deploy_key = deploy_key_block(source.deploy_key.as_ref()),
        repo_env = REPO_ENV,
        ref_env = REF_ENV,
        script_env = SCRIPT_ENV,
        deploy_key_env_name = DEPLOY_KEY_ENV,
        deploy_key_env = deploy_key_env(source.deploy_key.as_ref()),
        box_name = ruby_string(vm.box_name.as_str()),
        provider = vm.provider,
        cpus = vm.cpus,
        memory = vm.memory,
        bootstrap = ruby_string(BOOTSTRAP_NAME),
        project_env = project_env_block(&cfg.env),
        repo = ruby_string(source.repo.as_str()),
        git_ref = ruby_string(source.git_ref.as_str()),
        script = ruby_string(source.script.as_str()),
        host_env = crate::remote::VM_HOST_ENV,
        hostname_env = crate::remote::VM_HOSTNAME_ENV,
    )
}

/// What [`DEPLOY_KEY_ENV`] is set to for `key`.
///
/// `"1"` when a key is configured and `"0"` when none is.
/// [`DEPLOY_KEY_ENV`] says why it is never simply left out.
fn deploy_key_env(key: Option<&DeployKeyPath>) -> &'static str {
    if key.is_some() { "1" } else { "0" }
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
///
/// One call in the block can still fail, and it is worth
/// knowing because it fails on every verb. `File.expand_path`
/// raises `ArgumentError: non-absolute home` when `HOME` holds
/// an empty or relative value -- measured on ruby 3.2.3, where
/// an *unset* `HOME` falls back to the passwd entry and is
/// fine. Only a `~/`-anchored key reaches it. That is a broken
/// environment on the VM host rather than anything a config can
/// cause, so it is recorded rather than guarded.
fn deploy_key_block(key: Option<&DeployKeyPath>) -> String {
    let Some(key) = key else {
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
        BoxName, DeployKeyPath, EnvName, EnvValue, GitRef, Provider,
        RESERVED_PREFIX, RepoUrl, ScriptPath, Source, Vm,
    };

    /// A `deploy_key` value every rule accepts, written once so
    /// the tests below and the expected Ruby agree.
    const KEY: &str = "~/.secrets/myproject-deploy-key";

    /// [`BOOTSTRAP`] with line continuations joined and every
    /// whitespace run collapsed to one space.
    ///
    /// Needed because a command in that file may be wrapped
    /// across lines, so the text a reader sees as one command
    /// is not a contiguous substring -- the same wrap trap
    /// `CLAUDE.md` warns about for grepping canon prose.
    fn flat_bootstrap() -> String {
        BOOTSTRAP
            .replace("\\\n", " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// [`BOOTSTRAP`] as lines, each with its continuations
    /// joined and its whitespace collapsed.
    ///
    /// [`flat_bootstrap`] answers "does this text appear
    /// anywhere"; this one answers "what does each command look
    /// like", which is what a per-line invariant needs.
    fn flat_bootstrap_lines() -> Vec<String> {
        BOOTSTRAP
            .replace("\\\n", " ")
            .lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    }

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

    /// [`cfg_with`] on libvirt, carrying an `[env]` table.
    ///
    /// The two names are written out of order on purpose: the
    /// rendering has to sort them, so a fixture already in
    /// order could not tell a sorted rendering from an
    /// unsorted one.
    fn cfg_with_env() -> Config {
        let mut cfg = cfg_with(Provider::Libvirt);
        for (name, value) in [
            ("NODE_MAJOR", "22"),
            ("GIT_USER_NAME", "Igor Brejc (agent VM)"),
        ] {
            cfg.env.insert(
                EnvName::parse(name).expect("a valid fixture name"),
                EnvValue::parse(value).expect("a valid fixture value"),
            );
        }
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
    fn every_variable_bombyx_sets_is_one_the_env_table_refuses() {
        // A prefix protects only the names that happen to
        // carry it. This ties the two together: add a variable
        // to the provisioner without the prefix and this fails.
        // `config::env`'s `RESERVED_PREFIX` holds what a
        // collision would cost.
        for name in BOMBYX_ENV_NAMES {
            assert!(
                name.starts_with(RESERVED_PREFIX),
                "{name} is set by bombyx and falls outside the \
                 `[env]` reservation"
            );
            assert!(
                EnvName::parse(name).is_err(),
                "a project could write {name} and take it over"
            );
        }
    }

    #[test]
    fn the_env_hash_closes_with_the_projects_variables_in_it() {
        // The comma between entries is what keeps the Ruby hash
        // parseable, and searching for one entry at a time
        // cannot see it: delete the comma and every entry is
        // still there. So the whole tail is one literal, which
        // pins the separator, the indentation, the order and
        // the closing brace together.
        let out = render(&cfg_with_env());
        let tail = concat!(
            "\"BOMBYX_VM_HOSTNAME\" => ",
            "ENV.fetch(\"BOMBYX_VM_HOSTNAME\", \"unknown\"),\n",
            "      \"GIT_USER_NAME\" => \"Igor Brejc (agent VM)\",\n",
            "      \"NODE_MAJOR\" => \"22\"\n",
            "    }"
        );
        assert!(out.contains(tail), "tail missing from:\n{out}");
    }

    #[test]
    fn carries_the_projects_own_variables() {
        // Whole rendered lines, for the reason
        // `carries_every_configured_value` gives below.
        let out = render(&cfg_with_env());
        for needle in [
            "\"GIT_USER_NAME\" => \"Igor Brejc (agent VM)\"",
            "\"NODE_MAJOR\" => \"22\"",
        ] {
            assert!(out.contains(needle), "{needle} missing from:\n{out}");
        }
    }

    #[test]
    fn renders_the_projects_variables_in_name_order() {
        // Vagrant does not care about the order. A reader
        // diffing two generated files does, and so does anyone
        // asking whether a re-run changed anything.
        let out = render(&cfg_with_env());
        let git = out.find("GIT_USER_NAME").expect("the first name");
        let node = out.find("NODE_MAJOR").expect("the second name");
        assert!(git < node, "not in name order:\n{out}");
    }

    #[test]
    fn a_project_with_no_variables_renders_bombyxs_own_set() {
        // The absent table must not leave a stray comma or an
        // empty line behind in the hash literal.
        let out = render(&cfg_with(Provider::Libvirt));
        assert!(
            out.contains(
                "\"BOMBYX_VM_HOSTNAME\" => ENV.fetch(\"BOMBYX_VM_HOSTNAME\", \"unknown\")\n    }"
            ),
            "the hash literal does not close cleanly:\n{out}"
        );
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
        // docs/vm-host-setup.md warns about.
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
    fn the_upload_block_carries_no_raise_of_its_own() {
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
    fn a_configured_key_is_announced_in_the_environment() {
        // `bootstrap.sh` must not decide whether a key was
        // configured by looking at the guest's own filesystem:
        // the upload lands in a directory the agent's user
        // owns, so a leftover or a `touch` would answer for the
        // operator's config.
        let out = render(&cfg_with_key());
        assert!(
            out.contains(&format!("\"{DEPLOY_KEY_ENV}\" => \"1\"")),
            "{out}"
        );
    }

    #[test]
    fn no_key_announces_a_zero_rather_than_nothing() {
        // Rendering nothing would leave the guest's own
        // environment to answer. [`DEPLOY_KEY_ENV`] says why.
        let out = render(&cfg_with(Provider::Libvirt));
        assert!(
            out.contains(&format!("\"{DEPLOY_KEY_ENV}\" => \"0\"")),
            "{out}"
        );
    }

    #[test]
    fn the_bootstrap_script_branches_on_the_announcement() {
        // Two files have to agree on the variable's name and
        // neither can see the other.
        assert!(
            BOOTSTRAP.contains(DEPLOY_KEY_ENV),
            "{DEPLOY_KEY_ENV} is not in the bootstrap script"
        );
        // A configured key that did not arrive is a failure,
        // not a silent skip. Without this the guest would
        // clone with no credential and report success.
        assert!(
            BOOTSTRAP.contains("did not arrive"),
            "the bootstrap script skips a key that never arrived"
        );
    }

    #[test]
    fn every_variable_is_declared_before_it_is_expanded() {
        // `set -u` makes expanding an unset variable fatal, so
        // a refusal that removes the deploy key before its path
        // is declared aborts with "unbound variable" -- the
        // message never prints and the key stays. The other
        // text tests here cannot catch it, because they compare
        // offsets of things that are present rather than asking
        // whether the shell can run them.
        //
        // Checked for the two the script declares itself.
        // `readonly` lines are the declarations; anything of
        // the form `$NAME` or `"$NAME"` before one is a use.
        let flat = flat_bootstrap_lines();
        for name in ["DEPLOY_KEY", "CLONE_DIR"] {
            let decl = flat
                .iter()
                .position(|l| l.starts_with(&format!("readonly {name}=")))
                .unwrap_or_else(|| panic!("{name} must be declared"));
            let first_use = flat.iter().position(|l| {
                !l.starts_with('#') && l.contains(&format!("${name}"))
            });
            if let Some(u) = first_use {
                assert!(
                    decl <= u,
                    "${name} is expanded at line {} and declared at {}",
                    u + 1,
                    decl + 1
                );
            }
        }
    }

    #[test]
    fn every_refusal_clears_the_uploaded_key() {
        // Vagrant uploads the key before this script starts, so
        // a refusal that exits without removing it leaves a
        // credential at whatever mode `scp` gave it, in a guest
        // that never provisioned.
        //
        // The rule was written as prose and broken four times
        // by the file's own refusals. So it is structural now:
        // `refuse` removes the key and exits, and no bare
        // `exit 1` is allowed outside it. The checks on
        // `$HOME` sit above the key block and refuse through
        // it, which `an_unusable_home_is_refused_by_name`
        // pins.
        let mut inside = false;
        for line in flat_bootstrap_lines() {
            if line.starts_with("refuse() {") {
                inside = true;
                continue;
            }
            if inside && line == "}" {
                inside = false;
                continue;
            }
            if line.starts_with('#') || !line.contains("exit 1") {
                continue;
            }
            assert!(
                inside,
                "a refusal that does not clear the key: {line}\n\
                 Call `refuse \"message\"` instead of exiting."
            );
        }
        assert!(
            flat_bootstrap().contains("rm -f \"$DEPLOY_KEY\""),
            "refuse must remove the uploaded key"
        );
    }

    #[test]
    fn every_command_on_the_key_reports_its_own_failure() {
        // The agent does the key removal, and `rm` gives up on
        // a file in a directory it cannot write -- which the
        // project's own script can arrange, because it has
        // `sudo` and this guest to itself.
        //
        // Under `set -e` an unguarded failure aborts the script
        // *inside* `refuse`, before either `echo`. So the
        // operator gets a bare `rm: Permission denied` naming
        // no part of bombyx, and the credential stays in a
        // guest that never provisioned -- the one invariant
        // this file is arranged around.
        //
        // Per line, because all three commands act on the same
        // path and a rule naming one of them misses the next.
        for line in flat_bootstrap_lines() {
            if line.starts_with('#') || !line.contains("$DEPLOY_KEY") {
                continue;
            }
            if !["rm ", "chmod ", "install "]
                .iter()
                .any(|c| line.contains(*c))
            {
                continue;
            }
            // `if `, and no `|| true` alternative. Silencing
            // one of these commands produces the outcome this
            // guard exists to prevent, and for the removal it
            // is worse than the abort: the script would carry
            // on and print "any uploaded deploy key has been
            // removed from this guest" over a key that is still
            // there.
            assert!(
                line.starts_with("if "),
                "this command on the key can abort without reporting:\n  \
                 {line}\n\
                 Test its status: `if ! ... ; then refuse \"...\"; fi`. \
                 Do not silence it -- a key left behind quietly is \
                 what this guard is for."
            );
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
    fn only_one_user_ever_verifies_the_git_host() {
        // One account clones and pushes, so `ssh` reads that
        // account's own `~/.ssh/known_hosts`. Naming a file
        // here would point the clone at a path the agent can
        // rewrite.
        assert!(
            !BOOTSTRAP.contains("UserKnownHostsFile"),
            "the clone must use the agent's own known_hosts"
        );
    }

    #[test]
    fn the_clone_sits_in_the_home_the_provisioner_was_given() {
        // `$HOME` is the account's own home because the
        // provisioner is unprivileged, so the shell that runs
        // `bootstrap.sh` was started with it already set.
        //
        // The derivation is pinned as a literal. Asserting only
        // that `$HOME` appears somewhere would pass with
        // `CLONE_DIR=/srv/project` written underneath a comment
        // that mentions it.
        let flat = flat_bootstrap();
        assert!(
            flat.contains("readonly CLONE_DIR=\"$HOME/project\""),
            "the clone must sit in the account's own home"
        );
    }

    #[test]
    fn an_unusable_home_is_refused_by_name() {
        // A project's `[env]` table can set `HOME`, so the
        // value is not guaranteed sound, and each shape below
        // otherwise reaches a bare `git` error naming no part
        // of bombyx. Each check is asserted as a literal
        // rather than as "a `case` exists": changing `/?*)` to
        // `?*)` re-admits every relative home, which is the
        // defect this family was raised for.
        let flat = flat_bootstrap();

        // Unset or empty. `${HOME:-}` and not `$HOME`, because
        // `set -u` aborts with "unbound variable" before
        // `refuse` can clear the uploaded key.
        assert!(
            flat.contains("if [ -z \"${HOME:-}\" ]; then"),
            "an unset HOME must be refused before set -u aborts"
        );
        // Relative, `.`, `..` and `/` itself. A relative home
        // puts the clone wherever the provisioner started, and
        // `/` gives `//project` under a root-owned parent.
        assert!(
            flat.contains("case \"$HOME\" in /?*) ;;"),
            "the shape check must refuse a relative home"
        );
        // Named but absent. `/nonexistent` is what
        // `useradd -M` writes.
        assert!(
            flat.contains("if [ ! -d \"$HOME\" ]; then"),
            "the home must be required to exist"
        );
        // Writable *and* searchable: a directory at mode 0600
        // passes `test -w` while `mkdir` in it fails.
        assert!(
            flat.contains("if [ ! -w \"$HOME\" ] || [ ! -x \"$HOME\" ]; then"),
            "the home must be writable and searchable"
        );

        // Owned by this account. `HOME=/tmp` passes every
        // check above -- set, absolute, present, and mode 1777
        // gives both bits -- and the clone would then sit in a
        // world-writable directory, `.git/config` and the
        // `core.sshCommand` naming the deploy key with it.
        assert!(
            flat.contains("if [ ! -O \"$HOME\" ]; then"),
            "the home must be owned by the account cloning into it"
        );

        // And the unset check comes first. Every other check
        // expands `$HOME` bare, so under `set -u` an unset
        // `HOME` reaching one of them aborts the script with
        // "unbound variable" -- before `refuse` can remove the
        // uploaded deploy key, which then stays in a guest that
        // never provisioned.
        //
        // `every_refusal_clears_the_uploaded_key` cannot see
        // this: it compares the offsets of `exit 1` and the
        // removal, and an abort is neither.
        let lines = flat_bootstrap_lines();
        let guard = lines
            .iter()
            .position(|l| l.contains("[ -z \"${HOME:-}\" ]"))
            .expect("the unset check must be there");
        let first_bare = lines
            .iter()
            .position(|l| !l.starts_with('#') && l.contains("\"$HOME\""))
            .expect("HOME must be used");
        assert!(
            guard < first_bare,
            "an unset HOME is expanded at line {} and checked at {}",
            first_bare + 1,
            guard + 1
        );
    }

    #[test]
    fn a_fetch_or_checkout_that_cannot_finish_says_so() {
        // Both fail on a tracked file inside a directory the
        // agent cannot write. `git checkout --force` exits 1
        // with "unable to unlink old ... Permission denied"
        // *after* printing "Switched to branch" -- measured --
        // so the worktree is half-changed and `set -e` then
        // aborts with nothing naming bombyx.
        //
        // Checked here rather than by sweeping the tree for
        // foreign ownership beforehand. A sweep refuses
        // provisions over build output no git command touches,
        // cannot report itself when it meets an unsearchable
        // directory, and a symlink at the clone skips it.
        let flat = flat_bootstrap();
        for needle in [
            "if ! git -C \"$CLONE_DIR\" fetch",
            "if ! git -C \"$CLONE_DIR\" checkout",
        ] {
            assert!(flat.contains(needle), "unchecked: {needle}");
        }
        assert!(
            flat.contains("could not update the clone"),
            "a failure must name the path and say what to do"
        );
    }

    #[test]
    fn a_reclone_over_a_leftover_directory_says_so() {
        // A discard that failed part-way may already have
        // deleted `.git`, so the next provision skips the
        // mismatch branch entirely -- the message that
        // diagnosed it is unreachable -- and `git clone` dies
        // with "destination path already exists and is not an
        // empty directory". Measured.
        let flat = flat_bootstrap();
        assert!(
            flat.contains("is not empty"),
            "a non-empty clone directory must be refused by name"
        );
    }

    #[test]
    fn a_discard_that_cannot_finish_says_so() {
        // `rm -rf` fails on a directory inside the clone the
        // agent cannot write, and GNU `rm` does not chmod its
        // way in. Measured: exit 1, "Permission denied", and a
        // partly deleted tree. Nothing normalises such content,
        // deliberately: a root `chown -R` on a tree the agent
        // owns is the escalation this arrangement removes.
        //
        // A project script running one `sudo` step inside its
        // own checkout is enough to reach it -- and
        // `docs/tutorial.md` documents `sudo` as available. So
        // the status is tested and the operator is told what to
        // remove, rather than `set -e` aborting after the
        // "discarding the clone" message has already printed.
        let flat = flat_bootstrap();
        assert!(
            flat.contains("if ! rm -rf \"$CLONE_DIR\""),
            "the discard's failure must be caught"
        );
        assert!(
            flat.contains("could not remove"),
            "a failed discard must name the path and say what to do"
        );
    }

    #[test]
    fn the_clone_is_pinned_from_the_config_not_the_environment() {
        // `GIT_SSH_COMMAND` is inherited, and this file's own
        // header says whether a key was configured must never
        // be re-derived from the guest -- a `/etc/profile.d`
        // export reaches a login shell provisioner. Deciding
        // the `core.sshCommand` write from it would let the
        // guest re-pin the clone to a key the operator had just
        // removed.
        //
        // `--replace-all`, because a plain set refuses with
        // "cannot overwrite multiple values" and exits 5 when
        // the key already has two -- measured -- which `set -e`
        // would turn into an aborted provision after the clone
        // and fetch had already run.
        let flat = flat_bootstrap();
        assert!(
            flat.contains("config --replace-all core.sshCommand"),
            "the pin must use --replace-all"
        );
        assert!(
            !flat.contains("if [ -n \"${GIT_SSH_COMMAND:-}\" ]"),
            "the write must not be gated on the inherited value"
        );
        assert!(
            flat.contains("if [ \"${BOMBYX_DEPLOY_KEY:-}\" = 1 ]"),
            "the authoritative flag must be what decides"
        );
    }

    #[test]
    fn removing_the_key_unpins_the_clone_from_it() {
        // The mirror of the comment about a key nothing points
        // at: a pointer to no key. `core.sshCommand` names the
        // deleted identity, and `IdentitiesOnly=yes` with
        // `-F /dev/null` stops git falling back to one the
        // agent does hold -- so every fetch and push fails with
        // an ssh error naming nothing about bombyx.
        // `--unset-all`, not `--unset`: measured, `--unset`
        // against two values warns, exits 5 and removes
        // nothing -- indistinguishable from the exit 5 that
        // means "there was nothing to unset".
        let flat = flat_bootstrap();
        assert!(
            flat.contains("config --unset-all core.sshCommand"),
            "the clone keeps pointing at a key that is gone"
        );
        // And it has to run after the clone exists, since
        // there is no config file to unset anything from
        // before that.
        let clone = flat
            .find("git clone --depth 1")
            .expect("the clone must be there");
        let unset = flat
            .find("config --unset-all core.sshCommand")
            .expect("the unset must be there");
        assert!(clone < unset, "the clone must exist before the unset");
    }

    #[test]
    fn the_deploy_key_ends_up_readable_by_the_agent() {
        // The agent has to push with this key: `bootstrap.sh`
        // says committing in the guest does not survive a
        // provision and pushing is what does. A root-owned key
        // the agent cannot read makes that impossible, so the
        // key is left at 0600 where the provisioner put it
        // rather than moved out of reach.
        //
        // It is owned by the agent because Vagrant's file
        // provisioner uploads as that user -- not because
        // bombyx chowns it. There is therefore no chown to
        // assert. `docs/trust-boundary.md` under **What this
        // costs** states what the mode does and does not buy.
        assert!(
            BOOTSTRAP.contains("chmod 600 \"$DEPLOY_KEY\""),
            "the key must end up at 0600"
        );
        // The two negatives forbid the shapes that would move
        // the key out of the agent's reach: an `install` that
        // places it as another user, and an ownership flag on
        // the same operation. Either one leaves the agent
        // unable to push.
        for gone in ["-o root", "install -m 600"] {
            assert!(!BOOTSTRAP.contains(gone), "{gone} is back");
        }
    }

    #[test]
    fn the_shell_provisioner_runs_unprivileged() {
        // Vagrant runs a shell provisioner as root unless the
        // Vagrantfile says otherwise, and `bootstrap.sh` needs
        // no root: every command in it acts on the agent's own
        // home, and a project that has to install something
        // calls `sudo` from its own script.
        //
        // Root here would also put the operator's `[env]`
        // values into root's environment for the whole of
        // `bootstrap.sh`, where `PATH` decides which `git` and
        // which `readlink` run. Measured: an `[env]` entry
        // setting `PATH` reaches this script's own environment,
        // and `/etc/profile` does not overwrite it -- a garbage
        // value stops the run at the `#!/usr/bin/env bash`
        // line.
        //
        // Counted rather than found. A second shell
        // provisioner added to the template would leave a
        // single-substring check green while running as root,
        // and the flag's own guard has the failure mode the
        // comment above describes for the script.
        //
        // `cfg_with_key` rather than `cfg_with`, so the file
        // provisioner is rendered too and the count has
        // something to be wrong about.
        for provider in [Provider::Libvirt, Provider::Hyperv] {
            let mut cfg = cfg_with_key();
            cfg.vm.provider = provider;
            let out = render(&cfg);
            assert!(!out.contains("privileged: true"), "{out}");
            // Per shell provisioner, not per file. `privileged:`
            // is legal on a `file` provisioner as well, so
            // counting the flag over the whole rendering would
            // let one added there stand in for a second shell
            // provisioner that carries none.
            //
            // Each block runs to the next `config.vm.provision`
            // or to the end, and the flag has to be inside it.
            let shell_blocks: Vec<&str> = out
                .split("config.vm.provision ")
                .skip(1)
                .filter(|b| b.starts_with("\"shell\""))
                .collect();
            assert!(
                !shell_blocks.is_empty(),
                "the fixture must render a shell provisioner:\n{out}"
            );
            for block in shell_blocks {
                assert!(
                    block.contains("privileged: false"),
                    "a shell provisioner with no flag:\n{block}\n\
                     in:\n{out}"
                );
            }
            // And the whole clause, which pins the comma that
            // keeps the Ruby parseable. Only the shell
            // provisioner has an `env:` hash, so this also says
            // which provisioner the flag belongs to.
            assert!(
                out.contains("privileged: false,\n    env: {\n"),
                "the flag must sit on the shell provisioner:\n{out}"
            );
        }
    }

    #[test]
    fn nothing_in_the_bootstrap_script_asks_for_root() {
        // `the_shell_provisioner_runs_unprivileged` is one half
        // of the arrangement and this is the other. A line here
        // that raises privilege puts root back inside a tree the
        // agent owns, and the rendered flag would go on saying
        // the script is unprivileged.
        //
        // Root in that tree is a measured escalation rather than
        // a theoretical one. git trusts the uid in `SUDO_UID` as
        // well as root's own (see `safe.directory` in
        // git-config(1)), so a `post-checkout` hook the agent
        // planted was seen running with `uid=0`.
        //
        // Comments are skipped: they name `sudo` where they
        // describe what a project's own script may do, and that
        // is a true statement about a different script.
        //
        // The list enumerates the family, and there is no
        // allowance list to add a name to: a command that
        // changes the effective user has no business in this
        // file at all. `setpriv` and `sg` are here because
        // `setpriv --reuid 0` ships in util-linux beside
        // `runuser`, and `sg`/`newgrp` do the same for a group.
        // A new one has to be added here by hand, which is the
        // trade -- a test cannot parse shell.
        const RAISERS: [&str; 8] = [
            "runuser", "sudo", "su", "pkexec", "doas", "setpriv", "sg",
            "newgrp",
        ];

        for line in flat_bootstrap_lines() {
            if line.starts_with('#') {
                continue;
            }
            // The word, not the letters, so `issue` and
            // `status` are left alone while `/usr/sbin/runuser`
            // still counts. `.` and `/` count as part of a word
            // so that `${a%.git}` is not read as the command
            // `git`, while `/usr/bin/git` still is.
            let word_char = |c: char| {
                c.is_alphanumeric() || c == '_' || c == '/' || c == '.'
            };
            for word in line.split(|c: char| !word_char(c)) {
                // The last path component, so `/usr/sbin/runuser`
                // counts and `/usr/bin/sg` does. Comparing the
                // whole word would miss both.
                let command = word.rsplit('/').next().unwrap_or(word);
                assert!(
                    !RAISERS.contains(&command),
                    "this line raises privilege:\n  {line}\n\
                     The provisioner is unprivileged and every \
                     command here acts on the agent's own home. \
                     A project needing root calls `sudo` from \
                     its own script."
                );
            }
            // Both spellings of root's own home. `~root` expands
            // to it without the string `/root` appearing, so a
            // check on the path alone would let `~root/.ssh`
            // through.
            for reach in ["/root", "~root"] {
                assert!(
                    !line.contains(reach),
                    "this line reaches root's own home:\n  {line}\n\
                     The provisioner is unprivileged, so nothing \
                     here can write there."
                );
            }
        }
    }

    #[test]
    fn the_clone_is_told_which_key_to_push_with() {
        // Otherwise the agent has a key it may read and no
        // reason to know where it is. Recording it on the
        // clone means a plain `git push` in the guest works
        // without the project's script arranging anything.
        assert!(
            flat_bootstrap().contains("config --replace-all core.sshCommand"),
            "the clone must record the ssh command"
        );
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
    fn the_project_script_runs_as_the_agent_not_as_root() {
        // The whole provisioner is unprivileged, so this
        // `exec` changes no privilege. What it does decide is
        // what the process becomes: `exec` replaces this
        // script rather than starting a second process beside
        // it, so the project's script inherits the process and
        // its exit status is what Vagrant sees.
        // `docs/architecture.md` under **Who runs the
        // project's script** holds the argument.
        //
        // The needle is the whole `exec` line, because a call
        // that started the script as a child would still
        // contain the path somewhere in the file.
        assert!(
            BOOTSTRAP.contains("exec -- \"$script_real\""),
            "the hand-over must exec the project's script"
        );
        // And the bit `exec` needs is set first. `chmod +x` is
        // the one metadata change bombyx makes inside the
        // clone, so nothing else asserts it.
        assert!(
            BOOTSTRAP.contains("chmod +x \"$script_real\""),
            "the script must be made executable before the exec"
        );
    }

    #[test]
    fn the_bootstrap_script_guards_every_variable_it_needs() {
        // Without the `:?` guards an unset variable clones into
        // an empty path.
        for guard in ["BOMBYX_REPO:?", "BOMBYX_REF:?", "BOMBYX_SCRIPT:?"] {
            assert!(BOOTSTRAP.contains(guard), "{guard} missing");
        }
        assert!(BOOTSTRAP.contains("set -euo pipefail"));
    }
}
