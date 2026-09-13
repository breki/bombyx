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
use crate::hostkeys;

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
/// entry only for a configured key would leave the *no-key*
/// case forgeable in exactly the direction that matters: the
/// guest could claim a key was configured and keep a stale
/// credential alive. Naming it always means the config
/// answers either way.
const DEPLOY_KEY_ENV: &str = "BOMBYX_DEPLOY_KEY";

/// Repository the guest clones, as the guest's shell sees it.
const REPO_ENV: &str = "BOMBYX_REPO";

/// Branch or tag the guest checks out.
const REF_ENV: &str = "BOMBYX_REF";

/// Provisioning script the guest runs out of the clone.
const SCRIPT_ENV: &str = "BOMBYX_SCRIPT";

/// The secrets file's name in the project directory on the VM
/// host.
///
/// Public because `crate::plan` builds the command that writes
/// it and the command that removes it again, and both have to
/// spell it the same way [`render`] does.
///
/// Unlike [`VAGRANTFILE_NAME`] and [`BOOTSTRAP_NAME`] this file
/// is not generated, so it is absent from [`files`]. Its
/// contents come from the workstation and reach the VM host on
/// a pipe.
pub const ENV_FILE_NAME: &str = "bombyx.env";

/// Where the secrets file lands inside the guest.
///
/// Written with a `~` rather than spelled out, unlike
/// [`DEPLOY_KEY_GUEST_PATH`]. Vagrant expands an upload's
/// `destination:` by running `printf <destination>` through a
/// shell **inside the guest** before it sends anything -- read
/// in vagrant 2.4.9, `plugins/provisioners/file/provisioner.rb`
/// and `plugins/guests/linux/cap/shell_expand_guest_path.rb`.
/// That shell runs as the account vagrant logs in as, because
/// the communicator's `execute` defaults to `sudo: false`. So
/// the file lands in that account's real home whatever the box
/// calls the account.
///
/// [`BOOTSTRAP`] cannot spell the same path with `$HOME`. A
/// project's `[env]` table may set `HOME`, and the shell
/// provisioner carries that value while the upload above used
/// the account's real home. The script reads the passwd entry
/// instead, which is what the two agree on.
const ENV_FILE_GUEST_PATH: &str = "~/.bombyx-env";

/// Environment variable telling the guest that the operator's
/// config named an `env_file`.
///
/// Set on every render, `"1"` or `"0"`, for the reason
/// [`DEPLOY_KEY_ENV`] gives at length: a name bombyx leaves out
/// is left to `/etc/profile`, so the guest could answer the
/// question on the operator's behalf.
///
/// Distinct from `BOMBYX_ENV_FILE`, which [`BOOTSTRAP`] exports
/// for the project's own script and which holds the guest path
/// rather than a flag.
const ENV_FILE_ENV: &str = "BOMBYX_ENV_FILE_PRESENT";

/// Environment variable naming the git host, lower-cased, when
/// bombyx knows where that host publishes its ssh keys.
///
/// Empty for every other repository, which covers a host absent
/// from `crate::hostkeys`'s table and an `https` URL, where
/// `git` opens no ssh connection at all.
///
/// The guest writes this in front of each key to make a
/// `known_hosts` line, so it has to be the canonical spelling
/// rather than the operator's. `crate::hostkeys::HostKeys::host`
/// says why either matches.
const GIT_HOST_ENV: &str = "BOMBYX_GIT_HOST";

/// Environment variable holding the URL the guest fetches the
/// git host's ssh keys from, and empty when there is none.
///
/// **This one variable decides whether the guest verifies the
/// git host at all.** [`BOOTSTRAP`] fetches and insists on the
/// keys when it is set, and falls back to
/// `StrictHostKeyChecking=accept-new` when it is empty.
///
/// A guest cannot forge it or clear it, and one fact covers
/// both: [`DEPLOY_KEY_ENV`] explains that a provisioner's
/// `env:` block overrides what `/etc/profile.d` sets, and this
/// name is rendered on every render for that reason.
///
/// That is about the operator's config reaching the guest
/// intact, and not about defending the guest from itself --
/// `docs/trust-boundary.md` explains why the second is not on
/// offer.
///
/// Left *unset* -- rather than empty -- the guest falls back to
/// `accept-new`. The one route there is a `vagrant provision`
/// run by hand in a directory an older bombyx wrote, whose
/// Vagrantfile does not set this name. That guest then behaves
/// as the bombyx that wrote its directory did, which is the
/// answer that surprises nobody.
const HOST_KEYS_URL_ENV: &str = "BOMBYX_HOST_KEYS_URL";

/// Environment variable telling the guest what the fetched
/// response looks like, and empty when nothing is fetched.
///
/// `crate::hostkeys::KeyFormat::as_str` produces the two words
/// this may hold, and [`BOOTSTRAP`] matches them.
const HOST_KEYS_FORMAT_ENV: &str = "BOMBYX_HOST_KEYS_FORMAT";

