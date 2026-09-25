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

pub use command::{RemoteCommand, Stdin};
pub use quote::{quote_remote_path, shell_quote};
pub use write::{write_file, write_file_of_hidden_size};

use crate::config::{Config, Provider, Transport};

/// Environment variable carrying the VM host's SSH alias into
/// the `vagrant` process on the host.
///
/// The alias as bombyx knows it -- `homelab`, `my-vmhost` -- which
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
/// **Every project vagrant call carries it but one.** All but
/// the teardown name the configured provider. The teardown names
/// the one vagrant recorded the machine under. Its last-resort
/// destroy, for a recorded machine it cannot place, names none;
/// [`destroy_vm_if_present`] holds why.
///
/// Editing `provider` on a project that already has a VM keeps
/// the old one silently -- `provider-change-on-existing-vm` in
/// `docs/todo.md`.
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
/// builds it directly. Two do: `save_snapshot_if_absent` calls
/// this function to put `snapshot list` and `snapshot save`
/// inside one `if`, and `destroy_vm_if_present` calls
/// [`vagrant_command_as`] inside its guards, because it names the
/// recorded provider rather than the configured one. A builder
/// assembling its own string would run `vagrant` with none of
/// the three variables set: `VM_HOST_ENV`, `VM_HOSTNAME_ENV` and
/// `PROVIDER_ENV`.
fn vagrant_command(cfg: &Config, args: &[&str]) -> String {
    vagrant_command_as(cfg, Some(cfg.vm.provider), args)
}

/// [`vagrant_command`] naming `provider` rather than the
/// configured one, or naming none when `provider` is `None`.
///
/// `destroy_vm_if_present` is the only caller that needs this: it
/// names the provider vagrant recorded the machine under, and
/// none for a machine it cannot place.
fn vagrant_command_as(
    cfg: &Config,
    provider: Option<Provider>,
    args: &[&str],
) -> String {
    use std::fmt::Write as _;
    let mut cmd = vm_host_env(cfg);
    if let Some(provider) = provider {
        // `Provider` renders one of two fixed lowercase words,
        // so there is no operator input here for a quote to
        // protect. It is quoted anyway, so the assignment
        // matches every other one in the script.
        let _ =
            write!(cmd, " {PROVIDER_ENV}={}", shell_quote(provider.as_str()));
    }
    cmd.push_str(" vagrant");
    for arg in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(arg));
    }
    cmd
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
/// bombyx writes its own [`PROVIDER_ENV`] back in front of
/// every project vagrant call but one. That assignment comes
/// after this `unset` and wins, so clearing the pair costs those
/// calls nothing. The teardown writes back the provider vagrant
/// recorded. The one call with none written back is the
/// teardown's last-resort destroy, for a recorded machine it
/// cannot place, where leaving the value cleared is the point:
/// vagrant then picks the provider itself.
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
/// **Ends in `; ` rather than `&&`.** `&&` would work, and it
/// ties the whole script to the `unset` succeeding. A separator
/// that can fail is one the script does not need.
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

