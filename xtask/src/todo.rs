//! `todo list` / `todo add` / `todo done`: read and update the
//! queue in `docs/todo.md` without loading the whole (large) file
//! into an editor's context.
//!
//! Each item is a headed entry -- an `### <slug>` heading, a
//! `**Summary:**` field and any prose body -- the same shape the
//! reviewer logs use, read by the shared [`crate::records`] parser.
//! So a queue entry and a reviewer finding are one kind of thing to
//! one parser, and the integrity gate resolves cross-references
//! between them.
//!
//! - `list` prints `slug -- summary`, one entry per line.
//! - `add` appends a new entry, refusing a slug that already exists
//!   and a summary that would not fit on one line.
//! - `done` removes an entry whole. The queue holds live work only;
//!   what shipped is recorded by the commit and git history.
//!
//! The command owns *placement and mechanics*; the caller supplies
//! the *content* (slug, summary, body).

use std::fs;

use clap::Subcommand;

use crate::helpers::{
    MARKDOWN_WIDTH, rejoin, require_nonempty, to_owned_lines, workspace_root,
    wrap_markdown,
};
use crate::records;

/// `todo` subcommands.
#[derive(Subcommand)]
pub enum TodoAction {
    /// List queued entries as `slug -- summary`, one per line.
    List,
    /// Append a new entry to the queue.
    Add {
        /// Short kebab-case topic slug (must be unique).
        #[arg(long)]
        slug: String,
        /// One-line summary. Must fit on one `**Summary:**` line
        /// inside 80 columns, or the command errors and tells you
        /// the budget -- put detail in --body.
        #[arg(long)]
        summary: String,
        /// Optional longer body, wrapped as prose under the
        /// summary.
        #[arg(long)]
        body: Option<String>,
    },
    /// Remove a completed entry from the queue.
    Done {
        /// The slug to remove.
        slug: String,
    },
}

/// Entry point for `cargo xtask todo <action>`.
///
/// # Errors
///
/// Returns an error if `docs/todo.md` cannot be read/written, a
/// slug collides on `add`, or the slug is not found on `done`.
pub fn todo(action: TodoAction) -> Result<(), String> {
    match action {
        TodoAction::List => list(),
        TodoAction::Add {
            slug,
            summary,
            body,
        } => add(&slug, &summary, body.as_deref()),
        TodoAction::Done { slug } => done_cmd(&slug),
    }
}

/// The directory holding `todo.md`.
fn docs_dir() -> std::path::PathBuf {
    workspace_root().join("docs")
}

fn todo_path() -> std::path::PathBuf {
    docs_dir().join("todo.md")
}

fn read_todo() -> Result<String, String> {
    let path = todo_path();
    fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {e}", path.display()))
}

fn write_todo(content: &str) -> Result<(), String> {
    let path = todo_path();
    fs::write(&path, content)
        .map_err(|e| format!("write {}: {e}", path.display()))
}

fn list() -> Result<(), String> {
    let content = read_todo()?;
    for r in records::parse(&content) {
        match r.field("Summary") {
            Some(s) if !s.is_empty() => println!("{} -- {s}", r.id()),
            _ => println!("{}", r.id()),
        }
    }
    Ok(())
}

fn add(slug: &str, summary: &str, body: Option<&str>) -> Result<(), String> {
    let slug = Slug::new("todo --slug", slug)?;
    let slug = slug.as_str();
    require_nonempty("todo --summary", summary)?;
    // Render the entry before reading the file, so a bad argument
    // fails the same way with or without a readable docs/todo.md.
    let entry = entry_lines(slug, summary, body)?;
    let content = read_todo()?;
    check_slug_free(&content, slug)?;
    let updated = append_entry(&content, entry);
    write_todo(&updated)?;
    println!("Added pending todo '{slug}'.");
    Ok(())
}

fn done_cmd(slug: &str) -> Result<(), String> {
    // The slug is checked before `read_todo`, so a bad one fails
    // the same way with or without a readable `docs/todo.md`.
    let slug = Slug::new("todo done <slug>", slug)?;
    let content = read_todo()?;
    let updated = remove_entry(&content, &slug)?;
    write_todo(&updated)?;
    println!("Removed '{}' from the queue.", slug.as_str());
    Ok(())
}

