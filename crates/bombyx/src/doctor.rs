//! Checking bombyx's preconditions before they cost anything.
//!
//! `bombyx up` changes state before it runs `vagrant`: it
//! creates a directory on the host and ships a tarball there.
//! So a host that is missing something reports it half-way
//! through. This module models the up-front check instead.
//!
//! Nothing here runs a process. It owns the probe list, the
//! rules for reading a probe's result, the skip cascade and the
//! report -- everything that decides anything. The binary
//! supplies process spawning and nothing else, which matters
//! because `src/bin/` is outside the coverage gate.
//!
//! One rule shaped every probe: **a probe must carry a verdict,
//! not a value.** A check that merely prints something and
//! passes on a zero exit reports the state of the host it was
//! pointed at, not the state it exists to catch --
//! `remote::probe::posix_shell` records the case that taught
//! this. Where a probe cannot decide in the shell, the verdict
//! is applied here.

use std::fmt::Write as _;
use std::path::Path;

use crate::config::Config;
use crate::remote::{self, RemoteCommand};

/// Where a probe runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// On this workstation.
    Local,
    /// On the VM host, over SSH.
    Host,
}

/// What a probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The precondition holds, with a short supporting detail.
    Pass(String),
    /// The precondition does not hold, with the reason.
    Fail(String),
    /// Not run, because a gating probe already failed.
    Skip(String),
}

/// What running one probe produced.
///
/// A struct rather than three positional arguments: `stdout` and
/// `stderr` are both strings and are read differently -- a
/// verdict is applied to stdout, and a failure reason prefers
/// stderr -- so transposing them at a call site would compile
/// and silently invert the diagnosis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// Whether the command exited zero.
    pub success: bool,
    /// What it wrote to stdout.
    pub stdout: String,
    /// What it wrote to stderr.
    pub stderr: String,
}

impl ProbeResult {
    /// Builds a result from a finished child process.
    ///
    /// Constructing it here rather than in the binary is what
    /// keeps the field order from being a caller's problem.
    #[must_use]
    pub fn from_output(output: &std::process::Output) -> Self {
        Self {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

/// What asking a local tool for its version produced.
///
/// Named states rather than a nested `Option<Result<_, _>>`.
/// That shape had no name for any of its cases, so the caller's
/// `None` arm covered two unrelated situations -- "this tool has
/// no version flag worth asking" and "it was never resolved" --
/// and only a second argument told them apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionAnswer {
    /// There is no version flag worth asking for.
    ///
    /// `scp` is the case: it answers no version flag, and asking
    /// prints a usage message that would land in the report as
    /// noise.
    NotAsked,
    /// It ran. Whatever it printed is here, exit status included.
    Answered(ProbeResult),
    /// It was found on the `PATH` but would not start.
    WouldNotStart(String),
}

/// One precondition and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Where the probe ran.
    pub scope: Scope,
    /// Short label, e.g. `vagrant`.
    pub name: String,
    /// What the probe found.
    pub outcome: Outcome,
}

impl Finding {
    /// Builds a finding.
    #[must_use]
    pub fn new(scope: Scope, name: &str, outcome: Outcome) -> Self {
        Self {
            scope,
            name: name.to_owned(),
            outcome,
        }
    }
}

/// The reason reported when a program is not on the `PATH`.
///
/// One phrasing, used by every caller. Two hand-written variants
/// of the same sentence drift, and a report whose wording
/// depends on which code path produced it is harder to grep and
/// harder to trust.
#[must_use]
pub fn not_on_path(name: &str) -> String {
    format!("{name} not found on PATH")
}

/// The reason reported when a program was found but would not
/// start.
///
/// Present-but-unusable is a different fault from absent, and
/// naming it as such is the difference between "install this"
/// and "your install is broken".
#[must_use]
pub fn cannot_run(what: &str, err: &str) -> String {
    format!("cannot run {what}: {err}")
}

/// A check applied to a probe's stdout when a zero exit is not
/// the whole answer.
///
/// `Ok` means the precondition holds; `Err` carries the reason
/// it does not.
pub type Verdict = fn(&str) -> Result<(), String>;

/// A probe to run on the VM host.
#[derive(Debug, Clone)]
pub struct HostProbe {
    /// Label it reports under.
    pub name: &'static str,
    /// The command to run.
    pub command: RemoteCommand,
    /// When this probe fails, the rest are skipped.
    ///
    /// A flag rather than matching on `name`, so renaming a
    /// report column cannot silently disable the cascade.
    pub gates_the_rest: bool,
    /// Applies a verdict the shell could not express.
    ///
    /// `None` means a zero exit is the whole verdict.
    pub verdict: Option<Verdict>,
}

impl HostProbe {
    /// A probe whose exit status is the whole verdict.
    ///
    /// The plain shape is the default so the two probes that are
    /// *not* plain have to say so at their definition, where a
    /// reader looking at the list can see it.
    #[must_use]
    pub fn plain(name: &'static str, command: RemoteCommand) -> Self {
        Self {
            name,
            command,
            gates_the_rest: false,
            verdict: None,
        }
    }

    /// Marks this probe as the one whose failure skips the rest.
    #[must_use]
    pub fn gating(mut self) -> Self {
        self.gates_the_rest = true;
        self
    }

