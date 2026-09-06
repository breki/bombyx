//! Building the commands that drive Vagrant on the VM host.
//!
//! Every operation is a POSIX shell script handed to one
//! program: `ssh <host> "<script>"`, or `sh -c "<script>"` when
//! the VM host is the machine bombyx is running on.
//!
//! Two names close together, and they do different jobs.
//! `config::transport` **decides** the route once, while the
//! config loads, and stores it on `Config`. The private
//! `transport` function below **applies** that decision,
//! turning one script into one command. It chooses nothing.
//!
//! Nothing here runs a process: these functions return the argv
//! to run, which keeps the interesting logic (quoting, paths,
//! command composition) unit-testable without a VM host.
//!
//! This module is the builders, the VM-host identity constants,
//! and the constants deciding what the `vagrant` process on the
//! host finds in its environment -- which is where bombyx says
//! what provider to use.
//! Two neighbours hold the pieces they are built from: `command`
//! defines [`RemoteCommand`], and `quote` holds
//! the POSIX quoting primitives -- pure functions with their own
//! dense test block and no dependency on [`Config`], which is why
//! they read as a separate unit. Both are re-exported, so
//! `bombyx::remote::shell_quote` is an unchanged path.

mod command;
pub mod probe;
mod quote;
mod write;

pub use command::RemoteCommand;
pub use quote::{quote_remote_path, shell_quote};
pub use write::write_file;

use crate::config::{Config, Transport};

/// Environment variable carrying the VM host's SSH alias into
/// the `vagrant` process on the host.
///
/// The alias as bombyx knows it -- `frosti`, `my-vmhost` -- which
/// is the name the operator recognises, since they chose it.
///
/// # Why this exists
///
/// A VM booted by bombyx cannot work out which machine it is
/// running on. There is no synced folder to read, `hostname`
/// inside the guest answers with the guest's own name, and
/// libvirt does not pass the host's name in at all: the guest's
/// SMBIOS/DMI describes the *emulated* machine, so
/// `/sys/class/dmi/id/sys_vendor` reads `QEMU` and
/// `product_name` names a QEMU machine type. Measured inside a
/// live guest as the unprivileged user -- those files are
/// readable and simply hold nothing about the host, while the
/// root-only ones (`product_serial`) carry no host name either.
/// There is nothing to read at any privilege level.
///
/// So this and [`VM_HOSTNAME_ENV`] travel on the `vagrant`
/// invocation instead.
///
/// # The project's half
///
/// The variables reach the `vagrant` process on the host and no
/// further: **Vagrant does not export its own environment into a
/// guest.** A provisioner runs inside the guest under the guest's
/// environment, so anything from the host has to be handed over
/// deliberately. The `Vagrantfile` is Ruby running on the host,
/// so it can read these and pass them to a shell provisioner
/// through its `env:` option -- see the "Telling the VM which
/// host it runs on" section of `README.md`.
pub const VM_HOST_ENV: &str = "BOMBYX_VM_HOST";

/// Environment variable carrying what the VM host calls itself.
///
/// The machine's own short name, which need not match
/// [`VM_HOST_ENV`]: an alias in `~/.ssh/config` can be anything,
/// and often is. Both are passed so a guest can show the alias
/// and still tell that the two disagree.
///
/// The two *can* legitimately agree. A WSL2 distribution that has
/// not been given a name of its own reports the Windows machine's
/// name, so on that kind of host this value may equal the
/// workstation's -- expected, not a sign of the wrong-side
/// expansion that `vm_host_env` guards against. Not a doc link:
/// that function is private, and rustdoc rejects a public page
/// pointing at a private item.
pub const VM_HOSTNAME_ENV: &str = "BOMBYX_VM_HOSTNAME";

/// Vagrant's own variable naming the provider it must use.
///
/// Rendering `config.vm.provider :libvirt` in the generated
/// Vagrantfile only *configures* that provider. Vagrant still
/// chooses one for itself, and it chooses from what the host
/// offers, so a project asking for one provider can be handed
/// another with its settings block silently unapplied.
///
/// Setting this makes vagrant use the named provider or refuse,
/// which is the outcome bombyx wants: a machine that does not
/// boot says more than a machine built to the wrong shape.
///
/// The variable rather than `vagrant up --provider <name>`
/// because the two spell the same choice and only `up` accepts
/// the argument.
///
/// **Every project vagrant call carries it except the
/// teardown.** The reason it goes on more than the boot is the
/// `unset` in `DISARM_VAGRANT_REDIRECTS`, which clears the
/// operator's own exported value in front of every script. A
/// verb that did not write the configured one back would leave
/// vagrant choosing for itself, and on a host where the other
/// provider cannot answer a probe -- a WSL2 distribution with
/// no PowerShell for the Hyper-V provider to call -- vagrant
/// refuses the command instead. `docs/vm-host-wsl2.md`
/// describes that host. Not a doc link: the constant is
/// private, and rustdoc rejects a public page pointing at a
/// private item.
///
/// `doctor`'s probe is not a project call and carries none:
/// `vagrant plugin list` ignores this variable, measured under
/// `hyperv` on Linux and under a provider name that does not
/// exist.
///
/// **The teardown is the exception, and `tears_down` holds
/// it.** Three measured facts about vagrant, all checked on a
/// libvirt host. With a machine already created vagrant reads
/// the provider it recorded and ignores this variable. With no
/// machine yet an unusable provider makes it refuse `status`,
/// `halt` and `destroy` as readily as `up`. And with no such
/// variable set, `destroy` in a directory holding a
/// Vagrantfile and no machine reports "Domain is not created"
/// and exits 0.
///
/// So naming a provider on `destroy` can only ever refuse it,
/// and `execute` stops at the first failing step, which leaves
/// the directory removal behind it unrun and nothing else to
/// clear the directory. Omitting it there is safe for the same
/// reason: a refusal implies no machine exists, so there is no
/// recorded provider to disagree with.
///
/// The first of those facts is also a limit worth knowing:
/// editing `provider` and re-running `up` on a project that
/// already has a VM keeps the old one, silently.
/// `provider-change-on-existing-vm` in `docs/todo.md` holds
/// that.
pub const PROVIDER_ENV: &str = "VAGRANT_DEFAULT_PROVIDER";

