//! Builds the three files bombyx writes onto the VM host: the
//! Vagrantfile, the account script and the bootstrap script.
//!
//! A Vagrantfile tells Vagrant how to build a VM. Normally a
//! project writes its own and keeps it in its repo. bombyx
//! writes it instead, because Vagrant has to read that file
//! *before* the VM exists, so it cannot come from inside the
//! VM -- and outside the VM is where we are trying not to put
//! the project's files. `docs/trust-boundary.md` explains why.
//!
//! The split between the Vagrantfile and the two scripts is worth
//! understanding.
//!
//! The Vagrantfile changes per project -- different box,
//! different memory -- so it is built here, with config values
//! pasted into it.
//!
//! [`ACCOUNT`] and [`BOOTSTRAP`] are the same for every project,
//! always. Each is shipped exactly as written, and bombyx pastes
//! nothing into either. Anything they need to know arrives as an
//! environment variable that Vagrant sets. That is the point: pasting
//! config values into a shell script is where quoting bugs and
//! injection holes come from, so we simply never do it.

use std::collections::BTreeMap;

use crate::config::{
    Config, CpuMode, DeployKeyPath, Disk, EnvName, EnvValue, Provider, Staged,
};
use crate::hostkeys;

/// The script that clones the project and runs the project's own
/// script, as the agent, shipped to the host unchanged.
///
/// Uploaded rather than provisioned: [`ACCOUNT`] is what the
/// Vagrantfile's shell provisioner runs, and it hands this script
/// to the agent's account.
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

/// The account script's name on the VM host.
///
/// The Vagrantfile's one shell provisioner runs it, as root, and
/// its `path:` is relative to the Vagrantfile's directory, so the
/// two names agree through this constant.
pub const ACCOUNT_NAME: &str = "account.sh";

/// The script that sets up the agent's account, shipped to the
/// host unchanged.
///
/// It runs as root, before [`BOOTSTRAP`]: it creates the account
/// `guest_user` names, gives it `sudo`, moves the staged uploads
/// into its home, and hands [`BOOTSTRAP`] to it. The split keeps
/// root to this one short file, which never reads anything from
/// the project's repository.
pub const ACCOUNT: &str = include_str!("../templates/account.sh");

/// The directory every upload lands in, in the home of the
/// account Vagrant logs in as.
///
/// Vagrant's file provisioner uploads as that account, so it can
/// write only where that account can, and the agent's account
/// does not exist yet when the upload is planned. So every file
/// the guest needs is staged here first, and [`ACCOUNT`], running
/// as root, moves it on. It removes the directory once it has.
///
/// Written with a `~`, which Vagrant expands by running
/// `printf <destination>` through a shell **inside the guest**
/// before it sends anything -- read in vagrant 2.4.9,
/// `plugins/provisioners/file/provisioner.rb` and
/// `plugins/guests/linux/cap/shell_expand_guest_path.rb`. That
/// shell runs as the login account, so the directory lands in
/// that account's real home whatever the box calls it. The SSH
/// communicator's `upload` creates the directory when it is
/// missing.
///
/// Every staged path below is built from this one with `concat!`,
/// which takes only literals -- so the directory is a macro
/// rather than a `const`, and the constant beside it exists for
/// the tests that read it.
macro_rules! staging_dir {
    () => {
        "~/.bombyx-staging"
    };
}

/// [`staging_dir!`] as a value, for the tests that compare the
/// staged paths and the scripts against it.
#[cfg(test)]
const STAGING_DIR: &str = staging_dir!();

/// Where [`BOOTSTRAP`] is staged.
///
/// Vagrant's shell provisioner uploads and runs exactly one
/// script, and that script is [`ACCOUNT`]. So [`BOOTSTRAP`]
/// travels as a plain upload, and [`ACCOUNT`] installs it afresh on
/// every provision before it hands it to the agent, so the agent's
/// own edits to an earlier copy never run.
const BOOTSTRAP_STAGED_PATH: &str = concat!(staging_dir!(), "/bootstrap.sh");

/// Where the deploy key is staged.
///
/// [`ACCOUNT`] writes it on to `~/.ssh/bombyx-deploy-key` in the
/// agent's home, as the agent, where [`BOOTSTRAP`] tightens it
/// and leaves it, at `0600`, for the life of the VM. It is not
/// kept anywhere more private on purpose: the agent has to push
/// with this key, so a placement it could not read would be a
/// key that cannot do its job. `docs/trust-boundary.md` under
/// **What this costs** holds what that exposes.
const DEPLOY_KEY_STAGED_PATH: &str = concat!(staging_dir!(), "/deploy-key");

/// Environment variable telling the guest that the operator's
/// config named a `deploy_key`.
///
/// This flag reports the config; [`ENV_FILE_PRESENT_ENV`] and
/// [`CREDENTIAL_PRESENT_ENV`] report what bombyx staged. The
/// key never passes through bombyx: it is already on the VM
/// host and vagrant uploads it, so there is nothing to stage
/// and nothing the two answers could disagree about.
///
/// [`render`] sets it in the shell provisioner's `env:` block
/// on every render. [`ACCOUNT`] branches on it to decide whether
/// to place the key, and [`BOOTSTRAP`], which receives it because
/// the name is on [`PRESERVE_ENV`]'s list, to decide whether to
/// use or remove it.
///
/// **Why the guest is told rather than left to look.** The key
/// ends up in the agent's own `~/.ssh`, and the agent has `sudo`
/// besides. So a leftover from an interrupted provision, or one
/// `touch` by code running in the VM, would answer "was a key
/// configured?" on the operator's behalf.
///
/// **It is set on every render, `"1"` or `"0"`, and that is the
/// half that matters.** Vagrant runs a shell provisioner
/// through `config.ssh.shell`, whose default is `bash -l` -- a
/// login shell, which sources `/etc/profile` and
/// `/etc/profile.d/*.sh` before [`ACCOUNT`] runs. The `env:`
/// block is the only thing that overrides what those files set.
/// So a name bombyx does not render is left to them, and an
/// export placed there reaches [`ACCOUNT`] unopposed and, once
/// the name is on the preserve list, [`BOOTSTRAP`] too. Rendering the
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