/// Wraps `script` in the command that runs it on the VM host,
/// with the options an unattended run needs.
///
/// [`transport`] is the wrapper for a command a person is
/// watching. This is the one for a command nobody is watching
/// and whose reply bombyx parses: a `doctor` probe, and a
/// `list` status call. Each option closes a way such a command
/// can be worse than useless:
///
/// - `BatchMode=yes` -- without it, a host that will not accept
///   the key waits for a password, so the command hangs instead
///   of failing.
/// - `ConnectTimeout=10` -- `BatchMode` bounds interaction, not
///   duration. A host that blackholes TCP (a DROP rule, or a
///   dead address behind a live DNS record) would otherwise
///   block for the OS timeout, minutes, with no output at all.
///   It bounds the direct `connect()` and nothing more: it is
///   not inherited by a `ProxyCommand`/`ProxyJump`, and it does
///   not cover the banner exchange or authentication.
/// - `ServerAliveInterval=5` with `ServerAliveCountMax=3` --
///   which *does* bound a session that connects and then stalls,
///   proxied or not. Without it a hung sshd, or a jump host that
///   accepts the TCP connection and then goes quiet, hangs the
///   caller indefinitely.
/// - `LogLevel=ERROR` -- suppresses banners and host-key
///   notices that would otherwise be the text reported as the
///   failure reason.
///
/// Setting them in one place is what makes the guarantee
/// structural rather than something each builder remembers.
/// `bombyx list` contacts the hosts one after another, so an
/// unbounded wait on the first of them is a wait on all of
/// them -- and the documents promise that a machine which does
/// not answer costs the others nothing.
///
/// Every variant named, so a third route is a compile error here
/// rather than one that quietly takes the `ssh` arm.
fn unattended(cfg: &Config, script: &str) -> RemoteCommand {
    match cfg.transport() {
        // Running here, none of the options above has anything
        // to configure: there is no connection to time out, no
        // session to keep alive and no banner to suppress. The
        // script is unchanged, so the shared wrapper builds it
        // -- and it is the wrapper that adds the `unset`.
        Transport::Local => transport(cfg, script, Tty::NoPty),
        // This arm builds its own command, so it adds the
        // `unset` itself. `doctor` reports the environment
        // bombyx's own commands run in, so a probe reading an
        // environment bombyx clears would answer about a state
        // no other command ever sees.
        Transport::Ssh => RemoteCommand::new(
            "ssh",
            &[
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "LogLevel=ERROR",
                "-o",
                "ServerAliveInterval=5",
                "-o",
                "ServerAliveCountMax=3",
                cfg.host.as_str(),
                &format!("{DISARM_VAGRANT_REDIRECTS}{script}"),
            ],
        ),
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

/// Builds the command running `vagrant` in `dir`, then removing
/// `name` from `dir` whether `vagrant` succeeded or not.
///
/// Used for the files the VM host holds only while `vagrant` is
/// uploading them into the guest: the project's secrets, and
/// the git credential built from one variable inside them.
///
/// Every verb that writes the generated files runs this, not
/// only the ones that staged either. See below.
///
/// **The removal is inside this one command rather than a step
/// after it**, and that is the whole reason the function exists.
/// `run::Resolver::execute` stops a plan at the first command
/// that fails, so a removal written as its own step would be
/// skipped exactly when a boot failed -- and a failed boot is
/// the case where the secrets would otherwise sit on a machine
/// other accounts can log in to.
///
/// The shell reads this as `(cd && vagrant); rc=$?; rm; exit`,
/// because `&&` binds tighter than `;`. So `rc` holds the status
/// of the whole `cd`-and-`vagrant` list, and `exit $rc` hands it
/// back: a failed boot is still reported as a failure.
///
/// **A removal that failed is a failure too.** `rm -f` gives up
/// on a file in a directory it cannot write, which a full disk
/// or a changed ownership produces, and bombyx tells the
/// operator the VM host keeps no copy. So the `rm` prints what
/// happened, and it sets `rc` to 1 only when `rc` is still 0 --
/// a boot that already failed keeps its own status, which says
/// more than a 1 does, and the printed line reports the removal
/// either way.
///
/// **Every name is removed whether it was staged or not.** The
/// caller writes the secrets file and the git credential only
/// when the config names them, and a run interrupted after this
/// step began leaves a file the next run's config may no longer
/// mention. So the removals are unconditional and the message
/// says the file *may* hold secrets rather than that it does.
///
/// The names arrive as a slice because two files travel this
/// way. Both are removed even when the boot failed, and each
/// reports its own failure, so one file that cannot be removed
/// does not hide the other.
///
/// The path reaches `printf` as an argument rather than inside
/// the format string. A `%` in there is read as a conversion
/// specifier and eats the argument after it, and a `'` ends the
/// format string early. `require_file` does the same, for the
/// same reason.
///
/// **The message names the VM host on both routes**, and on the
/// local route that host is the machine the operator is sitting
/// at. The path it prints is the VM host's, so naming any other
/// machine would describe a path that does not exist there.
/// [`require_file`] makes the same choice for the opposite
/// reason -- its text arrives in a terminal on the workstation
/// after running on the far side of `ssh`.
///
/// The path is anchored rather than relative to the `cd`. With
/// the default `remote_root` it renders as `~/'vms/…'`, which
/// the remote shell expands against `$HOME`; either way it does
/// not depend on where the shell is standing. A `cd` that failed
/// leaves the shell in the login directory, where a bare
/// `rm -f bombyx.env` would name a different file.
#[must_use]
pub fn vagrant_in_then_remove(
    cfg: &Config,
    dir: &str,
    args: &[&str],
    tty: Tty,
    names: &[&str],
) -> RemoteCommand {
    // Split out so the script below stays readable. The braces
    // are doubled because `format!` reads a single one as the
    // start of a placeholder.
    let removes = names
        .iter()
        .map(|name| {
            let path = quote_remote_path(&format!("{dir}/{name}"));
            format!(
                "rm -f {path} || {{ printf 'bombyx: could not remove %s \
                 from the VM host; it may hold secrets for this \
                 project\\n' {path} >&2; [ \"$rc\" = 0 ] && rc=1; }}"
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let script = format!(
        "{run}; rc=$?; {removes}; exit $rc",
        run = vagrant_script(cfg, dir, args),
    );
    transport(cfg, &script, tty)
}

/// Builds the command running `vagrant` in the project
/// directory on the VM host.
#[must_use]
pub fn vagrant(cfg: &Config, args: &[&str], tty: Tty) -> RemoteCommand {
    vagrant_in(cfg, &cfg.remote_project_dir(), args, tty)
}

/// Introduces one project's block in a listing reply.
///
/// **The trailing space is part of the constant and is doing
/// work.** [`NEVER_BUILT`] below shares the `##bombyx` stem, and
/// the parser opens a block on this exact prefix -- so without
/// the space a `##bombyx-never-built` line would open a block
/// for a project called `-never-built`.
///
/// Crate-private: the exact bytes and the block layout are a
/// wire protocol between this module and `listing`, not
/// something a caller outside the crate should depend on.
///
/// The workstation splits the host's reply on these lines, so
/// the marker has to be something vagrant never prints. Every
/// machine-readable line vagrant writes begins with a timestamp,
/// and no line of it begins with `#`.
///
/// The project name follows the marker. It comes from the
/// operator's own config file, which bombyx trusts the way it
/// trusts a command-line argument, and the parser matches it
/// against the names it asked about rather than believing the
/// reply.
pub(crate) const LISTING_MARKER: &str = "##bombyx ";

/// What a project's block carries when bombyx never built it.
///
/// A *positive* statement, so "never built" is something the
/// host said rather than the absence of anything. An empty block
/// has a second cause that matters more: a `vagrant` the
/// non-interactive shell cannot find writes to stderr and leaves
/// stdout empty, and `docs/vm-host-setup.md` records that
/// `PATH` as this project's recurring VM-host failure. Read as
/// "never built", it would tell the operator bombyx looked at a
/// machine it could not ask.
pub(crate) const NEVER_BUILT: &str = "##bombyx-never-built";

/// Builds the one command that asks a VM host what every
/// project on it is doing.
///
/// `cfg` supplies the route and is the first project reported;
/// `rest` are the others on that same host. One command rather
/// than one per project, because each `ssh` invocation pays for
/// its own connection and a machine usually carries several
/// projects.
///
/// **Every config in `rest` must name `cfg`'s host.** The route
/// is built from `cfg` alone, so an entry belonging elsewhere
/// would be asked about on the wrong machine. What guarantees
/// it is `listing::HostGroup`, whose private fields make holding
/// one the proof; this function is crate-private so that type
/// stays the only way in. A `debug_assert` here would be no
/// guarantee at all, because a release build compiles one out.
///
/// The provider comes from each project's own config rather than
/// from `cfg`, because two projects sharing a machine may name
/// different ones.
///
/// Each fragment reads:
///
/// ```sh
/// printf '##bombyx %s\n' 'web'
/// if [ -f ~/'vms/web/Vagrantfile' ]; then
///   ( cd ~/'vms/web' && ... vagrant 'status' '--machine-readable' )
/// fi
/// ```
///
/// The parentheses keep the `cd` inside the fragment. `cd`
/// changes the shell's own working directory, so without them
/// the next project's `cd` would be relative to this project's
/// directory, and every fragment after the first would ask about
/// a path nobody named.
///
/// The `if` is what keeps one project from answering for the
/// rest. `vagrant status` in a directory holding no Vagrantfile
/// exits non-zero and explains itself on stderr, and a project
/// bombyx has never built is the ordinary case rather than a
/// fault. The `else` writes [`NEVER_BUILT`], so that case is
/// something the host stated; a block with nothing in it means
/// the host answered nothing, which is a different thing and
/// reads as unknown.
///
/// **The script's exit status answers for the last fragment
/// only**, because `;` joins them and a shell reports the last
/// command. So the status cannot say whether any particular
/// project succeeded, and `listing::entries` reads the reply
/// whatever the status is.
///
/// Built by `unattended`, so **no PTY is ever requested** and
/// the connection options are the ones a parsed, unwatched run
/// needs. The PTY half is not the caller's choice: `ssh -t`
/// merges the remote's stderr into stdout, so the `fog` warning
/// `vagrant-libvirt` writes would arrive inside a project's
/// block, and the remote tty translates `\n` to `\r\n`, so a
/// state would be read as `running\r`. [`shell_into_vm`] forces
/// the opposite for the mirror-image reason.
#[must_use]
pub(crate) fn vagrant_status_many(
    cfg: &Config,
    rest: &[&Config],
) -> RemoteCommand {
    let script = std::iter::once(cfg)
        .chain(rest.iter().copied())
        .map(status_fragment)
        .collect::<Vec<_>>()
        .join("; ");
    unattended(cfg, &script)
}

/// The part of a listing script that asks about one project.
///
/// `printf` rather than `echo`, because a project named `-n`
/// would be read as an option by some shells and swallowed
/// instead of printed. The name is an argument to the format
/// string rather than part of it, so a `%` in it cannot be read
/// as a conversion.
fn status_fragment(cfg: &Config) -> String {
    let dir = cfg.remote_project_dir();
    format!(
        "printf '{LISTING_MARKER}%s\\n' {name}; \
         if [ -f {vagrantfile} ]; then ( {run} ); \
         else printf '{NEVER_BUILT}\\n'; fi",
        name = shell_quote(cfg.project.as_str()),
        vagrantfile = quote_remote_path(&format!("{dir}/Vagrantfile")),
        run = vagrant_script(cfg, &dir, &["status", "--machine-readable"]),
    )
}

/// Builds the `status` command for one project, reporting a
/// never-built project cleanly rather than failing on it.
///
/// `vagrant status` needs a Vagrantfile. Run in a directory that
/// holds none -- or that a first `up` never created -- it exits
/// non-zero and prints a raw `cd` failure naming a path the operator
/// never typed. A project bombyx has not built yet is an ordinary
/// state, not a fault: it is what every project is in before the
/// first `up`, and the state an operator most often runs `status` in
/// first. So the guard answers it in bombyx's own words and exits
/// zero, the way `bombyx list` reports the same project as "not
/// created". `status` already exits zero for a built-but-halted VM,
/// so zero here keeps its contract that a reachable machine answers
/// successfully whatever state it is in.
///
/// The Vagrantfile is tested at its full path rather than after a
/// `cd`, because the directory itself may not exist and a `cd` into a
/// missing one is the failure this exists to avoid.
/// `vagrant_status_many` guards each project the same way for the
/// listing; this is the single-project twin, printing a message a
/// person reads rather than the `NEVER_BUILT` marker a parser does.
///
/// The message names the project as a `printf` argument, not inside
/// the format string, so a `%` or `'` in the name cannot be read as
/// a conversion or end the string early -- the reason
/// `status_fragment` does the same.
#[must_use]
pub fn status_or_never_built(cfg: &Config, tty: Tty) -> RemoteCommand {
    let dir = cfg.remote_project_dir();
    let script = format!(
        "if [ -f {vagrantfile} ]; then {run}; \
         else printf 'bombyx: %s has no VM yet; run bombyx up to \
         create it\\n' {name}; fi",
        vagrantfile = quote_remote_path(&format!("{dir}/Vagrantfile")),
        run = vagrant_script(cfg, &dir, &["status"]),
        name = shell_quote(cfg.project.as_str()),
    );
    transport(cfg, &script, tty)
}

/// Builds the command that creates `dir` on the VM host if it
/// does not yet exist.
#[must_use]
pub fn ensure_dir(cfg: &Config, dir: &str) -> RemoteCommand {
    let script = format!("mkdir -p {}", quote_remote_path(dir));
    transport(cfg, &script, Tty::NoPty)
}

/// Builds the command that fails when `path` is not a file on
/// the VM host.
///
/// `field` is the config key the path came from, and it is in
/// the message because the answer to "which line do I edit?" is
/// what the operator needs. A bare `test -f` would exit 1 and
/// say nothing.
///
/// It is `&'static str` so no value read at run time can reach
/// it, and it is passed to `printf` as an argument rather than
/// written into the format string: a `%` in there is read as a
/// conversion specifier and consumes the path argument, and a
/// `'` ends the format string early.
///
/// Unreadable counts as missing. The check runs as the VM
/// host's login user -- the same one that will run `vagrant`
/// there -- so a key that user cannot open is a mistake worth
/// catching here, where the message still names the config
/// key. Left to Vagrant's `file` provisioner it fails a long
/// way from the line at fault.
///
/// The message names the VM host rather than saying "this
/// machine". The `printf` runs on the far side of `ssh` while
/// the text arrives in a terminal on the workstation, so "this
/// machine" would name a machine the reader is not sitting at.
/// The host is an argument too, for the same reason `field` is:
/// `HostName`'s charset happens to exclude `%` and `'`, and a
/// rule kept in another file is the one somebody widens.
///
/// **This is a check on the VM host, run by bombyx, and that is
/// the point.** The generated Vagrantfile could test the file
/// itself and `raise`, which is fewer moving parts and one
/// fewer round trip. It cannot be done there: `vagrant destroy`
/// loads that file too, so the `raise` would leave a directory
/// no bombyx command could tear down --
/// [`destroy_vm_if_present`] holds why. bombyx knows which verb
/// it is running and the Vagrantfile does not.
///
/// The path is assigned to a shell variable first so that a
/// leading `~` is expanded once, and the message then quotes
/// the directory the far side really looked in rather than the
/// `~` the operator wrote. POSIX expands a tilde at the start
/// of an assignment's value, which `sh` and `dash` were both
/// checked for.
#[must_use]
pub fn require_file(
    cfg: &Config,
    path: &str,
    field: &'static str,
) -> RemoteCommand {
    let script = format!(
        "p={quoted}; if [ ! -f \"$p\" ] || [ ! -r \"$p\" ]; then \
         printf 'bombyx: %s names %s, which %s does not have or \
         cannot read\\n' {field} \"$p\" {host} >&2; exit 1; fi",
        quoted = quote_remote_path(path),
        field = shell_quote(field),
        host = shell_quote(cfg.host.as_str()),
    );
    transport(cfg, &script, Tty::NoPty)
}

/// Builds the command that destroys the VM defined in `dir`,
/// doing nothing when there is no Vagrantfile there or no
/// machine recorded.
///
/// The Vagrantfile guard makes teardown idempotent. A bare
/// `vagrant destroy -f` exits non-zero in a directory with no
/// Vagrantfile, and an `up` interrupted between the `mkdir` and
/// the Vagrantfile write leaves exactly that behind. The failure
/// would stop the removal step that follows.
///
/// **The script names the provider vagrant recorded the machine
/// under.** Vagrant writes a machine's id to a file named for
/// the provider, which `recorded_machine_id` spells out. The
/// script tests that file for each of [`Provider::ALL`] and runs
/// the destroy under the first one it finds.
///
/// The destroy needs a provider named. With none, vagrant picks a
/// default while it loads, by asking each provider whether it is
/// usable. On a WSL2 host VirtualBox answers with a refusal, and
/// the refusal comes before vagrant reads the machine's record,
/// so the destroy fails with a machine present (`vm-host-wsl2.md`
/// under "Vagrant treats WSL as Windows").
///
/// The recorded provider rather than the configured one, because
/// the two differ after an operator edits `provider` on a project
/// that already has a VM (`provider-change-on-existing-vm` in
/// `docs/todo.md`), and cleaning up that state is what the
/// teardown is for. The recorded provider built the machine on
/// this host, so it is the one sure to be usable here; whether
/// naming the configured one would be refused in that state was
/// not tried *(unverified)*.
///
/// **Any other recorded machine gets a last-resort destroy naming
/// no provider.** Its branch fires for any id the
/// provider-named branches did not match. For a `default` machine
/// under a provider bombyx does not support, vagrant picks that
/// provider from the record, where every provider's usability
/// probe answers -- on a libvirt host, say. On a WSL2 host the
/// probe refuses first, as above, and the refusal below keeps
/// the directory. For a machine under another name, the destroy
/// targets `default` alone and removes nothing, which the
/// refusal below then catches.
///
/// **The script refuses when a machine is still recorded after
/// the destroy.** Vagrant deletes a machine's id file when it
/// destroys the machine, or finds it not created. A leftover id
/// therefore means a machine vagrant did not remove -- one under
/// a machine name other than `default`, which vagrant never
/// targets because the generated Vagrantfile does not define it,
/// or one whose destroy vagrant refused. The removal behind the
/// teardown would delete the Vagrantfile while that machine
/// runs, so the script exits non-zero and `execute` stops before
/// it.
///
/// **With no machine recorded at all, the script runs no
/// `vagrant`.** There is nothing to destroy, and a vagrant that
/// cannot use the provider named would refuse and stop the
/// directory removal behind it -- so a misconfigured project
/// stays removable.
///
/// The script tests each path by name rather than with a glob. A
/// `zsh` login shell aborts a command whose glob matches nothing,
/// and over `ssh` the VM host's login shell runs the script.
///
/// Each destroy comes from `vagrant_command_as`, the private
/// helper behind every other builder's command, so it carries the
/// same identity prefix as every other invocation. It matters
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
    let destroy = ["destroy", "-f"];
    let mut branches: Vec<String> = Provider::ALL
        .into_iter()
        .map(|p| {
            format!(
                "[ -f {id} ]; then {cmd}; ",
                id = shell_quote(&recorded_machine_id(p)),
                cmd = vagrant_command_as(cfg, Some(p), &destroy),
            )
        })
        .collect();
    branches.push(format!(
        "{ANY_RECORDED_MACHINE}; then {cmd}; ",
        cmd = vagrant_command_as(cfg, None, &destroy),
    ));
    let script = format!(
        "cd {dir} && if [ -f Vagrantfile ]; then if {chain}fi; \
         if {ANY_RECORDED_MACHINE}; then printf 'bombyx: %s still \
         records a machine under %s/.vagrant/machines, so the \
         directory stays; destroy that machine by hand\\n' {host} \
         \"$PWD\" >&2; exit 1; fi; fi",
        dir = quote_remote_path(dir),
        chain = branches.join("elif "),
        host = shell_quote(cfg.host.as_str()),
    );
    transport(cfg, &script, tty)
}

/// The shell test for any machine id vagrant recorded in the
/// project, whatever its machine name or provider.
///
/// `find` rather than a glob, because a `zsh` login shell aborts
/// a command whose glob matches nothing. `2>/dev/null` covers a
/// project with no `.vagrant/machines` at all, where `find`
/// complains and prints nothing, so the test is false.
pub(crate) const ANY_RECORDED_MACHINE: &str =
    "find .vagrant/machines -name id -type f 2>/dev/null | grep -q .";

/// The file vagrant writes when it creates a machine under
/// `provider`, relative to the project directory.
///
/// The machine is `default` because the generated Vagrantfile
/// defines no machine name, and the directory is `.vagrant`
/// because `DISARM_VAGRANT_REDIRECTS` clears
/// `VAGRANT_DOTFILE_PATH`. [`destroy_vm_if_present`] tests for
/// it to learn which provider built the machine.
pub(crate) fn recorded_machine_id(provider: Provider) -> String {
    format!(".vagrant/machines/default/{provider}/id")
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
/// is what keeps the write path (`mkdir`, then the file writes)
/// and this removal path agreeing about which roots are usable.
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
/// On the guest side, `vagrant ssh -c` asks for a TTY by default.
///
/// **The shell opens as the agent's account, in its clone.**
/// `vagrant ssh` logs in as the box's own account, usually
/// `vagrant`, and the clone belongs to the account `guest_user`
/// names. The login account has passwordless `sudo` on a Vagrant
/// box, which is what lets it run `sudo -u <guest_user>`; the
/// grant `account.sh` writes is the agent's own. The shell that
/// `sudo` starts `cd`s into `$HOME/<project>` -- where
/// `bootstrap.sh` clones -- then `exec`s a login shell:
///
/// - `$HOME` and `$SHELL` are the agent's: `-H` sets `HOME` from
///   its passwd entry whatever the box's sudoers policy keeps, and
///   `sudo` sets `SHELL` the same way. Both sit inside single
///   quotes all the way down, so neither the VM host nor the
///   login shell expands them first.
/// - The project name travels as `$1`, a separate argument, so
///   the script is the same text for every project.
/// - `exec` replaces each shell on the way, so one `exit` leaves
///   the VM.
/// - `-l` makes the shell read the login profile, as the shell a
///   bare `vagrant ssh` starts does.
///
/// `|| cd` falls back to the account's home when the clone is
/// missing, so the operator still gets a shell, after `cd` prints
/// its error, to look into why. Without it the shell would open
/// in whatever directory `sudo` inherited. A project whose
/// `[env]` table sets `HOME` lands there too: its clone follows
/// that value, while this `$HOME` is the account's passwd home.
///
/// **A missing account gets a shell too.** The account is missing
/// when `account.sh` refused before creating it -- a box without
/// `useradd`, a renamed `guest_user` -- or when bombyx 0.7.0 or
/// earlier built the VM. `sudo -u` would then fail and `exec`
/// would end the session, exactly when the operator needs to look
/// around. So `id -u` checks first, and without the account the
/// guest says so and opens a login shell as the account Vagrant
/// logged in with.
///
/// **The clone path is spelled in the guest, not in Rust.** `$HOME`
/// here is the agent's, set by `sudo -H` after the switch, so
/// nothing on this side can name the directory ahead of time.
/// `the_shell_opens_where_the_bootstrap_script_clones` holds this
/// spelling and `bootstrap.sh`'s together.
///
/// **Three layers of single quotes nest here.** The script and
/// its arguments are quoted for the login shell, the whole guest
/// command is quoted once more for the VM host, and vagrant wraps
/// it in its own `bash -l -c '...'`, rewriting each inner `'`
/// first -- vagrant 2.4.9's `ssh_run.rb` shows it.
#[must_use]
pub fn shell_into_vm(cfg: &Config) -> RemoteCommand {
    let user = shell_quote(cfg.vm.guest_user.as_str());
    let guest = format!(
        "if id -u {user} >/dev/null 2>&1; \
         then exec sudo -u {user} -H -- sh -c {script} sh {project}; \
         else echo \"bombyx: this guest has no account {user}, so it \
         was never provisioned for it; run bombyx provision, or \
         bombyx destroy then bombyx up if provisioning refuses. \
         Opening a shell as $(id -un) instead.\" >&2; \
         exec \"$SHELL\" -l; fi",
        script = shell_quote(r#"cd "$HOME/$1" || cd; exec "$SHELL" -l"#),
        project = shell_quote(cfg.project.as_str()),
    );
    vagrant_in(
        cfg,
        &cfg.remote_project_dir(),
        &["ssh", "-c", &guest],
        Tty::Allocate,
    )
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
        // the redirection and the `$(hostname -s)` the far
        // side must evaluate. `sh -c` is the same POSIX shell
        // `ssh` starts on the host, so every builder keeps one
        // script. The only difference on this route is the two
        // words in front of it, so the scripts must be equal
        // character for character.
        /// One builder, named for the error message.
        type Builder = (&'static str, fn(&Config) -> RemoteCommand);

        let builders: [Builder; 10] = [
            ("vagrant", |c| vagrant(c, &["status"], Tty::NoPty)),
            ("status_or_never_built", |c| {
                status_or_never_built(c, Tty::NoPty)
            }),
            // A row because this builder does not go through
            // `transport`: `unattended` matches on the route
            // itself, so a script made conditional there is
            // exactly what this test exists to catch.
            ("listing", |c| vagrant_status_many(c, &[])),
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
            ("write", |c| write_file(c, "~/vms", "Vagrantfile", b"x\n")),
            // A row because this builder reads `cfg.host`
            // outside `vagrant_command`, so a script made
            // conditional on the route here would go unnoticed.
            // Note what it cannot catch: both fixtures carry
            // the same `host`, so a host that *varied* by route
            // would still compare equal.
            ("require_file", |c| {
                require_file(c, "~/.secrets/k", "deploy_key")
            }),
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
                write_file(&route, "~/vms", "Vagrantfile", b"x\n"),
                vagrant_status_many(&route, &[]),
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
    }

    #[test]
    fn an_interactive_shell_starts_in_the_project_clone() {
        // `vagrant ssh` logs in as the box's own account, and the
        // clone belongs to the agent's, so the guest switches
        // with `sudo -u` first. The clone is `$HOME/<project>` of
        // that account; `|| cd` falls back to its home when the
        // clone is missing, so the operator still gets a shell to
        // look into why. The project travels as an argument
        // rather than inside the script, so the script is the
        // same text for every project. The whole guest command is
        // one argument, quoted once more for the VM host.
        let guest = "if id -u 'agent' >/dev/null 2>&1; \
                     then exec sudo -u 'agent' -H -- sh -c \
                     'cd \"$HOME/$1\" || cd; exec \"$SHELL\" -l' \
                     sh 'myproject'; \
                     else echo \"bombyx: this guest has no account \
                     'agent', so it was never provisioned for it; \
                     run bombyx provision, or bombyx destroy then \
                     bombyx up if provisioning refuses. Opening a \
                     shell as $(id -un) instead.\" >&2; \
                     exec \"$SHELL\" -l; fi";
        let c = shell_into_vm(&cfg());
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && {} vagrant 'ssh' '-c' {}",
                vagrant_env(),
                shell_quote(guest)
            )
        );
    }

    #[test]
    fn the_shell_opens_where_the_bootstrap_script_clones() {
        // Two spellings of one path, in two files that cannot see
        // each other: `bootstrap.sh` clones into
        // `$HOME/$BOMBYX_PROJECT`, and the shell `cd`s into
        // `$HOME/$1` with the project as `$1`. A change to either
        // would open the shell outside the clone and fail nothing
        // else.
        assert!(
            crate::vagrantfile::BOOTSTRAP.contains(
                "readonly CLONE_DIR=\"$HOME/${BOMBYX_PROJECT:-project}\""
            ),
            "bootstrap.sh no longer clones into $HOME/<project>"
        );
        let script = remote_script(&shell_into_vm(&cfg()));
        assert!(script.contains(r#"cd "$HOME/$1""#), "{script}");
        assert!(script.contains("sh '\\''myproject'\\''"), "{script}");
    }

    #[test]
    fn an_interactive_shell_opens_as_the_configured_account() {
        let mut cfg = cfg();
        cfg.vm.guest_user =
            crate::config::GuestUser::parse("dev").expect("a plain name");
        let script = remote_script(&shell_into_vm(&cfg));
        assert!(script.contains("sudo -u '\\''dev'\\''"), "{script}");
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
    /// `every_other_project_vagrant_call_names_the_configured_provider`
    /// and `every_teardown_destroys_under_the_provider_it_finds_recorded`,
    /// both in `plan`, hold the provider half across the actions:
    /// the configured provider everywhere but the teardown, and
    /// the recorded one there, then one unnamed fallback.
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
        // The generated Vagrantfile *configures* a provider, and
        // configuring one does nothing unless vagrant independently
        // picks it -- so bombyx names it. Without that, a hyperv
        // project on a libvirt-only host boots a libvirt machine at
        // vagrant's defaults, the `:hyperv` settings block never
        // applying and nothing reporting the substitution.
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
        // Pins the teardown builder, which builds its command
        // with `vagrant_command_as` rather than `vagrant_script`;
        // `vagrant_command` says why.
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
            &["halt"],
            Tty::NoPty,
        );
        // `halt` rather than `destroy`: a teardown goes through
        // `destroy_vm_if_present`, never through `vagrant_in`.
        let env = vagrant_env();
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/scratch/myproject/pr-1234' && {env} \
                 vagrant 'halt'"
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
    fn require_file_names_the_path_in_its_own_message() {
        // The whole script, because the message is the point:
        // an operator who sees only "exit status 1" has to go
        // and read the generated Vagrantfile to find out which
        // file was missing.
        let c = require_file(&cfg(), "~/.secrets/k", "deploy_key");
        assert_eq!(
            remote_script(&c),
            "p=~/'.secrets/k'; if [ ! -f \"$p\" ] || \
             [ ! -r \"$p\" ]; then printf 'bombyx: %s names %s, \
             which %s does not have or cannot read\\n' \
             'deploy_key' \"$p\" 'vmhost' >&2; exit 1; fi"
        );
    }

    #[test]
    fn require_file_quotes_an_absolute_path() {
        let c = require_file(&cfg(), "/etc/keys/k", "deploy_key");
        assert!(
            remote_script(&c).starts_with("p='/etc/keys/k';"),
            "{}",
            remote_script(&c)
        );
    }

    #[test]
    fn require_file_names_the_vm_host_rather_than_this_machine() {
        // The `printf` runs on the far side of `ssh` and the
        // text arrives in a terminal on the workstation, so
        // "this machine" would name a machine the reader is not
        // sitting at. On the local route the two coincide and
        // the same wording still reads correctly.
        for cfg in [cfg(), local_cfg()] {
            let c = require_file(&cfg, "~/.secrets/k", "deploy_key");
            assert!(
                remote_script(&c).contains("'vmhost' >&2"),
                "{}",
                remote_script(&c)
            );
        }
    }

    #[test]
    fn require_file_refuses_a_file_it_cannot_read() {
        // The check runs as the user `vagrant` will run as, so
        // an unreadable key is a mistake this step can catch at
        // the one point where the message names the config key.
        // Left to Vagrant's `file` provisioner it fails a long
        // way from the config line at fault.
        let script =
            remote_script(&require_file(&cfg(), "~/.secrets/k", "deploy_key"));
        assert!(script.contains("! -f"), "{script}");
        assert!(script.contains("! -r"), "{script}");
    }

    #[test]
    fn require_file_passes_the_field_name_as_an_argument() {
        // Not interpolated into the `printf` format string. A
        // field containing `%` would otherwise be read as a
        // conversion specifier and eat the path argument, and
        // one containing `'` would end the format string early.
        let script = remote_script(&require_file(&cfg(), "/k", "de%s'ploy"));
        assert!(script.contains(r"'de%s'\''ploy'"), "{script}");
        assert!(
            script.contains("'bombyx: %s names %s,"),
            "the field name must not be in the format string: {script}"
        );
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
    fn the_teardown_script_is_spelled_exactly() {
        // Pins the whole teardown script: the Vagrantfile guard,
        // the provider-named branches, the fallback and the
        // refusal. The guard is there because an `up`
        // interrupted before the Vagrantfile write leaves the
        // directory made but empty, a bare `vagrant destroy -f`
        // fails there, and the failure would stop the removal
        // that follows. The `run_teardown` tests exercise what
        // each part does.
        let c = destroy_vm_if_present(&cfg(), "~/vms/myproject", Tty::NoPty);
        let env = vm_env();
        let id = |p| shell_quote(&recorded_machine_id(p));
        assert_eq!(
            remote_script(&c),
            format!(
                "cd ~/'vms/myproject' && if [ -f Vagrantfile ]; then \
                 if [ -f {} ]; then {env} {PROVIDER_ENV}='libvirt' \
                 vagrant 'destroy' '-f'; \
                 elif [ -f {} ]; then {env} {PROVIDER_ENV}='hyperv' \
                 vagrant 'destroy' '-f'; \
                 elif {ANY_RECORDED_MACHINE}; then {env} \
                 vagrant 'destroy' '-f'; fi; \
                 if {ANY_RECORDED_MACHINE}; then printf 'bombyx: %s \
                 still records a machine under %s/.vagrant/machines, \
                 so the directory stays; destroy that machine by \
                 hand\\n' 'vmhost' \"$PWD\" >&2; exit 1; fi; fi",
                id(Provider::Libvirt),
                id(Provider::Hyperv),
            )
        );
    }

    /// Runs the teardown script under `sh` in a fresh project
    /// directory and returns what a fake `vagrant` was called
    /// with, one line per call: the provider it saw, then its
    /// arguments. `Ok` when the script exited 0, `Err` when it
    /// refused.
    ///
    /// The fake does to the record what vagrant does: it deletes
    /// the id of the `default` machine under the provider named,
    /// or under any provider when none is named. It never touches
    /// another machine name, because vagrant targets only the
    /// machines the Vagrantfile defines, and bombyx's defines
    /// `default` alone.
    ///
    /// `vagrantfile` decides whether the directory holds a
    /// `Vagrantfile`, and `recorded` lists where under
    /// `.vagrant/machines` vagrant wrote a machine id, each as
    /// `<machine>/<provider>`. The
    /// operator's own `VAGRANT_DEFAULT_PROVIDER` is set to a
    /// third provider, so a call that saw it proves the `unset`
    /// was skipped.
    #[cfg(unix)]
    fn run_teardown(
        vagrantfile: bool,
        recorded: &[&str],
    ) -> Result<String, String> {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().expect("a temp dir");
        let bin = tmp.path().join("bin");
        let project = tmp.path().join("project");
        let log = tmp.path().join("calls.log");
        std::fs::create_dir_all(&bin).expect("mkdir bin");
        std::fs::create_dir_all(&project).expect("mkdir project");
        let fake = bin.join("vagrant");
        std::fs::write(
            &fake,
            r#"#!/bin/sh
printf '%s %s\n' "${VAGRANT_DEFAULT_PROVIDER-none}" "$*" >> "$LOG"
m=.vagrant/machines/default
if [ -n "${VAGRANT_DEFAULT_PROVIDER-}" ]; then
  rm -f "$m/$VAGRANT_DEFAULT_PROVIDER/id"
else
  rm -f "$m"/*/id
fi
"#,
        )
        .expect("write the fake vagrant");
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
            .expect("chmod the fake vagrant");
        if vagrantfile {
            std::fs::write(project.join("Vagrantfile"), "")
                .expect("write the Vagrantfile");
        }
        for m in recorded {
            // Built by hand rather than with `recorded_machine_id`,
            // because `recorded` can name a machine or a provider
            // bombyx does not use, which no `Provider` spells.
            let id = project.join(format!(".vagrant/machines/{m}/id"));
            std::fs::create_dir_all(id.parent().expect("a parent"))
                .expect("mkdir machine");
            std::fs::write(&id, "some-uuid").expect("write id");
        }

        let dir = project.display().to_string();
        let c = destroy_vm_if_present(&cfg(), &dir, Tty::NoPty);
        let path = std::env::var("PATH").unwrap_or_default();
        let status = std::process::Command::new("sh")
            .args(["-c", &raw_script(&c)])
            .env("PATH", format!("{}:{path}", bin.display()))
            .env("LOG", &log)
            .env(PROVIDER_ENV, "virtualbox")
            .status()
            .expect("sh runs");
        // No log means the fake was never called. Any other read
        // error has to fail, or it would pass the tests that
        // expect no call.
        let calls = match std::fs::read_to_string(&log) {
            Ok(calls) => calls,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => panic!("read the call log: {e}"),
        };
        if status.success() {
            Ok(calls)
        } else {
            Err(calls)
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_teardown_names_the_provider_vagrant_recorded() {
        // On a WSL2 host a destroy with no provider named has
        // vagrant probe VirtualBox, which refuses before vagrant
        // reads the machine's record (issue #111). So the
        // teardown names a provider, and names the recorded one
        // rather than the configured one: the test config says
        // libvirt, and a machine recorded under hyperv is still
        // destroyed as hyperv.
        let ok = |calls: &str| Ok(calls.to_owned());
        assert_eq!(
            run_teardown(true, &["default/libvirt"]),
            ok("libvirt destroy -f\n")
        );
        assert_eq!(
            run_teardown(true, &["default/hyperv"]),
            ok("hyperv destroy -f\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_default_machine_under_another_provider_gets_the_unnamed_destroy() {
        // A `default` machine vagrant recorded under a provider
        // bombyx does not support is still a machine. The
        // teardown falls back to a destroy naming no provider,
        // and vagrant reads the record.
        //
        // "none" is what the fake prints for an unset variable,
        // so it also proves the operator's exported value was
        // cleared.
        assert_eq!(
            run_teardown(true, &["default/virtualbox"]),
            Ok("none destroy -f\n".to_owned())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_machine_left_recorded_refuses_the_removal() {
        // Vagrant destroys only the machines the Vagrantfile
        // defines, which is `default` alone. A machine recorded
        // under another name survives every destroy, and the
        // removal behind the teardown would then delete its
        // Vagrantfile while it runs. So the script refuses when
        // an id is still recorded afterwards, and `execute` stops
        // before the removal.
        assert_eq!(
            run_teardown(true, &["web/libvirt"]),
            Err("none destroy -f\n".to_owned())
        );
        // The same when `default` goes but a second machine
        // stays.
        assert_eq!(
            run_teardown(true, &["default/libvirt", "web/libvirt"]),
            Err("libvirt destroy -f\n".to_owned())
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_teardown_skips_vagrant_when_no_machine_is_recorded() {
        // No machine means nothing for vagrant to destroy, and
        // a vagrant that cannot pick a usable provider would
        // refuse and leave the directory removal behind it
        // unrun.
        assert_eq!(run_teardown(true, &[]), Ok(String::new()));
        // An id with no Vagrantfile beside it is left alone
        // too, because vagrant fails in a directory with no
        // Vagrantfile.
        assert_eq!(
            run_teardown(false, &["default/libvirt"]),
            Ok(String::new())
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

    #[test]
    fn status_guards_a_never_built_project_and_says_run_up() {
        // The Vagrantfile is tested at its full path, before any
        // `cd`, so a directory that does not exist yet is answered
        // rather than `cd`-ed into; the else branch is a plain
        // message and a zero exit, not vagrant's raw failure.
        let c = status_or_never_built(&cfg(), Tty::NoPty);
        let env = vagrant_env();
        assert_eq!(
            remote_script(&c),
            format!(
                "if [ -f ~/'vms/myproject/Vagrantfile' ]; then \
                 cd ~/'vms/myproject' && {env} vagrant 'status'; \
                 else printf 'bombyx: %s has no VM yet; run bombyx up \
                 to create it\\n' 'myproject'; fi"
            )
        );
    }

    #[test]
    fn the_removal_runs_whether_vagrant_worked_or_not() {
        // A `&&` here would skip the removal on a failed boot,
        // which is the case where the secrets would otherwise be
        // left on a machine other accounts can log in to.
        let c = vagrant_in_then_remove(
            &cfg(),
            "/srv/x",
            &["up"],
            Tty::NoPty,
            &["bombyx.env"],
        );
        let env = vagrant_env();
        let file = "'/srv/x/bombyx.env'";
        assert_eq!(
            remote_script(&c),
            format!(
                "cd '/srv/x' && {env} vagrant 'up'; rc=$?; \
                 rm -f {file} || {{ printf 'bombyx: could not remove \
                 %s from the VM host; it may hold secrets for this \
                 project\\n' {file} >&2; \
                 [ \"$rc\" = 0 ] && rc=1; }}; exit $rc"
            )
        );
    }

    #[test]
    fn a_removal_that_failed_is_reported_and_fails_the_run() {
        // bombyx tells the operator the VM host keeps no copy.
        // An `rm` that quietly gave up -- a full disk, a
        // directory whose ownership changed -- would leave that
        // claim false with nothing said. The guest half of this
        // design tests every `rm` it runs; so does this half.
        let c = vagrant_in_then_remove(
            &cfg(),
            "/srv/x",
            &["up"],
            Tty::NoPty,
            &["bombyx.env"],
        );
        let s = remote_script(&c);
        assert!(
            s.contains("bombyx: could not remove"),
            "a failed removal must say so: {s}"
        );
        // And a boot that worked must stop reporting success.
        assert!(
            s.contains("rc=1"),
            "a failed removal must fail the run: {s}"
        );
        // And only when the boot itself did not already fail:
        // vagrant's own status says more than a bare 1.
        assert!(
            s.contains("[ \"$rc\" = 0 ] && rc=1"),
            "a failed boot must keep its own status: {s}"
        );
        // The newline reaches `printf` as the two characters it
        // converts, not as a real one. A raw newline inside the
        // command breaks a printed plan across lines.
        assert!(!s.contains('\n'), "the command must be one line: {s:?}");
    }

    #[test]
    fn the_removal_names_the_file_absolutely() {
        // The `cd` can fail -- a directory removed between the
        // `mkdir` and this step -- and the shell is then in the
        // login directory. A bare `rm -f bombyx.env` would name
        // a file there instead.
        let c = vagrant_in_then_remove(
            &cfg(),
            "/srv/x",
            &["up"],
            Tty::NoPty,
            &["bombyx.env"],
        );
        let s = remote_script(&c);
        assert!(
            s.contains("rm -f '/srv/x/bombyx.env'"),
            "the removal must carry the whole path: {s}"
        );
    }

    #[test]
    fn one_status_call_carries_every_project_on_the_host() {
        // The whole point of the builder: several projects share
        // a machine, and asking each of them separately would
        // pay for an ssh handshake per project.
        let web = cfg();
        let mut api = cfg();
        api.project = crate::name::ProjectName::parse("api").unwrap();
        let cmd = vagrant_status_many(&web, &[&api]);
        let script = remote_script(&cmd);

        for name in ["myproject", "api"] {
            assert!(
                script.contains(&format!("{LISTING_MARKER}%s\\n' '{name}'")),
                "{name} must be announced: {script}"
            );
        }
        assert_eq!(
            script
                .matches("vagrant 'status' '--machine-readable'")
                .count(),
            2,
            "one status call per project: {script}"
        );
    }

    #[test]
    fn a_listing_command_carries_the_unattended_connection_options() {
        // `list` contacts the hosts one after another, so an
        // unbounded wait on one is a wait on all of them -- and
        // the documents promise a machine that does not answer
        // costs the others nothing. Without `BatchMode` an `ssh`
        // wanting a password waits for input nobody is there to
        // give.
        let cmd = vagrant_status_many(&cfg(), &[]);
        let argv = cmd.args.join(" ");
        for opt in [
            "BatchMode=yes",
            "ConnectTimeout=10",
            "LogLevel=ERROR",
            "ServerAliveInterval=5",
            "ServerAliveCountMax=3",
        ] {
            assert!(argv.contains(opt), "{opt} missing from {argv}");
        }
    }

    #[test]
    fn a_listing_command_never_asks_for_a_remote_terminal() {
        // The reply is parsed. `ssh -t` merges the remote's
        // stderr into stdout, so the fog warning would land
        // inside a project's block, and the remote tty turns
        // every `\n` into `\r\n`, so a state would be read as
        // `running\r`. Neither is the caller's decision to get
        // wrong, so the builder does not take one.
        let cmd = vagrant_status_many(&cfg(), &[]);
        assert!(
            !cmd.args.iter().any(|a| a == "-t"),
            "no PTY may be requested: {:?}",
            cmd.args
        );
    }

    #[test]
    fn a_project_directory_is_entered_in_a_subshell() {
        // `cd` inside one project's fragment must not decide
        // where the next project's fragment runs. The
        // parentheses are what keep it local; without them the
        // second `cd` is relative to the first project's
        // directory and the guard above it has already answered
        // for the wrong path.
        let cmd = vagrant_status_many(&cfg(), &[]);
        let script = remote_script(&cmd);
        assert!(
            script.contains("then ( cd "),
            "the cd must run in a subshell: {script}"
        );
    }

    #[test]
    fn a_project_with_no_vagrantfile_is_never_asked() {
        // vagrant fails outright in a directory holding no
        // Vagrantfile, and its failure would be the whole host's
        // reply. The guard is what keeps one untouched project
        // from hiding the states of the others.
        let cmd = vagrant_status_many(&cfg(), &[]);
        let script = remote_script(&cmd);
        assert!(
            script.contains("if [ -f ~/'vms/myproject/Vagrantfile' ]"),
            "the guard must name the project's Vagrantfile: {script}"
        );
    }

    #[test]
    fn each_project_is_asked_with_its_own_provider() {
        // Two projects on one machine may name different
        // providers, and the reply for each has to come from the
        // one its own table names.
        let web = cfg();
        let mut api = cfg();
        api.project = crate::name::ProjectName::parse("api").unwrap();
        api.vm.provider = crate::config::Provider::Hyperv;
        let script = remote_script(&vagrant_status_many(&web, &[&api]));
        assert!(script.contains("=\'libvirt\'"), "{script}");
        assert!(script.contains("=\'hyperv\'"), "{script}");
    }
}
