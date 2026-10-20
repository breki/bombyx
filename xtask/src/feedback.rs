//! `cargo xtask feedback-add` -- the deterministic appender for
//! `docs/developer/template-feedback.md`.
//!
//! Entry placement, date stamping and dedup are mechanical, so
//! this command owns them rather than `/template-improve`'s
//! prose: it mints a stable `tf-<yyyy-mm-dd>-<slug>` ID
//! (mirroring the `rt-`/`aq-` log convention in
//! `docs/developer/`), inserts the entry at the top of the
//! chosen section, and skips a re-add of an ID already present.
//! The LLM supplies only the judgement: which section, the
//! title, and the pre-wrapped body prose.

use std::fs;
use std::io::Read as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use rustbase_xtask::markdown::rejoin_like;

use crate::helpers::workspace_root;

/// Repo-relative path of the template-feedback file. Single
/// source of truth shared by the backfeed, feedback, and sync
/// commands (the last via its never-sync set).
pub const FEEDBACK_REL: &str = "docs/developer/template-feedback.md";

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
/// line, then the trimmed body. Refused when `records-check` would
/// reject it, e.g. a body opening with an unknown `**Label:**`.
fn format_entry(id: &str, title: &str, body: &str) -> Result<String, String> {
    let block = format!("### {id} -- {title}\n\n{}", body.trim_end());
    rustbase_xtask::records::check_entry(&block)
        .map_err(|e| format!("feedback-add: {e}"))?;
    Ok(block)
}

/// True when `md` already contains an entry with exactly this
/// ID. Matches the entry header line (`### <id>` alone or
/// `### <id> -- ...`), not a bare substring, so an ID that is a
/// prefix of a different same-day ID -- or that appears inside
/// some entry's body prose -- does not spuriously suppress the
/// add.
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
/// sample cannot be chosen as the insertion point. Feedback entries
/// are written at column 0 (`### <id> -- <title>`, body unindented),
/// so `rustbase_xtask::markdown::section_bounds`'s column-0-only rule
/// does not hold here and fences must be tracked.
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

/// The line an empty section carries in place of entries.
const PLACEHOLDER: &str = "_None yet._";

/// Remove the `_None yet._` line from the section whose header is at
/// `sec`, with the blank line under it -- only when it is the first
/// non-blank line under the header. Anywhere else the same text is
/// content (an entry quoting the empty-section format), not the
/// placeholder.
fn drop_placeholder(lines: &mut Vec<&str>, sec: usize) {
    let Some(at) =
        (sec + 1..lines.len()).find(|&i| !lines[i].trim().is_empty())
    else {
        return;
    };
    if lines[at].trim() != PLACEHOLDER {
        return;
    }
    if lines.get(at + 1).is_some_and(|l| l.trim().is_empty()) {
        lines.remove(at + 1);
    }
    lines.remove(at);
}

/// Insert `block` at the top of the section identified by `kw`,
/// just below its header (past a single blank line if present)
/// and above the section's existing entries. Pure. Errors when
/// no matching section header is found.
fn insert_entry(md: &str, kw: &str, block: &str) -> Result<String, String> {
    let mut lines: Vec<&str> = md.lines().collect();
    let sec = find_section(&lines, kw).ok_or_else(|| {
        format!("no `## ` section header matching {kw:?} found")
    })?;
    drop_placeholder(&mut lines, sec);

    let mut ins = sec + 1;
    if lines.get(ins).is_some_and(|l| l.trim().is_empty()) {
        ins += 1;
    }

    let mut out: Vec<&str> = lines[..ins].to_vec();
    let block_lines: Vec<&str> = block.lines().collect();
    out.extend(&block_lines);
    out.push("");
    out.extend(&lines[ins..]);
    Ok(rejoin_like(&out, md))
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

    let block = format_entry(&id, title.trim(), &body)?;
    let updated = insert_entry(&md, section.header_keyword(), &block)?;
    fs::write(&path, updated)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    eprintln!("feedback-add: added {id}");
    Ok(())
}

/// Today's date as an ISO `YYYY-MM-DD` string (UTC).
///
/// Used to stamp `last-run` in the backfeed ledger and to
/// mint `tf-<date>-<slug>` feedback IDs. A clock read before
/// the Unix epoch (should never happen) degrades to day zero.
pub fn today_iso() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs() / 86_400).ok())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Convert a count of days since the Unix epoch back into a
/// civil `(year, month, day)`. Howard Hinnant's `civil_from_days`
/// (the inverse of the private `days_from_civil` in
/// `rustbase_xtask::dep_age`), valid for the whole proleptic
/// Gregorian range. Pure, so it is
/// unit-tested against known dates.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (y + i64::from(m <= 2), m, d)
}

