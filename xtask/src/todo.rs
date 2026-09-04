//! `todo list` / `todo add` / `todo done`: read and update
//! `docs/todo.md` without loading the whole (large) file into an
//! editor's context.
//!
//! - `list` prints the pending (or done) entries as
//!   `slug -- summary`, so a caller can see what is queued
//!   cheaply.
//! - `add` appends a new bullet under `## Pending`, refusing a
//!   slug that already exists (pending or done) and a summary
//!   that would not fit on one line.
//! - `done` moves a pending bullet to the top of `## Done`
//!   (newest first) and stamps the date. `--doc` names the
//!   document the entry links to, and omitting it writes no
//!   link -- see `move_to_done` for why the caller decides.
//!
//! The command owns *placement and mechanics*; the caller
//! supplies the *content* (slug, summary, body).

use std::fs;

use clap::Subcommand;

use crate::helpers::{
    MARKDOWN_WIDTH, rejoin, require_nonempty, section_bounds, to_owned_lines,
    workspace_root, wrap_markdown,
};

/// `todo` subcommands.
#[derive(Subcommand)]
pub enum TodoAction {
    /// List queued entries as `slug -- summary`, one per line.
    List {
        /// List the `## Done` entries instead of `## Pending`.
        #[arg(long)]
        done: bool,
    },
    /// Append a new bullet under `## Pending`.
    Add {
        /// Short kebab-case topic slug (must be unique).
        #[arg(long)]
        slug: String,
        /// One-line summary. Must fit on one line with the
        /// slug inside 80 columns, or the command errors and
        /// tells you the budget -- put detail in --body.
        #[arg(long)]
        summary: String,
        /// Optional longer body, wrapped and indented under the
        /// summary.
        #[arg(long)]
        body: Option<String>,
        /// Render the slug as a link to `issues/<slug>.md` (for
        /// an already-designed capture whose spec exists).
        #[arg(long)]
        issue: bool,
    },
    /// Move a pending entry to the top of `## Done`.
    Done {
        /// The slug to complete.
        slug: String,
        /// Done-entry summary; defaults to the pending summary.
        #[arg(long)]
        summary: Option<String>,
        /// Completion date `YYYY-MM-DD`. Required -- the caller
        /// supplies it (no implicit system-clock read, which
        /// would use UTC and mis-date near midnight).
        #[arg(long)]
        date: String,
        /// Document the entry links to, written relative to
        /// `docs/` (`issues/<slug>.md`, or a shared plan such as
        /// `issues/project-config-off-repo.md`). Omit it and the
        /// entry carries no link. A path naming no file is an
        /// error.
        #[arg(long)]
        doc: Option<String>,
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
        TodoAction::List { done } => list(done),
        TodoAction::Add {
            slug,
            summary,
            body,
            issue,
        } => add(&slug, &summary, body.as_deref(), issue),
        TodoAction::Done {
            slug,
            summary,
            date,
            doc,
        } => done_cmd(&slug, summary.as_deref(), &date, doc.as_deref()),
    }
}

/// The directory holding `todo.md`, and the directory a
/// `--doc` link resolves against.
///
/// One function for both, because `DocLink` checks the target
/// against the directory the link is written *from*: two
/// independent spellings of `docs/` could drift and the guard
/// would then vet a different directory than the one the reader
/// resolves in.
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

fn list(done: bool) -> Result<(), String> {
    let content = read_todo()?;
    let heading = if done { "## Done" } else { "## Pending" };
    for (slug, summary) in parse_section(&content, heading) {
        if summary.is_empty() {
            println!("{slug}");
        } else {
            println!("{slug} -- {summary}");
        }
    }
    Ok(())
}

fn add(
    slug: &str,
    summary: &str,
    body: Option<&str>,
    issue: bool,
) -> Result<(), String> {
    let slug = Slug::new("todo --slug", slug)?;
    let slug = slug.as_str();
    require_nonempty("todo --summary", summary)?;
    // Render the bullet before reading the file, so a bad
    // argument fails the same way with or without a readable
    // docs/todo.md.
    let mut bullet = bullet_lines(slug, summary, issue)?;
    if let Some(body) = body {
        bullet.extend(wrap_markdown(body, "  ", "  ", MARKDOWN_WIDTH));
    }
    let content = read_todo()?;
    check_slug_free(&content, slug)?;
    let updated = add_pending(&content, bullet)?;
    write_todo(&updated)?;
    println!("Added pending todo '{slug}'.");
    Ok(())
}

fn done_cmd(
    slug: &str,
    summary: Option<&str>,
    date: &str,
    doc: Option<&str>,
) -> Result<(), String> {
    // Every argument that reaches the file is checked before
    // `read_todo`, so a bad one fails the same way with or
    // without a readable `docs/todo.md` -- the property
    // `add_rejects_an_overlong_summary_before_any_io` pins for
    // `add`.
    let slug = Slug::new("todo done <slug>", slug)?;
    let date = DoneDate::new(date)?;
    let link = doc.map(|rel| DocLink::new(&docs_dir(), rel)).transpose()?;
    let content = read_todo()?;
    let updated = move_to_done(&content, &slug, &date, summary, link.as_ref())?;
    write_todo(&updated)?;
    println!("Moved '{}' to Done ({}).", slug.as_str(), date.as_str());
    Ok(())
}

/// A queue entry's identifier, proven to be the shape the file
/// can read back.
///
/// Holding one is the proof it passed [`valid_slug`]. The rule
/// is in that function; this type is what makes it run on the
/// way *in*, not only on lines read back out.
///
/// A slug is spliced into the file three ways -- as a bullet
/// label, as part of a link path in `bullet_lines`, and as the
/// key `move_to_done` searches for -- so a newline in one
/// writes a second bullet the parser accepts as a real entry
/// and leaves the original truncated. [`DocLink`] is the
/// sibling type, guarding the other value that reaches the
/// file.
#[derive(Debug)]
struct Slug(String);