/// The project's own script, which [`BOOTSTRAP`] runs out of the
/// clone.
const SCRIPT_ENV: &str = "BOMBYX_SCRIPT";

/// The project's name, which the guest uses as the last component
/// of its clone directory.
///
/// This is the `[projects.<name>]` table key, a
/// `crate::name::ProjectName` -- one benign path segment, already
/// checked -- so the guest can join it onto `$HOME` without a
/// parser of its own. Naming the clone after the project is what
/// lets several agent VMs be told apart by the directory alone,
/// rather than by asking `git` which repository each one holds.
///
/// [`BOOTSTRAP`] reads it as `${BOMBYX_PROJECT:-project}` only so
/// that `set -u` cannot abort on it; [`render`] always sets it.
const PROJECT_ENV: &str = "BOMBYX_PROJECT";

/// The secrets file's name in the project directory on the VM
/// host.
///
/// `pub(crate)` rather than `pub`, unlike [`VAGRANTFILE_NAME`]
/// and [`BOOTSTRAP_NAME`], which the integration suite opens by
/// name. Only `crate::plan` needs this one, to write the file
/// and to remove it again, and a `pub` constant would make the
/// name on the VM host something a release has to keep.
///
/// This file is not generated, so it is absent from [`files`].
/// Its contents come from the workstation and reach the VM host
/// on a pipe.
pub(crate) const ENV_FILE_NAME: &str = "bombyx.env";

/// Where the secrets file is staged.
///
/// [`ACCOUNT`] writes it on to `~/.bombyx-env` in the agent's
/// home. [`BOOTSTRAP`] cannot spell that path with `$HOME`: a
/// project's `[env]` table may set `HOME`, and the provisioner's
/// environment carries that value. The script reads the passwd
/// entry instead, which names the home [`ACCOUNT`] wrote into.
const ENV_FILE_STAGED_PATH: &str = concat!(staging_dir!(), "/env");

/// Environment variable telling the guest that a secrets file
/// is being staged for it.
///
/// Set on every render, `"1"` or `"0"`, for the reason
/// [`DEPLOY_KEY_ENV`] gives at length: a name bombyx leaves out
/// is left to `/etc/profile`, so the guest could answer the
/// question on the operator's behalf.
///
/// `BOMBYX_ENV_FILE`, which [`BOOTSTRAP`] exports for the
/// project's own script, is a different variable holding the
/// guest path. The `PRESENT` in both names here is what keeps
/// the two apart.
const ENV_FILE_PRESENT_ENV: &str = "BOMBYX_ENV_FILE_PRESENT";

/// The git credential file's name in the project directory on
/// the VM host.
///
/// A second file travelling the same way as [`ENV_FILE_NAME`],
/// for the same reason: its contents are a secret, so they
/// reach the VM host on a pipe rather than in a command line.
///
/// It is a separate file rather than a second variable inside
/// the first, because `git` reads it itself. The `store`
/// credential helper opens a file of its own and expects one
/// `https://user:token@host` line in it; a `.env` file is not
/// that shape, and the project's own script runs long after the
/// clone that needs it.
pub(crate) const CREDENTIAL_FILE_NAME: &str = "bombyx.git-credentials";

/// Where the git credential file is staged.
///
/// [`ACCOUNT`] writes it on to `~/.bombyx-git-credentials` in the
/// agent's home. That final path ends up inside a
/// `credential.helper` setting, which `git` hands to a shell, so
/// [`BOOTSTRAP`] builds it from `/home/` and the checked account
/// name rather than from `$HOME`; its banner on `GIT_CRED` holds
/// the measurement.
const CREDENTIAL_STAGED_PATH: &str =
    concat!(staging_dir!(), "/git-credentials");

/// The three paths [`ACCOUNT`] writes the staged credentials to,
/// relative to the agent's home, and which [`BOOTSTRAP`] reads.
///
/// Test-only: neither script is built from this list. It is what
/// lets a test assert both scripts spell each path the same way,
/// since neither file can see the other.
#[cfg(test)]
const GUEST_HOME_FILES: [&str; 3] = [
    ".ssh/bombyx-deploy-key",
    ".bombyx-env",
    ".bombyx-git-credentials",
];

/// Environment variable naming the account the agent works as.
///
/// [`ACCOUNT`] creates the account under this name and hands
/// [`BOOTSTRAP`] to it; [`BOOTSTRAP`] checks it is running as
/// that account and builds its bookkeeping paths from
/// `/home/<name>`.
const GUEST_USER_ENV: &str = "BOMBYX_GUEST_USER";

/// Environment variable listing, comma-separated, every other
/// name in the provisioner's `env:` hash.
///
/// [`ACCOUNT`] runs as root and ends with `sudo -u <guest_user>`,
/// which clears the environment by default. It passes this list
/// to `--preserve-env`, so [`BOOTSTRAP`] and the project's script
/// receive every variable the Vagrantfile set, the project's
/// `[env]` table included. [`render`] builds the list from
/// [`BOMBYX_ENV_NAMES`] and the `[env]` keys, and
/// `the_preserve_list_names_every_variable_the_hash_sets` checks
/// it against the hash that was actually rendered.
const PRESERVE_ENV: &str = "BOMBYX_PRESERVE_ENV";

