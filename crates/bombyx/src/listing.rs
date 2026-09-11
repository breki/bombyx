//! Listing the registered projects and what their VMs are doing.
//!
//! `bombyx list` answers two questions at once: which projects
//! the operator's config file holds, and what each project's VM
//! is doing right now. The first comes from the file and the
//! second from the machines named in it.
//!
//! Nothing here runs a process, for the reason `doctor` gives:
//! `src/bin/` is outside the coverage gate, so every decision
//! lives in the library and the binary supplies spawning. That
//! is what [`states`] takes its `run` argument for.
//!
//! One rule shaped the module: **a state bombyx cannot support
//! is printed as unknown.** A machine that does not answer, and
//! a reply naming no state, both reach [`VmState::Unknown`]
//! rather than a plausible guess, and the reason travels with
//! it so the table can say what went wrong.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::config::Config;
use crate::doctor::ProbeResult;
use crate::remote::{self, LISTING_MARKER, RemoteCommand};
use crate::term::{clip, fail_reason, sanitize};

/// The vagrant field carrying the state in one short word.
///
/// `vagrant status --machine-readable` writes one
/// `timestamp,target,type,data` record per line, and this is the
/// `type` whose `data` is the word an operator recognises --
/// `running`, `shutoff`, `not created`. Read from a run of
/// vagrant 2.4.9 against libvirt.
///
/// The sibling `state` field carries the provider's own spelling
/// (`not_created`), and `state-human-long` a whole paragraph.
/// This one is the only field of the three meant for a person.
const STATE_FIELD: &str = "state-human-short";

/// How wide a state may print before it is clipped.
///
/// The column is sized from its contents, so a host returning a
/// long state would otherwise decide the width of the whole
/// table.
const STATE_BUDGET: usize = 24;

/// What a project's VM turned out to be doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmState {
    /// vagrant answered, and this is the word it used.
    Reported(String),
    /// The project's directory on the host holds no
    /// `Vagrantfile`, so bombyx has never built this VM.
    ///
    /// Separate from a `Reported("not created")`, which is what
    /// vagrant says when it reads a Vagrantfile and finds no
    /// domain for it -- never booted, or destroyed. Both mean
    /// there is no VM, so the table prints them the same; they
    /// are separate here because this one never ran vagrant.
    NotCreated,
    /// No state could be established, with the reason.
    Unknown(String),
}

/// One row of the listing.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The project's settings, as [`Config::load_all`] assembled
    /// them.
    ///
    /// The whole config rather than the handful of fields the
    /// table prints, so the columns cannot drift from the values
    /// the rest of bombyx would act on.
    pub config: Config,
    /// What its VM is doing, and `None` when no machine was
    /// asked.
    pub state: Option<VmState>,
}

/// Splits `configs` into one group per VM host.
///
/// Each group's projects share a host, so one command can ask
/// about all of them. The groups come out in the order their
/// first project appears, and a project keeps its place inside
/// its group, so a listing built from these is ordered by the
/// key order [`Config::load_all`] produced.
///
/// Every group holds at least one project, which is what
/// [`states`] relies on when it takes the first as the one
/// supplying the route.
#[must_use]
pub fn group_by_host(configs: &[Config]) -> Vec<Vec<&Config>> {
    let mut groups: Vec<Vec<&Config>> = Vec::new();
    for cfg in configs {
        if let Some(group) = groups.iter_mut().find(|g| g[0].host == cfg.host) {
            group.push(cfg);
        } else {
            groups.push(vec![cfg]);
        }
    }
    groups
}

/// The commands [`states`] would run, in the order it runs them.
///
/// What `--dry-run` prints. It comes from the same builder the
/// live run uses, so the printed plan cannot describe a run
/// bombyx would not perform -- the rule `plan::plan` holds for
/// every VM action, kept here by sharing `command_for` rather
/// than by going through `plan`, which builds from one `Config`
/// and so has no shape for a command spanning several.
#[must_use]
pub fn status_commands(configs: &[Config]) -> Vec<RemoteCommand> {
    group_by_host(configs)
        .iter()
        .map(|group| command_for(group))
        .collect()
}

/// The one command that asks about `group`'s host.
///
/// The first project supplies the route, which is why
/// [`group_by_host`] never builds an empty group.
fn command_for(group: &[&Config]) -> RemoteCommand {
    let (first, rest) = group
        .split_first()
        .expect("group_by_host never builds an empty group");
    remote::vagrant_status_many(first, rest)
}