impl Slug {
    /// Checks `raw` and reports which rule it broke.
    ///
    /// `what` names the argument in the message, because the two
    /// callers spell it differently: `add` has a `--slug` flag
    /// and `done` takes the slug positionally. A constructor
    /// naming one of them sends the other's operator looking for
    /// a flag their subcommand does not have.
    ///
    /// # Errors
    ///
    /// Blank, or failing [`valid_slug`], which holds the shape
    /// and why it is that narrow.
    fn new(what: &str, raw: &str) -> Result<Self, String> {
        require_nonempty(what, raw)?;
        if !valid_slug(raw) {
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

/// A completion date, proven to be `YYYY-MM-DD`.
///
/// The date is written into the entry as `  (<date>)`, so a
/// newline in it closes the entry early and fabricates a bullet
/// underneath -- which `todo list --done` then reports as
/// completed work. Neither of the treatments its neighbours get
/// reaches it: a summary is whitespace-collapsed and a slug has
/// a shape rule, and a date has its own.
///
/// The shape is checked and the calendar is not. `2026-02-31`
/// passes. Rejecting it would mean a date library for a field a
/// human types from their own clock, and a wrong-but-plausible
/// date misleads nobody the way a spliced bullet does.
#[derive(Debug)]
struct DoneDate(String);

impl DoneDate {
    /// # Errors
    ///
    /// Anything that is not ten characters of `YYYY-MM-DD`.
    fn new(raw: &str) -> Result<Self, String> {
        let shaped = raw.len() == 10
            && raw.chars().enumerate().all(|(i, c)| {
                if i == 4 || i == 7 {
                    c == '-'
                } else {
                    c.is_ascii_digit()
                }
            });
        if !shaped {
            return Err(format!(
                "todo --date '{raw}' is not a YYYY-MM-DD date"
            ));
        }
        Ok(Self(raw.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether `rel` would anchor a markdown link to a filesystem
/// root rather than to `docs/`.
///
/// `Path::is_absolute` cannot answer this: it answers for the
/// machine the code was compiled for, and a link is read on
/// every other one. It calls `/etc/passwd` relative on Windows
/// and `C:\docs\plan.md` relative on Unix, and both are rooted
/// in a link. Windows CI is what surfaced the first of those. So
/// the shapes are matched directly: a leading separator, or a
/// drive letter.
fn is_rooted_link(rel: &str) -> bool {
    if rel.starts_with('/') || rel.starts_with('\\') {
        return true;
    }
    let mut chars = rel.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(c), Some(':')) if c.is_ascii_alphabetic()
    )
}

/// Whether `rel` uses any character outside the set `--doc`
/// allows.
///
/// **This is where the allowed set is written down.** Other
/// sites name this function rather than repeating it.
///
/// The set is ASCII letters, digits, `-`, `_`, `.` and `/`.
/// Stating what is allowed, rather than listing what is banned,
/// is what stops the rule and its description drifting apart:
/// an allowed set of six kinds of character does not grow, and
/// every path to a document in this repository is spelled with
/// them, so it costs nothing.
///
/// It keeps out two different kinds of thing, and the error
/// says the second rather than the first. Whitespace and
/// parentheses truncate a bare markdown destination; a
/// backslash is a literal in markdown, and `Path::join` on
/// Windows would treat it as a real separator, which is the
/// host deciding a question about a link; angle brackets are
/// the other destination syntax. But `#`, `?` and `%` break
/// nothing -- `(issues/plan.md#step-3)` renders and resolves.
/// They are refused because `--doc` takes a path and a fragment
/// is not part of one.
fn outside_allowed_chars(rel: &str) -> bool {
    !rel.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/')
    })
}

/// Whether `rel`, resolved from `docs/`, leaves the repository.
///
/// Counted rather than resolved on disk, so the answer does not
/// depend on which directories happen to exist. `docs/` is one
/// level below the root, so the walk starts at depth 1: each
/// ordinary component descends, each `..` climbs, and dropping
/// below zero means the path has climbed past the root.
///
/// One `..` is legitimate and must stay so -- `docs/todo.md`
/// linking to `../README.md` reaches the repository root, which
/// is where the README is.
fn escapes_repo(rel: &str) -> bool {
    let mut depth: i32 = 1;
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// A `--doc` target proven followable from `docs/todo.md`.
///
/// Holding one is the proof that it passed every rule below, so
/// `move_to_done` cannot render a target nobody checked. A
/// checking function beside the renderer would prove only that
/// the paths calling it were checked -- and this value goes
/// straight into a `format!`, on a file this project reads
/// daily and ships to a template downstream.
///
/// Being a distinct type also stops a swap: `move_to_done`
/// takes a summary and a link in adjacent positions, and as two
/// `Option<&str>` they were interchangeable to the compiler.
#[derive(Debug)]
struct DocLink(String);

impl DocLink {
    /// Checks `rel`, written relative to `docs/`, and refuses
    /// every shape that would not survive the trip to another
    /// reader.
    ///
    /// Five rules.
    ///
    /// **Blank** names nothing; omitting `--doc` is how you ask
    /// for no link.
    ///
    /// **Rooted** anchors the link to a filesystem root instead
    /// of to `docs/`, so it resolves only on a machine laid out
    /// like the author's. [`is_rooted_link`] decides this
    /// without asking the host, because `Path::is_absolute`
    /// answers for the machine that compiled the code and a link
    /// is read on every other one.
    ///
    /// **Not renderable** is decided by
    /// [`outside_allowed_chars`], which holds the set and the
    /// reasoning.
    ///
    /// **Outside the repository** is the one existence alone
    /// cannot see. `../../../../etc/passwd` is not rooted and
    /// *does* name a file here, so a check for existence passes
    /// it and writes a link dead for everybody else.
    /// [`escapes_repo`] answers it by counting components rather
    /// than touching the disk, so a missing directory cannot
    /// change the verdict. One `..` is legitimate: `docs/todo.md`
    /// linking to `../README.md` reaches the repository root.
    ///
    /// **Names no file** is the original defect -- the dead link
    /// that prompted all of this.
    fn new(docs: &std::path::Path, rel: &str) -> Result<Self, String> {
        if rel.trim().is_empty() {
            return Err("todo --doc is blank; omit it for no link".to_owned());
        }
        if is_rooted_link(rel) {
            return Err(format!(
                "todo --doc '{rel}' is rooted, so the link would resolve \
                 from a filesystem root rather than from docs/; write it \
                 relative to docs/"
            ));
        }
        if outside_allowed_chars(rel) {
            return Err(format!(
                "todo --doc '{rel}' is not a path this can link to; it \
                 takes a path spelled with letters, digits, '-', '_', '.' \
                 and '/', and not a URL or a #fragment"
            ));
        }
        if escapes_repo(rel) {
            return Err(format!(
                "todo --doc '{rel}' resolves outside the repository, so the \
                 link would be dead for every other reader"
            ));
        }
        if !docs.join(rel).is_file() {
            return Err(format!(
                "todo --doc '{rel}' names no file under docs/, so the link \
                 would be dead; pass the plan the item belongs to, or omit \
                 --doc"
            ));
        }
        Ok(Self(rel.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    /// A link that skipped the checks.
    ///
    /// **Test-only.** `move_to_done` is pure over the markdown
    /// and its tests are about placement and shape, against
    /// fixture files that do not exist on disk. Whether a target
    /// is followable is `new`'s subject and is covered by its own
    /// table, so requiring a real file here would only make those
    /// tests build a directory to prove something they are not
    /// testing.
    #[cfg(test)]
    fn unchecked(rel: &str) -> Self {
        Self(rel.to_owned())
    }
}

// ---- Pure helpers (unit-tested) -------------------------------

/// The `(body_start, body_end)` line range of a `## <heading>`
/// section body (just after the heading to the next `## ` or
/// EOF), built on the shared [`section_bounds`].
fn section_body(lines: &[String], heading: &str) -> Option<(usize, usize)> {
    section_bounds(lines, heading).map(|(h, end)| (h + 1, end))
}

/// Separator between a bullet's slug and its summary.
const SEP: &str = " -- ";

/// Whether `slug` has the kebab-case shape the `--slug` flag
/// documents.
///
/// This is what keeps prose out of the queue. The delimiters
/// alone are too weak a guard: inline code in a sentence, as in
/// `` - `bombyx.toml` -- lives at the repo root ``, is
/// structurally identical to a backticked entry. Requiring the
/// captured text to look like a slug rejects it.
///
/// **This is where the slug shape is written down.** The set is
/// lowercase ASCII, digits and `-`, and it is that narrow
/// because a slug reaches three places -- a bullet label, a
/// link path in `bullet_lines`, and the parser above -- and no
/// character in the set means anything to any of them.
/// [`Slug`] is what makes this run on a value coming *in*.
fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The text sitting in a bullet's slug position, before any
/// shape check.
///
/// Recognizes the three spellings the file uses:
/// `- **foo** -- summary` (written by `add`),
/// `` - `foo` -- summary `` (hand-written), and
/// `- [**foo**](issues/foo.md)` (written by `done`). The bold
/// and backticked forms require the [`SEP`] separator.
fn raw_slug(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("- [**") {
        let end = rest.find("**")?;
        return Some(rest[..end].to_owned());
    }
    for (open, close) in [("- **", "**"), ("- `", "`")] {
        let Some(rest) = line.strip_prefix(open) else {
            continue;
        };
        let end = rest.find(close)?;
        return rest[end + close.len()..]
            .starts_with(SEP)
            .then(|| rest[..end].to_owned());
    }
    None
}

/// Slug of a top-level bullet's first line, or `None` when the
/// bullet is not a queue entry.
fn parse_slug(line: &str) -> Option<String> {
    raw_slug(line).filter(|s| valid_slug(s))
}

/// Renders `<lead> <summary>` on exactly one line of at most
/// `width` `char`s, or reports why it cannot.
///
/// A summary has to occupy one line, because the body is
/// written with the same two-space indent a wrapped summary
/// would use: allow the wrap and the two become
/// indistinguishable on read-back.
///
/// Interior whitespace is collapsed, so a summary containing a
/// newline cannot splice an extra bullet into the file.
///
/// The error names the remaining budget but no remediation --
/// the flag to reach for differs per caller, so each adds its
/// own hint.
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

/// The `- <label> -- <summary>` first line of a Pending bullet.
fn pending_line(label: &str, summary: &str) -> Result<String, String> {
    summary_line(&format!("- {label} --"), summary, MARKDOWN_WIDTH)
}

/// The `  -- <summary>` continuation used when the slug's label
/// takes the whole first line.
fn continuation_line(summary: &str) -> Result<String, String> {
    summary_line("  --", summary, MARKDOWN_WIDTH)
        .map_err(|e| format!("{e}; pass a shorter --summary"))
}

/// The summary after the ` -- ` separator on a bullet's first
/// line, or empty when absent.
fn parse_summary(line: &str) -> String {
    line.split_once(SEP)
        .map_or_else(String::new, |(_, s)| s.trim().to_owned())
}

/// Parse `(slug, summary)` pairs from a section's top-level
/// bullets. A Pending entry carries its summary on the first
/// line (`- **slug** -- summary`); a Done entry carries it on a
/// `  -- summary` continuation line, with the link alone on the
/// first line. When the first line has no ` -- ` summary, the
/// first `  -- ` continuation line (before the next bullet) is
/// used, so `list --done` shows summaries rather than bare slugs.
fn parse_section(content: &str, heading: &str) -> Vec<(String, String)> {
    let lines = to_owned_lines(content);
    let Some((start, end)) = section_body(&lines, heading) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        let Some(slug) = lines[i]
            .starts_with("- ")
            .then(|| parse_slug(&lines[i]))
            .flatten()
        else {
            i += 1;
            continue;
        };
        let j = block_end(&lines, i, end);
        out.push((slug, block_summary(&lines, i, j)));
        i = j;
    }
    out
}

/// Where the bullet starting at `first` ends: the next
/// top-level bullet or heading, or `end`.
///
/// A bullet owns every line under it until one of those, which
/// is what lets an entry carry a summary continuation and a
/// wrapped body.
fn block_end(lines: &[String], first: usize, end: usize) -> usize {
    ((first + 1)..end)
        .find(|&i| lines[i].starts_with("- ") || lines[i].starts_with("## "))
        .unwrap_or(end)
}

/// A bullet's summary, wherever in its block it sits.
///
/// Two shapes carry it and the reader must accept both. A plain
/// entry puts it after ` -- ` on the first line. A linked entry
/// cannot -- the label carries the slug twice and leaves no room
/// -- so it goes on a `  -- ` continuation underneath.
///
/// A reader that stops at the first line destroys a linked
/// entry's summary rather than merely missing it, because
/// `move_to_done` splices the whole block away once it has read
/// what it wanted. `parse_section` and `move_to_done` both call
/// this, so they cannot disagree about where the summary is.
fn block_summary(lines: &[String], first: usize, end: usize) -> String {
    let summary = parse_summary(&lines[first]);
    if !summary.is_empty() {
        return summary;
    }
    ((first + 1)..end)
        .find_map(|i| lines[i].trim_start().strip_prefix("-- "))
        .map(|rest| rest.trim().to_owned())
        .unwrap_or_default()
}

/// The lines of a new Pending bullet.
///
/// A plain entry fits `- **slug** -- summary` on one line. An
/// `--issue` entry cannot: its label carries the slug twice
/// (`- [**slug**](issues/slug.md) --` is 24 columns plus twice
/// the slug), which for an ordinary slug leaves no room for a
/// summary at all. Those take the same two-line shape `done`
/// writes -- label alone, then a `  -- summary` continuation --
/// which `parse_section` already reads.
/// `add` still derives `issues/<slug>.md` while `done` no
/// longer does, and that is deliberate. `add` captures an item
/// *before* its spec is written, so the target legitimately may
/// not exist yet and a guard like [`DocLink`]'s would refuse a
/// correct call. Whether the flag should take a path instead,
/// or go, is open -- `add-issue-flag-unused` in `docs/todo.md`.
fn bullet_lines(
    slug: &str,
    summary: &str,
    issue: bool,
) -> Result<Vec<String>, String> {
    if issue {
        return Ok(vec![
            format!("- [**{slug}**](issues/{slug}.md)"),
            continuation_line(summary)?,
        ]);
    }
    // `add` is the caller with a `--body`, so the hint is
    // attached here rather than inside `pending_line`. `done`
    // shares that renderer and has no such flag.
    Ok(vec![
        pending_line(&format!("**{slug}**"), summary)
            .map_err(|e| format!("{e}; move the detail into --body"))?,
    ])
}

/// Rejects a slug already used anywhere in the file.
fn check_slug_free(content: &str, slug: &str) -> Result<(), String> {
    if slug_exists(content, slug) {
        return Err(format!(
            "slug '{slug}' already exists in docs/todo.md; pick another"
        ));
    }
    Ok(())
}

/// Whether `slug` heads any bullet anywhere in the file.
fn slug_exists(content: &str, slug: &str) -> bool {
    content
        .lines()
        .filter(|l| l.starts_with("- "))
        .filter_map(parse_slug)
        .any(|s| s == slug)
}

/// Append `bullet` to the end of the `## Pending` section.
fn add_pending(content: &str, bullet: Vec<String>) -> Result<String, String> {
    let ends_with_newline = content.ends_with('\n');
    let mut lines = to_owned_lines(content);
    let (start, end) = section_body(&lines, "## Pending")
        .ok_or("docs/todo.md has no '## Pending' section")?;
    let last_content =
        (start..end).rev().find(|&i| !lines[i].trim().is_empty());
    let at = last_content.map_or(start, |i| i + 1);
    let mut ins = vec![String::new()];
    ins.extend(bullet);
    lines.splice(at..at, ins);
    Ok(rejoin(&lines, ends_with_newline))
}

/// Move the pending bullet for `slug` to the top of `## Done`,
/// stamped with `date`.
///
/// `doc` is the document the entry links to, written relative to
/// `docs/`. The caller supplies it because only the caller knows
/// which document that is: `/implement` writes `issues/<slug>.md`
/// and passes it, while an item worked through `/issue` is one
/// step of a plan shared with its siblings, so its link is that
/// plan. Deriving `issues/<slug>.md` here instead wrote a link to
/// a file nobody had created, twice.
///
/// The two shapes are not cosmetic. With a `doc` the label
/// carries the slug twice and cannot share a line with the
/// summary, so the entry is the label, a `  -- <summary>`
/// continuation and the date. Without one, `- **slug** -- summary`
/// fits on one line and *must* stay on one: [`raw_slug`] accepts a
/// bold slug only when [`SEP`] follows it on the same line, so a
/// wrapped one parses as nothing, disappears from
/// `todo list --done`, and lets `add` mint a duplicate slug.
///
/// Whether the file `doc` names exists is checked by the caller,
/// which has the workspace root; this function is pure over the
/// markdown.
///
/// `slug` and `date` arrive as their own types rather than as
/// `&str`, for the same reason `doc` does. This function splices
/// all three into the file, and taking two of them unchecked
/// would mean a second caller could re-open the injections
/// [`Slug`] and [`DoneDate`] were added to close, with nothing
/// to stop it compiling.
fn move_to_done(
    content: &str,
    slug: &Slug,
    date: &DoneDate,
    summary: Option<&str>,
    doc: Option<&DocLink>,
) -> Result<String, String> {
    let ends_with_newline = content.ends_with('\n');
    let mut lines = to_owned_lines(content);

    let (p_start, p_end) = section_body(&lines, "## Pending")
        .ok_or("docs/todo.md has no '## Pending' section")?;
    let b = (p_start..p_end)
        .find(|&i| {
            lines[i].starts_with("- ")
                && parse_slug(&lines[i]).as_deref() == Some(slug.as_str())
        })
        .ok_or_else(|| {
            format!("no pending todo with slug '{}'", slug.as_str())
        })?;
    let b_end = block_end(&lines, b, p_end);

    // Read the summary from the whole block, not just the first
    // line: a linked entry keeps it on a continuation, and the
    // splice below destroys whatever is not read here.
    let done_summary =
        summary.map_or_else(|| block_summary(&lines, b, b_end), str::to_owned);
    // Checked here rather than on the flag, because this is
    // where the flag and the fallback meet. Guarding the flag
    // alone let the fallback through: an entry with no summary
    // to inherit produced `- **slug** -- `, the contentless stub
    // `require_nonempty` exists to prevent.
    require_nonempty("todo --summary", &done_summary)?;

    // Remove the pending block (and the blank lines after it, up
    // to the next bullet/heading).
    lines.splice(b..b_end, std::iter::empty());

    // Build and insert the Done entry at the top of `## Done`.
    let (d_start, _) = section_body(&lines, "## Done")
        .ok_or("docs/todo.md has no '## Done' section")?;
    let mut entry = match doc {
        Some(doc) => vec![
            format!("- [**{}**]({})", slug.as_str(), doc.as_str()),
            continuation_line(&done_summary)?,
        ],
        None => {
            vec![
                pending_line(&format!("**{}**", slug.as_str()), &done_summary)
                    .map_err(|e| {
                        format!(
                            "{e}; pass a shorter --summary, or a --doc, which \
                         moves the summary onto its own line"
                        )
                    })?,
            ]
        }
    };
    entry.push(format!("  ({})", date.as_str()));
    // `d_start` is the line after "## Done"; if it is the
    // customary blank, insert past it so we keep heading + blank.
    let (at, prepend_blank) =
        if lines.get(d_start).is_some_and(|l| l.trim().is_empty()) {
            (d_start + 1, false)
        } else {
            (d_start, true)
        };
    let mut ins = Vec::new();
    if prepend_blank {
        ins.push(String::new());
    }
    ins.extend(entry);
    ins.push(String::new());
    lines.splice(at..at, ins);

    Ok(rejoin(&lines, ends_with_newline))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# TODO

## Pending

- **alpha-task** -- do alpha
  more about alpha

- **beta-task** -- do beta

## Done

- [**old-task**](issues/old-task.md)
  -- did old
  (2026-01-01)
";

    /// A file mixing the two bullet spellings: hand-written
    /// entries use backticks, `todo add` writes bold.
    const MIXED: &str = "\
# TODO

## Pending

- `hand-written` -- typed by a human
  with a continuation line
- `second-hand` -- also typed
- **generated** -- written by todo add

## Done

- [**old-task**](issues/old-task.md)
  -- did old
  (2026-01-01)
";

    #[test]
    fn parse_slug_accepts_a_backticked_slug() {
        assert_eq!(
            parse_slug("- `hand-written` -- typed by a human").as_deref(),
            Some("hand-written")
        );
    }

    #[test]
    fn parse_slug_ignores_backticked_code_without_a_summary() {
        // Inline code in a prose bullet is not a slug.
        assert_eq!(parse_slug("- `cargo xtask todo` is the entry point"), None);
    }

    #[test]
    fn list_does_not_silently_drop_backticked_entries() {
        // The defect: hand-written entries were invisible to
        // `todo list`, so it reported an incomplete queue as
        // complete.
        let got = parse_section(MIXED, "## Pending");
        let slugs: Vec<&str> = got.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(slugs, vec!["hand-written", "second-hand", "generated"]);
        assert_eq!(got[0].1, "typed by a human");
    }

    #[test]
    fn slug_exists_finds_a_backticked_entry() {
        assert!(slug_exists(MIXED, "hand-written"));
        assert!(slug_exists(MIXED, "generated"));
    }

    #[test]
    fn check_slug_free_rejects_a_backticked_collision() {
        // Without backtick support this would allow a duplicate.
        let err = check_slug_free(MIXED, "second-hand").unwrap_err();
        assert!(err.contains("second-hand"), "got: {err}");
        assert!(err.contains("already exists"), "got: {err}");
        assert!(check_slug_free(MIXED, "brand-new").is_ok());
    }

    #[test]
    fn parse_slug_rejects_prose_in_inline_code() {
        // The delimiters alone are too weak a guard: a sentence
        // using inline code has the same shape as an entry.
        assert_eq!(parse_slug("- `bombyx.toml` -- lives at the root"), None);
        assert_eq!(parse_slug("- `cargo xtask deploy` -- missing"), None);
        assert_eq!(parse_slug("- `` -- x"), None);
        assert_eq!(parse_slug("- **Not A Slug** -- x"), None);
    }

    #[test]
    fn summary_line_collapses_interior_whitespace() {
        // A newline would otherwise be written verbatim and
        // splice a second bullet into the file.
        let got = summary_line("- **s** --", "a\n- **ghost** -- injected", 80)
            .unwrap();
        assert_eq!(got.lines().count(), 1, "must be one line: {got:?}");
        assert_eq!(got, "- **s** -- a - **ghost** -- injected");
    }

    #[test]
    fn issue_bullet_uses_two_lines_so_a_long_slug_still_fits() {
        // The linked label carries the slug twice and leaves no
        // room for a summary beside it.
        let slug = "todo-tooling-format-mismatch";
        let got = bullet_lines(slug, "a real summary", true).unwrap();
        assert_eq!(
            got,
            vec![
                format!("- [**{slug}**](issues/{slug}.md)"),
                "  -- a real summary".to_owned(),
            ]
        );
        // And the shape round-trips through the reader.
        let doc = format!("## Pending\n\n{}\n{}\n\n## Done\n", got[0], got[1]);
        assert_eq!(
            parse_section(&doc, "## Pending"),
            vec![(slug.to_owned(), "a real summary".to_owned())]
        );
    }

    #[test]
    fn plain_bullet_stays_on_one_line() {
        let got = bullet_lines("short", "a summary", false).unwrap();
        assert_eq!(got, vec!["- **short** -- a summary".to_owned()]);
    }

    #[test]
    fn move_to_done_finds_a_backticked_pending_entry() {
        let out = move_to_done(
            MIXED,
            &Slug::new("todo --slug", "hand-written").unwrap(),
            &DoneDate::new("2026-08-10").unwrap(),
            None,
            None,
        )
        .unwrap();
        assert!(!out.contains("- `hand-written`"));
        assert!(!out.contains("with a continuation line"));
        // No `doc`, so the entry is the one-line shape and the
        // summary sits beside the slug rather than below it.
        assert!(out.contains("- **hand-written** -- typed by a human"));
        assert!(out.contains("  (2026-08-10)"));
    }

    #[test]
    fn summary_line_keeps_a_short_summary_on_one_line() {
        let got = summary_line("- **slug** --", "short and sweet", 80).unwrap();
        assert_eq!(got, "- **slug** -- short and sweet");
    }

    #[test]
    fn summary_line_refuses_a_summary_that_would_wrap() {
        // A wrapped summary is indistinguishable from the first
        // body line when read back, so it must be refused at
        // write time rather than silently truncated at read time.
        let lead = "- **slug** --";
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
        assert!(summary_line("- **s** --", &s, 40).is_ok());
    }

    #[test]
    fn parses_pending_slugs_and_summaries() {
        let got = parse_section(SAMPLE, "## Pending");
        assert_eq!(
            got,
            vec![
                ("alpha-task".to_owned(), "do alpha".to_owned()),
                ("beta-task".to_owned(), "do beta".to_owned()),
            ]
        );
    }

    #[test]
    fn parses_linked_done_slug_with_continuation_summary() {
        // The Done summary lives on the `  -- ` continuation
        // line; `list --done` must surface it, not a bare slug.
        let got = parse_section(SAMPLE, "## Done");
        assert_eq!(got, vec![("old-task".to_owned(), "did old".to_owned())]);
    }

    #[test]
    fn slug_exists_across_sections() {
        assert!(slug_exists(SAMPLE, "alpha-task"));
        assert!(slug_exists(SAMPLE, "old-task"));
        assert!(!slug_exists(SAMPLE, "missing"));
    }

    #[test]
    fn add_pending_appends_after_last_bullet() {
        // Built the way `add` builds it, so the placement logic
        // is verified against a bullet production can emit.
        let bullet = bullet_lines("gamma", "do gamma", false).unwrap();
        let out = add_pending(SAMPLE, bullet).unwrap();
        // Lands after beta, before the Done heading.
        let gamma = out.find("- **gamma** -- do gamma").unwrap();
        let done = out.find("## Done").unwrap();
        let beta = out.find("- **beta-task**").unwrap();
        assert!(beta < gamma && gamma < done);
        // Blank line separates it from beta.
        assert!(
            out.contains(
                "- **beta-task** -- do beta\n\n- **gamma** -- do gamma"
            )
        );
    }

    #[test]
    fn move_to_done_moves_multiline_block_and_stamps_date() {
        let out = move_to_done(
            SAMPLE,
            &Slug::new("todo --slug", "alpha-task").unwrap(),
            &DoneDate::new("2026-07-23").unwrap(),
            None,
            Some(&DocLink::unchecked("issues/alpha-task.md")),
        )
        .unwrap();
        // Gone from Pending (including its body line).
        assert!(!out.contains("- **alpha-task** -- do alpha"));
        assert!(!out.contains("more about alpha"));
        // Present at the top of Done in the project convention:
        // link line, `  -- summary` continuation, trailing date.
        assert!(out.contains(
            "## Done\n\n- [**alpha-task**](issues/alpha-task.md)\n  -- do alpha\n  (2026-07-23)"
        ));
        let alpha = out.find("[**alpha-task**]").unwrap();
        let old = out.find("[**old-task**]").unwrap();
        assert!(alpha < old, "newest-first: alpha above old");
        // Beta remains pending.
        assert!(out.contains("- **beta-task** -- do beta"));
    }

    #[test]
    fn move_to_done_uses_summary_override() {
        let out = move_to_done(
            SAMPLE,
            &Slug::new("todo --slug", "beta-task").unwrap(),
            &DoneDate::new("2026-07-23").unwrap(),
            Some("a curated done summary"),
            Some(&DocLink::unchecked("issues/beta-task.md")),
        )
        .unwrap();
        assert!(out.contains("  -- a curated done summary\n  (2026-07-23)"));
    }

    #[test]
    fn move_to_done_without_a_doc_writes_no_link() {
        // The case that has fired twice: an item completed
        // through `/issue` whose plan is shared with six other
        // steps, so `issues/<slug>.md` names nothing.
        let out = move_to_done(
            SAMPLE,
            &Slug::new("todo --slug", "alpha-task").unwrap(),
            &DoneDate::new("2026-07-23").unwrap(),
            None,
            None,
        )
        .unwrap();
        assert!(
            !out.contains("[**alpha-task**]"),
            "no link may be written: {out}"
        );
        // One line, not the two-line label-plus-continuation
        // shape. `raw_slug` requires ` -- ` on the same line as a
        // bold slug, so splitting it here would make the entry
        // invisible to `todo list --done` and let `todo add`
        // mint a duplicate.
        assert!(
            out.contains(
                "## Done\n\n- **alpha-task** -- do alpha\n  (2026-07-23)"
            ),
            "{out}"
        );
        // Read back by the file's own parser, which is the
        // property the shape exists for.
        let done = parse_section(&out, "## Done");
        assert!(
            done.iter().any(|(slug, _)| slug == "alpha-task"),
            "parser must find it: {done:?}"
        );
    }

    #[test]
    fn move_to_done_links_the_document_it_is_given() {
        // A shared plan, not `issues/<slug>.md`.
        let out = move_to_done(
            SAMPLE,
            &Slug::new("todo --slug", "alpha-task").unwrap(),
            &DoneDate::new("2026-07-23").unwrap(),
            None,
            Some(&DocLink::unchecked("issues/project-config-off-repo.md")),
        )
        .unwrap();
        assert!(
            out.contains(
                "- [**alpha-task**](issues/project-config-off-repo.md)\n  -- do alpha\n  (2026-07-23)"
            ),
            "{out}"
        );
        let done = parse_section(&out, "## Done");
        assert!(
            done.iter().any(|(slug, _)| slug == "alpha-task"),
            "parser must find it: {done:?}"
        );
    }

    /// A `docs/`-shaped fixture: a repo root with one document
    /// under it.
    ///
    /// Built under `target/` rather than resolved against the
    /// real `docs/`, so renaming a plan file cannot break a
    /// `todo` unit test for reasons unrelated to `todo`.
    fn docs_fixture() -> std::path::PathBuf {
        let root = workspace_root().join("target/todo-doclink-fixture");
        let docs = root.join("docs");
        std::fs::create_dir_all(docs.join("issues")).unwrap();
        std::fs::write(docs.join("issues/plan.md"), "x\n").unwrap();
        std::fs::write(root.join("README.md"), "x\n").unwrap();
        docs
    }

    #[test]
    fn doclink_accepts_only_a_target_every_reader_can_follow() {
        let docs = docs_fixture();

        // A document beside the file, and one a level up: both
        // resolve from `docs/todo.md` on anybody's machine.
        assert_eq!(
            DocLink::new(&docs, "issues/plan.md").unwrap().as_str(),
            "issues/plan.md"
        );
        assert!(DocLink::new(&docs, "../README.md").is_ok());

        // The whole family, each asserting the reason it was
        // refused rather than merely that something was. A
        // shared assertion would pass with every branch wrong.
        for (rel, needle) in [
            ("", "blank"),
            ("   ", "blank"),
            ("/etc/passwd", "rooted"),
            ("/docs/plan.md", "rooted"),
            ("C:\\docs\\plan.md", "rooted"),
            ("\\\\server\\share", "rooted"),
            // Escapes the repository. It resolves on the author's
            // machine and nowhere else, which is the defect the
            // existence check alone cannot see.
            ("../../../../etc/passwd", "outside"),
            ("issues/../../../etc/passwd", "outside"),
            // Everything outside the allowed set, whatever the
            // reason it is unwelcome. Listing what is banned is
            // what fell behind twice: a backslash was added to
            // the list in one round and the descriptions of the
            // list were wrong by the next.
            ("issues/my plan.md", "not a path this can link to"),
            ("issues/a(b).md", "not a path this can link to"),
            ("issues/a)b.md", "not a path this can link to"),
            // A backslash is a literal in markdown, and on
            // Windows `Path::join` would make it a real
            // separator, so the host would decide -- the
            // mistake the rooted rule exists to avoid.
            ("issues\\plan.md", "not a path this can link to"),
            ("<x>.md", "not a path this can link to"),
            // These the ban list allowed. None of them belongs
            // in a path to a file in this repository, and an
            // allowed set refuses them without anybody having
            // thought of them first.
            ("issues/plan.md#heading", "not a path this can link to"),
            ("issues/plan.md?raw=1", "not a path this can link to"),
            ("issues/my%20plan.md", "not a path this can link to"),
            ("issues/plan\u{7f}.md", "not a path this can link to"),
            // Nothing there, or not a file.
            ("issues/absent.md", "names no file"),
            ("issues", "names no file"),
        ] {
            let err = DocLink::new(&docs, rel).unwrap_err();
            assert!(
                err.contains("--doc"),
                "{rel:?}: the error must name the flag: {err}"
            );
            assert!(
                err.contains(needle),
                "{rel:?}: expected the {needle:?} reason, got: {err}"
            );
        }
    }

    #[test]
    fn every_argument_reaching_the_file_is_refused_a_newline() {
        // `--doc` was guarded and its three neighbours were not,
        // though all four are interpolated into `docs/todo.md`.
        // A newline in any of them fabricates a bullet the
        // parser accepts as a real entry, so `todo list` reports
        // work nobody did and `check_slug_free` reserves that
        // slug for ever. Demonstrated on the real file for
        // `--date` and `--slug` before this test was written.
        let splice = "x\n- **ghost** -- injected";

        assert!(Slug::new("todo --slug", splice).is_err(), "slug");
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

        assert!(DoneDate::new(splice).is_err(), "date");
        assert!(DoneDate::new("2026-9-4").is_err(), "must be zero-padded");
        assert!(DoneDate::new("not-a-date").is_err());
        assert!(DoneDate::new("2026-09-04 ").is_err(), "trailing space");
        assert_eq!(DoneDate::new("2026-09-04").unwrap().as_str(), "2026-09-04");
    }

    #[test]
    fn the_no_link_branch_does_not_name_a_flag_done_lacks() {
        // Reachable, not theoretical: a linked pending entry
        // keeps its summary on a continuation, budget 75. Moving
        // it to Done *without* `--doc` needs the whole entry on
        // one line, so the budget drops to 73 minus the slug. An
        // entry legal as Pending can therefore fail here, and
        // the message was telling the operator to reach for
        // `--body`, which `done` does not have.
        let slug = "todo-tooling-format-mismatch";
        let src = format!(
            "# TODO\n\n## Pending\n\n- [**{slug}**](issues/{slug}.md)\n  -- {}\n\n## Done\n",
            "x".repeat(60)
        );
        let err = move_to_done(
            &src,
            &Slug::new("todo --slug", slug).unwrap(),
            &DoneDate::new("2026-09-04").unwrap(),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("too long"), "got: {err}");
        assert!(!err.contains("--body"), "done has no --body: {err}");
        assert!(err.contains("--summary"), "must name a real way out: {err}");
    }

    #[test]
    fn done_refuses_an_empty_summary_however_it_arrives() {
        // The guard was on the flag, so omitting the flag walked
        // past it: `move_to_done` falls back to the entry's own
        // summary, and a linked bullet with no ` -- ` anywhere
        // has none. `raw_slug`'s link branch accepts that first
        // line without requiring the separator, so such an entry
        // parses and then completes as `- **foo** -- ` with a
        // trailing space -- the contentless stub
        // `require_nonempty` exists to prevent.
        let src = "\
# TODO

## Pending

- [**foo**](issues/foo.md)
  some body text with no summary separator

## Done
";
        let err = move_to_done(
            src,
            &Slug::new("todo --slug", "foo").unwrap(),
            &DoneDate::new("2026-09-04").unwrap(),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("--summary"), "got: {err}");
    }

    #[test]
    fn done_refuses_a_blank_summary_flag() {
        // The other half of the test above: an explicit blank
        // flag, rather than a fallback with nothing to inherit.
        // Both meet at `done_summary`, which is why one check
        // there covers both.
        let src = "\
# TODO

## Pending

- **foo** -- a real summary

## Done
";
        let err = move_to_done(
            src,
            &Slug::new("todo --slug", "foo").unwrap(),
            &DoneDate::new("2026-09-04").unwrap(),
            Some("   "),
            None,
        )
        .unwrap_err();
        assert!(err.contains("--summary"), "got: {err}");
    }

    #[test]
    fn done_checks_its_slug_and_date_before_any_io() {
        // These two can be judged from the arguments alone, so
        // they fail the same way with or without a readable
        // docs/todo.md. The summary cannot: it may come from the
        // file, so `move_to_done` owns it -- see
        // `done_refuses_an_empty_summary_however_it_arrives`.
        // `done` takes its slug positionally, so the message
        // must not send the operator looking for a `--slug`
        // flag. `add` has one; this subcommand does not. Same
        // defect as RT-4 one round earlier, from the other
        // shared helper.
        let err = done_cmd("Not A Slug", None, "2026-09-04", None).unwrap_err();
        assert!(err.contains("todo done"), "got: {err}");
        assert!(!err.contains("--slug"), "done has no --slug: {err}");
        assert!(
            done_cmd("fine-slug", None, "yesterday", None)
                .unwrap_err()
                .contains("--date")
        );
    }

    #[test]
    fn move_to_done_keeps_a_two_line_entrys_summary() {
        // `add --issue` writes the summary on a continuation
        // line, because the linked label leaves no room beside
        // it. Reading only the bullet's first line finds no
        // ` -- ` there and returns an empty summary, and the
        // continuation is then spliced away with the rest of the
        // block -- so the text is gone, not merely unread.
        // `parse_section` already scans the block for it; these
        // two must agree.
        let src = "\
# TODO

## Pending

- [**linked**](issues/linked.md)
  -- a summary that must survive

## Done
";
        let link = DocLink::unchecked("issues/plan.md");
        for doc in [None, Some(&link)] {
            let out = move_to_done(
                src,
                &Slug::new("todo --slug", "linked").unwrap(),
                &DoneDate::new("2026-09-04").unwrap(),
                None,
                doc,
            )
            .unwrap();
            assert!(
                out.contains("a summary that must survive"),
                "doc={doc:?} lost the summary: {out}"
            );
            // And the file's own reader finds it, which is what
            // `todo list --done` reports.
            let done = parse_section(&out, "## Done");
            assert_eq!(
                done,
                vec![(
                    "linked".to_owned(),
                    "a summary that must survive".to_owned()
                )],
                "doc={doc:?}"
            );
        }
    }

    #[test]
    fn move_to_done_errors_on_unknown_slug() {
        let err = move_to_done(
            SAMPLE,
            &Slug::new("todo --slug", "nope").unwrap(),
            &DoneDate::new("2026-07-23").unwrap(),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn add_rejects_an_overlong_summary_before_any_io() {
        // Argument errors must not depend on docs/todo.md being
        // readable, so the render happens before the read.
        let err = add("real-slug", &"x".repeat(200), None, false).unwrap_err();
        assert!(err.contains("too long"), "got: {err}");
        assert!(err.contains("--body"), "caller hint missing: {err}");
    }

    #[test]
    fn add_rejects_blank_summary_before_any_io() {
        // The guard fires before `read_todo`, so this never
        // touches the real `docs/todo.md`.
        let err = add("real-slug", "   ", None, false).unwrap_err();
        assert!(err.contains("--summary"));
    }

    #[test]
    fn parse_slug_ignores_a_non_slug_bold_bullet() {
        // A bullet that only uses ** for emphasis is not a slug.
        assert_eq!(parse_slug("- **NOTE:** grouped by area"), None);
        assert_eq!(
            parse_slug("- **real-slug** -- x").as_deref(),
            Some("real-slug")
        );
        assert_eq!(
            parse_slug("- [**linked-slug**](issues/linked-slug.md) -- y")
                .as_deref(),
            Some("linked-slug")
        );
    }
}