/// The environment prefix that tells the guest which machine it
/// is running on.
///
/// See [`VM_HOST_ENV`] for why the guest cannot find this out for
/// itself, and what the project's `Vagrantfile` has to do with the
/// values.
///
/// **`$(hostname -s)` is left unexpanded on purpose.** bombyx
/// writes the substitution into the script verbatim and lets
/// the shell that receives it answer. Over `ssh` that shell is on the VM
/// host; running here it is the `sh` bombyx started, and this
/// machine is the VM host. Either route answers with the VM
/// host's name.
///
/// Expanding it while building the script would be right on one
/// route and wrong on the other, and the wrong answer is the
/// workstation's own name -- plausible-looking, which is the
/// failure worth guarding against. It is unquoted because a
/// shell assignment does not field-split its value, so the
/// quotes would only add noise to the dry-run output.
///
/// A host with no `hostname` command leaves the variable empty
/// rather than failing the boot. Reporting an unknown host name is
/// a smaller problem than refusing to start a VM over a status
/// line.
fn vm_host_env(cfg: &Config) -> String {
    format!(
        "{VM_HOST_ENV}={host} {VM_HOSTNAME_ENV}=$(hostname -s)",
        host = shell_quote(cfg.host.as_str()),
    )
}

/// Builds the `vagrant` command itself: the identity and
/// provider prefix, the program, and its quoted arguments.
///
/// Split out from [`vagrant_script`] so every shape bombyx emits
/// carries the same prefix. [`vagrant_script`] puts the command
/// after a bare `cd`, so a builder needing it somewhere else
/// calls this function instead. Two do:
/// `destroy_vm_if_present` nests it inside an
/// `if [ -f Vagrantfile ]` guard, and `save_snapshot_if_absent`
/// puts `snapshot list` and `snapshot save` inside one `if`. A
/// builder assembling its own string would run `vagrant` with
/// none of the three variables set.
fn vagrant_command(cfg: &Config, args: &[&str]) -> String {
    use std::fmt::Write as _;
    let mut cmd = vm_host_env(cfg);
    if !tears_down(args) {
        // `Provider` renders one of two fixed lowercase words,
        // so there is no operator input here for a quote to
        // protect. It is quoted anyway, so the assignment
        // matches every other one in the script.
        let _ = write!(
            cmd,
            " {PROVIDER_ENV}={}",
            shell_quote(cfg.vm.provider.as_str())
        );
    }
    cmd.push_str(" vagrant");
    for arg in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(arg));
    }
    cmd
}

/// Whether `args` names the vagrant verb that removes a
/// machine.
///
/// Read from the verb rather than passed in by the caller. The
/// callers hand `vagrant` its arguments, so a rule derived from
/// those arguments cannot disagree with the command that gets
/// run, while a separate flag could be set wrongly on a new
/// call site. [`PROVIDER_ENV`] holds why the teardown is the
/// one verb that names no provider.
fn tears_down(args: &[&str]) -> bool {
    args.first() == Some(&"destroy")
}

/// Builds the remote script that enters `dir` and runs
/// `vagrant` with `args`.
///
/// Every vagrant invocation that runs **inside a project
/// directory** carries [`vm_host_env`], not just the ones that
/// provision. `halt` and `status` have no use for the values, and
/// setting them in one place is what keeps the action that *does*
/// need them from being the one that was forgotten.
///
/// The one exemption is `doctor`, whose probes
/// ([`probe::provider`]) inspect the host's own vagrant
/// installation rather than a project. `vagrant plugin list`
/// reads no `Vagrantfile` -- checked by running it in a
/// directory holding one that raises -- so there is nothing
/// there to read the variables.
fn vagrant_script(cfg: &Config, dir: &str, args: &[&str]) -> String {
    format!(
        "cd {dir} && {cmd}",
        dir = quote_remote_path(dir),
        cmd = vagrant_command(cfg, args),
    )
}

/// Whether `ssh` should allocate a remote pseudo-terminal (`-t`).
///
/// It decides more than interactivity, which is why it is a
/// parameter rather than a detail of the one command that obviously
/// needs it. Without a PTY the remote program's stdout is a pipe, so
/// the remote tty layer never translates `\n` to `\r\n`, and every
/// line arrives with a bare line feed. Measured against a real host:
/// `vagrant status` returned 206 bytes containing six line feeds and
/// **no** carriage returns, which a Windows console renders as a
/// staircase -- each line starting at the column where the last one
/// ended.
///
/// So the choice is not cosmetic, and it is not free either.
///
/// [`Tty::Allocate`] buys readable line endings, and lets the remote
/// colourize. It costs three things: the remote's stderr is merged
/// into stdout, the local terminal goes into raw mode, and it needs a
/// local terminal to allocate against at all -- without one, ssh
/// prints `Pseudo-terminal will not be allocated because stdin is
/// not a terminal.` and carries on with no PTY, which is measured
/// and is why the caller inspects stdin.
///
/// It also carries `-o LogLevel=ERROR`, and that is not tidying.
/// A tty session makes ssh print `Connection to <host> closed.` to
/// stderr when it ends, so without this every `status`, `up` and
/// teardown would gain a spurious trailing line. Measured: the
/// message appears at the default level and not at `ERROR`, while a
/// genuine failure still reports identically -- an unresolvable host
/// gives the same message and the same 255 either way. `QUIET`
/// suppresses the line too and was rejected: it would also swallow
/// real diagnostics from a tool whose failures matter.
///
/// [`Tty::NoPty`] keeps the bytes exactly as the remote wrote them,
/// which is what a pipe, a redirect or a parsed probe needs.
///
/// `doctor`'s probes are the case that must stay [`Tty::NoPty`]:
/// their output is compared and sanitized, and a PTY would fold
/// control characters and CRs into the text being checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tty {
    /// Pass `-t`, so the remote gets a pseudo-terminal.
    Allocate,
    /// No `-t`; the remote writes to a pipe.
    ///
    /// Spelled `NoPty` rather than `None` so a `match` arm cannot be
    /// misread as an absent [`Option`].
    NoPty,
}

