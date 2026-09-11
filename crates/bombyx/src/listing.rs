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
//! is what [`entries`] takes its `run` argument for.
//!
//! One rule shaped the module: **a state bombyx cannot support
//! is printed as unknown.** A machine that does not answer, and
//! a reply naming no state, both reach [`VmState::Unknown`]
//! rather than a plausible guess, and the reason travels with
//! it so the table can say what went wrong.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::config::{Config, ProjectName};
use crate::doctor::ProbeResult;
use crate::remote::{self, LISTING_MARKER, NEVER_BUILT, RemoteCommand};
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

/// What vagrant writes where a value contains a comma.
///
/// Read from a run of vagrant 2.4.9: the `state-human-long`
/// field came back as `To stop this machine%!(VAGRANT_COMMA) you
/// can run`. Spelled once here rather than at the call site, so
/// the escape and the split that requires it stay together.
const COMMA_ESCAPE: &str = "%!(VAGRANT_COMMA)";

/// How wide a state may print before it is clipped.
///
/// The column is sized from its contents, so a host returning a
/// long state would otherwise decide the width of the whole
/// table.
///
/// 24 is a budget rather than a measurement. The longest state
/// seen from vagrant 2.4.9 is `not created`, at 11 characters,
/// so this clips nothing a working host produces and leaves the
/// table inside 80 columns for the host names and boxes bombyx
/// is used with. Change it if a real provider needs more.
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

impl std::fmt::Display for VmState {
    /// The word an operator reads, safe to put on a terminal.
    ///
    /// `Reported` carries whatever the VM host printed, so this
    /// sanitizes it here rather than telling the caller to. The
    /// rule that makes that necessary is on `term::sanitize`,
    /// which is crate-private. A caller outside this crate
    /// cannot call it, so a doc comment telling them to would be
    /// advice they cannot act on. That is why the guard runs
    /// here instead.
    ///
    /// Not clipped, because a width belongs to a table rather
    /// than to a value; `describe` clips for this module's own.
    /// An `Unknown` prints as the bare word: its reason is a
    /// sentence from `ssh` and belongs in [`notes`], not a cell.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reported(word) => f.write_str(&sanitize(word)),
            Self::NotCreated => f.write_str("not created"),
            Self::Unknown(_) => f.write_str("unknown"),
        }
    }
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

/// The projects on one VM host.
///
/// Private fields, and `group_by_host` is the only thing that
/// builds one, so holding a `HostGroup` *is* the proof of the
/// two rules it carries: it names at least one project, and
/// every project in it runs on the same host.
///
/// A type rather than an assertion, because a release build
/// compiles a `debug_assert` out and the second rule decides
/// which machine a project is asked about.
struct HostGroup<'a> {
    /// The project that supplies the route, and the first row
    /// this group contributes.
    first: &'a Config,
    /// The others on the same host, in the order they arrived.
    rest: Vec<&'a Config>,
}

impl<'a> HostGroup<'a> {
    /// Every project on this host, first one first.
    fn configs(&self) -> impl Iterator<Item = &'a Config> {
        std::iter::once(self.first).chain(self.rest.clone())
    }

    /// The one command that asks this host about its projects.
    fn status_command(&self) -> RemoteCommand {
        remote::vagrant_status_many(self.first, &self.rest)
    }
}

/// Splits `configs` into one group per VM host.
///
/// Each group's projects share a host, so one command can ask
/// about all of them. The groups come out in the order their
/// first project appears, and a project keeps its place inside
/// its group.
fn group_by_host(configs: &[Config]) -> Vec<HostGroup<'_>> {
    let mut groups: Vec<HostGroup<'_>> = Vec::new();
    for cfg in configs {
        if let Some(group) =
            groups.iter_mut().find(|g| g.first.host == cfg.host)
        {
            group.rest.push(cfg);
        } else {
            groups.push(HostGroup {
                first: cfg,
                rest: Vec::new(),
            });
        }
    }
    groups
}

/// The commands [`entries`] would run, in that order.
///
/// What `--dry-run` prints. [`entries`] builds its commands the
/// same way, through `group_by_host` and
/// `HostGroup::status_command`, so a printed plan cannot
/// describe a run bombyx would not perform.
///
/// That property belongs to every VM subcommand and `plan::plan`
/// is where the others get it, by being the one place a plan is
/// built. A `plan` is built from a single `Config`, and this
/// command spans all of them, so there is no `Action` shape for
/// it and the property is kept by sharing a builder instead.
#[must_use]
pub fn status_commands(configs: &[Config]) -> Vec<RemoteCommand> {
    group_by_host(configs)
        .iter()
        .map(HostGroup::status_command)
        .collect()
}