    /// Attaches a check the shell could not express.
    #[must_use]
    pub fn with_verdict(mut self, verdict: Verdict) -> Self {
        self.verdict = Some(verdict);
        self
    }
}

/// The host probes, in order.
///
/// Reachability is first and gates the rest: five more waits on
/// a dead host teach nothing the first failure did not.
#[must_use]
pub fn host_probes(cfg: &Config) -> Vec<HostProbe> {
    vec![
        HostProbe::plain("ssh", remote::probe::reachable(cfg)).gating(),
        HostProbe::plain("login shell", remote::probe::posix_shell(cfg))
            .with_verdict(posix_shell_verdict),
        HostProbe::plain("tar", remote::probe::command(cfg, "tar")),
        HostProbe::plain("scp", remote::probe::command(cfg, "scp")),
        HostProbe::plain("vagrant", remote::probe::command(cfg, "vagrant")),
        HostProbe::plain(
            "project dir",
            remote::probe::dir_writable(cfg, &cfg.remote_project_dir()),
        ),
        HostProbe::plain("libvirt provider", remote::probe::provider(cfg)),
    ]
}

/// The commands `probes` would run, in order.
///
/// The single renderer behind `--dry-run`, used by both `plan`
/// and the binary, so no path can advertise a probe list the
/// live run would not use.
#[must_use]
pub fn probe_commands(probes: &[HostProbe]) -> Vec<RemoteCommand> {
    probes.iter().map(|p| p.command.clone()).collect()
}

/// Commands whose purpose is to write, matched on **word
/// boundaries**.
///
/// One list, shared by the unit test over the probe builders and
/// the CLI-level test over the rendered dry run. Two separately
/// maintained lists were the original problem: they disagreed
/// about what read-only meant, so each test proved something
/// slightly different and weaker.
///
/// Matching is by word, not by substring. An earlier version
/// listed `"rm "` and `"> "` with trailing spaces -- the ordinary
/// spelling, and not the only one. `>file`, `1>file`, `>|file`
/// and a tab-separated `rm` all slipped past while the tests read
/// as though the whole family were covered.
///
/// # What this does not claim
///
/// It inspects the text of **bombyx's own script** and nothing
/// else. It says nothing about what an invoked tool does once it
/// runs; see `remote::probe::provider`, where
/// `vagrant plugin list` initialises `~/.vagrant.d` on a host
/// where vagrant has never run. A passing
/// `no_probe_changes_the_host` therefore means "no probe reaches
/// for a tool whose purpose is to write", not "doctor leaves the
/// host byte-identical".
///
/// A blocklist, knowingly. A real allowlist needs a shell parser
/// to find the command in every segment of a script, and a parser
/// that is subtly wrong inspires more confidence than this list
/// while catching less.
const MUTATING_COMMANDS: &[&str] = &[
    "mkdir",
    "rmdir",
    "rm",
    "touch",
    "unzip",
    "scp",
    "cp",
    "mv",
    "dd",
    "ln",
    "chmod",
    "chown",
    "truncate",
    "tee",
    "install",
    "mkfifo",
    "mknod",
    "sed",
    "git",
    "apt",
    "apt-get",
    "systemctl",
    "tar",
];

/// `vagrant` subcommands that change something.
///
/// Enumerated rather than allow-listing the read-only ones,
/// because `vagrant` grows subcommands and a new one is likelier
/// to write than not. `plugin` is split further below, since
/// `plugin list` is the one bombyx actually needs.
const MUTATING_VAGRANT: &[&str] = &[
    "up",
    "destroy",
    "halt",
    "reload",
    "provision",
    "snapshot",
    "init",
    "box",
    "suspend",
    "resume",
    "package",
    "upload",
    "push",
];

/// `vagrant plugin` subcommands that change something.
const MUTATING_VAGRANT_PLUGIN: &[&str] = &[
    "install",
    "uninstall",
    "update",
    "repair",
    "expunge",
    "license",
];

/// Words that precede a command rather than being one.
///
/// A segment can open with a keyword (`then rm -rf x`) or with
/// environment assignments (`LC_ALL=C rm -rf x`), so the command
/// is not always the first word.
const NOT_A_COMMAND: &[&str] = &[
    "if", "then", "else", "elif", "fi", "while", "until", "do", "done", "for",
    "case", "esac", "in", "!", "time", "exec", "eval",
];

/// Commands that run *another* command, so the interesting word
/// is further along the segment.
///
/// Without this the guard stops at the wrapper and never looks
/// past it, and `sudo mkdir -p "$d"` reads as read-only. That is
/// worse than a gap: `sudo` in front of `systemctl`, `apt` or
/// `mkdir` is exactly what a probe author reaches for, so the
/// blind spot sat precisely where the command list was aimed. The
/// substring version this replaced did catch these.
const TRANSPARENT_PREFIX: &[&str] = &[
    "sudo", "doas", "env", "command", "nohup", "nice", "ionice", "setsid",
    "stdbuf", "xargs", "timeout",
];

/// Shells, which hide whatever `-c` hands them.
///
/// A probe running `sh -c '<anything>'` cannot be judged by
/// reading the outer script, so the wrapper itself is treated as
/// the objection rather than pretended to be read-only.
const SHELL_COMMANDS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "ash"];

/// Splits a script into segments, each a list of words.
///
/// Every character that can end one command and begin another is
/// a separator, `(` included -- that is what makes the `vagrant`
/// inside `out=$(vagrant plugin list)` the start of its own
/// segment rather than an argument of the assignment.
fn command_segments(script: &str) -> Vec<Vec<&str>> {
    script
        .split(|c: char| {
            matches!(c, ';' | '|' | '&' | '(' | ')' | '`' | '{' | '}' | '\n')
        })
        .map(|seg| seg.split_whitespace().collect::<Vec<&str>>())
        .filter(|words| !words.is_empty())
        .collect()
}

/// The command a segment runs, and its arguments.
///
/// Leading keywords and `VAR=value` assignments are skipped. A
/// segment that is nothing but assignments (`p=$d`) runs no
/// command and yields `None`.
///
/// A wrapper from [`TRANSPARENT_PREFIX`] is stepped past, along
/// with its own flags, so the command it runs is the one judged.
///
/// # What this does not see
///
/// It is not a shell parser. A command assembled by expansion, or
/// reached through `find . -delete`, is invisible to it. `xargs`
/// and the shells are handled -- the first as a transparent
/// prefix, the second by objecting to the wrapper -- but the
/// general case is not solvable here. The guard covers a probe
/// author writing a mutating command, which is the realistic
/// mistake; it is not a sandbox.
fn command_of<'a>(words: &'a [&'a str]) -> Option<(&'a str, &'a [&'a str])> {
    let mut i = 0;
    while i < words.len() {
        let word = base_name(words[i]);
        if NOT_A_COMMAND.contains(&word) || is_assignment(word) {
            i += 1;
            continue;
        }
        if TRANSPARENT_PREFIX.contains(&word) {
            i += 1;
            // The wrapper's own flags, then -- for `timeout` --
            // its duration, which is a bare word and would
            // otherwise be read as the command.
            let mut is_lookup = false;
            while i < words.len() && words[i].starts_with('-') {
                // `command -v tar` asks *where* `tar` is and runs
                // nothing, so the name after it is not a command
                // being invoked. Every `command -v` probe bombyx
                // has would otherwise be reported as running the
                // tool it is only looking for.
                is_lookup |=
                    word == "command" && matches!(words[i], "-v" | "-V");
                i += 1;
            }
            if is_lookup {
                return None;
            }
            if word == "timeout" && i < words.len() {
                i += 1;
            }
            continue;
        }
        return Some((words[i], &words[i + 1..]));
    }
    None
}

