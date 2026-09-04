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
//!   (newest first), stamping the date and linking it to
//!   `issues/<slug>.md` whether or not that file exists -- see
//!   `move_to_done`.
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
        } => done_cmd(&slug, summary.as_deref(), &date),
    }
}

fn todo_path() -> std::path::PathBuf {
    workspace_root().join("docs").join("todo.md")
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
    require_nonempty("todo --slug", slug)?;
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
) -> Result<(), String> {
    let content = read_todo()?;
    let updated = move_to_done(&content, slug, date, summary)?;
    write_todo(&updated)?;
    println!("Moved '{slug}' to Done ({date}).");
    Ok(())
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
/// captured text to look like a slug rejects it, and rejects a
/// space-bearing capture that `done` would otherwise splice
/// into a link path.
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
        .map_err(|e| format!("{e}; move the detail into --body"))
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
        let mut summary = parse_summary(&lines[i]);
        let mut j = i + 1;
        while j < end && !lines[j].starts_with("- ") {
            if summary.is_empty()
                && let Some(rest) = lines[j].trim_start().strip_prefix("-- ")
            {
                summary.push_str(rest.trim());
            }
            j += 1;
        }
        out.push((slug, summary));
        i = j;
    }
    out
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
    Ok(vec![pending_line(&format!("**{slug}**"), summary)?])
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
/// in the project's Done convention: the issue link alone on the
/// first line, a `  -- <summary>` continuation, then a trailing
/// `  (<date>)` line. The entry always links to
/// `issues/<slug>.md`, whether or not that document exists.
/// `/implement` calls this and writes that document first, so
/// its links resolve. Nothing else calls it: an item worked
/// through `/issue` is completed by hand, and an item split out
/// of a shared plan has no document of its own, so a link
/// written that way can dangle. `todo-done-link` in
/// `docs/todo.md` holds the fix.
fn move_to_done(
    content: &str,
    slug: &str,
    date: &str,
    summary: Option<&str>,
) -> Result<String, String> {
    let ends_with_newline = content.ends_with('\n');
    let mut lines = to_owned_lines(content);

    let (p_start, p_end) = section_body(&lines, "## Pending")
        .ok_or("docs/todo.md has no '## Pending' section")?;
    let b = (p_start..p_end)
        .find(|&i| {
            lines[i].starts_with("- ")
                && parse_slug(&lines[i]).as_deref() == Some(slug)
        })
        .ok_or_else(|| format!("no pending todo with slug '{slug}'"))?;
    // Block runs to the next top-level bullet or the next H2.
    let block_end = ((b + 1)..p_end)
        .find(|&i| lines[i].starts_with("- ") || lines[i].starts_with("## "))
        .unwrap_or(p_end);

    let done_summary =
        summary.map_or_else(|| parse_summary(&lines[b]), str::to_owned);

    // Remove the pending block (and the blank lines after it, up
    // to the next bullet/heading).
    lines.splice(b..block_end, std::iter::empty());

    // Build and insert the Done entry at the top of `## Done`.
    let (d_start, _) = section_body(&lines, "## Done")
        .ok_or("docs/todo.md has no '## Done' section")?;
    let mut entry = vec![format!("- [**{slug}**](issues/{slug}.md)")];
    entry.push(continuation_line(&done_summary)?);
    entry.push(format!("  ({date})"));
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
        let out =
            move_to_done(MIXED, "hand-written", "2026-08-10", None).unwrap();
        assert!(!out.contains("- `hand-written`"));
        assert!(!out.contains("with a continuation line"));
        assert!(out.contains("  -- typed by a human"));
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
        let out =
            move_to_done(SAMPLE, "alpha-task", "2026-07-23", None).unwrap();
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
            "beta-task",
            "2026-07-23",
            Some("a curated done summary"),
        )
        .unwrap();
        assert!(out.contains("  -- a curated done summary\n  (2026-07-23)"));
    }

    #[test]
    fn move_to_done_errors_on_unknown_slug() {
        let err = move_to_done(SAMPLE, "nope", "2026-07-23", None).unwrap_err();
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