impl Tty {
    /// The choice for a run whose streams are (or are not)
    /// terminals.
    ///
    /// **Both have to be terminals**, and each for its own reason.
    /// `ssh -t` needs a local terminal to allocate against, and
    /// merely warns and carries on without one. And the reason to
    /// want a PTY is that the remote tty then translates `\n` to
    /// `\r\n`, which only helps when the output is going to a
    /// terminal -- piped or redirected, the bytes must stay exactly
    /// as the remote wrote them, or a captured log gains carriage
    /// returns and, since the remote colourizes under a PTY, escape
    /// sequences too.
    ///
    /// A pure function of two booleans so the rule is testable; the
    /// binary supplies them from `IsTerminal`.
    #[must_use]
    pub fn for_streams(stdin_tty: bool, stdout_tty: bool) -> Self {
        if stdin_tty && stdout_tty {
            Self::Allocate
        } else {
            Self::NoPty
        }
    }
}

/// Cleared from the environment before a script runs, on
/// either route.
///
/// The rule the list must satisfy: **every vagrant variable that
/// decides which directory, which machine or which provider a
/// command acts on**. Check a new name against that rather than
/// against the length of the list.
///
/// Three of them redirect the directory. Every script that runs
/// `vagrant` on a project bounds it by starting `cd <dir> &&`,
/// and `destroy` narrows that further with
/// `if [ -f Vagrantfile ]`. `VAGRANT_CWD` moves where vagrant
/// looks, `VAGRANT_VAGRANTFILE` renames the file it reads there,
/// and `VAGRANT_DOTFILE_PATH` moves the state directory naming
/// the machine. So an operator with one of them exported would
/// have `destroy` test one project and destroy another.
///
/// Two redirect the provider. [`PROVIDER_ENV`] names one
/// outright, and `VAGRANT_PREFERRED_PROVIDERS` ranks the usable
/// ones when [`PROVIDER_ENV`] is unset. An operator with
/// [`PROVIDER_ENV`] exported to a provider this host cannot
/// supply gets a refused `vagrant destroy`, and since `execute`
/// stops at the first failing step, the directory removal
/// behind it never runs. Measured on a libvirt host with
/// `hyperv` exported.
///
/// bombyx then writes its own [`PROVIDER_ENV`] in front of
/// every vagrant call. That assignment comes after this `unset`
/// and wins, so clearing the pair costs nothing.
///
/// **Both routes need it, for different reasons.** `sh -c` is a
/// child of bombyx and inherits everything the operator
/// exported. bombyx's own environment does not cross `ssh`, but
/// the VM host builds one of its own. Three sources reach the
/// command sshd runs: `pam_env` applies `/etc/environment`,
/// `zsh` sources `~/.zshenv` on every invocation and so on
/// `zsh -c` too, and a `bash` export placed above the
/// non-interactive return guard in `~/.bashrc` survives.
///
/// The `ssh` command is neither interactive nor a login shell,
/// which is why the list names those three and not `~/.profile`.
/// The `zsh` and `bash` halves are read from those shells'
/// documented startup order rather than measured
/// *(unverified)*.
///
/// Ends in `; ` so it prefixes any script, `cat` heredoc
/// included.
const DISARM_VAGRANT_REDIRECTS: &str = "unset VAGRANT_CWD \
     VAGRANT_VAGRANTFILE VAGRANT_DOTFILE_PATH \
     VAGRANT_DEFAULT_PROVIDER VAGRANT_PREFERRED_PROVIDERS; ";

/// The script `c` carries, without the prefix every route puts
/// in front of it.
///
/// One accessor rather than a strip in each test module, so the
/// two cannot disagree on how strict the strip is. It panics on
/// a command without the prefix, which is the assertion:
/// `every_route_disarms_the_vagrant_redirects` and
/// `every_probe_disarms_the_vagrant_redirects_on_both_routes`
/// state the rule, and this is what every other test relies on
/// having held.
#[cfg(test)]
pub(crate) fn script_without_disarm(c: &RemoteCommand) -> String {
    c.args
        .last()
        .expect("a remote command")
        .strip_prefix(DISARM_VAGRANT_REDIRECTS)
        .expect("every script carries the disarming prefix")
        .to_owned()
}

/// `c` as [`Display`](std::fmt::Display) renders it, with the
/// same prefix removed.
///
/// The whole command rather than the script alone, for a test
/// asserting on the line `--dry-run` prints. It panics on a
/// command without the prefix, for the reason
/// [`script_without_disarm`] gives.
#[cfg(test)]
pub(crate) fn rendered_without_disarm(c: &RemoteCommand) -> String {
    let shown = c.to_string();
    assert!(
        shown.contains(DISARM_VAGRANT_REDIRECTS),
        "every command carries the disarming prefix: {shown}"
    );
    shown.replacen(DISARM_VAGRANT_REDIRECTS, "", 1)
}

/// Wraps `script` in the command that runs it on the VM host.
///
/// The one wrapper every VM command goes through, so no builder
/// can grow its own opinion about the route.
/// [`Config::transport`] holds the decision and
/// `config::transport` explains how it was reached.
///
/// Over `ssh`, options come before the destination and everything
/// after it is the remote command, which is why `-t` sits where
/// it does. Running here, `sh -c` starts the same POSIX shell
/// `ssh` would have started on the host, so `script` is handed
/// over untouched -- and `tty` has nothing to ask for, because
/// the shell inherits whatever stdio bombyx itself was given.
///
/// [`DISARM_VAGRANT_REDIRECTS`] goes in front of the script on
/// every route, because the shell running it can carry an
/// exported vagrant variable on either one.
fn transport(cfg: &Config, script: &str, tty: Tty) -> RemoteCommand {
    let disarmed = format!("{DISARM_VAGRANT_REDIRECTS}{script}");
    let script = disarmed.as_str();
    match (cfg.transport(), tty) {
        (Transport::Local, _) => RemoteCommand::new("sh", &["-c", script]),
        (Transport::Ssh, Tty::Allocate) => RemoteCommand::new(
            "ssh",
            &["-t", "-o", "LogLevel=ERROR", cfg.host.as_str(), script],
        ),
        (Transport::Ssh, Tty::NoPty) => {
            RemoteCommand::new("ssh", &[cfg.host.as_str(), script])
        }
    }
}