/// Asks every host what its projects are doing.
///
/// `run` carries out one command. It is a parameter so this
/// module stays free of process spawning; the binary passes the
/// real one and tests pass a canned reply. `Err` is for a
/// command that could not be started at all, which is a
/// different failure from one that started and exited non-zero.
///
/// One host that cannot be reached costs only its own projects.
/// The alternative -- returning an error -- would mean a single
/// sleeping machine hid the states of every project on every
/// other machine, and a listing that refuses to list is worth
/// less than one with gaps in it.
///
/// The result is keyed by project name, and only the names in
/// `configs` reach it: a reply is read for the projects it was
/// asked about rather than for the ones it mentions.
pub fn states<F>(configs: &[Config], mut run: F) -> BTreeMap<String, VmState>
where
    F: FnMut(&RemoteCommand) -> Result<ProbeResult, String>,
{
    let mut out = BTreeMap::new();
    for group in group_by_host(configs) {
        // The states the host reported, and what to say about a
        // project it did not mention. One match, so the two
        // cannot describe different replies.
        let (mut parsed, fallback) = match run(&command_for(&group)) {
            Ok(result) if result.success => (
                parse_states(&result.stdout),
                "the host did not report this project".to_owned(),
            ),
            Ok(result) => {
                (BTreeMap::new(), fail_reason(&result.stdout, &result.stderr))
            }
            Err(why) => (BTreeMap::new(), sanitize(&why)),
        };
        for cfg in &group {
            let name = cfg.project.as_str();
            let state = parsed
                .remove(name)
                .unwrap_or_else(|| VmState::Unknown(fallback.clone()));
            out.insert(name.to_owned(), state);
        }
    }
    out
}

/// Reads one host's reply into a state per project.
///
/// The reply is a run of blocks, each introduced by a
/// [`LISTING_MARKER`] line naming the project.
///
/// Anything before the first marker belongs to no project and is
/// dropped. Nothing bombyx sends is expected to put text there:
/// the `vagrant-libvirt` fog warning, the obvious candidate,
/// goes to stderr, and that stays a separate stream because
/// [`remote::vagrant_status_many`] never allocates a PTY.
/// Dropping such a line is what stops it being attributed to
/// whichever project happens to come first.
///
/// A marker with no lines after it is [`VmState::NotCreated`]:
/// the `if [ -f Vagrantfile ]` guard in
/// [`remote::vagrant_status_many`] emitted the marker and never
/// ran vagrant. A block that carries lines but names no state is
/// [`VmState::Unknown`], because vagrant answered with something
/// this parser does not recognise and inventing a state would be
/// a claim bombyx cannot support.
#[must_use]
pub fn parse_states(reply: &str) -> BTreeMap<String, VmState> {
    let mut out = BTreeMap::new();
    let mut open: Option<Block> = None;
    for line in reply.lines() {
        if let Some(name) = line.strip_prefix(LISTING_MARKER) {
            close(&mut out, open.take());
            open = Some(Block::new(name.trim()));
            continue;
        }
        // Before the first marker, so it belongs to no project.
        let Some(block) = open.as_mut() else { continue };
        block.read(line);
    }
    close(&mut out, open);
    out
}

/// One project's block, part-way through being read.
struct Block {
    name: String,
    lines: usize,
    state: Option<String>,
}

impl Block {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            lines: 0,
            state: None,
        }
    }

    /// Takes one line of vagrant's reply.
    ///
    /// The first state wins. bombyx generates a Vagrantfile
    /// defining one machine, so a second would mean the operator
    /// edited the generated file, and reporting the first of
    /// them beats reporting none.
    fn read(&mut self, line: &str) {
        self.lines += 1;
        if self.state.is_none() {
            self.state = state_of(line);
        }
    }

    fn finish(self) -> (String, VmState) {
        let state = match (self.lines, self.state) {
            (0, _) => VmState::NotCreated,
            (_, Some(word)) => VmState::Reported(word),
            (_, None) => VmState::Unknown("vagrant named no state".to_owned()),
        };
        (self.name, state)
    }
}

/// Adds `block`'s verdict to `out`, if there is a block.
fn close(out: &mut BTreeMap<String, VmState>, block: Option<Block>) {
    if let Some(block) = block {
        let (name, state) = block.finish();
        out.insert(name, state);
    }
}

/// The state `line` carries, if it is the record that holds one.
///
/// vagrant escapes a comma inside the data as
/// `%!(VAGRANT_COMMA)`, so splitting the record on commas cannot
/// cut a value in half.
fn state_of(line: &str) -> Option<String> {
    let mut fields = line.split(',');
    let _timestamp = fields.next()?;
    let _target = fields.next()?;
    if fields.next()? != STATE_FIELD {
        return None;
    }
    Some(fields.next()?.to_owned())
}