/// Every variable bombyx sets in the provisioner itself.
///
/// Test-only, because nothing in the rendering reads it: the
/// nine names are written into the template one by one, in
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
const BOMBYX_ENV_NAMES: [&str; 9] = [
    REPO_ENV,
    REF_ENV,
    SCRIPT_ENV,
    DEPLOY_KEY_ENV,
    GIT_HOST_ENV,
    HOST_KEYS_URL_ENV,
    HOST_KEYS_FORMAT_ENV,
    crate::remote::VM_HOST_ENV,
    crate::remote::VM_HOSTNAME_ENV,
];

/// [`BOOTSTRAP`] as code, with every comment line dropped and
/// the remaining text flattened to single spaces.
///
/// **A positive needle asserted over the whole file can be
/// satisfied by the script's own prose**, which is how a lint
/// comes to guard nothing: that script explains each thing it
/// does in a comment above the doing of it, so the words are
/// there either way. `GIT_SSH_COMMAND`, `IdentitiesOnly=yes`,
/// `-F /dev/null` and `BOMBYX_DEPLOY_KEY` are all in that
/// position.
///
/// So an assertion that the script *does* something goes
/// through here. An assertion that it does *not* contain
/// something is better off over the raw text, where a mention
/// in a comment is also worth refusing.
///
/// Line continuations are joined first, so a command wrapped
/// across lines is one string here. It lives beside the
/// production code rather than in either test module because
/// both of them need it, the same reason [`BOMBYX_ENV_NAMES`]
/// sits here.
#[cfg(test)]
fn bootstrap_code() -> String {
    BOOTSTRAP
        .replace("\\\n", " ")
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

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
    // `None` when `repo` reaches the server by something other
    // than ssh, and when it names a host bombyx has no key
    // source for.
    let host_keys = source.repo.ssh_host().and_then(hostkeys::for_host);
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

{deploy_key}{env_file}  config.vm.provision \"shell\",
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
      \"{env_file_env_name}\" => \"{env_file_env}\",
      # Which git host the guest is about to clone from, and
      # where that host publishes its ssh keys. All three are
      # empty when bombyx does not know the host, and
      # bootstrap.sh then accepts the key it is offered.
      \"{git_host_env}\" => {git_host},
      \"{host_keys_url_env}\" => {host_keys_url},
      \"{host_keys_format_env}\" => {host_keys_format},
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
        env_file = env_file_block(source.env_file.is_some()),
        repo_env = REPO_ENV,
        ref_env = REF_ENV,
        script_env = SCRIPT_ENV,
        deploy_key_env_name = DEPLOY_KEY_ENV,
        deploy_key_env = deploy_key_env(source.deploy_key.as_ref()),
        env_file_env_name = ENV_FILE_ENV,
        env_file_env = if source.env_file.is_some() { "1" } else { "0" },
        git_host_env = GIT_HOST_ENV,
        git_host = ruby_string(host_keys.map_or("", |k| k.host())),
        host_keys_url_env = HOST_KEYS_URL_ENV,
        host_keys_url = ruby_string(host_keys.map_or("", |k| k.url())),
        host_keys_format_env = HOST_KEYS_FORMAT_ENV,
        host_keys_format =
            ruby_string(host_keys.map_or("", |k| k.format().as_str())),
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

/// The Ruby that uploads the project's secrets file, or nothing
/// at all.
///
/// An empty string when the config names no `env_file`, so a
/// project without one gets a Vagrantfile with no upload block.
///
/// [`render`] places this ahead of the shell provisioner, so
/// [`BOOTSTRAP`] finds the file already there.
///
/// The upload is conditional for the reason
/// [`deploy_key_block`] gives: `vagrant destroy` loads this file
/// too, and by then `crate::plan` has removed the secrets file
/// from the VM host, so a `raise` would strand a directory no
/// bombyx command could clear.
///
/// The `source:` is resolved against the Vagrantfile's own
/// directory rather than the process's. Vagrant runs the file
/// through `Kernel.load`, so `__dir__` names that directory.
fn env_file_block(configured: bool) -> String {
    if !configured {
        return String::new();
    }
    format!(
        "  # The project's secrets, carried from the workstation.
  # bombyx wrote this file beside the Vagrantfile a moment ago
  # and removes it again when vagrant finishes, so the VM host
  # keeps no copy. docs/trust-boundary.md says what keeping it
  # inside the guest costs.
  #
  # The destination is expanded by a shell inside the guest, so
  # it lands in the real home of the account vagrant logs in as.
  bombyx_env_file = File.expand_path({name}, __dir__)
  if File.exist?(bombyx_env_file)
    config.vm.provision \"file\",
      source: bombyx_env_file,
      destination: {dest}
  end

",
        name = ruby_string(ENV_FILE_NAME),
        dest = ruby_string(ENV_FILE_GUEST_PATH),
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

// The shell text of `bootstrap.sh` is linted in
// `bootstrap_tests.rs`, whose header holds the argument for the
// split. A plain comment rather than `///`: neither rustdoc
// pass renders a `cfg(test)` item, so a doc link here would
// never be checked.
#[cfg(test)]
mod bootstrap_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    use crate::config::{
        BoxName, DeployKeyPath, EnvFilePath, EnvName, EnvValue, GitRef,
        Provider, RESERVED_PREFIX, RepoUrl, ScriptPath, Source, Vm,
    };

    /// A `deploy_key` value every rule accepts, written once so
    /// the tests below and the expected Ruby agree.
    const KEY: &str = "~/.secrets/myproject-deploy-key";

    /// An `env_file` value every rule accepts, distinctive
    /// enough that a test can assert it reaches no rendered
    /// file.
    const ENV_FILE: &str = "~/.secrets/myproject-unrendered.env";

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
            env_file: None,
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

    /// [`cfg_with`] on libvirt, cloning `repo` over ssh.
    fn cfg_cloning(repo: &str) -> Config {
        let mut cfg = cfg_with(Provider::Libvirt);
        cfg.source.repo = RepoUrl::parse(repo).expect("a valid fixture URL");
        cfg
    }

    /// [`cfg_with`] on libvirt, carrying a `deploy_key`.
    fn cfg_with_key() -> Config {
        let mut cfg = cfg_with(Provider::Libvirt);
        cfg.source.deploy_key =
            Some(DeployKeyPath::parse(KEY).expect("a valid fixture path"));
        cfg
    }

    /// [`cfg_with`] on libvirt, carrying an `env_file`.
    fn cfg_with_env_file() -> Config {
        let mut cfg = cfg_with(Provider::Libvirt);
        cfg.source.env_file =
            Some(EnvFilePath::parse(ENV_FILE).expect("a valid fixture path"));
        cfg
    }

    /// What one provisioner variable renders as in `out`,
    /// quotes included.
    ///
    /// It has to appear exactly once: a second rendering of the
    /// same name would make the value read back here depend on
    /// which one came first.
    ///
    /// One name at a time, so a failed assertion names the
    /// variable that was wrong. Reading all three into a
    /// positional list made every failure read as three quoted
    /// strings against three others.
    fn rendered(out: &str, name: &str) -> String {
        let head = format!("\"{name}\" => ");
        assert_eq!(
            out.matches(&head).count(),
            1,
            "{name} is not rendered exactly once"
        );
        let rest = out
            .split_once(&head)
            .unwrap_or_else(|| panic!("{name} is not rendered"))
            .1;
        rest.split_once(",\n")
            .unwrap_or_else(|| panic!("{name} has no value"))
            .0
            .to_owned()
    }

    /// Asserts all three host-key variables of `out` at once.
    fn assert_host_keys(out: &str, host: &str, url: &str, format: &str) {
        assert_eq!(rendered(out, GIT_HOST_ENV), host, "{GIT_HOST_ENV}");
        assert_eq!(
            rendered(out, HOST_KEYS_URL_ENV),
            url,
            "{HOST_KEYS_URL_ENV}"
        );
        assert_eq!(
            rendered(out, HOST_KEYS_FORMAT_ENV),
            format,
            "{HOST_KEYS_FORMAT_ENV}"
        );
    }

    #[test]
    fn a_github_clone_over_ssh_is_told_where_the_keys_are() {
        let out = render(&cfg_cloning("git@github.com:you/private.git"));
        assert_host_keys(
            &out,
            "\"github.com\"",
            "\"https://api.github.com/meta\"",
            "\"json\"",
        );
    }

    #[test]
    fn a_bitbucket_clone_needs_no_json_parsing() {
        let out = render(&cfg_cloning("ssh://git@bitbucket.org/you/p.git"));
        assert_host_keys(
            &out,
            "\"bitbucket.org\"",
            "\"https://bitbucket.org/site/ssh\"",
            "\"lines\"",
        );
    }

    #[test]
    fn the_operators_own_spelling_never_reaches_the_guest() {
        // The guest puts this in front of every fetched key, so
        // it has to be the table's name. A capital here would
        // not break the clone -- OpenSSH folds case -- but it
        // would put a spelling in `known_hosts` that came from
        // the config rather than from bombyx.
        let out = render(&cfg_cloning("git@GitHub.COM:you/p.git"));
        assert_eq!(rendered(&out, GIT_HOST_ENV), "\"github.com\"");
    }

    #[test]
    fn the_three_names_are_rendered_empty_rather_than_left_out() {
        // Empty is what tells `bootstrap.sh` to fall back, and
        // rendering the names on every render is what stops an
        // /etc/profile.d export answering for the config.
        // `HOST_KEYS_URL_ENV` holds that argument.
        for repo in [
            // No ssh connection at all.
            "https://github.com/you/public.git",
            // ssh, and a host bombyx has been taught no key
            // source for.
            "git@gitlab.com:you/p.git",
            "ssh://git@git.example.invalid/p.git",
            // A lookalike domain must not inherit github's
            // entry.
            "git@github.com.example.invalid:you/p.git",
        ] {
            let out = render(&cfg_cloning(repo));
            assert_host_keys(&out, "\"\"", "\"\"", "\"\"");
        }
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
        // The name is a literal here, not `BOOTSTRAP_NAME`.
        // Asserting the constant against a rendering that
        // interpolates the same constant compares it with
        // itself: measured, renaming it to
        // `not-the-script-at-all.sh` left this test green.
        //
        // What the literal pins is that the provisioner is
        // still handed a `path:`, and what that path spells.
        // `files` writes the script under the same constant, so
        // the two sides cannot disagree -- there is no
        // cross-file agreement to check here, which is why this
        // is a plain rendering test.
        let out = render(&cfg_with(Provider::Libvirt));
        assert!(out.contains("path: \"bootstrap.sh\""), "{out}");
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
        // The clause, over a comment-stripped view. The bare
        // name appears in this script's own comments twice, so
        // asserting it over the raw text would stay green after
        // a rename in the shell half -- and the guest would
        // then take the key-deleting branch on every provision
        // while the Vagrantfile announced `1`.
        let code = bootstrap_code();
        assert!(
            code.contains(&format!("if [ \"${{{DEPLOY_KEY_ENV}:-}}\" = 1 ]")),
            "{DEPLOY_KEY_ENV} is not branched on in the bootstrap script"
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
    fn an_env_file_adds_an_upload_and_no_env_file_adds_none() {
        let with = render(&cfg_with_env_file());
        assert!(
            with.contains("bombyx_env_file = File.expand_path"),
            "the upload block is missing:\n{with}"
        );
        assert!(
            with.contains(&format!("destination: {ENV_FILE_GUEST_PATH:?}")),
            "the destination is not the guest path:\n{with}"
        );
        assert!(
            with.contains(&format!("File.expand_path({ENV_FILE_NAME:?}")),
            "the source is not the staged file:\n{with}"
        );

        let without = render(&cfg_with(Provider::Libvirt));
        assert!(
            !without.contains("bombyx_env_file"),
            "a project with no env_file gets no upload block:\n{without}"
        );
    }

    #[test]
    fn the_upload_is_conditional_so_a_destroy_can_still_load_the_file() {
        // `crate::plan` removes the staged file as soon as
        // vagrant finishes, and `vagrant destroy` loads this
        // Vagrantfile afterwards. A `raise` on a missing file
        // would strand a directory no bombyx command could clear
        // -- the same argument the deploy key's block carries.
        let out = render(&cfg_with_env_file());
        assert!(
            out.contains("if File.exist?(bombyx_env_file)"),
            "the upload must be guarded by an existence test:\n{out}"
        );
        assert!(
            !out.contains("raise"),
            "nothing here may raise on a missing file:\n{out}"
        );
    }

    #[test]
    fn the_env_file_flag_is_rendered_either_way() {
        // The reason `DEPLOY_KEY_ENV` gives: a login shell
        // sources /etc/profile first, so a name bombyx leaves
        // out is one the guest can set for itself -- and the
        // guest would then be answering a question about the
        // operator's config.
        assert_eq!(
            rendered(&render(&cfg_with_env_file()), ENV_FILE_ENV),
            "\"1\""
        );
        assert_eq!(
            rendered(&render(&cfg_with(Provider::Libvirt)), ENV_FILE_ENV),
            "\"0\""
        );
    }

    #[test]
    fn the_env_file_path_reaches_neither_generated_file() {
        // The opposite of `deploy_key`, whose path *is* written
        // into the Vagrantfile because vagrant is what opens it.
        // This file is opened on the workstation, so the VM host
        // has no use for the path -- and the path is a location
        // on the operator's own machine, which the generated
        // files have no business recording.
        //
        // The contents are covered separately, in `plan`: they
        // travel on a pipe and reach no command line at all.
        for (name, contents) in files(&cfg_with_env_file()) {
            assert!(
                !contents.contains(ENV_FILE),
                "{name} holds the workstation path"
            );
            assert!(
                !contents.contains("myproject-unrendered"),
                "{name} holds part of the workstation path"
            );
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
        // The other half of this arrangement lives in
        // `bootstrap_tests`, as
        // `nothing_in_the_bootstrap_script_asks_for_root`. This
        // test asserts the flag is set; that one asserts no
        // line in the script defeats it.
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
}