/// Builds the command running `vagrant` in `dir` on the VM host.
///
/// See [`Tty`] for what the last argument costs and buys. The
/// private `transport` function turns the script into one of
/// the two command shapes.
#[must_use]
pub fn vagrant_in(
    cfg: &Config,
    dir: &str,
    args: &[&str],
    tty: Tty,
) -> RemoteCommand {
    let script = vagrant_script(cfg, dir, args);
    transport(cfg, &script, tty)
}

/// Builds the command running `vagrant` in the project
/// directory on the VM host.
#[must_use]
pub fn vagrant(cfg: &Config, args: &[&str], tty: Tty) -> RemoteCommand {
    vagrant_in(cfg, &cfg.remote_project_dir(), args, tty)
}

/// Builds the command that creates `dir` on the VM host if it
/// does not yet exist.
#[must_use]
pub fn ensure_dir(cfg: &Config, dir: &str) -> RemoteCommand {
    let script = format!("mkdir -p {}", quote_remote_path(dir));
    transport(cfg, &script, Tty::NoPty)
}

/// Builds the command that destroys the VM defined in `dir`,
/// doing nothing when there is no Vagrantfile there.
///
/// The guard makes teardown idempotent. A bare
/// `vagrant destroy -f` exits non-zero in a directory with no
/// Vagrantfile, and an `up` interrupted between the `mkdir` and
/// the Vagrantfile write leaves exactly that behind. The failure
/// would stop the removal step that follows.
///
/// The command comes from the same private `vagrant_command`
/// helper the other builders use, so it carries the same identity
/// prefix as every other invocation. It matters
/// here more than it looks: teardown still *evaluates* the
/// project's `Vagrantfile`, so one reading
/// `ENV.fetch("BOMBYX_VM_HOST")` without a default would raise on
/// `destroy` after working on `up` -- and since execution stops at
/// the first failing step, the directory removal that follows
/// would never run.
///
/// Takes a [`Tty`] like the other vagrant builders, and for the
/// same reason: `vagrant destroy -f` prints several lines of
/// progress, so without one `destroy` and `discard` staircase
/// on the console this parameter exists to fix.
#[must_use]
pub fn destroy_vm_if_present(
    cfg: &Config,
    dir: &str,
    tty: Tty,
) -> RemoteCommand {
    let script = format!(
        "cd {dir} && if [ -f Vagrantfile ]; then {cmd}; fi",
        dir = quote_remote_path(dir),
        cmd = vagrant_command(cfg, &["destroy", "-f"]),
    );
    transport(cfg, &script, tty)
}

/// The snapshot name bombyx saves and restores.
///
/// A primitive because the value carries no rule: bombyx chooses
/// the name rather than reading it from the operator, so there
/// is nothing for a constructor to check. One constant rather
/// than a literal per call site, because the saving commands and
/// the restoring one have to name the same snapshot.
pub const FRESH_SNAPSHOT: &str = "fresh-install";

/// Builds the command that restores the VM in `dir` to its
/// [`FRESH_SNAPSHOT`].
#[must_use]
pub fn restore_snapshot(cfg: &Config, dir: &str, tty: Tty) -> RemoteCommand {
    vagrant_in(cfg, dir, &["snapshot", "restore", FRESH_SNAPSHOT], tty)
}

/// Builds the command that saves the VM in `dir` as
/// [`FRESH_SNAPSHOT`], replacing one that is already there.
///
/// `-f` is what makes the command re-takeable. Vagrant refuses a
/// name it already holds without it, printing `You must include
/// the --force option to replace an existing snapshot.` and
/// exiting 1.
#[must_use]
pub fn save_snapshot(cfg: &Config, dir: &str, tty: Tty) -> RemoteCommand {
    vagrant_in(cfg, dir, &["snapshot", "save", "-f", FRESH_SNAPSHOT], tty)
}

/// Builds the command that saves the VM in `dir` as
/// [`FRESH_SNAPSHOT`] when it does not already hold that name.
///
/// The guard reads `vagrant snapshot list` rather than letting a
/// plain save fail. Two measured facts require that: `snapshot
/// list` exits 0 whether or not any snapshot exists, so its
/// status answers nothing, and a save over an existing name
/// exits 1, which would stop `up` at its last step on every run
/// after the first.
///
/// The listing is captured into a variable, and the `&&` after
/// it is what stops a failed listing being read as an empty one.
/// Piping it straight into `grep` would hide that: a shell
/// pipeline reports only its last command's status, so a machine
/// vagrant could not read would look indistinguishable from one
/// holding no snapshots, and the save would run on that reading.
///
/// `grep -qx` requires a whole-line match. `snapshot list`
/// prints each name bare on its own line, and when the machine
/// has none it prints an explanation of how to take one, no line
/// of which is a bare name. `printf` rather than `echo` because
/// a snapshot the operator named `-n` would be swallowed as an
/// option instead of compared.
///
/// The trailing `|| printf ... >&2` makes the snapshot advisory.
/// It is the last step of `up`, and `execute` stops at the first
/// failing step and returns its status, so without it a VM that
/// booted and provisioned correctly reports failure because a
/// snapshot could not be taken. Two machines reach that on every
/// run: a provider with no snapshot support, whose listing
/// raises, and one whose listing decorates the name, so the
/// guard reads "absent" and vagrant then refuses the unforced
/// save.
///
/// The braces are what keep the `cd` out of that. `&&` and `||`
/// have equal precedence and associate left, so an unbraced
/// `||` would also answer for the `cd` -- and a project
/// directory that has gone away would report success behind a
/// message naming a snapshot. Grouping the listing and the save
/// leaves the `cd` failing the step, as it does in every other
/// builder here.
///
/// The message names the project and asks for one word to be
/// changed, rather than spelling a command out that would drop
/// the operator's other arguments. `confirm_destroy` states the
/// same rule for the same reason.
#[must_use]
pub fn save_snapshot_if_absent(
    cfg: &Config,
    dir: &str,
    tty: Tty,
) -> RemoteCommand {
    let script = format!(
        "cd {dir} && {{ names=$({list}) && \
         if ! printf '%s\\n' \"$names\" | grep -qx {name}; \
         then {save}; fi \
         || printf 'bombyx: could not save the {name_bare} snapshot \
         for %s; re-run this command with snapshot in place of \
         up\\n' {project} >&2; }}",
        dir = quote_remote_path(dir),
        list = vagrant_command(cfg, &["snapshot", "list"]),
        name = shell_quote(FRESH_SNAPSHOT),
        save = vagrant_command(cfg, &["snapshot", "save", FRESH_SNAPSHOT]),
        name_bare = FRESH_SNAPSHOT,
        project = shell_quote(cfg.project.as_str()),
    );
    transport(cfg, &script, tty)
}