/// One row per project, with the state its host reported.
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
/// A reply is read for the projects bombyx asked about rather
/// than for the ones it mentions, so a host naming a project
/// that is not in `configs` contributes no row.
///
/// The rows come back in `configs` order, not host order, so
/// the table reads the same whichever machines answered.
/// [`Config::load_all`] is what decides that order.
pub fn entries<F>(configs: Vec<Config>, mut run: F) -> Vec<Entry>
where
    F: FnMut(&RemoteCommand) -> Result<ProbeResult, String>,
{
    let mut states: BTreeMap<ProjectName, VmState> = BTreeMap::new();
    for group in group_by_host(&configs) {
        // The states the host reported, and what to say about a
        // project it did not mention. One match, so the two
        // cannot describe different replies.
        // The match produces both the parsed states and the
        // reason to give a project the host said nothing useful
        // about, so the two cannot describe different replies.
        //
        // The reply is read whatever the exit status, because
        // that status answers for one project only --
        // `remote::vagrant_status_many` says why.
        let (mut parsed, host_reason) = match run(&group.status_command()) {
            Ok(result) => (
                parse_states(&result.stdout),
                (!result.success)
                    .then(|| fail_reason(&result.stdout, &result.stderr)),
            ),
            Err(why) => (BTreeMap::new(), Some(sanitize(&why))),
        };
        for cfg in group.configs() {
            let state = parsed.remove(&cfg.project).unwrap_or_else(|| {
                VmState::Unknown(
                    "the host did not report this project".to_owned(),
                )
            });
            // Where no state could be established, the host's own
            // words beat this module's guess at why. A project
            // vagrant did answer about keeps its state, so one
            // project's problem cannot overwrite a sibling's row.
            //
            // The guess is worth little here: the marker is
            // printed before the `Vagrantfile` guard by `printf`,
            // a shell builtin, so a project always has a block
            // and an empty one says only that vagrant wrote
            // nothing to stdout. `sh: vagrant: not found` is on
            // stderr, and it is the sentence the operator needs.
            let state = match (state, &host_reason) {
                (VmState::Unknown(_), Some(why)) => {
                    VmState::Unknown(why.clone())
                }
                (state, _) => state,
            };
            states.insert(cfg.project.clone(), state);
        }
    }
    configs
        .into_iter()
        .map(|config| {
            let state = states.remove(&config.project);
            Entry { config, state }
        })
        .collect()
}

/// One row per project, with no state and no machine contacted.
///
/// What `--offline` produces. A separate function rather than a
/// flag on [`entries`], so the route that asks nothing cannot
/// reach the code that spawns anything.
#[must_use]
pub fn offline_entries(configs: Vec<Config>) -> Vec<Entry> {
    configs
        .into_iter()
        .map(|config| Entry {
            config,
            state: None,
        })
        .collect()
}

/// Reads one host's reply into a state per project.
///
/// The reply is a run of blocks, each introduced by a
/// `remote::LISTING_MARKER` line naming the project.
///
/// Anything before the first marker belongs to no project and is
/// dropped. Charging such a line to whichever project came first
/// would report something about a project the host never said,
/// and the parser does not need to know what could produce one.
///
/// A marker with no lines after it is [`VmState::NotCreated`]:
/// the `if [ -f Vagrantfile ]` guard in
/// [`remote::vagrant_status_many`] emitted the marker and never
/// ran vagrant. A block that carries lines but names no state is
/// [`VmState::Unknown`], because vagrant answered with something
/// this parser does not recognise and inventing a state would be
/// a claim bombyx cannot support.
#[must_use]
fn parse_states(reply: &str) -> BTreeMap<ProjectName, VmState> {
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
    /// Whether the host stated that bombyx never built this VM.
    never_built: bool,
}

impl Block {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            lines: 0,
            state: None,
            never_built: false,
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
        if line.trim() == NEVER_BUILT {
            self.never_built = true;
            return;
        }
        if self.state.is_none() {
            self.state = state_of(line);
        }
    }

    fn finish(self) -> (String, VmState) {
        let state = match (self.never_built, self.lines, self.state) {
            (true, _, _) => VmState::NotCreated,
            (_, _, Some(word)) => VmState::Reported(word),
            (_, 0, _) => VmState::Unknown(
                "the host said nothing about this project".to_owned(),
            ),
            (_, _, None) => {
                VmState::Unknown("vagrant named no state".to_owned())
            }
        };
        (self.name, state)
    }
}

