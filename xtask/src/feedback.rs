//! `cargo xtask feedback-add` -- the deterministic appender for
//! `docs/developer/template-feedback.md`.
//!
//! `/template-improve` used to hand-edit the feedback file,
//! which meant the LLM owned entry placement, date stamping,
//! and dedup -- all mechanical work that drifts. This command
//! owns that: it mints a stable `tf-<yyyy-mm-dd>-<slug>` ID
//! (mirroring the `rt-`/`aq-` log convention in
//! `docs/developer/`), inserts the entry at the top of the
//! chosen section, and skips a re-add of an ID already present.
//! The LLM supplies only the judgement: which section, the
//! title, and the pre-wrapped body prose.

use std::fs;
use std::io::Read as _;
use std::path::PathBuf;

use clap::ValueEnum;

use crate::helpers::{FEEDBACK_REL, is_fence, today_iso, workspace_root};

/// Which lifecycle section of the feedback file an entry
/// belongs in. clap renders these as `open` / `resolved` /
/// `suggestion` and rejects anything else at the CLI boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum FeedbackSection {
    /// Known-suboptimal / pending template issues.
    Open,
    /// Entries closed out by a fix.
    Resolved,
    /// Ideas a derived project wants pushed upstream.
    Suggestion,
}

impl FeedbackSection {
    /// Lowercase substring that identifies this section's `##`
    /// header in the feedback file.
    fn header_keyword(self) -> &'static str {
        match self {
            FeedbackSection::Open => "open divergences",
            FeedbackSection::Resolved => "resolved",
            FeedbackSection::Suggestion => "suggestions to flow back",
        }
    }
}

/// Turn a title into an ID slug: lowercase ASCII alphanumerics,
/// every other run collapsed to a single `-`, trimmed, capped
/// at 48 chars on a `-` boundary. Empty titles slug to `entry`.
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !slug.is_empty() && !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.len() > 48 {
        slug.truncate(48);
        while slug.ends_with('-') {
            slug.pop();
        }
    }
    if slug.is_empty() {
        slug.push_str("entry");
    }
    slug
}

/// Mint the `tf-<date>-<slug>` entry ID.
fn make_id(date: &str, title: &str) -> String {
    format!("tf-{date}-{}", slugify(title))
}

/// Render an entry block: `### <id> -- <title>` header, a blank
/// line, then the trimmed body.
fn format_entry(id: &str, title: &str, body: &str) -> String {
    format!("### {id} -- {title}\n\n{}", body.trim_end())
}

/// True when `md` already contains an entry with exactly this
/// ID. Matches the entry header line (`### <id>` alone or
/// `### <id> -- ...`), not a bare substring, so an ID that is a
/// prefix of a different same-day ID -- or that appears inside
/// some entry's body prose -- does not spuriously suppress the
/// add (RT-3).
fn has_entry(md: &str, id: &str) -> bool {
    md.lines().any(|l| {
        l.strip_prefix("### ").is_some_and(|rest| {
            rest == id || rest.starts_with(&format!("{id} "))
        })
    })
}

/// True when `line` is the level-2 section header carrying `kw`.
fn is_target_section(line: &str, kw: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("## ") && t.to_ascii_lowercase().contains(kw)
}

/// Index of the first section header matching `kw`, skipping
/// any `## `-looking line inside a fenced code block so a code
/// sample cannot be chosen as the insertion point (RT-4).
fn find_section(lines: &[&str], kw: &str) -> Option<usize> {
    let mut in_fence = false;
    for (idx, l) in lines.iter().enumerate() {
        if is_fence(l) {
            in_fence = !in_fence;
        } else if !in_fence && is_target_section(l, kw) {
            return Some(idx);
        }
    }
    None
}

/// Insert `block` at the top of the section identified by `kw`,
/// just below its header (past a single blank line if present)
/// and above the section's existing entries. Pure. Errors when
/// no matching section header is found.
fn insert_entry(md: &str, kw: &str, block: &str) -> Result<String, String> {
    let lines: Vec<&str> = md.lines().collect();
    let sec = find_section(&lines, kw).ok_or_else(|| {
        format!("no `## ` section header matching {kw:?} found")
    })?;

    let mut ins = sec + 1;
    if lines.get(ins).is_some_and(|l| l.trim().is_empty()) {
        ins += 1;
    }

    let mut out: Vec<&str> = lines[..ins].to_vec();
    let block_lines: Vec<&str> = block.lines().collect();
    out.extend(&block_lines);
    out.push("");
    out.extend(&lines[ins..]);
    Ok(out.join("\n") + "\n")
}

/// Absolute path of this template's feedback file.
fn feedback_path() -> PathBuf {
    workspace_root().join(FEEDBACK_REL)
}

/// Read the entry body from `--body-file` or, when absent, from
/// stdin.
fn read_body(body_file: Option<&str>) -> Result<String, String> {
    if let Some(path) = body_file {
        return fs::read_to_string(path)
            .map_err(|e| format!("cannot read --body-file {path}: {e}"));
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("cannot read body from stdin: {e}"))?;
    Ok(buf)
}