/// True when `line` opens or closes a fenced code block (its
/// first non-whitespace run is ```` ``` ```` or `~~~`). A copy of
/// `rustbase_xtask::markdown::is_fence`, pending
/// fr-2026-10-20-feedback-keeps-its-own-is-fence.
pub fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
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
        assert_eq!(e, Ok("### tf-x -- Title\n\nbody text".to_owned()));
    }

    #[test]
    fn format_entry_refuses_a_body_the_records_check_would_reject() {
        // The body's first line sits where an entry's fields go.
        let e =
            format_entry("tf-x", "Title", "**Fix:** pinned it").unwrap_err();
        assert!(e.contains("`Fix`"), "{e}");
        let e =
            format_entry("tf-x", "Title", "text\n\n### tf-y\n").unwrap_err();
        assert!(e.contains("one entry"), "{e}");
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
    fn a_header_with_trailing_spaces_keeps_other_sections_placeholders() {
        let doc = "## Open divergences \n\n### tf-x -- X\n\n## Resolved\n\n_None yet._\n";
        let out = insert_entry(doc, "open", "### tf-a -- A").unwrap();
        assert!(out.contains("_None yet._"), "{out}");
    }

    #[test]
    fn an_unclosed_fence_in_an_entry_keeps_later_placeholders() {
        let doc = "## Open divergences\n\n### tf-1 -- one\n\nRun:\n```sh\ncargo x\n\n\
                   ## Resolved\n\n_None yet._\n";
        let out = insert_entry(doc, "open", "### tf-a -- A").unwrap();
        assert!(out.contains("_None yet._"), "{out}");
    }

    #[test]
    fn a_placeholder_quoted_in_an_entry_or_a_fenced_header_copy_is_left_alone()
    {
        // A fenced copy of a header before the real one must not
        // crash, and a placeholder quoted inside an entry is content.
        let doc = "## Open divergences\n\n### tf-1 -- one\n\n```md\n## Resolved\n\n\
                   _None yet._\n```\n\n## Resolved\n\n### tf-2 -- two\n\n```md\n\
                   _None yet._\n```\n";
        let out = insert_entry(doc, "resolved", "### tf-a -- A").unwrap();
        assert_eq!(out.matches("_None yet._").count(), 2, "{out}");
    }

    #[test]
    fn insert_entry_keeps_crlf_line_endings() {
        let doc = "# F\r\n\r\n## Open divergences\r\n\r\n## Resolved\r\n";
        let out = insert_entry(doc, "open", "### tf-a -- A").unwrap();
        assert!(out.contains("### tf-a -- A\r\n"), "{out:?}");
        assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
    }

    #[test]
    fn insert_entry_drops_the_empty_section_placeholder() {
        // A section with no entries carries `_None yet._`; the first
        // entry replaces it, and other sections keep theirs.
        let doc = "# F\n\n## Open divergences\n\n_None yet._\n\n\
                   ## Suggestions\n\n_None yet._\n";
        let out = insert_entry(doc, "open", "### tf-a -- A\n\nBody.").unwrap();
        let open = &out[..out.find("## Suggestions").unwrap()];
        assert!(!open.contains("_None yet._"), "{out}");
        assert!(open.contains("tf-a"), "{out}");
        assert!(
            out[out.find("## Suggestions").unwrap()..].contains("_None yet._")
        );
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
        // chosen as the section.
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

    #[test]
    fn civil_from_days_epoch_and_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(31), (1970, 2, 1));
        // 2026-07-16 is 20650 days after the epoch.
        assert_eq!(civil_from_days(20650), (2026, 7, 16));
    }

    #[test]
    fn today_iso_is_well_formed() {
        // `dddd-dd-dd` shape, without depending on the
        // ISO-date validator (`rustbase_xtask::text::is_iso_date`).
        let s = today_iso();
        let b = s.as_bytes();
        assert_eq!(b.len(), 10);
        assert_eq!(b[4], b'-');
        assert_eq!(b[7], b'-');
        assert!(
            b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        );
    }

    #[test]
    fn is_fence_detects_both_markers() {
        assert!(is_fence("```"));
        assert!(is_fence("```rust"));
        assert!(is_fence("  ~~~"));
        assert!(!is_fence("### heading"));
        assert!(!is_fence("plain"));
    }
}
