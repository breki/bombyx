//! `todo list` / `todo add` / `todo done`: read and update
//! `docs/todo.md` without loading the whole (large) file into an
//! editor's context.
//!
//! - `list` prints the pending (or done) entries as
//!   `slug -- summary`, so a caller can see what is queued
//!   cheaply.
//! - `add` appends a new bullet under `## Pending`, refusing a
//!   slug that already exists (pending or done).
//! - `done` moves a pending bullet to the top of `## Done`
//!   (newest first), stamping the date and linking it to
//!   `issues/<slug>.md`.
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
        /// One-line summary (<= 80 chars recommended).
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
    let content = read_todo()?;
    if slug_exists(&content, slug) {
        return Err(format!(
            "slug '{slug}' already exists in docs/todo.md; pick another"
        ));
    }
    let label = if issue {
        format!("[**{slug}**](issues/{slug}.md)")
    } else {
        format!("**{slug}**")
    };
    let mut bullet = wrap_markdown(
        &format!("{label} -- {summary}"),
        "- ",
        "  ",
        MARKDOWN_WIDTH,
    );
    if let Some(body) = body {
        bullet.extend(wrap_markdown(body, "  ", "  ", MARKDOWN_WIDTH));
    }
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

/// Slug of a top-level bullet's first line:
/// `- **foo** -- summary` or `- [**foo**](issues/foo.md) …` ->
/// `foo`. The linked form is recognized by its `[**…**](`
/// structure; the plain form additionally requires the `** -- `
/// separator, so a bullet that merely uses `**bold**` for
/// emphasis (e.g. `- **NOTE:** …`) is not mistaken for a slug.
fn parse_slug(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("- [**") {
        let end = rest.find("**")?;
        return Some(rest[..end].to_owned());
    }
    let rest = line.strip_prefix("- **")?;
    let end = rest.find("**")?;
    if rest[end..].starts_with("** -- ") {
        Some(rest[..end].to_owned())
    } else {
        None
    }
}

/// The summary after the ` -- ` separator on a bullet's first
/// line, or empty when absent.
fn parse_summary(line: &str) -> String {
    line.split_once(" -- ")
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
/// `issues/<slug>.md`; the only caller, `/implement`, creates
/// that spec doc before finalising, so the link resolves.
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
    entry.extend(wrap_markdown(
        &format!("-- {done_summary}"),
        "  ",
        "  ",
        MARKDOWN_WIDTH,
    ));
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
        let bullet =
            wrap_markdown("**gamma** -- do gamma", "- ", "  ", MARKDOWN_WIDTH);
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