/// A word reduced to the name it invokes.
///
/// Strips a leading path so `/bin/rm` is judged as `rm`, and
/// surrounding quotes so `'rm'` is too -- quoting the command word
/// is the cheapest way to slip a name past a comparison.
fn base_name(word: &str) -> &str {
    let bare = word.trim_matches(|c| matches!(c, '\'' | '"'));
    bare.rsplit(['/', '\\']).next().unwrap_or(bare)
}

/// Whether `word` is a `VAR=value` assignment rather than a
/// command.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Whether the text after a `>&` names a file descriptor rather
/// than a file.
///
/// This distinction is the whole correctness of the redirection
/// check, and getting it wrong is a two-character bypass.
/// `>&2` and `>&-` duplicate a descriptor and create nothing;
/// bare `>&word` is a documented bash synonym for `&>word` and
/// **truncates the file** `word`. Treating every `>&` as a
/// duplication let `vagrant plugin list >&out.txt` through.
fn is_descriptor(after_amp: &str) -> bool {
    let token: String = after_amp
        .chars()
        .take_while(|c| {
            !c.is_whitespace() && !matches!(c, ';' | '|' | '&' | ')' | '}')
        })
        .collect();
    token == "-"
        || (!token.is_empty() && token.chars().all(|c| c.is_ascii_digit()))
}

/// Up to eight characters of `text`, for a failure message.
///
/// Characters, not bytes. Slicing at `i + 8` bytes panics when a
/// multi-byte character straddles the boundary, and this runs over
/// script text that can carry a non-ASCII path.
fn excerpt(text: &str) -> String {
    text.chars()
        .take(8)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// The write-capable redirection in `script`, if any.
///
/// Structural rather than a list of spellings: any `>` opens a
/// file for writing, with three exceptions that are not
/// redirections at all --- a descriptor duplication (`2>&1`,
/// `>&2`), the comparison `>=`, and the arrow `->`.
///
/// Quoted runs are skipped. Without that, a probe printing
/// `'expected >= 2 vCPUs'` is reported as writing a file, and a
/// guard that misfires on ordinary prose is a guard the next
/// author relaxes rather than obeys.
fn redirection_that_writes(script: &str) -> Option<String> {
    let mut quote: Option<char> = None;
    for (i, c) in script.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if c != '>' {
            continue;
        }
        // `->`: the previous character makes it an arrow.
        if script[..i].ends_with('-') {
            continue;
        }
        let rest = &script[i + 1..];
        // `>>file` writes just as `>file` does; step past the
        // second `>` to read what follows either form.
        let after = rest.strip_prefix('>').unwrap_or(rest);
        // `>=`: a comparison.
        if after.starts_with('=') {
            continue;
        }
        if let Some(dup) = after.strip_prefix('&')
            && is_descriptor(dup)
        {
            continue;
        }
        return Some(excerpt(&script[i..]));
    }
    None
}

/// The mutating `vagrant` use in `args`, if any.
///
/// The subcommand is the first non-flag word. `ssh` is judged on
/// its flags rather than its name: `vagrant ssh` alone is
/// interactive and harmless here, while `vagrant ssh -c '<cmd>'`
/// runs an arbitrary command inside the guest, and what that
/// command is cannot be read from the outer script.
fn mutating_vagrant_use(args: &[&str]) -> Option<String> {
    let mut words = args.iter().filter(|w| !w.starts_with('-'));
    let sub = words.next()?;
    if *sub == "plugin" {
        let action = words.next()?;
        return MUTATING_VAGRANT_PLUGIN
            .contains(action)
            .then(|| format!("plugin {action}"));
    }
    if *sub == "ssh" && args.contains(&"-c") {
        return Some("ssh -c".to_owned());
    }
    MUTATING_VAGRANT.contains(sub).then(|| (*sub).to_owned())
}

/// The first sign in `script` that it would change the host.
///
/// Returns the offending word so a failure names what it objected
/// to, not merely that it objected.
#[must_use]
pub fn mutating_token(script: &str) -> Option<String> {
    if let Some(redirect) = redirection_that_writes(script) {
        return Some(format!("redirection {redirect}"));
    }
    for words in command_segments(script) {
        let Some((command, args)) = command_of(&words) else {
            continue;
        };
        let bare = base_name(command);
        if MUTATING_COMMANDS.contains(&bare) {
            return Some(command.to_owned());
        }
        if SHELL_COMMANDS.contains(&bare) && args.contains(&"-c") {
            return Some(format!("{bare} -c"));
        }
        if bare == "vagrant"
            && let Some(found) = mutating_vagrant_use(args)
        {
            return Some(format!("vagrant {found}"));
        }
    }
    None
}

/// Confirms the host shell ran a POSIX construct correctly.
///
/// # Errors
///
/// Returns the reason when the expected token is absent.
fn posix_shell_verdict(stdout: &str) -> Result<(), String> {
    if stdout.lines().any(|l| l.trim() == "posix") {
        return Ok(());
    }
    Err("shell did not run a POSIX construct; bombyx sends \
         POSIX sh scripts"
        .to_owned())
}

/// Reads a probe's result into an outcome.
///
/// `verdict` applies a check the shell could not express, and
/// runs only once the command itself succeeded.
#[must_use]
pub fn classify(result: &ProbeResult, verdict: Option<Verdict>) -> Outcome {
    if !result.success {
        return Outcome::Fail(fail_reason(&result.stdout, &result.stderr));
    }
    if let Some(check) = verdict
        && let Err(reason) = check(&result.stdout)
    {
        return Outcome::Fail(sanitize(&reason));
    }
    Outcome::Pass(first_line(&result.stdout))
}

