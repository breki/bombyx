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

use crate::config::{Config, DeployKeyPath};

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
/// box is free to set another, and some do. `bootstrap.sh`'s
/// `OWNER` rests on the same assumption. On a box that sets a
/// different user the upload fails inside Vagrant, a long way
/// from `box` in the config.
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
/// `/etc/profile.d/*.sh` before the script. An export placed
/// there reaches `bootstrap.sh` unopposed, because a
/// provisioner's `env:` block is what overrides the guest's own
/// environment. Rendering the entry only for a configured key
/// would leave the *no-key* case forgeable in exactly the
/// direction that matters: the guest could claim a key was
/// configured and keep a stale credential alive. Naming it
/// always means the config answers either way.
const DEPLOY_KEY_ENV: &str = "BOMBYX_DEPLOY_KEY";

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
      \"{deploy_key_env_name}\" => \"{deploy_key_env}\",
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
        deploy_key = deploy_key_block(source.deploy_key.as_ref()),
        deploy_key_env_name = DEPLOY_KEY_ENV,
        deploy_key_env = deploy_key_env(source.deploy_key.as_ref()),
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
        BoxName, DeployKeyPath, GitRef, Provider, RepoUrl, ScriptPath, Source,
        Vm,
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
    fn nothing_exits_before_the_key_is_dealt_with() {
        // Vagrant has already uploaded the key by the time this
        // script starts, so *any* `exit` above the point where
        // the key is tightened or removed leaves a credential
        // in the agent's own directory for the life of the VM.
        //
        // The predecessor of this test compared against
        // `command -v git` and located the key block by the
        // string `BOMBYX_DEPLOY_KEY`, whose first occurrence is
        // a comment 50 lines above the block -- so it passed
        // while a `runuser` refusal sat above the key. This one
        // takes the first `exit` in the file and the first line
        // that actually removes the key.
        let flat = flat_bootstrap();
        let first_exit =
            flat.find("exit 1").expect("the script refuses somewhere");
        let removes_key = flat
            .find("rm -f \"$DEPLOY_KEY\"")
            .expect("the script must be able to remove the key");
        assert!(
            removes_key < first_exit,
            "an exit above the key handling strands the uploaded key"
        );
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
    fn root_never_changes_metadata_on_a_path_the_agent_owns() {
        // `chmod` and `chown` follow symlinks, and the key
        // lands in a directory the agent's own user owns. Root
        // running either one there lets the agent point it at
        // any file in the guest and have root act on that
        // instead. Doing the work as $OWNER removes the
        // asymmetry: a symlink then buys nothing the agent did
        // not already have.
        let forbidden = "chown \"$OWNER:$OWNER\" \"$DEPLOY_KEY\"";
        assert!(
            !BOOTSTRAP.contains(forbidden),
            "{forbidden} would run as root on an agent-owned path"
        );
        for needed in [
            "\"$runuser_bin\" -u \"$OWNER\" -- chmod 600 \"$DEPLOY_KEY\"",
            "\"$runuser_bin\" -u \"$OWNER\" -- rm -f \"$DEPLOY_KEY\"",
            "\"$runuser_bin\" -u \"$OWNER\" -- chmod +x \"$script_real\"",
        ] {
            assert!(BOOTSTRAP.contains(needed), "{needed} is missing");
        }
    }

    #[test]
    fn runuser_is_resolved_and_checked_before_the_clone() {
        // A box without it currently downloads a shallow
        // clone, takes delivery of a private-repo credential
        // and chowns a tree before refusing -- everything the
        // refusal was meant to prevent, already done.
        let check = BOOTSTRAP
            .find("runuser_bin=")
            .expect("the binary must be resolved");
        // The real command, not the comment above that
        // mentions `git clone` in passing.
        let clone = BOOTSTRAP
            .find("git clone --depth 1")
            .expect("the clone must be in the script");
        assert!(check < clone, "resolve runuser before cloning");

        // `command -v` searches PATH, and runuser lives in
        // /usr/sbin. A root login shell has it; a root
        // environment somebody else arranged need not, and the
        // message must not blame a missing package then.
        assert!(BOOTSTRAP.contains("/usr/sbin/runuser"), "{BOOTSTRAP}");
        assert!(BOOTSTRAP.contains("was not found on PATH"), "{BOOTSTRAP}");
    }

    #[test]
    fn only_one_user_ever_verifies_the_git_host() {
        // Every git command runs as the agent, so one user
        // clones and pushes and ssh uses that user's own
        // `~/.ssh/known_hosts`. Naming a file here would only
        // split the host-key trust across two principals
        // again: root would record the key on the clone and
        // the agent would meet the host afresh on its first
        // push, or they would share a file the agent can
        // rewrite.
        assert!(
            !BOOTSTRAP.contains("UserKnownHostsFile"),
            "a shared known_hosts is not needed any more"
        );
    }

    #[test]
    fn no_git_command_in_the_guest_runs_as_root() {
        // Root running `git` inside a tree the agent owns is a
        // measured escalation, not a theoretical one: git
        // trusts the uid in `SUDO_UID` as well as root's own
        // (see `safe.directory` in git-config(1)) and Vagrant
        // runs this script through `sudo`. A `post-checkout`
        // hook planted as the agent was seen running with
        // `uid=0`. This test is the only thing standing between
        // a future edit and its return.
        //
        // The rule is deliberately crude: every line holding
        // `git` as a word must also hold `$runuser_bin`, and
        // the lines that legitimately do not are named here.
        //
        // Crude because a test cannot parse shell. Anything
        // cleverer has to decide where a command starts, and
        // `git` can start one after `=`, after `$(`, after
        // `while` or `!`, inside `{ }`, or behind `env`,
        // `eval`, `xargs` or a variable holding its path. A
        // check that enumerates those misses the next one.
        //
        // **This over-approximates on purpose.** A line that
        // merely mentions git -- an assignment holding a path,
        // an error message -- fails until it is named below.
        // That is the trade: a test cannot parse shell, and the
        // two attempts that tried to approximate it both let
        // the escalation shape through. Being told to justify a
        // new mention of `git` in a file that runs as root in
        // the guest is cheap; the failure message says so, so
        // nobody reads it as the test being broken.
        const ALLOWED_WITHOUT_RUNUSER: [&str; 3] = [
            // Asks whether git exists. Runs nothing.
            "if ! command -v git >/dev/null 2>&1; then",
            // Two error messages. The second is written
            // across continued lines and joins into one.
            "echo \"bombyx: git is not installed in this box.\" >&2",
            "echo \"bombyx: install it in the box, or choose one with\" \
             \"git, so the guest can clone the project.\" >&2",
        ];
        let mut allowed_seen = [false; 3];

        for line in flat_bootstrap_lines() {
            if line.starts_with('#') {
                continue;
            }
            // The word `git`, not the letters. `.` and `/`
            // count as part of a word so that `${a%.git}` and
            // `"$CLONE_DIR/.git"` are not read as the command
            // `git`, while `/usr/bin/git` still is. That
            // distinction is why the earlier versions of this
            // test failed on correct lines.
            let word_char = |c: char| {
                c.is_alphanumeric() || c == '_' || c == '/' || c == '.'
            };
            let is_word = line
                .split(|c: char| !word_char(c))
                .any(|w| w == "git" || w.ends_with("/git"));
            if !is_word {
                continue;
            }
            if let Some(i) =
                ALLOWED_WITHOUT_RUNUSER.iter().position(|a| *a == line)
            {
                allowed_seen[i] = true;
                continue;
            }
            assert!(
                line.contains("$runuser_bin"),
                "this line may run git as root:\n  {line}\n\
                 Run it through `\"$runuser_bin\" -u \"$OWNER\" --`. \
                 If it does not invoke git at all -- an \
                 assignment, a message -- add it verbatim to \
                 ALLOWED_WITHOUT_RUNUSER above, which is \
                 deliberately a list somebody has to read."
            );
        }

        // Every allowance is still earned. One that stops
        // matching is a line that changed, and it should be
        // re-read rather than left in the list.
        for (i, seen) in allowed_seen.iter().enumerate() {
            assert!(
                *seen,
                "stale allowance, no line matches: {}",
                ALLOWED_WITHOUT_RUNUSER[i]
            );
        }
    }

    #[test]
    fn root_prepares_the_clone_directory_and_nothing_else() {
        // `/opt` belongs to root, so only root can create the
        // directory or remove it. Everything inside it is the
        // agent's, which is why the recursive chown comes
        // first rather than last: it normalises a tree an
        // earlier bombyx left with root-owned files in it.
        let mkdir = BOOTSTRAP
            .find("mkdir -p \"$CLONE_DIR\"")
            .expect("root must create the clone directory");
        let chown = BOOTSTRAP
            .find("chown -R \"$OWNER:$OWNER\" \"$CLONE_DIR\"")
            .expect("root must hand the directory over");
        let clone = BOOTSTRAP
            .find("git clone --depth 1")
            .expect("the clone must be in the script");
        assert!(mkdir < chown, "create before handing over");
        assert!(chown < clone, "hand over before cloning");
    }

    #[test]
    fn the_placement_an_earlier_bombyx_used_is_cleared() {
        // Guests built by an earlier bombyx have a root-owned
        // key at the old path, and nothing would ever remove
        // it -- so "removing `deploy_key` removes the key from
        // the guest", which the CHANGELOG and
        // `docs/trust-boundary.md` both promise, would be false
        // on every VM that already exists.
        //
        // Root does the removal, and that is safe here in a way
        // it would not be for the new path: `/root/.ssh` is
        // root's own, so there is no directory the agent could
        // put a symlink in.
        assert!(
            BOOTSTRAP.contains("rm -f /root/.ssh/bombyx-deploy-key"),
            "the old key placement is never cleared"
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
        // And it has to run after the chown that normalises a
        // tree an earlier bombyx left root-owned files in,
        // because git as $OWNER refuses a repository it does
        // not own.
        let chown = flat
            .find("chown -R \"$OWNER:$OWNER\" \"$CLONE_DIR\"")
            .expect("the normalising chown must be there");
        let unset = flat
            .find("config --unset-all core.sshCommand")
            .expect("the unset must be there");
        assert!(chown < unset, "normalise ownership before unsetting");
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
        // assert, and root must not run one here, which
        // `root_never_changes_metadata_on_a_path_the_agent_owns`
        // pins. `docs/trust-boundary.md` under **What this
        // costs** states what the mode does and does not buy.
        assert!(
            BOOTSTRAP.contains("chmod 600 \"$DEPLOY_KEY\""),
            "the key must end up at 0600"
        );
        // The negatives name the commands the old placement
        // used, not the path: forbidding the string `/root/.ssh`
        // would also forbid a comment explaining why the key is
        // not there, which is a test failing for a correct
        // change.
        for gone in ["-o root", "install -m 600"] {
            assert!(!BOOTSTRAP.contains(gone), "{gone} is back");
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
        // Everything above the hand-over needs root: the
        // clone, the chown, the deploy key. The project's own
        // script does not, and running it as root puts its
        // toolchain in root's home rather than in the account
        // the agent logs in as. `docs/architecture.md` under
        // **Who runs the project's script** holds the
        // argument.
        //
        // The needle is the whole `exec` line, because the
        // point is what the process becomes -- an `exec` that
        // dropped the `runuser` would still contain both words
        // somewhere in the file.
        assert!(
            BOOTSTRAP.contains(
                "exec -- \"$runuser_bin\" -u \"$OWNER\" -- \"$script_real\""
            ),
            "the hand-over must drop to $OWNER"
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