/// Environment variable telling the guest that a git credential
/// is being staged for it.
///
/// Set on every render, `"1"` or `"0"`, for the reason
/// [`DEPLOY_KEY_ENV`] gives at length: a name bombyx leaves out
/// is left to `/etc/profile`, so the guest could answer the
/// question on the operator's behalf and keep a credential
/// alive that the config no longer names.
const CREDENTIAL_PRESENT_ENV: &str = "BOMBYX_GIT_CRED_PRESENT";

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
/// `accept-new` too, because [`BOOTSTRAP`] reads it as
/// `${VAR:-}` so that `set -u` cannot abort on it. [`render`]
/// always sets it.
const HOST_KEYS_URL_ENV: &str = "BOMBYX_HOST_KEYS_URL";

/// Environment variable telling the guest what the fetched
/// response looks like, and empty when nothing is fetched.
///
/// `crate::hostkeys::KeyFormat::as_str` produces the two words
/// this may hold, and [`BOOTSTRAP`] matches them.
const HOST_KEYS_FORMAT_ENV: &str = "BOMBYX_HOST_KEYS_FORMAT";

/// Every variable bombyx sets in the provisioner itself, apart
/// from [`PRESERVE_ENV`].
///
/// The names are written into the template one by one, in shapes
/// that differ, so this list is kept beside them by hand.
/// [`render`] reads it to build [`PRESERVE_ENV`], and
/// `the_preserve_list_names_every_variable_the_hash_sets` fails
/// when a name reaches the template without reaching this list.
/// No count here on purpose -- the array grows, and a figure in
/// prose costs the next reader a recount.
///
/// The `[env]` table refuses a name carrying the prefix
/// `RESERVED_PREFIX` names, and `config::env` holds why. This
/// array is what makes that reservation checkable: a test walks
/// it and asserts each name is one the reservation covers.
/// Without the list, the guard protects whichever names happen
/// to start with the prefix, and a bombyx variable added
/// without it would fall outside the reservation with every
/// test still green.
const BOMBYX_ENV_NAMES: [&str; 13] = [
    GUEST_USER_ENV,
    REPO_ENV,
    REF_ENV,
    SCRIPT_ENV,
    PROJECT_ENV,
    DEPLOY_KEY_ENV,
    ENV_FILE_PRESENT_ENV,
    CREDENTIAL_PRESENT_ENV,
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
    script_code(BOOTSTRAP)
}

/// [`ACCOUNT`] as code, the way [`bootstrap_code`] gives
/// [`BOOTSTRAP`].
#[cfg(test)]
fn account_code() -> String {
    script_code(ACCOUNT)
}