/// The most useful line explaining a failure.
///
/// Prefers the **last** non-blank stderr line. OpenSSH writes
/// the server's `Banner`, host-key notices and other chatter
/// before the real error, so taking the first line reports a
/// legal notice instead of `Permission denied (publickey)`.
/// Falls back to stdout, then to a fixed string, because a
/// failing `command -v` prints nothing at all.
fn fail_reason(stdout: &str, stderr: &str) -> String {
    for text in [stderr, stdout] {
        if let Some(line) = text.lines().map(str::trim).rfind(|l| !l.is_empty())
        {
            return sanitize(line);
        }
    }
    "not found".to_owned()
}

/// The first non-blank line of `text`, sanitized.
fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(sanitize)
        .unwrap_or_default()
}

/// Whether `c` is safe to print in the report verbatim.
///
/// An **allowlist**, deliberately: printable ASCII and the space,
/// nothing else. The blocklist version was tried first and could
/// not be completed. It covered control characters and an
/// enumerated slice of the bidirectional and formatting
/// characters, and still missed `U+2028`/`U+2029` (line and
/// paragraph separators, which are not `is_control`), the
/// variation selectors, and the whole tag block
/// `U+E0000`-`U+E007F` -- which renders as nothing at all in
/// every terminal and is the standard way to hide text inside
/// text. Enumerating what to reject means tracking Unicode; the
/// report is ASCII everywhere else by design, so enumerating what
/// to keep is both shorter and finishable.
///
/// The cost is that a genuinely non-ASCII path on the VM host
/// renders with `?` in place of each such character. That is the
/// right trade for a report the operator reads to decide whether
/// to push: an unreadable character is obvious, and a character
/// that alters how the rest of the line appears is not.
fn is_safe_to_print(c: char) -> bool {
    c.is_ascii_graphic() || c == ' '
}

/// Replaces anything that could misrepresent the report with
/// `?`.
///
/// Probe details are text from the VM host, printed straight to
/// the operator's terminal. Without this, a host can emit
/// cursor-movement escapes and repaint the report -- turning a
/// `FAIL` line into `ok` on the screen while the exit code says
/// otherwise. The report is the artifact the operator trusts to
/// decide whether to push, so the host must not be able to write
/// it.
///
/// [`Report::render`] is the enforcement point, not this
/// function's callers. Details reach a `Finding` from several
/// places -- including the binary, which builds them from
/// spawn errors and tool banners -- and requiring each one to
/// remember the call is how one of them eventually does not.
/// The earlier calls stay because an `Outcome` is also read
/// programmatically.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if is_safe_to_print(c) { c } else { '?' })
        .collect()
}

/// Shortens `detail` to at most `budget` characters.
///
/// ASCII `...`, not an ellipsis character: every other byte
/// bombyx prints is ASCII, and a legacy Windows console code
/// page renders U+2026 as mojibake.
///
/// A budget too small for the marker degrades to as much of the
/// marker as fits. Returning the untruncated detail would be
/// worse than useless -- the caller asked for a width because it
/// is building an aligned line, and one over-long detail there
/// pushes every column out of place.
fn clip(detail: &str, budget: usize) -> String {
    if detail.chars().count() <= budget {
        return detail.to_owned();
    }
    let marker = "...";
    if budget < marker.len() + 1 {
        return marker.chars().take(budget).collect();
    }
    let kept: String = detail.chars().take(budget - marker.len()).collect();
    format!("{kept}{marker}")
}

/// Width the rendered report aims to fit inside.
const LINE_WIDTH: usize = 80;

/// Indent every report line carries.
const INDENT: usize = 2;

/// Gap between report columns.
const GAP: usize = 2;

/// Width of the `ok`/`FAIL`/`skip` column.
const TAG_WIDTH: usize = 4;

/// Least detail a line will show, whatever the column widths.
///
/// The budget is `LINE_WIDTH` minus the prefix, and the prefix
/// grows with the host name, which `Config` does not bound. A
/// 49-character host left three characters of detail and a
/// 52-character one left none, so `FAIL` printed with no reason
/// at all -- a report that still looked complete and aligned
/// while having discarded the only actionable content in it.
/// Past this floor the line is allowed to run over 80 columns
/// instead: a wrapped reason can be read, a deleted one cannot.
const MIN_DETAIL: usize = 24;

/// Scope label for a check that runs on this workstation.
///
/// A constant because it is both printed and measured, and the
/// two have to be the same string: written twice, renaming it to
/// anything longer would silently misalign every line while the
/// column width still described the old label.
const LOCAL_LABEL: &str = "local";

/// Runs `probes` in order, skipping the rest once a gating
/// probe fails.
///
/// `run` performs one probe. Keeping it a parameter is what
/// makes the cascade testable without a host: the tests pass a
/// closure returning canned outcomes.
///
/// A skipped probe names the gate that stopped it, read from the
/// gate itself. Hardcoding the reason meant renaming the gating
/// probe left the report explaining the skip in terms of a
/// column that no longer existed.
pub fn run_probes<F>(probes: &[HostProbe], mut run: F) -> Vec<Finding>
where
    F: FnMut(&HostProbe) -> Outcome,
{
    let mut blocked_by: Option<&str> = None;
    let mut findings = Vec::with_capacity(probes.len());
    for probe in probes {
        if let Some(gate) = blocked_by {
            findings.push(Finding::new(
                Scope::Host,
                probe.name,
                Outcome::Skip(format!("no {gate}")),
            ));
            continue;
        }
        let outcome = run(probe);
        if probe.gates_the_rest && matches!(outcome, Outcome::Fail(_)) {
            blocked_by = Some(probe.name);
        }
        findings.push(Finding::new(Scope::Host, probe.name, outcome));
    }
    findings
}

/// The detail for a local tool, from whatever it printed.
///
/// Keeps the name and version, which for `tar` matters:
/// `bsdtar` and GNU `tar` differ on `--exclude` pattern
/// matching and the push depends on it. Some builds print the
/// banner on stderr -- `ssh -V` always does -- so both streams
/// are considered rather than reporting a pass with nothing in
/// it.
fn tool_banner(result: &ProbeResult) -> String {
    let text = if result.stdout.trim().is_empty() {
        &result.stderr
    } else {
        &result.stdout
    };
    let mut words = text.split_whitespace();
    let Some(name) = words.next() else {
        return "version unknown".to_owned();
    };
    // The first version-looking token, not simply the second
    // word: GNU tar announces itself as "tar (GNU tar) 1.35",
    // where the second word is "(GNU".
    let version =
        words.find(|w| w.chars().any(char::is_numeric) && w.contains('.'));
    // `ssh -V` prints "OpenSSH_for_Windows_9.5p1, LibreSSL
    // 3.8.2", so the comma can land on either token.
    let name = name.trim_end_matches(',');
    let short = match version {
        Some(v) => format!("{name} {}", v.trim_end_matches(',')),
        None => name.to_owned(),
    };
    sanitize(&short)
}

