//! `todo list` / `todo add` / `todo done`: read and update
//! `docs/todo.md` without loading the whole (large) file into an
//! editor's context.
//!
//! - `list` prints the pending entries as `slug -- summary`, so
//!   a caller can see what is queued cheaply.
//! - `add` appends a new bullet under `## Pending`, refusing a
//!   slug that already exists and a summary that would not fit
//!   on one line.
//! - `done` removes a pending entry. The queue holds live work
//!   only; what shipped is recorded by the commit and git
//!   history, so there is no `## Done` section.
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
    List,
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
            issue,
        } => add(&slug, &summary, body.as_deref(), issue),
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
    for (slug, summary) in parse_section(&content, "## Pending") {
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

fn done_cmd(slug: &str) -> Result<(), String> {
    // The slug is checked before `read_todo`, so a bad one fails
    // the same way with or without a readable `docs/todo.md`.
    let slug = Slug::new("todo done <slug>", slug)?;
    let content = read_todo()?;
    let updated = remove_pending(&content, &slug)?;
    write_todo(&updated)?;
    println!("Removed '{}' from the queue.", slug.as_str());
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
/// key `remove_pending` searches for -- so a newline in one
/// writes a second bullet the parser accepts as a real entry
/// and leaves the original truncated.
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
/// `` - `config.toml` -- lives outside the repo ``, is
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
/// A linked (`--issue`) entry keeps its summary on a `  -- `
/// continuation line rather than beside the slug, so a reader
/// that stops at the first line would report a bare slug.
/// `parse_section` calls this so `todo list` surfaces the
/// summary either way.
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
/// summary at all. Those take a two-line shape -- label alone,
/// then a `  -- summary` continuation -- which `parse_section`
/// already reads. `add` derives `issues/<slug>.md` and does not
/// check that it exists: it captures an item *before* its spec
/// is written, so the target legitimately may not exist yet.
/// Whether the flag should take a path instead, or go, is open
/// -- `add-issue-flag-unused` in `docs/todo.md`.
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

/// Remove the pending bullet for `slug` from `## Pending`.
///
/// The queue holds live work only. Completing an item takes it
/// off the list; what shipped is recorded by the commit, the
/// CHANGELOG and git history, so there is no `## Done` section to
/// move the entry into. [`block_end`] finds where the bullet's
/// body stops, so a multi-line entry is removed whole rather than
/// leaving its continuation behind.
///
/// `slug` arrives as a [`Slug`] rather than a `&str` because it is
/// matched against [`parse_slug`]'s output; the type is the proof
/// the value has the one shape the file writes.
fn remove_pending(content: &str, slug: &Slug) -> Result<String, String> {
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
    lines.splice(b..b_end, std::iter::empty());
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
        assert_eq!(
            parse_slug("- `config.toml` -- lives outside the repo"),
            None
        );
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
        let doc = format!("## Pending\n\n{}\n{}\n", got[0], got[1]);
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
    fn parses_a_linked_pending_entrys_continuation_summary() {
        // An `--issue` entry keeps its summary on a `  -- `
        // continuation line, so `block_summary` must scan the
        // block rather than read only the first line; otherwise
        // `todo list` reports a bare slug.
        let src = "\
# TODO

## Pending

- [**linked-task**](issues/linked-task.md)
  -- do linked
";
        assert_eq!(
            parse_section(src, "## Pending"),
            vec![("linked-task".to_owned(), "do linked".to_owned())]
        );
    }

    #[test]
    fn slug_exists_finds_a_pending_entry() {
        assert!(slug_exists(SAMPLE, "alpha-task"));
        assert!(slug_exists(SAMPLE, "beta-task"));
        assert!(!slug_exists(SAMPLE, "missing"));
    }

    #[test]
    fn add_pending_appends_after_last_bullet() {
        // Built the way `add` builds it, so the placement logic
        // is verified against a bullet production can emit.
        let bullet = bullet_lines("gamma", "do gamma", false).unwrap();
        let out = add_pending(SAMPLE, bullet).unwrap();
        // Lands after beta, the last pending bullet.
        let gamma = out.find("- **gamma** -- do gamma").unwrap();
        let beta = out.find("- **beta-task**").unwrap();
        assert!(beta < gamma);
        // Blank line separates it from beta.
        assert!(
            out.contains(
                "- **beta-task** -- do beta\n\n- **gamma** -- do gamma"
            )
        );
    }

    #[test]
    fn remove_pending_removes_a_single_line_entry() {
        let out = remove_pending(
            SAMPLE,
            &Slug::new("todo done <slug>", "beta-task").unwrap(),
        )
        .unwrap();
        assert!(!out.contains("beta-task"), "beta gone: {out}");
        // Alpha and its body untouched.
        assert!(out.contains("- **alpha-task** -- do alpha"));
        assert!(out.contains("more about alpha"));
    }

    #[test]
    fn remove_pending_removes_a_multiline_block() {
        // The queue holds live work only, so completing an item
        // takes the whole bullet -- summary line and body -- off
        // the list, leaving no orphaned continuation behind.
        let out = remove_pending(
            SAMPLE,
            &Slug::new("todo done <slug>", "alpha-task").unwrap(),
        )
        .unwrap();
        assert!(!out.contains("- **alpha-task** -- do alpha"));
        assert!(!out.contains("more about alpha"), "body gone: {out}");
        // Beta remains.
        assert!(out.contains("- **beta-task** -- do beta"));
    }

    #[test]
    fn remove_pending_finds_a_backticked_entry() {
        // Hand-written entries use backticks; `done` must find
        // them, not only the bold ones `add` writes.
        let out = remove_pending(
            MIXED,
            &Slug::new("todo done <slug>", "hand-written").unwrap(),
        )
        .unwrap();
        assert!(!out.contains("hand-written"), "gone: {out}");
        assert!(
            !out.contains("with a continuation line"),
            "body gone: {out}"
        );
        assert!(out.contains("- `second-hand`"), "sibling kept: {out}");
    }

    #[test]
    fn remove_pending_errors_on_unknown_slug() {
        let err = remove_pending(
            SAMPLE,
            &Slug::new("todo done <slug>", "nope").unwrap(),
        )
        .unwrap_err();
        assert!(err.contains("nope"), "got: {err}");
    }

    #[test]
    fn slug_new_refuses_a_newline_that_would_splice_a_bullet() {
        // A slug is interpolated into `docs/todo.md`, so a
        // newline in it fabricates a bullet the parser accepts as
        // a real entry -- `todo list` then reports work nobody
        // did and `check_slug_free` reserves that slug for ever.
        let splice = "x\n- **ghost** -- injected";
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
        // docs/todo.md. `done` takes its slug positionally, so
        // the message must not send the operator looking for a
        // `--slug` flag: `add` has one, this subcommand does not.
        let err = done_cmd("Not A Slug").unwrap_err();
        assert!(err.contains("todo done"), "got: {err}");
        assert!(!err.contains("--slug"), "done has no --slug: {err}");
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