/// Renders the listing as an aligned table.
///
/// This is where text from a VM host is made safe to print;
/// `term::sanitize` says what that protects. (Named rather than
/// linked because it is crate-private, so the doc build has no
/// public page for it.) A state is the one value in the table
/// the operator did not write.
///
/// The `STATE` column appears only when some entry carries a
/// state. `--offline` asked no machine anything, so a column of
/// dashes would stand in for a question nobody put.
///
/// A host that could not answer contributes an `unknown` cell
/// and one note under the table. The reason goes there rather
/// than in the cell because it is a sentence from `ssh`, and a
/// column wide enough for it would push every other column off
/// the screen.
#[must_use]
pub fn render(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return "no projects registered\n".to_owned();
    }
    let cells: Vec<Row> = entries.iter().map(Row::of).collect();
    let with_state = entries.iter().any(|e| e.state.is_some());
    let mut out = String::new();
    let widths = Widths::over(&cells);
    widths.write(&mut out, &Row::heading(), with_state);
    for row in &cells {
        widths.write(&mut out, row, with_state);
    }
    for note in notes(entries) {
        let _ = writeln!(out, "{note}");
    }
    out
}

/// One row's six cells, already rendered as text.
struct Row {
    name: String,
    host: String,
    box_name: String,
    cpus: String,
    memory: String,
    state: String,
}

impl Row {
    fn heading() -> Self {
        Self {
            name: "NAME".to_owned(),
            host: "HOST".to_owned(),
            box_name: "BOX".to_owned(),
            cpus: "CPUS".to_owned(),
            memory: "MEM".to_owned(),
            state: "STATE".to_owned(),
        }
    }

    fn of(entry: &Entry) -> Self {
        let vm = &entry.config.vm;
        Self {
            name: entry.config.project.as_str().to_owned(),
            host: entry.config.host.as_str().to_owned(),
            box_name: vm.box_name.as_str().to_owned(),
            cpus: vm.cpus.to_string(),
            memory: vm.memory.to_string(),
            state: entry.state.as_ref().map(describe).unwrap_or_default(),
        }
    }
}

/// The word a state prints as.
///
/// Sanitized and clipped here because [`VmState::Reported`]
/// carries whatever the host said.
fn describe(state: &VmState) -> String {
    match state {
        VmState::Reported(word) => sanitize(&clip(word, STATE_BUDGET)),
        VmState::NotCreated => "not created".to_owned(),
        VmState::Unknown(_) => "unknown".to_owned(),
    }
}

/// One note per host that could not answer, in host order.
///
/// Keyed by host and reason together, so two hosts failing for
/// the same reason each get their own line and one host does not
/// get a line per project.
fn notes(entries: &[Entry]) -> Vec<String> {
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for entry in entries {
        if let Some(VmState::Unknown(why)) = &entry.state {
            seen.insert((entry.config.host.as_str(), why.as_str()));
        }
    }
    seen.into_iter()
        .map(|(host, why)| format!("bombyx: {host}: {}", sanitize(why)))
        .collect()
}

/// How wide each column has to be.
struct Widths {
    name: usize,
    host: usize,
    box_name: usize,
    cpus: usize,
    memory: usize,
}

/// Blanks between two columns.
const GAP: usize = 2;

impl Widths {
    /// The widest cell in each column, heading included.
    ///
    /// Measured in characters rather than bytes, because a byte
    /// count misaligns a row holding anything outside ASCII.
    fn over(rows: &[Row]) -> Self {
        let heading = Row::heading();
        let width = |pick: fn(&Row) -> &str| {
            rows.iter()
                .chain(std::iter::once(&heading))
                .map(|r| pick(r).chars().count())
                .max()
                .unwrap_or(0)
        };
        Self {
            name: width(|r| &r.name),
            host: width(|r| &r.host),
            box_name: width(|r| &r.box_name),
            cpus: width(|r| &r.cpus),
            memory: width(|r| &r.memory),
        }
    }