/// Turns a local tool lookup into a finding.
///
/// The binary resolves the program and, when there is a version
/// flag worth asking for, runs it -- and does nothing else. Every
/// decision about what those results *mean* is here, because
/// `src/bin/` is excluded from the coverage gate and a diagnostic
/// whose own reasoning is untested is not much of a diagnostic.
///
/// The four cases it distinguishes:
///
/// - Not on the `PATH` -- a failure, and the whole diagnosis.
/// - Present, with no version to ask for. The directory alone is
///   the useful fact: it is what makes a hijacked binary
///   visible, since `tool::resolve` never searches the working
///   directory but the operator still wants to see where it did
///   look.
/// - Present but the version call would not start. Still a
///   failure, and a different one from absent.
/// - Present, started, and answered unhelpfully -- a *pass*.
///   Telling the operator to install something they already have
///   would be worse than a missing version string.
#[must_use]
pub fn local_tool_finding(
    name: &str,
    resolved: Option<&Path>,
    version: &VersionAnswer,
) -> Finding {
    let Some(path) = resolved else {
        return Finding::new(
            Scope::Local,
            name,
            Outcome::Fail(not_on_path(name)),
        );
    };
    let where_from = path
        .parent()
        .map_or_else(String::new, |p| p.display().to_string());
    let outcome = match version {
        VersionAnswer::NotAsked => Outcome::Pass(where_from),
        VersionAnswer::Answered(result) => {
            Outcome::Pass(format!("{} in {where_from}", tool_banner(result)))
        }
        VersionAnswer::WouldNotStart(err) => {
            Outcome::Fail(cannot_run(&path.display().to_string(), err))
        }
    };
    Finding::new(Scope::Local, name, outcome)
}

/// Checks the project has a Vagrantfile to push.
///
/// The cheapest way to catch a typo in `vagrant_dir`, which
/// otherwise surfaces as a `tar` failure after bombyx has
/// already created the remote directory.
#[must_use]
pub fn vagrantfile_finding(local_dir: &Path) -> Finding {
    let outcome = if local_dir.join("Vagrantfile").is_file() {
        // The path says nothing "ok" does not; on failure it is
        // the whole diagnosis.
        Outcome::Pass(String::new())
    } else {
        Outcome::Fail(format!("not in {}", local_dir.display()))
    };
    Finding::new(Scope::Local, "Vagrantfile", outcome)
}

/// The collected findings of a doctor run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    findings: Vec<Finding>,
}

impl Report {
    /// Appends a finding, preserving probe order.
    ///
    /// Named `add` rather than `push`/`extend`: those are the
    /// `Vec` and `Extend` spellings, and borrowing them for a
    /// type that is not a collection invites a reader to assume
    /// the rest of that interface exists.
    pub fn add(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    /// Appends several findings.
    pub fn add_all(&mut self, findings: impl IntoIterator<Item = Finding>) {
        self.findings.extend(findings);
    }

    /// The findings, in probe order.
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Whether every probe that ran found its precondition met.
    ///
    /// A skip does not count against the report: it is not
    /// evidence of a problem, and the failure that caused it is
    /// already on its own line.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.failures() == 0
    }