/// Adds `block`'s verdict to `out`, if there is a block whose
/// name is a legal project name.
///
/// The name arrives as text from the VM host. Parsing it here
/// means the map is keyed by the same checked type `Registry`
/// keys its own by, so the join back to a `Config` cannot
/// succeed on a string no project could be called. A name that
/// does not parse is dropped: bombyx asked about names it took
/// from the operator's file, so anything else is the host
/// answering a question nobody put.
///
/// **A name that arrives twice reports no state at all.** Only
/// bombyx's own `printf` should open a block, but the marker is
/// a fixed string and this is host text: any stdout line
/// beginning with it opens one, including a line a project's
/// `Vagrantfile` printed while vagrant loaded it. A second block
/// cannot be told from the first, so taking either would let one
/// project write another's row. Refusing both says what is known
/// -- that the reply is not trustworthy about this project.
fn close(out: &mut BTreeMap<ProjectName, VmState>, block: Option<Block>) {
    let Some(block) = block else { return };
    let (name, state) = block.finish();
    let Ok(name) = ProjectName::parse(&name) else {
        return;
    };
    match out.entry(name) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(state);
        }
        std::collections::btree_map::Entry::Occupied(mut slot) => {
            slot.insert(VmState::Unknown(
                "the reply named this project twice".to_owned(),
            ));
        }
    }
}