    /// Writes one row, and trims it.
    ///
    /// A line ending in whitespace is noise in a paste and in a
    /// diff, which is why the last column is never padded.
    fn write(&self, out: &mut String, row: &Row, with_state: bool) {
        let line = format!(
            "{name:<nw$}{blank:GAP$}{host:<hw$}{blank:GAP$}\
             {box_name:<bw$}{blank:GAP$}{cpus:>cw$}{blank:GAP$}\
             {memory:>mw$}{blank:GAP$}{state}",
            blank = "",
            name = row.name,
            nw = self.name,
            host = row.host,
            hw = self.host,
            box_name = row.box_name,
            bw = self.box_name,
            cpus = row.cpus,
            cw = self.cpus,
            memory = row.memory,
            mw = self.memory,
            state = if with_state { row.state.as_str() } else { "" },
        );
        let _ = writeln!(out, "{}", line.trim_end());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::name::ProjectName;

    /// A config for `name` on `host`.
    fn cfg(name: &str, host: &str) -> Config {
        let mut c = Config::for_tests();
        c.project = ProjectName::parse(name).unwrap();
        c.host = crate::config::HostName::parse(host).unwrap();
        c
    }

    #[test]
    fn projects_are_grouped_by_the_host_that_owns_them() {
        let configs =
            vec![cfg("api", "one"), cfg("web", "two"), cfg("db", "one")];
        let groups = group_by_host(&configs);
        let names: Vec<Vec<&str>> = groups
            .iter()
            .map(|g| g.iter().map(|c| c.project.as_str()).collect())
            .collect();
        assert_eq!(names, vec![vec!["api", "db"], vec!["web"]]);
    }

    #[test]
    fn a_state_is_read_from_the_reply_vagrant_really_sends() {
        // Copied from a run of vagrant 2.4.9 against libvirt, so
        // the parser is written against the format rather than
        // against what the format was assumed to be.
        let reply = "##bombyx web\n\
             1789149586,default,metadata,provider,libvirt\n\
             1789149586,default,provider-name,libvirt\n\
             1789149586,default,state,running\n\
             1789149586,default,state-human-short,running\n";
        let states = parse_states(reply);
        assert_eq!(
            states.get("web"),
            Some(&VmState::Reported("running".into()))
        );
    }

    #[test]
    fn a_project_whose_marker_carries_no_block_was_never_built() {
        // The `if [ -f Vagrantfile ]` guard emitted the marker
        // and skipped vagrant, which is the shape this reads.
        let reply = "##bombyx api\n##bombyx web\n\
             1789149586,default,state-human-short,running\n";
        let states = parse_states(reply);
        assert_eq!(states.get("api"), Some(&VmState::NotCreated));
        assert_eq!(
            states.get("web"),
            Some(&VmState::Reported("running".into()))
        );
    }

    #[test]
    fn a_block_naming_no_state_is_unknown_rather_than_invented() {
        // vagrant answered and said nothing this parser
        // recognises. Printing a state anyway would be a claim
        // bombyx cannot support.
        let reply = "##bombyx web\n1789149586,default,provider-name,libvirt\n";
        assert!(matches!(
            parse_states(reply).get("web"),
            Some(VmState::Unknown(_))
        ),);
    }

    #[test]
    fn text_before_the_first_marker_is_ignored() {
        // A line ahead of every marker belongs to no project,
        // so it must not be counted as the first project's
        // output. The text is the fog warning vagrant-libvirt
        // writes, which reaches stderr rather than this reply.
        let reply = "[fog][WARNING] Unrecognized arguments\n\
             ##bombyx web\n1789149586,default,state-human-short,running\n";
        let states = parse_states(reply);
        assert_eq!(states.len(), 1);
        assert_eq!(
            states.get("web"),
            Some(&VmState::Reported("running".into()))
        );
    }

    #[test]
    fn the_dry_run_prints_one_command_per_host() {
        // The same builder the live run uses, so a printed plan
        // cannot describe a run bombyx would not perform.
        let configs =
            vec![cfg("api", "one"), cfg("web", "two"), cfg("db", "one")];
        let cmds = status_commands(&configs);
        assert_eq!(cmds.len(), 2, "two hosts: {cmds:?}");
        let first = cmds[0].args.last().unwrap();
        assert!(first.contains("##bombyx %s\\n' 'api'"), "{first}");
        assert!(first.contains("##bombyx %s\\n' 'db'"), "{first}");
    }

    #[test]
    fn a_host_that_cannot_be_reached_leaves_its_projects_unknown() {
        // One sleeping machine must not cost the states of the
        // projects on the other machines.
        let configs = vec![cfg("api", "one"), cfg("web", "two")];
        let states = states(&configs, |cmd| {
            if cmd.args.iter().any(|a| a.contains("api")) {
                Err("ssh: connect: no route to host".to_owned())
            } else {
                Ok(crate::doctor::ProbeResult {
                    success: true,
                    stdout: "##bombyx web\n\
                         1789149586,default,state-human-short,running\n"
                        .to_owned(),
                    stderr: String::new(),
                })
            }
        });
        assert!(matches!(states.get("api"), Some(VmState::Unknown(r))
            if r.contains("no route to host")));
        assert_eq!(
            states.get("web"),
            Some(&VmState::Reported("running".into()))
        );
    }

    #[test]
    fn a_host_that_answers_with_a_failure_leaves_its_projects_unknown() {
        let configs = vec![cfg("api", "one")];
        let states = states(&configs, |_| {
            Ok(crate::doctor::ProbeResult {
                success: false,
                stdout: String::new(),
                stderr: "Permission denied (publickey).".to_owned(),
            })
        });
        assert!(matches!(states.get("api"), Some(VmState::Unknown(r))
            if r.contains("Permission denied")));
    }

    #[test]
    fn the_table_carries_a_state_column_only_when_a_state_was_read() {
        let entry = |state| Entry {
            config: cfg("web", "one"),
            state,
        };
        let with = render(&[entry(Some(VmState::Reported("running".into())))]);
        assert!(with.contains("STATE"), "{with}");
        assert!(with.contains("running"), "{with}");

        // `--offline` asked no machine anything, so a column of
        // dashes would be noise standing in for a question that
        // was never put.
        let without = render(&[entry(None)]);
        assert!(!without.contains("STATE"), "{without}");
    }

    #[test]
    fn the_columns_line_up_under_their_headings() {
        // One long name must move every column after it on both
        // rows, or the table reads as though a value belonged to
        // the heading beside it.
        let table = render(&[
            Entry {
                config: cfg("a", "one"),
                state: Some(VmState::NotCreated),
            },
            Entry {
                config: cfg("a-much-longer-name", "one"),
                state: Some(VmState::Reported("running".into())),
            },
        ]);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 3, "a heading and two rows: {table}");
        let host_at = lines[0].find("HOST").expect("a HOST heading");
        for line in &lines[1..] {
            assert_eq!(
                line.find("one"),
                Some(host_at),
                "the host column must start at one place: {table}"
            );
        }
    }