    /// How many probes failed.
    #[must_use]
    pub fn failures(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| matches!(f.outcome, Outcome::Fail(_)))
            .count()
    }

    /// Renders the report, aligned, with a closing summary.
    ///
    /// This is where host-supplied text is made safe to print;
    /// see [`sanitize`].
    #[must_use]
    pub fn render(&self, host: &str) -> String {
        let name_width = self
            .findings
            .iter()
            .map(|f| f.name.chars().count())
            .max()
            .unwrap_or(0);
        // Measured in characters, like the host name beside it:
        // a byte count would misalign a non-ASCII label.
        let scope_width = host.chars().count().max(LOCAL_LABEL.chars().count());
        let prefix =
            INDENT + scope_width + GAP + name_width + GAP + TAG_WIDTH + GAP;
        let budget = LINE_WIDTH.saturating_sub(prefix).max(MIN_DETAIL);
        let mut out = String::new();
        for f in &self.findings {
            let scope = match f.scope {
                Scope::Local => LOCAL_LABEL,
                Scope::Host => host,
            };
            let (tag, detail) = match &f.outcome {
                Outcome::Pass(d) => ("ok", d.as_str()),
                Outcome::Fail(d) => ("FAIL", d.as_str()),
                Outcome::Skip(d) => ("skip", d.as_str()),
            };
            // One format, then trim: a line ending in
            // whitespace is noise in a diff and in a paste.
            let line = format!(
                "{blank:INDENT$}{scope:<scope_width$}{blank:GAP$}\
                 {name:<name_width$}{blank:GAP$}{tag:<TAG_WIDTH$}\
                 {blank:GAP$}{detail}",
                blank = "",
                name = f.name,
                detail = sanitize(&clip(detail, budget)),
            );
            let _ = writeln!(out, "{}", line.trim_end());
        }
        match self.failures() {
            0 => out.push_str("all checks passed\n"),
            1 => out.push_str("1 check failed\n"),
            n => {
                let _ = writeln!(out, "{n} checks failed");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::for_tests()
    }

    fn host_finding(name: &str, outcome: Outcome) -> Finding {
        Finding::new(Scope::Host, name, outcome)
    }

    fn ran(success: bool, stdout: &str, stderr: &str) -> ProbeResult {
        ProbeResult {
            success,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn classify_reads_a_zero_exit_as_a_pass_with_detail() {
        assert_eq!(
            classify(&ran(true, "/usr/bin/vagrant\n", ""), None),
            Outcome::Pass("/usr/bin/vagrant".to_owned())
        );
    }

    #[test]
    fn classify_passes_with_no_detail_when_output_is_empty() {
        assert_eq!(
            classify(&ran(true, "  \n\n", ""), None),
            Outcome::Pass(String::new())
        );
    }

    #[test]
    fn classify_prefers_the_last_stderr_line() {
        // OpenSSH prints the server Banner and host-key notices
        // before the real error, so the first line is the least
        // useful one.
        let stderr = "*** AUTHORIZED USE ONLY ***\n\
                      Warning: Permanently added 'h'\n\
                      Permission denied (publickey).\n";
        assert_eq!(
            classify(&ran(false, "", stderr), None),
            Outcome::Fail("Permission denied (publickey).".to_owned())
        );
    }

    #[test]
    fn classify_falls_back_when_a_failure_is_silent() {
        assert_eq!(
            classify(&ran(false, "", ""), None),
            Outcome::Fail("not found".to_owned())
        );
    }

    #[test]
    fn classify_applies_a_verdict_a_zero_exit_cannot() {
        // The whole point: `echo` succeeds whatever it prints,
        // so the exit code alone would pass a fish shell.
        let out = classify(
            &ran(true, "/usr/bin/fish\n", ""),
            Some(posix_shell_verdict),
        );
        assert!(matches!(out, Outcome::Fail(_)), "{out:?}");
        assert_eq!(
            classify(&ran(true, "posix\n", ""), Some(posix_shell_verdict)),
            Outcome::Pass("posix".to_owned())
        );
    }

    #[test]
    fn posix_shell_verdict_rejects_silence() {
        // An unset variable or a shell that printed nothing must
        // not pass.
        assert!(posix_shell_verdict("").is_err());
        assert!(posix_shell_verdict("\n\n").is_err());
        assert!(posix_shell_verdict("posix").is_ok());
    }

    #[test]
    fn details_cannot_repaint_the_report() {
        // A host emitting cursor escapes could otherwise
        // overwrite a FAIL line with `ok`.
        let evil = "\x1b[2A\x1b[Kspoofed";
        let Outcome::Pass(detail) = classify(&ran(true, evil, ""), None) else {
            panic!("expected a pass");
        };
        assert_eq!(detail, "?[2A?[Kspoofed");
        let mut r = Report::default();
        r.add(host_finding("x", Outcome::Fail(evil.to_owned())));
        assert!(!r.render("h").contains('\x1b'));
    }

    #[test]
    fn render_is_the_enforcement_point_for_untrusted_detail() {
        // A `Finding` can be built anywhere, including in the
        // binary, so the guarantee cannot depend on each
        // producer remembering to sanitize.
        let mut r = Report::default();
        r.add(Finding::new(
            Scope::Local,
            "tar",
            // A right-to-left override reverses the run after
            // it, so the tag can be made to read backwards.
            Outcome::Pass("bsdtar\u{202e}3.8.4\u{200b}".to_owned()),
        ));
        let out = r.render("h");
        assert!(out.contains("bsdtar?3.8.4?"), "{out:?}");
        assert!(!out.contains('\u{202e}'), "{out:?}");
    }

    #[test]
    fn clip_never_exceeds_the_budget_it_was_given() {
        assert_eq!(clip("abcdefgh", 5), "ab...");
        assert_eq!(clip("abc", 5), "abc");
        // A budget with no room for the marker must still shrink
        // the detail: the caller is building an aligned line, and
        // returning the full text would push every column out.
        for budget in 0..4 {
            let out = clip("abcdefgh", budget);
            assert_eq!(out.chars().count(), budget, "{budget}: {out:?}");
        }
    }

    #[test]
    fn a_skip_is_not_a_failure_but_does_not_inflate_the_count() {
        let mut r = Report::default();
        r.add(host_finding("ssh", Outcome::Fail("down".into())));
        r.add(host_finding("tar", Outcome::Skip("no ssh".into())));
        assert!(!r.ok());
        assert_eq!(r.failures(), 1);

        let mut only_skips = Report::default();
        only_skips.add(host_finding("tar", Outcome::Skip("no ssh".into())));
        assert!(only_skips.ok(), "skips alone must not fail the report");
    }

    #[test]
    fn run_probes_skips_the_rest_once_the_gate_fails() {
        let probes = host_probes(&cfg());
        let findings = run_probes(&probes, |p| {
            if p.gates_the_rest {
                Outcome::Fail("unreachable".to_owned())
            } else {
                panic!("must not run {} after the gate failed", p.name);
            }
        });
        assert_eq!(findings.len(), probes.len());
        assert!(matches!(findings[0].outcome, Outcome::Fail(_)));
        assert!(
            findings[1..]
                .iter()
                .all(|f| matches!(f.outcome, Outcome::Skip(_))),
            "{findings:?}"
        );
    }

    #[test]
    fn a_skip_names_the_gate_that_caused_it() {
        // Read from the gating probe, so renaming a report
        // column cannot leave the report explaining the skip in
        // terms of a column that no longer exists.
        let probes = vec![
            HostProbe::plain("uplink", remote::probe::reachable(&cfg()))
                .gating(),
            HostProbe::plain("tar", remote::probe::command(&cfg(), "tar")),
        ];
        let findings =
            run_probes(&probes, |_| Outcome::Fail("unreachable".to_owned()));
        assert_eq!(findings[1].outcome, Outcome::Skip("no uplink".to_owned()));
    }

    #[test]
    fn run_probes_runs_everything_when_the_gate_passes() {
        let probes = host_probes(&cfg());
        let mut ran = 0;
        let findings = run_probes(&probes, |_| {
            ran += 1;
            Outcome::Pass(String::new())
        });
        assert_eq!(ran, probes.len());
        assert!(
            findings
                .iter()
                .all(|f| matches!(f.outcome, Outcome::Pass(_)))
        );
    }

    #[test]
    fn exactly_one_probe_gates_the_rest_and_it_is_first() {
        let probes = host_probes(&cfg());
        assert!(probes[0].gates_the_rest, "reachability must gate");
        assert_eq!(probes[0].name, "ssh");
        assert_eq!(
            probes.iter().filter(|p| p.gates_the_rest).count(),
            1,
            "a second gate would skip probes that could still run"
        );
    }

    #[test]
    fn probe_labels_are_unique_and_non_empty() {
        // Labels key the report columns.
        let probes = host_probes(&cfg());
        let mut names: Vec<&str> = probes.iter().map(|p| p.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        assert!(names.iter().all(|n| !n.is_empty()));
    }

    #[test]
    fn no_probe_changes_the_host() {
        for p in host_probes(&cfg()) {
            let script = p.command.args.last().unwrap();
            assert_eq!(mutating_token(script), None, "{script:?}");
        }
    }

    #[test]
    fn mutating_token_catches_the_whole_family_not_one_spelling() {
        // The guard is only worth having if it fires, and the
        // earlier substring version fired on exactly the
        // spellings it happened to list. Each line below is a
        // form that slipped past it.
        for (script, want) in [
            // Trailing-space matching missed every one of these.
            ("printf x >out", "redirection >out"),
            ("printf x 1>out", "redirection >out"),
            ("printf x >|out", "redirection >|out"),
            ("printf x >>out", "redirection >>out"),
            ("printf x >\tout", "redirection >\tout"),
            // A path-qualified command, and a tab separator.
            ("/bin/rm -rf x", "/bin/rm"),
            ("rm\t-rf x", "rm"),
            // Reached through a keyword or an assignment rather
            // than at the start of the segment.
            ("if true; then rm -rf x; fi", "rm"),
            ("LC_ALL=C rm -rf x", "rm"),
            ("out=$(mkdir -p y)", "mkdir"),
            // vagrant subcommands beyond the four once listed.
            ("vagrant init", "vagrant init"),
            ("vagrant box add x", "vagrant box"),
            ("vagrant plugin uninstall x", "vagrant plugin uninstall"),
            ("vagrant upload f g", "vagrant upload"),
            // Not the subcommand's name but its flags: `-c` runs
            // an arbitrary command inside the guest.
            ("vagrant ssh -c 'rm -rf /vagrant'", "vagrant ssh -c"),
            ("cd x && mkdir -p y", "mkdir"),
            // A wrapper that runs another command. Stopping at
            // the wrapper made `sudo mkdir` read as read-only --
            // and `sudo` in front of `mkdir`, `apt` or `systemctl`
            // is exactly what a probe author reaches for.
            ("sudo mkdir -p \"$d\"", "mkdir"),
            ("sudo systemctl restart libvirtd", "systemctl"),
            ("env mkdir -p y", "mkdir"),
            ("command rm -f f", "rm"),
            ("nohup tar cf a.tar .", "tar"),
            ("timeout 5 rm -rf x", "rm"),
            ("xargs rm", "rm"),
            // Quoting the command word is the cheapest bypass.
            ("'rm' -rf x", "'rm'"),
            ("\"mkdir\" -p y", "\"mkdir\""),
            // A shell hides its payload, so the wrapper itself is
            // the objection.
            ("sh -c 'rm -rf /'", "sh -c"),
            // `>&word` is a bash synonym for `&>word`: it
            // truncates the file. Only a descriptor is harmless.
            ("vagrant plugin list >&out.txt", "redirection >&out.tx"),
            ("printf x >>&out", "redirection >>&out"),
        ] {
            assert_eq!(
                mutating_token(script).as_deref(),
                Some(want),
                "{script:?}"
            );
        }
    }

    #[test]
    fn mutating_token_leaves_a_read_only_script_alone() {
        // The other half: a guard that flags everything is as
        // useless as one that flags nothing. `tar` and `scp` are
        // mutating commands appearing here as *arguments*, which
        // is why the check reads the command word of each segment
        // rather than every word.
        for script in [
            "command -v 'tar'",
            "command -v 'scp'",
            "true",
            "x=1; if [ \"$x\" = 1 ]; then printf 'posix\\n'; fi",
            "out=$(VAGRANT_CHECKPOINT_DISABLE=1 vagrant plugin list 2>&1) \
             || { printf 'failed\\n%s\\n' \"$out\" >&2; exit 1; }",
            "echo \"$p is not writable\" >&2",
            // Descriptor duplication, which creates nothing.
            "printf x >&2",
            "printf x >&-",
            "exec 3>&1",
            // A `>` that is not a redirection at all. Misfiring
            // on ordinary prose is how a guard gets relaxed, and
            // the relaxation is what widens the real hole.
            "printf 'expected >= 2 vCPUs\\n'",
            "echo \"$p -> $d\"",
            "vagrant ssh",
            "vagrant plugin list",
            "vagrant status",
        ] {
            assert_eq!(mutating_token(script), None, "{script:?}");
        }
    }

    #[test]
    fn mutating_token_answers_rather_than_panicking() {
        // It is a `pub` function reading script text that can
        // carry a non-ASCII path. Taking eight *bytes* of context
        // after the `>` panicked when a character straddled the
        // boundary; eight characters cannot.
        assert!(mutating_token(">\u{3b1}\u{3b1}\u{3b1}\u{3b1}").is_some());
        assert!(mutating_token("echo \u{65e5}\u{672c}\u{8a9e}").is_none());
        assert_eq!(mutating_token(""), None);
        assert_eq!(mutating_token("   ;;  && ||"), None);
    }

    #[test]
    fn probe_commands_preserves_the_probe_order() {
        let probes = host_probes(&cfg());
        let cmds = probe_commands(&probes);
        assert_eq!(cmds.len(), probes.len());
        for (cmd, probe) in cmds.iter().zip(probes.iter()) {
            assert_eq!(*cmd, probe.command);
        }
    }

    #[test]
    fn tool_banner_keeps_the_flavour_and_never_lies() {
        assert_eq!(
            tool_banner(&ran(true, "bsdtar 3.8.4 - libarchive\n", "")),
            "bsdtar 3.8.4"
        );
        // GNU tar's second word is "(GNU", so the version has
        // to be found rather than counted to.
        assert_eq!(
            tool_banner(&ran(true, "tar (GNU tar) 1.35\n", "")),
            "tar 1.35"
        );
        // `ssh -V` prints to stderr and puts a comma after the
        // version.
        assert_eq!(
            tool_banner(&ran(
                true,
                "",
                "OpenSSH_for_Windows_9.5p1, LibreSSL 3.8.2\n"
            )),
            "OpenSSH_for_Windows_9.5p1 3.8.2"
        );
        // A version-less banner still names the tool.
        assert_eq!(tool_banner(&ran(true, "busybox\n", "")), "busybox");
        // Silence must not render as an identified flavour.
        assert_eq!(tool_banner(&ran(true, "", "")), "version unknown");
    }

    #[test]
    fn a_local_tool_absent_from_the_path_is_the_whole_diagnosis() {
        let f = local_tool_finding("tar", None, &VersionAnswer::NotAsked);
        assert_eq!(f.scope, Scope::Local);
        assert_eq!(
            f.outcome,
            Outcome::Fail("tar not found on PATH".to_owned())
        );
    }

    #[test]
    fn a_local_tool_with_no_version_flag_reports_where_it_came_from() {
        // `scp` answers no version flag, so the directory is the
        // useful fact -- it is what makes a hijacked binary
        // visible.
        let f = local_tool_finding(
            "scp",
            Some(Path::new("/usr/bin/scp")),
            &VersionAnswer::NotAsked,
        );
        assert_eq!(f.outcome, Outcome::Pass("/usr/bin".to_owned()));
    }

    #[test]
    fn a_local_tool_reports_its_flavour_and_directory() {
        let f = local_tool_finding(
            "tar",
            Some(Path::new("/usr/bin/tar")),
            &VersionAnswer::Answered(ran(true, "tar (GNU tar) 1.35\n", "")),
        );
        assert_eq!(f.outcome, Outcome::Pass("tar 1.35 in /usr/bin".to_owned()));
    }

    #[test]
    fn a_local_tool_that_answers_unhelpfully_still_passes() {
        // Present but uncooperative is not absent. Telling the
        // operator to install a tool they already have would be
        // worse than a missing version string.
        let f = local_tool_finding(
            "tar",
            Some(Path::new("/usr/bin/tar")),
            &VersionAnswer::Answered(ran(false, "", "")),
        );
        assert_eq!(
            f.outcome,
            Outcome::Pass("version unknown in /usr/bin".to_owned())
        );
    }

    #[test]
    fn a_local_tool_that_will_not_start_is_a_different_failure() {
        let f = local_tool_finding(
            "tar",
            Some(Path::new("/usr/bin/tar")),
            &VersionAnswer::WouldNotStart("Permission denied".to_owned()),
        );
        assert_eq!(
            f.outcome,
            Outcome::Fail(
                "cannot run /usr/bin/tar: Permission denied".to_owned()
            )
        );
    }

    #[test]
    fn render_aligns_names_the_host_and_leaves_no_trailing_space() {
        let mut r = Report::default();
        r.add(Finding::new(
            Scope::Local,
            "tar",
            Outcome::Pass("bsdtar 3.8.4".into()),
        ));
        r.add(host_finding("ssh", Outcome::Pass(String::new())));
        // Compared line by line: one concatenated string with
        // escaped runs of spaces is unreadable, so a wrong
        // expectation is as easy to write as a right one.
        assert_eq!(
            r.render("frosti").lines().collect::<Vec<_>>(),
            vec![
                "  local   tar  ok    bsdtar 3.8.4",
                "  frosti  ssh  ok",
                "all checks passed",
            ]
        );
        for line in r.render("frosti").lines() {
            assert_eq!(line, line.trim_end());
        }
    }

    #[test]
    fn render_keeps_lines_inside_the_width_for_a_long_host_name() {
        // The budget is derived from the prefix, so a long host
        // name still yields a line that fits.
        let mut r = Report::default();
        r.add(host_finding(
            "libvirt provider",
            Outcome::Pass("x".repeat(200)),
        ));
        let out = r.render("vmhost.internal.example.com");
        let line = out.lines().next().unwrap();
        assert!(line.chars().count() <= LINE_WIDTH, "{line:?}");
        assert!(line.ends_with("..."), "{line:?}");
    }

    #[test]
    fn a_long_host_name_cannot_delete_the_failure_reason() {
        // The budget is 80 minus the prefix, and the prefix grows
        // with the host name, which `Config` does not bound. At 49
        // characters the detail was three dots; at 52 it was
        // empty, so `FAIL` printed with no reason at all while the
        // report still looked complete. Past the floor the line is
        // allowed to run long instead.
        let host = "deploy-user@vmhost.internal.example.company.com.au";
        let mut r = Report::default();
        r.add(host_finding(
            "libvirt provider",
            Outcome::Fail("Permission denied (publickey).".to_owned()),
        ));
        let line = r.render(host).lines().next().unwrap().to_owned();
        assert!(line.contains("Permission denied"), "{line:?}");
        assert!(
            line.len() > LINE_WIDTH,
            "expected an over-long line: {line}"
        );
    }

    #[test]
    fn only_printable_ascii_survives_into_the_report() {
        // An allowlist, so the characters a blocklist kept
        // missing are covered by construction: the line and
        // paragraph separators (which are not `is_control`), the
        // variation selectors, and the tag block that renders as
        // nothing at all and is the standard way to hide text
        // inside text.
        for hidden in [
            '\u{1b}',    // escape, the cursor-movement lead-in
            '\u{202e}',  // right-to-left override
            '\u{200b}',  // zero-width space
            '\u{2028}',  // line separator
            '\u{2029}',  // paragraph separator
            '\u{fe0f}',  // variation selector
            '\u{e0041}', // tag block: invisible everywhere
            '\u{feff}',  // byte-order mark
        ] {
            let detail = format!("vagrant-libvirt{hidden}-fork");
            let mut r = Report::default();
            r.add(host_finding("x", Outcome::Pass(detail)));
            let out = r.render("h");
            assert!(!out.contains(hidden), "{hidden:?} survived: {out:?}");
            assert!(out.contains("vagrant-libvirt?-fork"), "{out:?}");
        }
    }

    #[test]
    fn render_counts_failures_in_the_summary() {
        let mut r = Report::default();
        r.add(host_finding("a", Outcome::Fail("x".into())));
        assert!(r.render("h").ends_with("1 check failed\n"));
        r.add(host_finding("b", Outcome::Fail("y".into())));
        assert!(r.render("h").ends_with("2 checks failed\n"));
    }

    #[test]
    fn vagrantfile_finding_reports_the_path_only_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let missing = vagrantfile_finding(dir.path());
        assert!(matches!(missing.outcome, Outcome::Fail(_)));
        std::fs::write(dir.path().join("Vagrantfile"), "x").unwrap();
        assert_eq!(
            vagrantfile_finding(dir.path()).outcome,
            Outcome::Pass(String::new())
        );
    }
}