/// `script` with every comment line dropped and the rest
/// flattened to single spaces; [`bootstrap_code`] says why.
#[cfg(test)]
fn script_code(script: &str) -> String {
    script
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

/// The Ruby that stages [`BOOTSTRAP`] for [`ACCOUNT`].
///
/// Unconditional, unlike the credential uploads: every provision
/// needs the script. [`render`] places it ahead of the shell
/// provisioner, because [`ACCOUNT`] refuses when the staged copy
/// is missing.
fn bootstrap_block() -> String {
    format!(
        "  # bootstrap.sh travels as a plain upload, because the shell
  # provisioner below runs account.sh. Every upload lands in the
  # staging directory of the account vagrant logs in as, and
  # account.sh moves each file on.
  config.vm.provision \"file\",
    source: File.expand_path({bootstrap}, __dir__),
    destination: {staged}

",
        bootstrap = ruby_string(BOOTSTRAP_NAME),
        staged = ruby_string(BOOTSTRAP_STAGED_PATH),
    )
}

/// The value of [`PRESERVE_ENV`]: every name bombyx sets, then
/// every `[env]` name, comma-separated.
///
/// `sudo --preserve-env` reads it as a list of names, so a name
/// holding a comma would split in two. None can: bombyx's own are
/// constants, and an [`EnvName`] is a shell identifier.
fn preserve_list(env: &BTreeMap<EnvName, EnvValue>) -> String {
    BOMBYX_ENV_NAMES
        .iter()
        .copied()
        .chain(env.keys().map(EnvName::as_str))
        .collect::<Vec<_>>()
        .join(",")
}

/// Panics unless `staged` was built from `cfg`.
///
/// A mispaired call would otherwise fail in the guest, where it is
/// quiet: told `0` for a secrets file, the guest deletes the file an
/// earlier provision left and provisions on without it. So [`render`]
/// checks the pairing here instead. See [`render`]'s `# Panics`.
fn assert_staged_matches(cfg: &Config, staged: &Staged) {
    assert_eq!(
        cfg.source.env_file.is_some(),
        staged.secrets().is_some(),
        "a config naming an env_file must be rendered against \
         the secrets read from it"
    );
    assert_eq!(
        cfg.source.repo_token.is_some(),
        staged.credential().is_some(),
        "a config naming a repo_token must be rendered against \
         the credential built from it"
    );
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
/// expect to fix something. See [`Provider`].
///
/// `staged` decides whether the guest is told a secrets file and
/// a git credential are coming, and it is the same value
/// `crate::plan` writes those two files from. Reading
/// `cfg.source.env_file` here instead would let the rendered
/// Vagrantfile announce a file the plan never stages, and the
/// guest would refuse minutes after booting.
///
/// # Panics
///
/// Panics when `staged` did not come from `cfg`: when the
/// config names an `env_file` or a `repo_token` and `staged` is
/// missing the matching half, or when `staged` carries a half
/// the config names nowhere.
/// [`Config::read_staged`](crate::config::Config::read_staged)
/// builds a pair that cannot fail this.
#[must_use]
pub fn render(cfg: &Config, staged: &Staged) -> String {
    assert_staged_matches(cfg, staged);

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
  config.vm.hostname = {hostname}

  # The default share would mount the workstation's copy of the
  # project at /vagrant, which is the copy this design exists to
  # keep out of the guest. It also hangs on a host whose
  # firewall refuses NFS from the guest bridge.
  config.vm.synced_folder \".\", \"/vagrant\", disabled: true

  config.vm.provider :{provider} do |v|
    v.cpus = {cpus}
    v.memory = {memory}{disk}{cpu_mode}
  end

{bootstrap_upload}\
{deploy_key}{env_file}{credential}  config.vm.provision \"shell\",
    path: {account},
    # As root, for one step: account.sh creates the agent's
    # account, moves the staged files into its home, and hands
    # bootstrap.sh to it through `sudo -u`. Nothing from the
    # project's repository runs before that hand-over.
    privileged: true,
    env: {{
      \"{guest_user_env}\" => {guest_user},
      # Every name below, which account.sh passes to
      # `sudo --preserve-env` so that bootstrap.sh receives them.
      \"{preserve_env_name}\" => {preserve_env},
      \"{repo_env}\" => {repo},
      \"{ref_env}\" => {git_ref},
      \"{script_env}\" => {script},
      \"{clone_project_env}\" => {clone_project},
      \"{deploy_key_env_name}\" => \"{deploy_key_env}\",
      \"{env_file_env_name}\" => \"{env_file_env}\",
      \"{credential_env_name}\" => \"{credential_env}\",
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
        env_file = env_file_block(staged.secrets().is_some()),
        credential = credential_block(staged.credential().is_some()),
        repo_env = REPO_ENV,
        ref_env = REF_ENV,
        script_env = SCRIPT_ENV,
        clone_project_env = PROJECT_ENV,
        deploy_key_env_name = DEPLOY_KEY_ENV,
        deploy_key_env = deploy_key_env(source.deploy_key.as_ref()),
        env_file_env_name = ENV_FILE_PRESENT_ENV,
        env_file_env = if staged.secrets().is_some() { "1" } else { "0" },
        credential_env_name = CREDENTIAL_PRESENT_ENV,
        credential_env =
            if staged.credential().is_some() { "1" } else { "0" },
        git_host_env = GIT_HOST_ENV,
        git_host = ruby_string(host_keys.map_or("", |k| k.host())),
        host_keys_url_env = HOST_KEYS_URL_ENV,
        host_keys_url = ruby_string(host_keys.map_or("", |k| k.url())),
        host_keys_format_env = HOST_KEYS_FORMAT_ENV,
        host_keys_format =
            ruby_string(host_keys.map_or("", |k| k.format().as_str())),
        box_name = ruby_string(vm.box_name.as_str()),
        hostname = ruby_string(cfg.vm_hostname().as_str()),
        provider = vm.provider,
        cpus = vm.cpus,
        memory = vm.memory.mib(),
        disk = disk_setting(vm.disk),
        cpu_mode = cpu_mode_setting(vm.provider, vm.cpu_mode),
        bootstrap_upload = bootstrap_block(),
        account = ruby_string(ACCOUNT_NAME),
        guest_user_env = GUEST_USER_ENV,
        guest_user = ruby_string(vm.guest_user.as_str()),
        preserve_env_name = PRESERVE_ENV,
        preserve_env = ruby_string(&preserve_list(&cfg.env)),
        project_env = project_env_block(&cfg.env),
        repo = ruby_string(source.repo.as_str()),
        git_ref = ruby_string(source.git_ref.as_str()),
        script = ruby_string(source.script.as_str()),
        clone_project = ruby_string(cfg.project.as_str()),
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

/// The libvirt disk-size line for the provider block, or nothing.
///
/// `v.machine_virtual_size` sizes the disk in whole GiB. It is a
/// libvirt setting, and [`Vm`](crate::config::Vm) refuses a `disk`
/// on any other provider, so this renders for libvirt alone. `None`
/// leaves the box's own disk size and adds no line.
///
/// The leading newline places the line inside the `do |v|` block,
/// after `v.memory`.
fn disk_setting(disk: Option<Disk>) -> String {
    disk.map(|d| format!("\n    v.machine_virtual_size = {}", d.gib()))
        .unwrap_or_default()
}

/// The libvirt CPU-mode line for the provider block, or nothing.
///
/// `v.cpu_mode` selects the guest CPU. It is a libvirt setting, so
/// this renders only for the libvirt provider; a non-libvirt guest
/// gets no line, and [`Vm`](crate::config::Vm) refuses an explicit
/// `cpu_mode` on one. When a libvirt project sets none, `mode` is
/// `None` and bombyx defaults to [`CpuMode::HostPassthrough`], which
/// exposes the host CPU's full feature set -- a free win for compile
/// times on a machine that never migrates.
///
/// The value is a compile-time constant from [`CpuMode::as_str`], so
/// no operator string reaches the rendered Ruby.
fn cpu_mode_setting(provider: Provider, mode: Option<CpuMode>) -> String {
    if provider != Provider::Libvirt {
        return String::new();
    }
    format!(
        "\n    v.cpu_mode = \"{}\"",
        mode.unwrap_or_default().as_str()
    )
}

/// The Ruby that uploads the deploy key, or nothing at all.
///
/// An empty string when the config names no key, so a public
/// repository's Vagrantfile carries no upload block.
///
/// [`render`] places this ahead of the shell provisioner.
/// Vagrant runs provisioners in the order the file declares
/// them, and [`ACCOUNT`] moves the staged key on before it hands
/// over to [`BOOTSTRAP`].
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
        dest = ruby_string(DEPLOY_KEY_STAGED_PATH),
    )
}

/// The Ruby that uploads the project's secrets file, or nothing
/// at all.
///
/// An empty string when nothing was staged, so a project whose
/// config names no `env_file` gets a Vagrantfile with no upload
/// block.
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
fn env_file_block(staged: bool) -> String {
    if !staged {
        return String::new();
    }
    format!(
        "  # The project's secrets, carried from the workstation.
  # bombyx wrote this file beside the Vagrantfile a moment ago
  # and removes it again when vagrant finishes. A run somebody
  # interrupted does not get that far, so finding the file here
  # means the last run stopped early. docs/trust-boundary.md
  # says what keeping a copy inside the guest costs.
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
        dest = ruby_string(ENV_FILE_STAGED_PATH),
    )
}

/// The Ruby that uploads the git credential file, or nothing at
/// all.
///
/// The same shape as [`env_file_block`], and conditional for
/// the same reason: `vagrant destroy` loads this file too, and
/// by then `crate::plan` has removed the staged copy from the
/// VM host, so a `raise` would strand a directory no bombyx
/// command could clear.
///
/// [`render`] places this ahead of the shell provisioner, so
/// [`BOOTSTRAP`] finds the file already there -- which it must,
/// because the clone that needs it runs inside that same
/// script. (Not the script's first network call: the git host's
/// published ssh keys are fetched before it, over https.)
fn credential_block(staged: bool) -> String {
    if !staged {
        return String::new();
    }
    format!(
        "  # The credential git clones with, carried from the
  # workstation. bombyx built it from one variable inside the
  # secrets file and removes the staged copy again when vagrant
  # finishes.
  bombyx_git_cred = File.expand_path({name}, __dir__)
  if File.exist?(bombyx_git_cred)
    config.vm.provision \"file\",
      source: bombyx_git_cred,
      destination: {dest}
  end

",
        name = ruby_string(CREDENTIAL_FILE_NAME),
        dest = ruby_string(CREDENTIAL_STAGED_PATH),
    )
}

/// Every file bombyx *generates* for the project directory on
/// the VM host, as `(name, contents)` pairs.
///
/// Not every file that lands there. A `staged` carrying secrets
/// sends one more and one carrying a credential sends another,
/// and neither is generated here: the first comes off the
/// operator's workstation and the second is built from a value
/// inside it. This module holds both names, `ENV_FILE_NAME` and
/// `CREDENTIAL_FILE_NAME`, and `crate::plan` writes both. Not
/// rustdoc links: those constants are crate-private, and a
/// public page may not link to one. `crate::remote::write`'s
/// own header lists all five.
///
/// The list exists once, here, and everything else reads it:
/// `plan` to build the write commands, and the tests to check
/// each file is safe to send.
///
/// That is the whole reason for the function. If the list were
/// written out separately in each of those places, adding another
/// file would mean remembering all of them -- and the one
/// people forget is the test, so the new file would be written
/// to the host without ever being checked.
///
/// # Panics
///
/// Whenever [`render`] does, and for the same reason.
#[must_use]
pub fn files(cfg: &Config, staged: &Staged) -> [(&'static str, String); 3] {
    [
        (VAGRANTFILE_NAME, render(cfg, staged)),
        (BOOTSTRAP_NAME, BOOTSTRAP.to_owned()),
        (ACCOUNT_NAME, ACCOUNT.to_owned()),
    ]
}

// The shell text of `bootstrap.sh` is linted in
// `bootstrap_tests.rs`, whose header holds the argument for the
// split. A plain comment rather than `///`: neither rustdoc
// pass renders a `cfg(test)` item, so a doc link here would
// never be checked.
#[cfg(test)]
mod bootstrap_tests;

// The same kind of lint, over `account.sh`; its header says what
// the lints pin.
#[cfg(test)]
mod account_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    use crate::config::{
        BoxName, CpuMode, DeployKeyPath, EnvFilePath, EnvName, EnvValue,
        GitRef, GuestUser, Hostname, Memory, Provider, RESERVED_PREFIX,
        RepoUrl, ScriptPath, Source, Vm,
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
            memory: Memory::from_mib(
                NonZeroU32::new(8192).expect("a positive fixture size"),
            ),
            disk: None,
            cpu_mode: None,
            hostname: None,
            guest_user: GuestUser::default(),
        };
        cfg.source = Source {
            repo: RepoUrl::parse("https://example.invalid/p.git")
                .expect("a valid fixture URL"),
            git_ref: GitRef::parse("main").expect("a valid fixture ref"),
            script: ScriptPath::parse("vagrant/provision.sh")
                .expect("a valid fixture path"),
            deploy_key: None,
            env_file: None,
            repo_token: None,
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

    /// [`render`] against the [`Staged`] this config implies.
    ///
    /// Every test here renders a config the operator could have
    /// written, so the pair always comes from one place --
    /// `Config::staged_for_tests` builds it.
    fn rendered_for(cfg: &Config) -> String {
        render(cfg, &cfg.staged_for_tests())
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
        let out = rendered_for(&cfg_cloning("git@github.com:you/private.git"));
        assert_host_keys(
            &out,
            "\"github.com\"",
            "\"https://api.github.com/meta\"",
            "\"json\"",
        );
    }

    #[test]
    fn a_bitbucket_clone_needs_no_json_parsing() {
        let out =
            rendered_for(&cfg_cloning("ssh://git@bitbucket.org/you/p.git"));
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
        let out = rendered_for(&cfg_cloning("git@GitHub.COM:you/p.git"));
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
            let out = rendered_for(&cfg_cloning(repo));
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
        let out = rendered_for(&cfg_with_env());
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
        let out = rendered_for(&cfg_with_env());
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
        let out = rendered_for(&cfg_with_env());
        let git = out.find("GIT_USER_NAME").expect("the first name");
        let node = out.find("NODE_MAJOR").expect("the second name");
        assert!(git < node, "not in name order:\n{out}");
    }

    #[test]
    fn a_project_with_no_variables_renders_bombyxs_own_set() {
        // The absent table must not leave a stray comma or an
        // empty line behind in the hash literal.
        let out = rendered_for(&cfg_with(Provider::Libvirt));
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
        let out = rendered_for(&cfg_with(Provider::Libvirt));
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
    fn a_disk_size_renders_only_when_set() {
        // With no disk the provider block carries no size line, so
        // the guest keeps the base box's own disk.
        let none = rendered_for(&cfg_with(Provider::Libvirt));
        assert!(
            !none.contains("machine_virtual_size"),
            "an unset disk must add no line:\n{none}"
        );

        // With a disk the line sizes it in whole GiB, inside the
        // provider block.
        let mut cfg = cfg_with(Provider::Libvirt);
        cfg.vm.disk = Some(Disk::from_gib(
            NonZeroU32::new(40).expect("a positive size"),
        ));
        let out = rendered_for(&cfg);
        assert!(
            out.contains("v.machine_virtual_size = 40"),
            "the disk line is missing from:\n{out}"
        );
    }

    #[test]
    fn a_libvirt_guest_gets_host_passthrough_by_default() {
        // No cpu_mode set: bombyx renders its default, passthrough,
        // rather than leaving vagrant's slower host-model.
        let out = rendered_for(&cfg_with(Provider::Libvirt));
        assert!(
            out.contains("v.cpu_mode = \"host-passthrough\""),
            "the default cpu_mode line is missing from:\n{out}"
        );

        // An explicit host-model overrides that default.
        let mut cfg = cfg_with(Provider::Libvirt);
        cfg.vm.cpu_mode = Some(CpuMode::HostModel);
        let out = rendered_for(&cfg);
        assert!(
            out.contains("v.cpu_mode = \"host-model\""),
            "the explicit cpu_mode line is missing from:\n{out}"
        );
    }

    #[test]
    fn a_non_libvirt_guest_gets_no_cpu_mode_line() {
        // cpu_mode is a libvirt setting; a hyperv block must not
        // carry it, even though bombyx defaults libvirt to one.
        let out = rendered_for(&cfg_with(Provider::Hyperv));
        assert!(
            !out.contains("cpu_mode"),
            "a hyperv block must carry no cpu_mode line:\n{out}"
        );
    }

    #[test]
    fn the_clone_directory_is_named_after_the_project() {
        // The guest joins this onto `$HOME`, so an operator with
        // several VMs in flight tells them apart by the directory
        // rather than by asking each one which repository it
        // holds. `myproject` is the fixture's `[projects.<name>]`
        // key.
        let out = rendered_for(&cfg_with(Provider::Libvirt));
        assert_eq!(rendered(&out, PROJECT_ENV), "\"myproject\"");
    }

    #[test]
    fn the_guest_is_given_a_hostname_derived_from_the_project() {
        // With no `hostname` in `[vm]`, the guest would otherwise
        // answer to the box's default name, so two agent VMs on
        // one host could not be told apart. The fixture project is
        // `myproject`, so the derived name is `myproject-agent`.
        let out = rendered_for(&cfg_with(Provider::Libvirt));
        assert!(
            out.contains("config.vm.hostname = \"myproject-agent\""),
            "the guest must be given a hostname:\n{out}"
        );
    }

    #[test]
    fn an_explicit_hostname_overrides_the_derived_one() {
        let mut cfg = cfg_with(Provider::Libvirt);
        cfg.vm.hostname =
            Some(Hostname::parse("chosen-name").expect("a valid fixture"));
        let out = rendered_for(&cfg);
        assert!(
            out.contains("config.vm.hostname = \"chosen-name\""),
            "an explicit hostname must reach the Vagrantfile:\n{out}"
        );
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
            let out = rendered_for(&cfg_with(provider));
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
        let out = rendered_for(&cfg_with(Provider::Libvirt));
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
        assert!(
            rendered_for(&cfg_with(Provider::Libvirt)).contains(":libvirt")
        );
        assert!(rendered_for(&cfg_with(Provider::Hyperv)).contains(":hyperv"));
    }

    #[test]
    fn points_the_provisioner_at_the_account_script() {
        // The name is a literal here, not `ACCOUNT_NAME`.
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
        let out = rendered_for(&cfg_with(Provider::Libvirt));
        assert!(out.contains("path: \"account.sh\""), "{out}");
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
        let out = rendered_for(&cfg_with_key());
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
            out.contains(&format!("destination: \"{DEPLOY_KEY_STAGED_PATH}\"")),
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
        let out = rendered_for(&cfg_with_key());
        assert!(out.contains("if File.exist?"), "{out}");
        assert!(!out.contains("raise"), "a raise breaks destroy:\n{out}");
    }

    #[test]
    fn no_deploy_key_renders_no_key_upload() {
        // A public repository needs no credential, and an
        // upload block with an empty path would fail every
        // `up`. The one upload left is `bootstrap.sh`'s.
        let out = rendered_for(&cfg_with(Provider::Libvirt));
        assert_eq!(
            out.matches("config.vm.provision \"file\"").count(),
            1,
            "{out}"
        );
        for absent in ["bombyx_deploy_key", DEPLOY_KEY_STAGED_PATH] {
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
        let out = rendered_for(&cfg_with_key());
        assert!(
            out.contains(&format!("\"{DEPLOY_KEY_ENV}\" => \"1\"")),
            "{out}"
        );
    }

    #[test]
    fn no_key_announces_a_zero_rather_than_nothing() {
        // Rendering nothing would leave the guest's own
        // environment to answer. [`DEPLOY_KEY_ENV`] says why.
        let out = rendered_for(&cfg_with(Provider::Libvirt));
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

    /// Every staged path, as the Vagrantfile writes it.
    const STAGED_PATHS: [&str; 4] = [
        BOOTSTRAP_STAGED_PATH,
        DEPLOY_KEY_STAGED_PATH,
        ENV_FILE_STAGED_PATH,
        CREDENTIAL_STAGED_PATH,
    ];

    #[test]
    fn the_account_script_reads_every_path_the_vagrantfile_stages() {
        // Two files have to agree on each path and neither can
        // see the other: the Vagrantfile uploads to it and
        // `account.sh` reads it. Over the code, not the raw
        // text, so a name mentioned only in a comment does not
        // count.
        let code = account_code();
        let dir = STAGING_DIR
            .strip_prefix("~/")
            .expect("the staging directory is under the login home");
        assert!(
            code.contains(&format!("/{dir}\"")),
            "account.sh does not name {dir}:\n{code}"
        );
        for path in STAGED_PATHS {
            let name = path
                .strip_prefix(&format!("{STAGING_DIR}/"))
                .expect("every staged path is in the staging directory");
            assert!(
                code.contains(&format!("\"$STAGING/{name}\"")),
                "account.sh never reads the staged {name}"
            );
        }
    }

    #[test]
    fn both_guest_scripts_spell_each_credential_path_the_same_way() {
        // `account.sh` writes each credential into the agent's
        // home and `bootstrap.sh` reads it there. A rename in one
        // file would leave the other looking at an empty path,
        // and the guest would refuse the key as never arriving.
        let account = account_code();
        let bootstrap = bootstrap_code();
        for file in GUEST_HOME_FILES {
            assert!(
                account.contains(&format!("\"$home/{file}\"")),
                "account.sh does not write {file}"
            );
            assert!(
                bootstrap.contains(&format!("/{file}\"")),
                "bootstrap.sh does not read {file}"
            );
        }
    }

    #[test]
    fn both_guest_scripts_refuse_the_same_account_names() {
        // Each script checks the name for itself, because each
        // uses it in a place a shell reads. One pattern in both
        // means a tightening in one cannot miss the other.
        let pattern = "\"\" | [!a-z_]* | *[!a-z0-9_-]*)";
        assert!(account_code().contains(pattern), "account.sh");
        assert!(bootstrap_code().contains(pattern), "bootstrap.sh");
    }

    #[test]
    fn the_one_shell_provisioner_runs_the_account_script_as_root() {
        // Root is needed to create the account and its sudoers
        // file, read the staging directory and install
        // `bootstrap.sh`, and `account.sh` is the only file that
        // gets it: it hands `bootstrap.sh` to the agent before
        // anything from the repository runs. The other
        // half lives in `bootstrap_tests`, as
        // `nothing_in_the_bootstrap_script_asks_for_root`.
        //
        // Counted rather than found. A second shell provisioner
        // added to the template would run a second script, and
        // a check for one substring would stay green beside it.
        //
        // `cfg_with_key` rather than `cfg_with`, so a second
        // file provisioner is rendered too and the count has
        // something to be wrong about.
        for provider in [Provider::Libvirt, Provider::Hyperv] {
            let mut cfg = cfg_with_key();
            cfg.vm.provider = provider;
            let out = rendered_for(&cfg);
            let shell_blocks: Vec<&str> = out
                .split("config.vm.provision ")
                .skip(1)
                .filter(|b| b.starts_with("\"shell\""))
                .collect();
            assert_eq!(shell_blocks.len(), 1, "one shell provisioner:\n{out}");
            let block = shell_blocks[0];
            assert!(block.contains("path: \"account.sh\""), "{block}");
            // The whole clause, which pins the comma that keeps
            // the Ruby parseable.
            assert!(
                block.contains("privileged: true,\n    env: {\n"),
                "the flag must sit on the shell provisioner:\n{block}"
            );
        }
    }

    #[test]
    fn the_bootstrap_script_is_staged_before_the_account_script_runs() {
        // Vagrant runs provisioners in the order the file
        // declares them, and `account.sh` refuses when the
        // staged script is missing.
        let out = rendered_for(&cfg_with(Provider::Libvirt));
        let upload = out
            .find(&format!("destination: \"{BOOTSTRAP_STAGED_PATH}\""))
            .expect("bootstrap.sh must be staged");
        let shell = out
            .find("config.vm.provision \"shell\"")
            .expect("the shell provisioner must be rendered");
        assert!(upload < shell, "the upload must come first:\n{out}");
        assert!(
            out.contains("File.expand_path(\"bootstrap.sh\", __dir__)"),
            "{out}"
        );
    }

    #[test]
    fn every_upload_lands_in_the_staging_directory() {
        // The login account can write only its own home, and
        // `account.sh` clears exactly this directory once it has
        // moved the files on. An upload anywhere else would be
        // left behind in the login account's home.
        let mut cfg = cfg_with_credential();
        cfg.source.deploy_key =
            Some(DeployKeyPath::parse(KEY).expect("a valid fixture path"));
        let out = rendered_for(&cfg);
        let dests: Vec<&str> = out
            .lines()
            .filter_map(|l| l.trim().strip_prefix("destination: "))
            .collect();
        assert_eq!(dests.len(), STAGED_PATHS.len(), "{out}");
        for dest in dests {
            assert!(
                dest.starts_with(&format!("\"{STAGING_DIR}/")),
                "{dest} is outside the staging directory"
            );
        }
        for path in STAGED_PATHS {
            assert!(
                path.starts_with(&format!("{STAGING_DIR}/")),
                "{path} is outside the staging directory"
            );
        }
    }

    #[test]
    fn the_preserve_list_names_every_variable_the_hash_sets() {
        // `sudo` drops every variable the list leaves out, so a
        // name missing here reaches the root script and never
        // reaches `bootstrap.sh` or the project's own script.
        // The hash is read back from the rendering rather than
        // from `BOMBYX_ENV_NAMES`, which is the list this checks.
        let out = rendered_for(&cfg_with_env());
        let hash = out
            .split_once("env: {\n")
            .expect("the provisioner has an env hash")
            .1
            .split_once("\n    }")
            .expect("the hash closes")
            .0;
        let mut set: Vec<&str> = hash
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix('"'))
            .filter_map(|l| l.split_once('"').map(|(name, _)| name))
            .filter(|name| *name != PRESERVE_ENV)
            .collect();
        set.sort_unstable();
        let listed = rendered(&out, PRESERVE_ENV);
        let mut names: Vec<&str> =
            listed.trim_matches('"').split(',').collect();
        names.sort_unstable();
        assert_eq!(names, set);
        assert!(set.contains(&"NODE_MAJOR"), "the fixture's [env]:\n{out}");
    }

    #[test]
    fn the_guest_user_is_rendered_from_the_config() {
        let mut cfg = cfg_with(Provider::Libvirt);
        assert_eq!(rendered(&rendered_for(&cfg), GUEST_USER_ENV), "\"agent\"");
        cfg.vm.guest_user = GuestUser::parse("dev").expect("a plain name");
        assert_eq!(rendered(&rendered_for(&cfg), GUEST_USER_ENV), "\"dev\"");
    }

    #[test]
    fn files_carries_both_scripts_unchanged() {
        let cfg = cfg_with(Provider::Libvirt);
        let out = files(&cfg, &cfg.staged_for_tests());
        let find = |name: &str| {
            out.iter()
                .find(|(n, _)| *n == name)
                .unwrap_or_else(|| panic!("{name} is not written"))
                .1
                .clone()
        };
        assert_eq!(find(BOOTSTRAP_NAME), BOOTSTRAP);
        assert_eq!(find(ACCOUNT_NAME), ACCOUNT);
        assert!(find(VAGRANTFILE_NAME).contains("Vagrant.configure"));
    }

    /// [`cfg_with_env_file`], carrying a `repo_token` too.
    fn cfg_with_credential() -> Config {
        use crate::config::{RepoToken, RepoTokenVar, RepoUser};

        let mut cfg = cfg_with_env_file();
        cfg.source.repo_token = Some(RepoToken {
            var: RepoTokenVar::parse("TOKEN").expect("a plain name"),
            user: RepoUser::parse("x-token-auth").expect("a plain username"),
        });
        cfg
    }

    #[test]
    fn a_repo_token_adds_an_upload_and_no_repo_token_adds_none() {
        let with = rendered_for(&cfg_with_credential());
        assert!(
            with.contains("bombyx_git_cred = File.expand_path"),
            "the upload block is missing:\n{with}"
        );
        assert!(
            with.contains(&format!("destination: {CREDENTIAL_STAGED_PATH:?}")),
            "the destination is not the guest path:\n{with}"
        );
        assert!(
            with.contains(&format!(
                "File.expand_path({CREDENTIAL_FILE_NAME:?}"
            )),
            "the source is not the staged file:\n{with}"
        );

        let without = rendered_for(&cfg_with_env_file());
        assert!(
            !without.contains("bombyx_git_cred"),
            "a project with no repo_token gets no upload block:\n{without}"
        );
    }

    #[test]
    fn the_credential_flag_is_rendered_either_way() {
        // The forgeable direction is the *absent* one: a name
        // bombyx leaves out is left to /etc/profile, so a guest
        // could claim a token was configured and keep a stale
        // credential alive.
        assert_eq!(
            rendered(
                &rendered_for(&cfg_with_credential()),
                CREDENTIAL_PRESENT_ENV
            ),
            "\"1\""
        );
        assert_eq!(
            rendered(
                &rendered_for(&cfg_with_env_file()),
                CREDENTIAL_PRESENT_ENV
            ),
            "\"0\""
        );
    }

    #[test]
    fn an_env_file_adds_an_upload_and_no_env_file_adds_none() {
        let with = rendered_for(&cfg_with_env_file());
        assert!(
            with.contains("bombyx_env_file = File.expand_path"),
            "the upload block is missing:\n{with}"
        );
        assert!(
            with.contains(&format!("destination: {ENV_FILE_STAGED_PATH:?}")),
            "the destination is not the guest path:\n{with}"
        );
        assert!(
            with.contains(&format!("File.expand_path({ENV_FILE_NAME:?}")),
            "the source is not the staged file:\n{with}"
        );

        let without = rendered_for(&cfg_with(Provider::Libvirt));
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
        let out = rendered_for(&cfg_with_env_file());
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
            rendered(&rendered_for(&cfg_with_env_file()), ENV_FILE_PRESENT_ENV),
            "\"1\""
        );
        assert_eq!(
            rendered(
                &rendered_for(&cfg_with(Provider::Libvirt)),
                ENV_FILE_PRESENT_ENV
            ),
            "\"0\""
        );
    }

    #[test]
    #[should_panic(expected = "a config naming an env_file")]
    fn a_config_naming_a_file_cannot_be_rendered_against_nothing() {
        // Announcing `0` for a config that names an `env_file`
        // is the quiet half of a mismatch. `bootstrap.sh`
        // refuses the loud half -- announced `1`, nothing
        // uploaded -- but a `0` sends the guest down the branch
        // that deletes the file an earlier provision left and
        // provisions on with no secrets and no complaint.
        //
        // `Config::read_staged` is the only production route to
        // a `Staged`, so this cannot happen on a bombyx run. The
        // check is here because both arguments are public.
        let _ = render(&cfg_with_env_file(), &Staged::default());
    }

    #[test]
    fn what_the_guest_is_told_follows_what_was_staged() {
        // The render and the write step read one value. A
        // Vagrantfile announcing a secrets file that `plan`
        // never stages boots a VM that refuses minutes later,
        // reading "an env_file is configured but nothing
        // arrived" -- so the config's `env_file` key does not
        // decide this on its own.
        let cfg = cfg_with_env_file();
        let announced = render(&cfg, &cfg.staged_for_tests());
        assert_eq!(rendered(&announced, ENV_FILE_PRESENT_ENV), "\"1\"");
        assert!(
            announced.contains("bombyx_env_file = File.expand_path"),
            "the upload block is missing:\n{announced}"
        );

        // The other side of the pair: a config naming no file,
        // rendered against the `Staged` that config implies.
        let plain = cfg_with(Provider::Libvirt);
        let unstaged = render(&plain, &plain.staged_for_tests());
        assert_eq!(rendered(&unstaged, ENV_FILE_PRESENT_ENV), "\"0\"");
        assert!(
            !unstaged.contains("bombyx_env_file = File.expand_path"),
            "an upload block for a file nothing stages:\n{unstaged}"
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
        let cfg = cfg_with_env_file();
        for (name, contents) in files(&cfg, &cfg.staged_for_tests()) {
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
}