    #[test]
    fn a_registry_naming_no_project_says_so_instead_of_a_heading() {
        // A bare heading over nothing reads as a table that
        // failed to load rather than as an empty registry.
        let table = render(&[]);
        assert_eq!(table, "no projects registered\n");
        assert!(!table.contains("NAME"), "{table}");
    }

    #[test]
    fn a_host_that_could_not_answer_is_explained_under_the_table() {
        // `unknown` in the cell says the state is missing. The
        // note is what says why, and it is one line per host
        // rather than one per project on it.
        let unreachable = |name| Entry {
            config: cfg(name, "one"),
            state: Some(VmState::Unknown("no route to host".to_owned())),
        };
        let table = render(&[unreachable("api"), unreachable("web")]);
        assert_eq!(
            table.matches("no route to host").count(),
            1,
            "one note per host: {table}"
        );
        assert!(table.contains("bombyx: one: no route"), "{table}");
        assert_eq!(table.matches("unknown").count(), 2, "{table}");
    }

    #[test]
    fn a_reason_from_the_host_cannot_repaint_the_table_either() {
        // The note carries text from `ssh` and from the host's
        // stderr, so it needs the same protection the cells get.
        let table = render(&[Entry {
            config: cfg("web", "one"),
            state: Some(VmState::Unknown("den\u{1b}[2Jied".to_owned())),
        }]);
        assert!(!table.contains('\u{1b}'), "{table}");
    }

    #[test]
    fn an_over_long_state_cannot_widen_the_whole_table() {
        // The state is host-supplied, and the column is sized
        // from its contents, so an unclipped one would decide the
        // width of every row.
        let table = render(&[Entry {
            config: cfg("web", "one"),
            state: Some(VmState::Reported("x".repeat(200))),
        }]);
        for line in table.lines() {
            assert!(line.chars().count() < 80, "{line}");
        }
    }

    #[test]
    fn a_state_the_host_made_up_cannot_repaint_the_table() {
        // The state is text from the VM host. An escape sequence
        // in it would let that host rewrite rows it does not own.
        let table = render(&[Entry {
            config: cfg("web", "one"),
            state: Some(VmState::Reported("run\u{1b}[2Jning".into())),
        }]);
        assert!(!table.contains('\u{1b}'), "{table}");
    }
}