/// A queue entry's slug, proven to be the shape the file can read
/// back.
///
/// Holding one is the proof it passed [`records::valid_id`], the
/// same rule the gate applies to every entry id. A slug is spliced
/// into the file two ways -- as an `### <slug>` heading and as the
/// key `remove_entry` searches for -- so a newline in it writes a
/// second heading the parser accepts as a real entry and leaves the
/// original truncated. The type is what makes the check run on the
/// way *in*.
#[derive(Debug)]
struct Slug(String);

impl Slug {
    /// Checks `raw` and reports which rule it broke.
    ///
    /// `what` names the argument in the message, because the two
    /// callers spell it differently: `add` has a `--slug` flag and
    /// `done` takes the slug positionally. A constructor naming one
    /// of them sends the other's operator looking for a flag their
    /// subcommand does not have.
    ///
    /// # Errors
    ///
    /// Blank, or failing [`records::valid_id`].
    fn new(what: &str, raw: &str) -> Result<Self, String> {
        require_nonempty(what, raw)?;
        if !records::valid_id(raw) {
            return Err(format!(
                "{what} '{raw}' is not a slug; use lowercase letters, \
                 digits and dashes, which is the only shape docs/todo.md \
                 can be read back as"
            ));
        }
        Ok(Self(raw.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

// ---- Pure helpers (unit-tested) -------------------------------

/// Renders `<lead> <summary>` on exactly one line of at most
/// `width` `char`s, or reports why it cannot.
///
/// A summary has to occupy one line: the field block ends at the
/// first blank or non-field line, so a wrapped summary's second
/// line would be read as the start of the body.
///
/// Interior whitespace is collapsed, so a summary containing a
/// newline cannot splice an extra entry into the file.
///
/// The error names the remaining budget but no remediation -- the
/// flag to reach for differs per caller, so each adds its own hint.
fn summary_line(
    lead: &str,
    summary: &str,
    width: usize,
) -> Result<String, String> {
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    let lead_width = lead.chars().count() + 1;
    if lead_width >= width {
        return Err(format!(
            "'{lead}' is {lead_width} columns wide on its own, leaving no \
             room for a summary within {width}"
        ));
    }
    let line = format!("{lead} {summary}");
    let len = line.chars().count();
    if len > width {
        return Err(format!(
            "summary is too long: {len} columns, limit {width}; it must fit \
             on one line, so keep it to {} characters after '{lead}'",
            width - lead_width
        ));
    }
    Ok(line)
}

/// The lines of a new queue entry: the `### <slug>` heading, a
/// blank, the `**Summary:**` field, then an optional wrapped body.
///
/// `add` is the caller with a `--body`, so the too-long hint is
/// attached here; `done` shares the reader and has no such flag.
fn entry_lines(
    slug: &str,
    summary: &str,
    body: Option<&str>,
) -> Result<Vec<String>, String> {
    let summary = summary_line("**Summary:**", summary, MARKDOWN_WIDTH)
        .map_err(|e| format!("{e}; move the detail into --body"))?;
    let mut out = vec![format!("### {slug}"), String::new(), summary];
    if let Some(body) = body {
        out.push(String::new());
        out.extend(wrap_markdown(body, "", "", MARKDOWN_WIDTH));
    }
    Ok(out)
}

/// Rejects a slug already used by an entry.
fn check_slug_free(content: &str, slug: &str) -> Result<(), String> {
    if slug_exists(content, slug) {
        return Err(format!(
            "slug '{slug}' already exists in docs/todo.md; pick another"
        ));
    }
    Ok(())
}

/// Whether `slug` heads any entry.
fn slug_exists(content: &str, slug: &str) -> bool {
    records::parse(content).iter().any(|r| r.id() == slug)
}

/// Append `entry` after the last non-empty line, separated by a
/// blank line, so a new item lands at the tail of the queue.
fn append_entry(content: &str, entry: Vec<String>) -> String {
    let ends_with_newline = content.ends_with('\n');
    let mut lines = to_owned_lines(content);
    let last = (0..lines.len())
        .rev()
        .find(|&i| !lines[i].trim().is_empty());
    let at = last.map_or(lines.len(), |i| i + 1);
    let mut ins = vec![String::new()];
    ins.extend(entry);
    lines.splice(at..at, ins);
    rejoin(&lines, ends_with_newline)
}

/// Remove the entry for `slug`, heading through body.
///
/// The queue holds live work only. Completing an item takes it off
/// the list; what shipped is recorded by the commit, the CHANGELOG
/// and git history. The block runs from the entry's heading to the
/// next entry's heading (or end of file), so a multi-line entry is
/// removed whole and the blank line before the next heading stays as
/// its separator.
///
/// `slug` arrives as a [`Slug`] rather than a `&str` because it is
/// matched against the parsed entry ids; the type is the proof the
/// value has the one shape the file writes.
fn remove_entry(content: &str, slug: &Slug) -> Result<String, String> {
    let ends_with_newline = content.ends_with('\n');
    let records = records::parse(content);
    let idx = records
        .iter()
        .position(|r| r.id() == slug.as_str())
        .ok_or_else(|| {
            format!("no pending todo with slug '{}'", slug.as_str())
        })?;
    let mut lines = to_owned_lines(content);
    let start = records[idx].heading_line() - 1;
    let end = records
        .get(idx + 1)
        .map_or(lines.len(), |next| next.heading_line() - 1);
    lines.splice(start..end, std::iter::empty());
    Ok(rejoin(&lines, ends_with_newline))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# TODO

Project work queue.

---

### alpha-task

**Summary:** do alpha

more about alpha

### beta-task

**Summary:** do beta
";

    #[test]
    fn list_reads_slug_and_summary_via_the_shared_parser() {
        let got: Vec<(String, Option<String>)> = records::parse(SAMPLE)
            .iter()
            .map(|r| (r.id().to_owned(), r.field("Summary").map(str::to_owned)))
            .collect();
        assert_eq!(
            got,
            vec![
                ("alpha-task".to_owned(), Some("do alpha".to_owned())),
                ("beta-task".to_owned(), Some("do beta".to_owned())),
            ]
        );
    }

    #[test]
    fn slug_exists_finds_an_entry() {
        assert!(slug_exists(SAMPLE, "alpha-task"));
        assert!(slug_exists(SAMPLE, "beta-task"));
        assert!(!slug_exists(SAMPLE, "missing"));
    }

    #[test]
    fn check_slug_free_rejects_a_collision() {
        let err = check_slug_free(SAMPLE, "beta-task").unwrap_err();
        assert!(err.contains("beta-task"), "got: {err}");
        assert!(err.contains("already exists"), "got: {err}");
        assert!(check_slug_free(SAMPLE, "brand-new").is_ok());
    }

    #[test]
    fn entry_lines_builds_a_headed_entry() {
        let got = entry_lines("gamma", "do gamma", None).unwrap();
        assert_eq!(
            got,
            vec![
                "### gamma".to_owned(),
                String::new(),
                "**Summary:** do gamma".to_owned(),
            ]
        );
    }

    #[test]
    fn entry_lines_wraps_a_body_under_the_summary() {
        let got =
            entry_lines("gamma", "do gamma", Some("a longer note")).unwrap();
        assert_eq!(got[0], "### gamma");
        assert_eq!(got[2], "**Summary:** do gamma");
        assert_eq!(got[3], "");
        assert_eq!(got[4], "a longer note");
    }

    #[test]
    fn summary_line_collapses_interior_whitespace() {
        // A newline would otherwise be written verbatim and start a
        // second entry-shaped line in the file.
        let got =
            summary_line("**Summary:**", "a\n### ghost\n\n**Summary:** x", 80)
                .unwrap();
        assert_eq!(got.lines().count(), 1, "must be one line: {got:?}");
        assert_eq!(got, "**Summary:** a ### ghost **Summary:** x");
    }

    #[test]
    fn summary_line_refuses_a_summary_that_would_wrap() {
        // A wrapped summary's second line reads as the body's start,
        // so it must be refused at write time rather than truncated.
        let lead = "**Summary:**";
        let budget = 80 - lead.chars().count() - 1;
        let err = summary_line(lead, &"x".repeat(80), 80).unwrap_err();
        assert!(err.contains("too long"), "got: {err}");
        assert!(
            err.contains(&budget.to_string()),
            "must state the budget: {err}"
        );
        // The remediation hint belongs to the caller, not here.
        assert!(!err.contains("--body"), "hint leaked into helper: {err}");
    }

    #[test]
    fn summary_line_counts_characters_not_bytes() {
        // 20 multi-byte chars must not be judged as 40+ columns.
        let s = "é".repeat(20);
        assert!(summary_line("**Summary:**", &s, 40).is_ok());
    }

    #[test]
    fn append_entry_lands_after_the_last_entry() {
        let entry = entry_lines("gamma", "do gamma", None).unwrap();
        let out = append_entry(SAMPLE, entry);
        let gamma = out.find("### gamma").unwrap();
        let beta = out.find("### beta-task").unwrap();
        assert!(beta < gamma, "gamma lands after beta");
        // A blank line separates it from beta's summary.
        assert!(
            out.contains(
                "**Summary:** do beta\n\n### gamma\n\n**Summary:** do gamma"
            ),
            "got: {out}"
        );
    }

    #[test]
    fn remove_entry_removes_an_entry_with_a_body() {
        // Completing an item takes the whole entry -- heading,
        // summary and body -- off the list.
        let out = remove_entry(
            SAMPLE,
            &Slug::new("todo done <slug>", "alpha-task").unwrap(),
        )
        .unwrap();
        assert!(!out.contains("### alpha-task"), "alpha gone: {out}");
        assert!(!out.contains("more about alpha"), "body gone: {out}");
        // Beta and its summary remain.
        assert!(out.contains("### beta-task"));
        assert!(out.contains("**Summary:** do beta"));
    }

    #[test]
    fn remove_entry_removes_the_last_entry() {
        let out = remove_entry(
            SAMPLE,
            &Slug::new("todo done <slug>", "beta-task").unwrap(),
        )
        .unwrap();
        assert!(!out.contains("### beta-task"), "beta gone: {out}");
        assert!(!out.contains("do beta"), "summary gone: {out}");
        // Alpha and its body untouched.
        assert!(out.contains("### alpha-task"));
        assert!(out.contains("more about alpha"));
    }

    #[test]
    fn remove_entry_errors_on_unknown_slug() {
        let err = remove_entry(
            SAMPLE,
            &Slug::new("todo done <slug>", "nope").unwrap(),
        )
        .unwrap_err();
        assert!(err.contains("nope"), "got: {err}");
    }

    #[test]
    fn slug_new_refuses_a_newline_that_would_splice_an_entry() {
        // A slug is interpolated into `docs/todo.md`, so a newline
        // in it fabricates a heading the parser accepts as a real
        // entry.
        let splice = "x\n### ghost";
        assert!(Slug::new("todo --slug", splice).is_err(), "newline");
        assert!(
            Slug::new("todo --slug", "has space").is_err(),
            "a space is not a slug"
        );
        assert!(
            Slug::new("todo --slug", "UPPER").is_err(),
            "kebab-case only"
        );
        assert!(Slug::new("todo --slug", "").is_err(), "empty");
        assert_eq!(
            Slug::new("todo --slug", "real-slug-2").unwrap().as_str(),
            "real-slug-2"
        );
    }

    #[test]
    fn done_checks_its_slug_before_any_io() {
        // The slug can be judged from the argument alone, so it
        // fails the same way with or without a readable
        // docs/todo.md. `done` takes its slug positionally, so the
        // message must not send the operator looking for a `--slug`
        // flag: `add` has one, this subcommand does not.
        let err = done_cmd("Not A Slug").unwrap_err();
        assert!(err.contains("todo done"), "got: {err}");
        assert!(!err.contains("--slug"), "done has no --slug: {err}");
    }

    #[test]
    fn add_rejects_an_overlong_summary_before_any_io() {
        // Argument errors must not depend on docs/todo.md being
        // readable, so the render happens before the read.
        let err = add("real-slug", &"x".repeat(200), None).unwrap_err();
        assert!(err.contains("too long"), "got: {err}");
        assert!(err.contains("--body"), "caller hint missing: {err}");
    }

    #[test]
    fn add_rejects_blank_summary_before_any_io() {
        // The guard fires before `read_todo`, so this never touches
        // the real `docs/todo.md`.
        let err = add("real-slug", "   ", None).unwrap_err();
        assert!(err.contains("--summary"));
    }
}