/// The state `line` carries, if it is the record that holds one.
///
/// A record is comma-separated, so vagrant cannot put a bare
/// comma in a value and writes [`COMMA_ESCAPE`] instead. That
/// means splitting on commas cannot cut a value in half, and it
/// means the escape has to be turned back before anybody reads
/// it.
fn state_of(line: &str) -> Option<String> {
    let mut fields = line.split(',');
    let _timestamp = fields.next()?;
    let _target = fields.next()?;
    if fields.next()? != STATE_FIELD {
        return None;
    }
    Some(fields.next()?.replace(COMMA_ESCAPE, ","))
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
/// A host that could not answer contributes an `unknown` cell,
/// and [`notes`] carries the reason. The reason is not a cell
/// because it is a sentence from `ssh`, and a column wide enough
/// for one would push every other column off the screen.
///
/// The table alone, so the caller can send it to stdout and the
/// notes to stderr. Appending the notes here would put
/// `bombyx: ...` prose in the middle of output somebody pipes
/// into `awk`.
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

/// The word a state prints as, fitted to the column.
///
/// [`Display`](std::fmt::Display) has already made it safe to
/// print; this only clips it, because the column is sized from
/// its contents and [`VmState::Reported`] carries whatever the
/// host said.
fn describe(state: &VmState) -> String {
    clip(&state.to_string(), STATE_BUDGET)
}

/// One note per host that could not answer, in host order.
///
/// Keyed by host and reason together, so two hosts failing for
/// the same reason each get their own line and one host does not
/// get a line per project.
///
/// Separate from [`render`] because these belong on stderr: they
/// carry the `bombyx:` prefix every other diagnostic in the
/// binary uses, and the reason inside one is text from `ssh`
/// rather than a row of the table.
///
/// **Non-empty exactly when some project's state is
/// [`VmState::Unknown`]**, whatever left it that way. That is
/// the condition `bombyx list` exits non-zero on, so this is
/// where the documented rule is decided.
#[must_use]
pub fn notes(entries: &[Entry]) -> Vec<String> {
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

    /// The state of the row named `name`, if there is one.
    fn state_named<'a>(rows: &'a [Entry], name: &str) -> Option<&'a VmState> {
        rows.iter()
            .find(|e| e.config.project.as_str() == name)?
            .state
            .as_ref()
    }

    /// The state `parse_states` read for `name`.
    fn parsed_state(reply: &str, name: &str) -> Option<VmState> {
        parse_states(reply).remove(&ProjectName::parse(name).unwrap())
    }

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
            .map(|g| g.configs().map(|c| c.project.as_str()).collect())
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
    fn a_comma_in_a_state_is_decoded_rather_than_printed_raw() {
        // vagrant cannot put a bare comma in a record, because
        // the record is comma-separated, so it writes the
        // escape instead. Left alone, the operator reads
        // `not%!(VAGRANT_COMMA)created`.
        let reply = "##bombyx web\n\
             1789149586,default,state-human-short,not%!(VAGRANT_COMMA)yet\n";
        assert_eq!(
            parsed_state(reply, "web"),
            Some(VmState::Reported("not,yet".into()))
        );
    }

    #[test]
    fn a_block_ends_where_the_next_marker_begins() {
        // Two projects in one reply: the first was never built
        // and says so, and its block must not swallow the state
        // that belongs to the second.
        let reply = format!(
            "##bombyx api\n{NEVER_BUILT}\n##bombyx web\n\
             1789149586,default,state-human-short,running\n"
        );
        assert_eq!(parsed_state(&reply, "api"), Some(VmState::NotCreated));
        assert_eq!(
            parsed_state(&reply, "web"),
            Some(VmState::Reported("running".into()))
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
        // output.
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
    fn the_host_s_own_words_beat_this_module_s_guess() {
        // vagrant missing from the non-interactive PATH is this
        // project's recurring VM-host failure. The marker is
        // printed by `printf`, a shell builtin, so the block
        // exists and is empty -- and saying "the host said
        // nothing" is both unactionable and untrue, because the
        // host said plenty on stderr.
        let configs = vec![cfg("web", "one")];
        let rows = entries(configs, |_| {
            Ok(crate::doctor::ProbeResult {
                success: false,
                stdout: "##bombyx web\n".to_owned(),
                stderr: "sh: 1: vagrant: not found".to_owned(),
            })
        });
        let Some(VmState::Unknown(why)) = state_named(&rows, "web") else {
            panic!("expected an unknown state");
        };
        assert!(why.contains("vagrant: not found"), "{why}");
    }

    #[test]
    fn a_host_that_answered_keeps_its_good_rows_when_another_failed() {
        // The failure reason stands in only where no state was
        // established. A project vagrant answered about must not
        // be overwritten by a sibling's problem.
        let configs = vec![cfg("api", "one"), cfg("web", "one")];
        let rows = entries(configs, |_| {
            Ok(crate::doctor::ProbeResult {
                success: false,
                stdout: "##bombyx api\n\
                     1789149586,default,state-human-short,running\n\
                     ##bombyx web\n"
                    .to_owned(),
                stderr: "web: the provider plugin is not installed".to_owned(),
            })
        });
        assert_eq!(
            state_named(&rows, "api"),
            Some(&VmState::Reported("running".into()))
        );
        let Some(VmState::Unknown(why)) = state_named(&rows, "web") else {
            panic!("expected an unknown state");
        };
        assert!(why.contains("provider plugin"), "{why}");
    }

    #[test]
    fn one_failing_project_does_not_blank_its_neighbours() {
        // The fragments are joined with `;`, so the script's exit
        // status is the last fragment's alone. Reading the reply
        // only on a zero status throws away correct blocks for
        // every other project on a reachable host.
        let configs = vec![cfg("api", "one"), cfg("zzz", "one")];
        let rows = entries(configs, |_| {
            Ok(crate::doctor::ProbeResult {
                success: false,
                stdout: "##bombyx api\n\
                     1789149586,default,state-human-short,running\n\
                     ##bombyx zzz\n"
                    .to_owned(),
                stderr: "zzz: the provider plugin is not installed".to_owned(),
            })
        });
        assert_eq!(
            state_named(&rows, "api"),
            Some(&VmState::Reported("running".into())),
            "a reachable project must keep its state"
        );
        assert!(matches!(
            state_named(&rows, "zzz"),
            Some(VmState::Unknown(_))
        ));
    }

    #[test]
    fn a_project_the_host_said_nothing_about_is_unknown() {
        // An empty block is not proof the VM was never built.
        // vagrant missing from the non-interactive PATH writes to
        // stderr and leaves stdout empty, and reading that as
        // `not created` tells the operator bombyx looked when it
        // could not.
        let reply = "##bombyx web\n";
        assert!(matches!(
            parsed_state(reply, "web"),
            Some(VmState::Unknown(_))
        ));
    }

    #[test]
    fn the_never_built_token_is_what_says_never_built() {
        // The guard says so positively, so "never built" is a
        // statement the host made rather than an absence.
        let reply = format!("##bombyx web\n{NEVER_BUILT}\n");
        assert_eq!(parsed_state(&reply, "web"), Some(VmState::NotCreated));
    }

    #[test]
    fn rows_keep_the_config_order_whatever_the_grouping_did() {
        // Grouping reorders: `api` and `db` share a host and
        // `web` sits between them. The rows must come back in
        // the order they were given, so that whatever order
        // `Config::load_all` chose survives the grouping.
        let configs =
            vec![cfg("api", "one"), cfg("web", "two"), cfg("db", "one")];
        let rows = entries(configs, |_| {
            Ok(crate::doctor::ProbeResult {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        });
        let names: Vec<&str> =
            rows.iter().map(|e| e.config.project.as_str()).collect();
        assert_eq!(names, ["api", "web", "db"]);
    }

    #[test]
    fn a_project_named_twice_in_one_reply_is_unknown() {
        // Only bombyx's own `printf` should open a block, but
        // the marker is a fixed string and the reply is text
        // from the host: anything on stdout that starts with it
        // opens one. A second block for a name bombyx already
        // has cannot be told from the first, so neither is
        // trustworthy and the row says so rather than picking.
        let reply = "##bombyx web\n\
             1789149586,default,state-human-short,running\n\
             ##bombyx web\n\
             1789149586,default,state-human-short,shutoff\n";
        let Some(VmState::Unknown(why)) = parsed_state(reply, "web") else {
            panic!("a doubled name must not report a state");
        };
        assert!(why.contains("twice"), "{why}");
    }

    #[test]
    fn a_marker_naming_no_legal_project_is_dropped() {
        // The name comes back as text from the VM host. A value
        // no project could be called cannot key the map bombyx
        // joins on, so it contributes nothing.
        let reply = "##bombyx ../etc\n\
             1789149586,default,state-human-short,running\n";
        assert!(parse_states(reply).is_empty(), "{reply}");
    }

    #[test]
    fn every_project_on_a_host_reaches_the_one_command() {
        // The group's first project supplies the route and the
        // rest ride along; a group that dropped its tail would
        // leave those projects `unknown` on a reachable machine.
        let configs =
            vec![cfg("api", "one"), cfg("db", "one"), cfg("web", "one")];
        let groups = group_by_host(&configs);
        assert_eq!(groups.len(), 1, "one host");
        let named: Vec<&str> =
            groups[0].configs().map(|c| c.project.as_str()).collect();
        assert_eq!(named, ["api", "db", "web"]);
    }

    #[test]
    fn a_host_that_cannot_be_reached_leaves_its_projects_unknown() {
        // One sleeping machine must not cost the states of the
        // projects on the other machines.
        let configs = vec![cfg("api", "one"), cfg("web", "two")];
        let rows = entries(configs, |cmd| {
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
        assert!(
            matches!(state_named(&rows, "api"), Some(VmState::Unknown(r))
            if r.contains("no route to host"))
        );
        assert_eq!(
            state_named(&rows, "web"),
            Some(&VmState::Reported("running".into()))
        );
    }

    #[test]
    fn a_host_that_answers_with_a_failure_leaves_its_projects_unknown() {
        let configs = vec![cfg("api", "one")];
        let rows = entries(configs, |_| {
            Ok(crate::doctor::ProbeResult {
                success: false,
                stdout: String::new(),
                stderr: "Permission denied (publickey).".to_owned(),
            })
        });
        assert!(
            matches!(state_named(&rows, "api"), Some(VmState::Unknown(r))
            if r.contains("Permission denied"))
        );
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
        let entries = [unreachable("api"), unreachable("web")];
        let table = render(&entries);
        let notes = notes(&entries);
        assert_eq!(notes.len(), 1, "one note per host: {notes:?}");
        assert!(notes[0].starts_with("bombyx: one: no route"), "{notes:?}");
        assert_eq!(table.matches("unknown").count(), 2, "{table}");
        // The reason belongs on stderr, so it must not be in
        // the text the caller sends to stdout.
        assert!(!table.contains("no route to host"), "{table}");
    }

    #[test]
    fn a_reason_from_the_host_cannot_repaint_the_table_either() {
        // The note carries text from `ssh` and from the host's
        // stderr, so it needs the same protection the cells get.
        let entries = [Entry {
            config: cfg("web", "one"),
            state: Some(VmState::Unknown("den\u{1b}[2Jied".to_owned())),
        }];
        for note in notes(&entries) {
            assert!(!note.contains('\u{1b}'), "{note}");
        }
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
    fn a_state_is_safe_to_print_however_a_caller_reaches_it() {
        // `VmState` is public and so is `Entry.state`, so a
        // library caller can print one without going through
        // `render`. `term::sanitize` is crate-private, so an
        // instruction to call it would be one they cannot
        // follow: the guard has to be in `Display` itself.
        let hostile = VmState::Reported("run\u{1b}[2Jning".into());
        assert_eq!(hostile.to_string(), "run?[2Jning");
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