/// Builds the command that recursively removes `dir` on the VM
/// host.
///
/// This is the widest-reaching command bombyx emits: its blast
/// radius is bounded by a path rather than by Vagrant's notion
/// of a machine. Nothing is checked here, deliberately --
/// `RemoteRoot`'s constructor refuses a root that is unrooted,
/// contains a `.` or `..` segment, or is shallower than one
/// segment. bombyx then joins the project name onto that root,
/// so every path derived from a loaded `Config` is already at
/// least two real segments deep. A type running the rules once
/// is what
/// keeps the write path (`mkdir`, then the heredocs) and this
/// removal path agreeing about which roots are usable.
///
/// The `debug_assert` catches a caller that builds a path some
/// other way; it is not the safety mechanism.
#[must_use]
pub fn remove_dir(cfg: &Config, dir: &str) -> RemoteCommand {
    debug_assert!(
        crate::config::path_segments(dir).len() >= 2,
        "remove_dir given a path shallower than Config permits: {dir:?}"
    );
    let script = format!("rm -rf {}", quote_remote_path(dir));
    transport(cfg, &script, Tty::NoPty)
}

/// Builds the command that opens an interactive shell inside
/// the project's VM.
///
/// Always [`Tty::Allocate`], and unconditionally so: `vagrant ssh`
/// needs a TTY when invoked through a non-interactive SSH command,
/// and an interactive shell without one is unusable whatever the
/// local stdio looks like. Every other vagrant call decides per run.
#[must_use]
pub fn shell_into_vm(cfg: &Config) -> RemoteCommand {
    vagrant_in(cfg, &cfg.remote_project_dir(), &["ssh"], Tty::Allocate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::ScratchName;

    /// The ssh options that precede the destination, in order.
    ///
    /// Asserted as a whole rather than by index. A test pinning
    /// `args[1]` as the host passes an argv where a new option
    /// has pushed the host somewhere else: the index still holds
    /// a string, just the wrong one. Comparing the whole list is
    /// what notices.
    fn opts_before_host(c: &RemoteCommand) -> Vec<String> {
        c.args
            .iter()
            .take_while(|a| *a != "vmhost")
            .cloned()
            .collect()
    }

    /// The remote script as built, whatever precedes it.
    fn raw_script(c: &RemoteCommand) -> String {
        c.args.last().expect("a remote command").clone()
    }

    /// The remote script without the `unset` every route
    /// carries.
    ///
    /// Every test below asks about the part of the script the
    /// builder wrote, so stripping here keeps the prefix out of
    /// two dozen expected strings.
    /// `every_route_disarms_the_vagrant_redirects` is the one
    /// test that reads the prefix, and it uses `raw_script`.
    fn remote_script(c: &RemoteCommand) -> String {
        script_without_disarm(c)
    }

    fn local_cfg() -> Config {
        Config::for_tests_local()
    }

    #[test]
    fn the_local_route_runs_the_same_script_through_sh() {
        // The script is the delicate part: quoting, the `cd`,
        // the heredoc delimiter and the `$(hostname -s)` the far
        // side must evaluate. `sh -c` is the same POSIX shell
        // `ssh` starts on the host, so every builder keeps one
        // script. The only difference on this route is the two
        // words in front of it, so the scripts must be equal
        // character for character.
        /// One builder, named for the error message.
        type Builder = (&'static str, fn(&Config) -> RemoteCommand);

        let builders: [Builder; 7] = [
            ("vagrant", |c| vagrant(c, &["status"], Tty::NoPty)),
            ("ensure_dir", |c| ensure_dir(c, "~/vms")),
            ("remove_dir", |c| remove_dir(c, "~/vms/myproject")),
            ("destroy", |c| {
                destroy_vm_if_present(c, "~/vms/myproject", Tty::NoPty)
            }),
            ("snapshot", |c| {
                save_snapshot(c, &c.remote_project_dir(), Tty::NoPty)
            }),
            ("guarded snapshot", |c| {
                save_snapshot_if_absent(c, &c.remote_project_dir(), Tty::NoPty)
            }),
            ("write", |c| write_file(c, "~/vms", "Vagrantfile", "x\n")),
        ];
        for (name, build) in builders {
            let over_ssh = build(&cfg());
            let here = build(&local_cfg());
            assert_eq!(here.program, "sh", "{name}");
            assert_eq!(here.args.len(), 2, "{name}: {:?}", here.args);
            assert_eq!(here.args[0], "-c", "{name}");
            assert_eq!(
                remote_script(&here),
                remote_script(&over_ssh),
                "{name}"
            );
        }
    }

    #[test]
    fn every_route_disarms_the_vagrant_redirects() {
        // Both routes hand the script a shell that may already
        // carry the operator's exported variables. `sh -c` is a
        // child of bombyx and inherits its whole environment.
        // Over `ssh` bombyx's own environment stays behind, but
        // the VM host builds one of its own: `pam_env` applies
        // `/etc/environment` to a non-interactive command, and
        // `zsh` sources `~/.zshenv` on every invocation, and
        // a `bash` export above the non-interactive return
        // guard in `~/.bashrc` survives. Either way
        // three vagrant variables override the directory the
        // script just `cd`'d into, so `destroy` would test
        // `[ -f Vagrantfile ]` in one project and destroy the
        // machine defined in another.
        for route in [cfg(), local_cfg()] {
            for c in [
                vagrant(&route, &["status"], Tty::NoPty),
                destroy_vm_if_present(&route, "~/vms/p", Tty::NoPty),
                save_snapshot(&route, "~/vms/p", Tty::NoPty),
                save_snapshot_if_absent(&route, "~/vms/p", Tty::NoPty),
                ensure_dir(&route, "~/vms"),
                write_file(&route, "~/vms", "Vagrantfile", "x\n"),
            ] {
                // The prefix alone, not the whole script.
                // `vagrant_command` writes `PROVIDER_ENV` into
                // the same string, so a search over everything
                // would find that assignment and report the
                // variable disarmed with the `unset` gone.
                let raw = raw_script(&c);
                let prefix = raw
                    .split_once("; ")
                    .expect("the script carries the unset prefix")
                    .0;
                assert!(prefix.starts_with("unset "), "{raw}");
                for var in [
                    "VAGRANT_CWD",
                    "VAGRANT_VAGRANTFILE",
                    "VAGRANT_DOTFILE_PATH",
                    PROVIDER_ENV,
                    "VAGRANT_PREFERRED_PROVIDERS",
                ] {
                    assert!(
                        prefix.contains(var),
                        "{var} not disarmed: {prefix}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_local_route_asks_for_no_pty() {
        // `-t` is an `ssh` option. `sh -c` inherits whatever
        // stdio bombyx itself was given, so an interactive
        // `bombyx shell` still gets the operator's terminal and
        // there is nothing to request.
        for c in [
            vagrant(&local_cfg(), &["status"], Tty::Allocate),
            destroy_vm_if_present(&local_cfg(), "~/vms/p", Tty::Allocate),
            shell_into_vm(&local_cfg()),
        ] {
            assert_eq!(c.program, "sh");
            assert!(!c.args.iter().any(|a| a == "-t"), "{:?}", c.args);
        }
    }

    #[test]
    fn a_tty_run_asks_for_a_pty_and_silences_the_closing_notice() {
        // Order matters to ssh: options come before the destination,
        // and everything after it is the remote command, so `-t`
        // landing after the host would be handed to the remote
        // shell instead.
        //
        // `LogLevel=ERROR` is measured, not decoration: a tty
        // session makes ssh print `Connection to <host> closed.` to
        // stderr, which would end every status and up with a
        // spurious line. A genuine failure still reports at this
        // level.
        let c = vagrant(&cfg(), &["status"], Tty::Allocate);
        assert_eq!(c.program, "ssh");
        assert_eq!(opts_before_host(&c), vec!["-t", "-o", "LogLevel=ERROR"]);
        assert!(remote_script(&c).contains("vagrant 'status'"));
    }

    #[test]
    fn no_tty_passes_no_options_at_all() {
        // The default for a pipe or a redirect: the remote's bytes
        // arrive unchanged, which is what a captured log needs, and
        // ssh emits no pseudo-terminal warning.
        let c = vagrant(&cfg(), &["status"], Tty::NoPty);
        assert!(opts_before_host(&c).is_empty(), "{:?}", c.args);
        assert_eq!(c.args.len(), 2);
    }

    #[test]
    fn the_tty_choice_does_not_disturb_the_remote_script() {
        // Only the argv ahead of the host differs. If the script
        // itself changed with the tty, the printed plan and the
        // executed one would describe different work.
        let with = vagrant(&cfg(), &["status"], Tty::Allocate);
        let without = vagrant(&cfg(), &["status"], Tty::NoPty);
        assert_eq!(remote_script(&with), remote_script(&without));
    }

    #[test]
    fn an_interactive_shell_always_gets_a_tty() {
        // Unconditional here, unlike every other vagrant call:
        // `vagrant ssh` needs a TTY through a non-interactive SSH
        // command, and a shell without one is unusable whatever the
        // local stdio looks like.
        let c = shell_into_vm(&cfg());
        assert_eq!(opts_before_host(&c), vec!["-t", "-o", "LogLevel=ERROR"]);
        assert_eq!(
            remote_script(&c),
            remote_script(&vagrant(&cfg(), &["ssh"], Tty::NoPty))
        );
    }

    #[test]
    fn teardown_takes_a_tty_like_every_other_vagrant_call() {
        // `vagrant destroy -f` streams progress, so without a PTY
        // it staircases on the console this parameter exists to
        // fix.
        let with = destroy_vm_if_present(&cfg(), "~/vms/p", Tty::Allocate);
        assert_eq!(opts_before_host(&with), vec!["-t", "-o", "LogLevel=ERROR"]);
        let without = destroy_vm_if_present(&cfg(), "~/vms/p", Tty::NoPty);
        assert!(opts_before_host(&without).is_empty());
        assert_eq!(remote_script(&with), remote_script(&without));
    }

    #[test]
    fn the_stream_rule_needs_both_streams() {
        // stdin, because ssh needs a local terminal to allocate
        // against and merely warns without one; stdout, because the
        // translation only helps output that reaches a terminal.
        assert_eq!(Tty::for_streams(true, true), Tty::Allocate);
        assert_eq!(Tty::for_streams(true, false), Tty::NoPty);
        assert_eq!(Tty::for_streams(false, true), Tty::NoPty);
        assert_eq!(Tty::for_streams(false, false), Tty::NoPty);
    }
    fn cfg() -> Config {
        Config::for_tests()
    }

    /// The identity prefix every vagrant script carries.
    ///
    /// `vagrant_carries_the_vm_host_identity` spells it out in
    /// full. Everything else references this or [`vagrant_env`],
    /// because their subject is the directory and the arguments;
    /// repeating the prefix in each of them would push every
    /// assertion past the line limit and give it several places
    /// to drift.
    ///
    /// Built from the exported constants rather than hardcoding
    /// their values. Hardcoded, renaming either constant would
    /// leave this module green while bombyx exported a different
    /// variable name -- which is the one failure these assertions
    /// exist to catch.
    fn vm_env() -> String {
        format!("{VM_HOST_ENV}='vmhost' {VM_HOSTNAME_ENV}=$(hostname -s)")
    }

    /// The whole prefix on every vagrant call: the identity and
    /// the provider.
    ///
    /// [`vm_env`] is the identity half alone, which is what the
    /// assertions about the guest's two names use.
    /// `every_project_vagrant_call_names_the_provider` in `plan`
    /// is what holds the provider half across the actions.
    ///
    /// The provider is read back from the test config rather
    /// than spelled out, for the reason [`vm_env`] gives about
    /// the variable names: what these assertions are about is
    /// that the configured provider reaches the script, not
    /// that the word `libvirt` appears in it.
    fn vagrant_env() -> String {
        format!("{} {PROVIDER_ENV}='{}'", vm_env(), cfg().vm.provider)
    }

    #[test]
    fn vagrant_names_the_provider_the_config_asks_for() {
        // The defect this closes: the generated Vagrantfile
        // *configures* a provider, and configuring one does
        // nothing unless vagrant independently picks it. A
        // hyperv project on a libvirt-only host booted a libvirt
        // machine at vagrant's defaults, because the `:hyperv`
        // settings block never applied and nothing reported the
        // substitution.
        let mut cfg = cfg();
        cfg.vm.provider = crate::config::Provider::Hyperv;
        let script = remote_script(&vagrant(&cfg, &["up"], Tty::NoPty));
        assert!(
            script.contains(&format!("{PROVIDER_ENV}='hyperv'")),
            "{script}"
        );
    }

    #[test]
    fn vagrant_carries_the_vm_host_identity() {
        // The guest cannot work out which machine it runs on:
        // there is no synced folder, `hostname` inside the VM
        // answers with the guest's own name, and libvirt puts
        // nothing about the host anywhere a non-root process can
        // read. So the two names ride in on the one command that
        // crosses the boundary, and the guest's provisioning
        // writes them down.
        let c = vagrant(&cfg(), &["up"], Tty::NoPty);
        assert_eq!(
            remote_script(&c),
            "cd ~/'vms/myproject' && BOMBYX_VM_HOST='vmhost' \
             BOMBYX_VM_HOSTNAME=$(hostname -s) \
             VAGRANT_DEFAULT_PROVIDER='libvirt' vagrant 'up'"
        );
    }

    #[test]
    fn the_hostname_is_evaluated_on_the_far_side() {
        // `$(...)` in a remote command is the wrong-side
        // expansion trap: expanded here it would report the
        // *workstation's* name, which is plausible enough that
        // nobody would question it. bombyx spawns `ssh` directly
        // rather than through a shell, so the substitution
        // reaches the host verbatim -- and the dry run has to
        // show it escaped, or a pasted line would answer with
        // the wrong machine.
        let c = vagrant(&cfg(), &["up"], Tty::NoPty);
        assert!(
            remote_script(&c).contains("$(hostname -s)"),
            "{}",
            remote_script(&c)
        );
        assert!(c.to_string().contains(r"\$(hostname -s)"), "{c}");
    }

    #[test]
    fn teardown_carries_the_identity_too() {
        // Pins one of the two builders that call
        // `vagrant_command` directly; `vagrant_command` names
        // both and says why neither can use `vagrant_script`.
        //
        // It matters most here. Teardown still evaluates the
        // project's Vagrantfile, so one reading the variable
        // without a default would raise on `destroy` after working
        // on `up`, and the directory removal that follows would
        // never run.
        //
        // Exhaustiveness across actions is asserted in `plan`,
        // which can enumerate them.
        let script =
            destroy_vm_if_present(&cfg(), "~/vms/myproject", Tty::NoPty).args
                [1]
            .clone();
        assert!(script.contains(&vm_env()), "{script}");
    }

    #[test]
    fn the_vm_host_alias_is_quoted_in_the_script() {
        // The alias is interpolated into a remote script, so it
        // goes through `shell_quote` rather than being trusted
        // because `config::host` checked it. A hostile alias
        // cannot be assigned to a `HostName`, so what this
        // asserts is the wiring: delete the `shell_quote` call
        // in `vm_host_env` and the quotes go missing here.
        // `remote::quote` tests what quoting does to a value
        // that needs it.
        let script = remote_script(&vagrant(&cfg(), &["up"], Tty::NoPty));
        assert!(script.contains("BOMBYX_VM_HOST='vmhost'"), "{script}");
    }

    #[test]
    fn builds_a_vagrant_command() {
        let c = vagrant(&cfg(), &["up"], Tty::NoPty);
        let env = vagrant_env();
        assert_eq!(c.program, "ssh");
        assert_eq!(c.args[0], "vmhost");
        assert_eq!(
            remote_script(&c),
            format!("cd ~/'vms/myproject' && {env} vagrant 'up'")
        );
    }

    #[test]
    fn builds_a_vagrant_command_with_several_args() {
        let c = vagrant(
            &cfg(),
            &["snapshot", "restore", "fresh-install"],
            Tty::NoPty,
        );
        let env = vagrant_env();
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && {env} vagrant 'snapshot' \
                 'restore' 'fresh-install'"
            )
        );
    }

    #[test]
    fn builds_a_scratch_command() {
        let cfg = cfg();
        let name = ScratchName::parse("pr-1234").unwrap();
        let c = vagrant_in(
            &cfg,
            &cfg.remote_scratch_dir(&name),
            &["destroy", "-f"],
            Tty::NoPty,
        );
        // The teardown names no provider; the sibling test
        // `the_teardown_verb_names_no_provider` in `plan` holds
        // that rule across the actions.
        let env = vm_env();
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/scratch/myproject/pr-1234' && {env} \
                 vagrant 'destroy' '-f'"
            )
        );
    }

    #[test]
    fn vagrant_runs_in_the_project_dir() {
        // `vagrant up` reads the Vagrantfile from the directory
        // it runs in, so the command has to cd there first.
        let cfg = cfg();
        let quoted = quote_remote_path(&cfg.remote_project_dir());
        assert!(
            remote_script(&vagrant(&cfg, &["up"], Tty::NoPty))
                .starts_with(&format!("cd {quoted} &&"))
        );
    }

    #[test]
    fn ensure_dir_keeps_the_tilde_expandable() {
        let c = ensure_dir(&cfg(), "~/vms/scratch/pr-1");
        assert_eq!(remote_script(&c), "mkdir -p ~/'vms/scratch/pr-1'");
    }

    #[test]
    fn ensure_dir_quotes_an_absolute_dir() {
        let c = ensure_dir(&cfg(), "/srv/vms/p");
        assert_eq!(remote_script(&c), "mkdir -p '/srv/vms/p'");
    }

    #[test]
    fn remove_dir_quotes_the_path_and_keeps_the_tilde() {
        let c = remove_dir(&cfg(), "~/vms/myproject");
        assert_eq!(c.program, "ssh");
        assert_eq!(c.args[0], "vmhost");
        assert_eq!(remote_script(&c), "rm -rf ~/'vms/myproject'");
    }

    #[test]
    fn remove_dir_removes_an_absolute_path() {
        let c = remove_dir(&cfg(), "/srv/vms/myproject");
        assert_eq!(remote_script(&c), "rm -rf '/srv/vms/myproject'");
    }

    #[test]
    fn remove_dir_quotes_injection_in_the_path() {
        // Config rejects these characters, so this is the
        // second line of defence rather than the first.
        let c = remove_dir(&cfg(), "~/vms/a b; rm /");
        assert_eq!(remote_script(&c), "rm -rf ~/'vms/a b; rm /'");
    }

    #[test]
    fn destroy_tolerates_a_directory_with_no_vagrantfile() {
        // An `up` interrupted before the Vagrantfile write
        // leaves the directory made
        // but empty. A bare `vagrant destroy -f` fails there,
        // and would stop the removal that follows.
        let c = destroy_vm_if_present(&cfg(), "~/vms/myproject", Tty::NoPty);
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && if [ -f Vagrantfile ]; then \
                 {} vagrant 'destroy' '-f'; fi",
                vm_env()
            )
        );
    }

    #[test]
    fn saving_the_snapshot_replaces_one_that_is_already_there() {
        // `--force` is what makes the command re-takeable. Without
        // it vagrant refuses a name it already holds, exiting 1
        // with `You must include the --force option to replace an
        // existing snapshot.` -- measured on a libvirt host.
        let c = save_snapshot(&cfg(), "~/vms/myproject", Tty::NoPty);
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && {} vagrant 'snapshot' 'save' \
                 '-f' 'fresh-install'",
                vagrant_env()
            )
        );
    }

    #[test]
    fn restoring_names_the_snapshot_the_saves_write() {
        // The pairing the three builders exist for, pinned where
        // the shell is spelled. `plan` still has its own test
        // that `reset` is handed this builder and not another.
        let c = restore_snapshot(&cfg(), "~/vms/myproject", Tty::NoPty);
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && {} vagrant 'snapshot' \
                 'restore' 'fresh-install'",
                vagrant_env()
            )
        );
    }

    #[test]
    fn the_guarded_save_asks_vagrant_what_it_already_holds() {
        // `up` runs this, and every `up` after the first follows
        // arbitrary use of the machine. Saving only when the name
        // is absent keeps `fresh-install` describing a fresh
        // install.
        //
        // The test is on the listing rather than on vagrant's own
        // refusal because `execute` stops at the first failing
        // step: an unguarded save would make the second `up`
        // report failure.
        let c = save_snapshot_if_absent(&cfg(), "~/vms/myproject", Tty::NoPty);
        let env = vagrant_env();
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && {{ names=$({env} vagrant \
                 'snapshot' 'list') && if ! printf '%s\\n' \"$names\" \
                 | grep -qx 'fresh-install'; then {env} vagrant 'snapshot' \
                 'save' 'fresh-install'; fi || printf 'bombyx: could not \
                 save the fresh-install snapshot for %s; re-run this \
                 command with snapshot in place of up\\n' 'myproject' \
                 >&2; }}"
            )
        );
    }

    #[test]
    fn a_listing_that_fails_stops_the_guarded_save() {
        // A shell pipeline reports only its last command's
        // status. Piping the listing straight into `grep` would
        // make a machine vagrant cannot read look exactly like
        // one holding no snapshots. Capturing it and joining with
        // `&&` is what fails the step instead.
        let script = remote_script(&save_snapshot_if_absent(
            &cfg(),
            "~/vms/p",
            Tty::NoPty,
        ));
        assert!(script.contains("names=$("), "{script}");
        let after_listing = script
            .split_once("'list')")
            .expect("the listing is captured")
            .1;
        assert!(
            after_listing.starts_with(" && "),
            "the listing must gate what follows: {script}"
        );
    }

    #[test]
    fn a_snapshot_that_cannot_be_saved_does_not_fail_up() {
        // `execute` stops at the first failing step and returns
        // its status, and this is the last step of `up`. Without
        // the trailing `||`, a VM that booted and provisioned
        // correctly reports failure because of a snapshot.
        //
        // Two machines reach that on every run, not as an edge
        // case: a provider whose `snapshot list` raises because
        // it has no snapshot support, and one whose listing
        // decorates the name so the guard reads "absent" and the
        // unforced save is then refused.
        let script = remote_script(&save_snapshot_if_absent(
            &cfg(),
            "~/vms/p",
            Tty::NoPty,
        ));
        assert!(script.contains("|| printf 'bombyx: "), "{script}");
        // The braces keep the `cd` out of the advisory. Every
        // other builder here fails its step on a missing
        // directory, and this one must not differ.
        assert!(script.contains("&& { names=$("), "{script}");
        assert!(script.trim_end().ends_with(">&2; }"), "{script}");
    }

    #[test]
    fn the_guarded_save_does_not_force() {
        // The guard and `-f` answer the same question, and only
        // one of them may. A guarded save carrying `-f` would
        // overwrite the snapshot whenever the listing test was
        // wrong about what is there, which is the failure the
        // guard exists to prevent.
        assert!(
            !remote_script(&save_snapshot_if_absent(
                &cfg(),
                "~/vms/p",
                Tty::NoPty
            ))
            .contains("'-f'")
        );
    }

    #[test]
    fn vagrant_in_runs_in_the_given_dir() {
        let c = vagrant_in(&cfg(), "/srv/x", &["halt"], Tty::NoPty);
        let env = vagrant_env();
        assert_eq!(
            remote_script(&c),
            format!("cd '/srv/x' && {env} vagrant 'halt'")
        );
    }
}