/// `cargo xtask feedback-add --section <open|resolved|suggestion>
/// --title <title> [--body-file <path>]` -- append a
/// feedback entry with a minted ID. Body comes from
/// `--body-file` or stdin. Idempotent: an ID already present is
/// left untouched.
pub fn feedback_add(
    section: FeedbackSection,
    title: &str,
    body_file: Option<&str>,
) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("--title must not be empty".to_string());
    }
    let body = read_body(body_file)?;
    let path = feedback_path();
    let md = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let id = make_id(&today_iso(), title);
    if has_entry(&md, &id) {
        eprintln!("feedback-add: {id} already present -- skipped");
        return Ok(());
    }

    let block = format_entry(&id, title.trim(), &body);
    let updated = insert_entry(&md, section.header_keyword(), &block)?;
    fs::write(&path, updated)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    eprintln!("feedback-add: added {id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_normalizes_titles() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("  spaced   out  "), "spaced-out");
        assert_eq!(slugify("CamelCase_and.dots"), "camelcase-and-dots");
        assert_eq!(slugify("!!!"), "entry");
    }

    #[test]
    fn slugify_caps_length_on_dash_boundary() {
        let title = "a very long title that keeps going and going far past \
                     the cap";
        let slug = slugify(title);
        assert!(slug.len() <= 48);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn make_id_combines_date_and_slug() {
        assert_eq!(
            make_id("2026-07-16", "Something Broke"),
            "tf-2026-07-16-something-broke"
        );
    }

    #[test]
    fn format_entry_has_header_and_trimmed_body() {
        let e = format_entry("tf-x", "Title", "body text\n\n\n");
        assert_eq!(e, "### tf-x -- Title\n\nbody text");
    }

    const DOC: &str = "\
# Template feedback

## Open divergences

### existing-open -- old open entry

Body.

## Resolved

### existing-resolved -- old resolved entry

Body.

## Suggestions to flow back to the template

Nothing yet.
";

    #[test]
    fn insert_entry_places_at_section_top() {
        let block = "### tf-new -- New Entry\n\nNew body.";
        let out = insert_entry(DOC, "resolved", block).unwrap();
        let new_pos = out.find("tf-new").unwrap();
        let old_pos = out.find("existing-resolved").unwrap();
        let open_pos = out.find("existing-open").unwrap();
        // New entry precedes the existing Resolved entry...
        assert!(new_pos < old_pos);
        // ...but comes after the (untouched) Open section.
        assert!(open_pos < new_pos);
    }

    #[test]
    fn insert_entry_targets_the_right_section() {
        let block = "### tf-new -- New Entry\n\nBody.";
        let out = insert_entry(DOC, "open divergences", block).unwrap();
        let new_pos = out.find("tf-new").unwrap();
        let resolved_hdr = out.find("## Resolved").unwrap();
        // Inserted into Open, so it sits above the Resolved header.
        assert!(new_pos < resolved_hdr);
    }

    #[test]
    fn find_section_skips_headers_in_code_fences() {
        // A `## Resolved` line inside a code fence must not be
        // chosen as the section (RT-4).
        let md = "\
## Open divergences

### existing -- entry

```
## Resolved
```

## Resolved

### real -- entry
";
        let lines: Vec<&str> = md.lines().collect();
        let idx = find_section(&lines, "resolved").unwrap();
        // The real header, not the fenced one.
        assert_eq!(lines[idx], "## Resolved");
        assert!(idx > 5);
    }

    #[test]
    fn has_entry_matches_full_id_not_prefix() {
        let md = "### tf-2026-07-16-cache-warmup-fix -- a\n\nbody\n";
        // Exact ID present.
        assert!(has_entry(md, "tf-2026-07-16-cache-warmup-fix"));
        // A prefix of the present ID must NOT count as present.
        assert!(!has_entry(md, "tf-2026-07-16-cache-warmup"));
        // An ID appearing only in body prose must not count.
        let body = "### other -- x\n\nsee tf-2026-07-16-note here\n";
        assert!(!has_entry(body, "tf-2026-07-16-note"));
    }

    #[test]
    fn insert_entry_errors_on_missing_section() {
        let err = insert_entry(DOC, "nonexistent", "x").unwrap_err();
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn insert_entry_keeps_trailing_newline_and_is_idempotent_shape() {
        let block = "### tf-new -- E\n\nB.";
        let out = insert_entry(DOC, "resolved", block).unwrap();
        assert!(out.ends_with('\n'));
        // The header line is preserved exactly once.
        assert_eq!(out.matches("## Resolved").count(), 1);
    }

    #[test]
    fn is_target_section_matches_level_two_only() {
        assert!(is_target_section("## Resolved", "resolved"));
        assert!(is_target_section("## Open divergences", "open"));
        // Entry-level headers are not section headers.
        assert!(!is_target_section("### tf-x -- resolved thing", "resolved"));
        assert!(!is_target_section("plain text", "resolved"));
    }
}
