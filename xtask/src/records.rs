//! Shared parser for the record-collection files and the
//! `records-check` integrity gate over them.
//!
//! A record file holds a flat list of entries. Each entry opens
//! with an `### <id>` heading, carries a **field block** of
//! `**Label:** value` lines directly beneath it, and then a prose
//! body. The tooling reads the fields; the prose is for people.
//!
//! The gate validates four things and nothing about phrasing: an id
//! used twice across the set, a field label outside the known set, a
//! heading id of the wrong shape, and a `Depends on` / `Supersedes`
//! id that names no entry. Only ids in those two fields are checked
//! -- a durable id in prose is unchecked provenance, which is where a
//! citation of a since-removed entry lives, so it does not dangle the
//! gate.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::helpers::workspace_root;

/// The record files the gate reads, repo-relative.
///
/// `docs/todo.md` joins this set once its bullets become headed
/// entries (increment 3b of `record-files-typed-header`).
const RECORD_FILES: &[&str] = &[
    "docs/developer/redteam-log.md",
    "docs/developer/artisan-log.md",
    "docs/developer/fresh-reader-log.md",
    "docs/developer/template-feedback.md",
];

/// Field labels an entry may carry. A label outside this set is a
/// finding: it is either a typo or a body line mistaken for a field,
/// and either way the vocabulary should not grow silently.
const KNOWN_LABELS: &[&str] = &[
    "Category",
    "Summary",
    "Source",
    "Issue",
    "Depends on",
    "Supersedes",
];

/// The two fields whose values are durable ids the gate resolves.
const CROSS_REF_LABELS: &[&str] = &["Depends on", "Supersedes"];

/// One parsed entry.
struct Record {
    /// The durable id, taken from the heading up to a ` -- `.
    id: String,
    /// 1-indexed source line of the `###` heading.
    line: usize,
    /// The field block, in source order.
    fields: Vec<Field>,
}

/// One `**Label:** value` line from an entry's field block.
struct Field {
    /// The text between `**` and `:**`.
    label: String,
    /// Everything after `:** `, trimmed.
    value: String,
    /// 1-indexed source line of the field.
    line: usize,
}

/// One thing the gate found wrong.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    /// Short tag naming which check fired.
    kind: &'static str,
    /// Repo-relative file the entry is in.
    file: String,
    /// 1-indexed line the finding is on.
    line: usize,
    /// What is wrong, in one clause.
    message: String,
}

/// The id a heading declares: the text up to a ` -- ` separator if
/// present, else the whole heading. So template-feedback's
/// `### tf-... -- title` yields the id alone and keeps its human
/// title, and `feedback-add` needs no change.
fn heading_id(heading: &str) -> String {
    let h = heading.trim();
    match h.split_once(" -- ") {
        Some((id, _)) => id.trim().to_string(),
        None => h.to_string(),
    }
}

/// Parse one line as a field, or `None` if it is not field-shaped.
///
/// A field is `**<label>:** <value>`. The label may hold spaces
/// (`Depends on`) but not `*`, so a bold run that closes before the
/// colon -- a prose `**Supersedes `id`.**` -- is not a field.
fn parse_field(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("**")?;
    let (label, after) = rest.split_once(":**")?;
    if label.contains('*') {
        return None;
    }
    let value = after.strip_prefix(' ').unwrap_or(after).trim();
    Some((label.trim().to_string(), value.to_string()))
}

/// Parse a record file into its entries.
///
/// The field block is the run of field lines directly under the
/// heading, after an optional blank line; it ends at the first blank
/// or non-field line. So a `**Status:**` deeper in the body is prose,
/// not a field.
fn parse_records(content: &str) -> Vec<Record> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(rest) = lines[i].strip_prefix("### ") else {
            i += 1;
            continue;
        };
        let id = heading_id(rest);
        let heading_line = i + 1;
        let mut j = i + 1;
        if j < lines.len() && lines[j].trim().is_empty() {
            j += 1;
        }
        let mut fields = Vec::new();
        while j < lines.len() && !lines[j].trim().is_empty() {
            let Some((label, value)) = parse_field(lines[j]) else {
                break;
            };
            fields.push(Field {
                label,
                value,
                line: j + 1,
            });
            j += 1;
        }
        out.push(Record {
            id,
            line: heading_line,
            fields,
        });
        i = j;
    }
    out
}

/// True when `id` has one of the two legal shapes: a durable
/// `<rt|aq|fr|tf>-<YYYY-MM-DD>-<slug>` or a dateless kebab slug (the
/// queue's shape). Kebab already covers a durable id, so this only
/// refuses an id with uppercase, spaces, underscores or stray
/// punctuation.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// The ids a cross-reference value names: comma-separated, trimmed,
/// with an em-dash or hyphen placeholder for "none" dropped.
fn ref_ids(value: &str) -> Vec<&str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != "\u{2014}" && *t != "-")
        .collect()
}

/// Run every check over already-read files, returning findings
/// sorted for a stable report.
fn findings_for(files: &[(String, String)]) -> Vec<Finding> {
    let parsed: Vec<(String, Vec<Record>)> = files
        .iter()
        .map(|(name, text)| (name.clone(), parse_records(text)))
        .collect();

    // The id universe, and where each id was declared.
    let mut sites: BTreeMap<&str, Vec<(&str, usize)>> = BTreeMap::new();
    for (name, records) in &parsed {
        for r in records {
            sites.entry(&r.id).or_default().push((name, r.line));
        }
    }

    let mut findings = Vec::new();
    for (name, records) in &parsed {
        for r in records {
            if !valid_id(&r.id) {
                findings.push(Finding {
                    kind: "malformed-id",
                    file: name.clone(),
                    line: r.line,
                    message: format!("heading id `{}` is not a legal id", r.id),
                });
            }
            for f in &r.fields {
                if !KNOWN_LABELS.contains(&f.label.as_str()) {
                    findings.push(Finding {
                        kind: "unknown-label",
                        file: name.clone(),
                        line: f.line,
                        message: format!("unknown field label `{}`", f.label),
                    });
                }
                if CROSS_REF_LABELS.contains(&f.label.as_str()) {
                    for id in ref_ids(&f.value) {
                        if !sites.contains_key(id) {
                            findings.push(Finding {
                                kind: "dangling-ref",
                                file: name.clone(),
                                line: f.line,
                                message: format!(
                                    "{} names `{id}`, which is in no entry",
                                    f.label
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    // Duplicate ids, reported once at each later declaration site.
    for (id, where_) in &sites {
        if where_.len() > 1 {
            for (name, line) in where_.iter().skip(1) {
                findings.push(Finding {
                    kind: "duplicate-id",
                    file: (*name).to_string(),
                    line: *line,
                    message: format!("id `{id}` is already used"),
                });
            }
        }
    }

    findings.sort();
    findings
}

/// Read the record files and run the checks.
fn collect() -> Result<(Vec<Finding>, usize, usize), String> {
    let root = workspace_root();
    let mut files = Vec::new();
    for f in RECORD_FILES {
        let text = std::fs::read_to_string(root.join(f))
            .map_err(|e| format!("cannot read {f}: {e}"))?;
        files.push(((*f).to_string(), text));
    }
    let records: usize =
        files.iter().map(|(_, t)| parse_records(t).len()).sum();
    let findings = findings_for(&files);
    Ok((findings, files.len(), records))
}

/// The findings rendered one per line, for an error message.
fn render(findings: &[Finding]) -> String {
    let mut msg = String::new();
    for f in findings {
        let _ =
            writeln!(msg, "  [{}] {}:{} {}", f.kind, f.file, f.line, f.message);
    }
    let _ = write!(msg, "{} record finding(s)", findings.len());
    msg
}

/// Run the checks and return a one-line summary, for `validate` to
/// print beside the step name.
pub fn records_check_detail() -> Result<String, String> {
    let (findings, files, records) = collect()?;
    if findings.is_empty() {
        Ok(format!("{files} files, {records} entries"))
    } else {
        Err(render(&findings))
    }
}

/// Run the checks standalone, printing the outcome.
pub fn records_check() -> Result<(), String> {
    let detail = records_check_detail()?;
    println!("Records OK ({detail})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_id_drops_a_trailing_title() {
        assert_eq!(
            heading_id("tf-2026-09-11-foo -- Foo the bar"),
            "tf-2026-09-11-foo"
        );
        assert_eq!(heading_id("rt-2026-09-13-baz"), "rt-2026-09-13-baz");
    }

    #[test]
    fn parse_field_reads_a_labelled_line() {
        assert_eq!(
            parse_field("**Category:** Correctness (low)"),
            Some(("Category".to_string(), "Correctness (low)".to_string()))
        );
        assert_eq!(
            parse_field("**Depends on:** a-slug, b-slug"),
            Some(("Depends on".to_string(), "a-slug, b-slug".to_string()))
        );
    }

    #[test]
    fn a_bold_run_that_closes_before_the_colon_is_not_a_field() {
        // A prose supersede citation: the bold closes before any
        // colon, so it is body text, not a field the gate reads.
        assert_eq!(parse_field("**Supersedes `tf-2026-08-10-old`.**"), None);
    }

    #[test]
    fn the_field_block_ends_at_the_first_blank_line() {
        // A `**Status:**` in the body, past the blank line, is prose.
        let src = "\
### rt-2026-09-13-thing

**Category:** Correctness

Body text follows.
**Status:** Resolved 2026-09-13 -- not a field.
";
        let records = parse_records(src);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].fields.len(), 1);
        assert_eq!(records[0].fields[0].label, "Category");
    }

    #[test]
    fn several_fields_parse_in_order() {
        let src = "\
### destroy-confirmation-shape

**Summary:** what destroy's positional becomes
**Issue:** #27
**Depends on:** project-selection-flag

Prose body.
";
        let records = parse_records(src);
        let labels: Vec<&str> =
            records[0].fields.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(labels, vec!["Summary", "Issue", "Depends on"]);
    }

    #[test]
    fn a_heading_with_no_blank_before_its_field_still_parses() {
        let src = "### a-slug\n**Category:** X\n\nBody.\n";
        let records = parse_records(src);
        assert_eq!(records[0].fields.len(), 1);
    }

    fn files(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, t)| ((*n).to_string(), (*t).to_string()))
            .collect()
    }

    #[test]
    fn a_clean_set_has_no_findings() {
        let f = files(&[
            (
                "a.md",
                "### rt-2026-09-13-one\n\n**Category:** X\n\nBody.\n",
            ),
            (
                "b.md",
                "### aq-2026-09-13-two\n\n**Depends on:** rt-2026-09-13-one\n\nBody.\n",
            ),
        ]);
        assert!(findings_for(&f).is_empty());
    }

    #[test]
    fn a_dangling_dependency_is_reported_and_a_resolved_one_is_not() {
        let f = files(&[(
            "a.md",
            "### aq-2026-09-13-two\n\n**Depends on:** no-such-slug\n\nBody.\n",
        )]);
        let found = findings_for(&f);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "dangling-ref");
        assert!(found[0].message.contains("no-such-slug"));
    }

    #[test]
    fn supersedes_is_resolved_like_depends_on() {
        let f = files(&[(
            "a.md",
            "### tf-2026-09-13-new\n\n**Supersedes:** tf-2026-01-01-gone\n\nBody.\n",
        )]);
        let found = findings_for(&f);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "dangling-ref");
    }

    #[test]
    fn a_placeholder_dash_is_not_a_dependency() {
        let f = files(&[(
            "a.md",
            "### rt-2026-09-13-one\n\n**Depends on:** \u{2014}\n\nBody.\n",
        )]);
        assert!(findings_for(&f).is_empty());
    }

    #[test]
    fn a_duplicate_id_is_reported_once() {
        let f = files(&[
            (
                "a.md",
                "### rt-2026-09-13-one\n\n**Category:** X\n\nBody.\n",
            ),
            (
                "b.md",
                "### rt-2026-09-13-one\n\n**Category:** Y\n\nBody.\n",
            ),
        ]);
        let found = findings_for(&f);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "duplicate-id");
        assert_eq!(found[0].file, "b.md");
    }

    #[test]
    fn an_unknown_field_label_is_reported() {
        let f = files(&[(
            "a.md",
            "### rt-2026-09-13-one\n\n**Catgory:** typo\n\nBody.\n",
        )]);
        let found = findings_for(&f);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "unknown-label");
    }

    #[test]
    fn a_malformed_heading_id_is_reported() {
        let f = files(&[(
            "a.md",
            "### RT-7 the finding\n\n**Category:** X\n\nBody.\n",
        )]);
        let found = findings_for(&f);
        assert!(found.iter().any(|x| x.kind == "malformed-id"));
    }

    #[test]
    fn a_durable_id_in_prose_is_not_checked() {
        // The body cites a since-removed id; only fields are resolved.
        let f = files(&[(
            "a.md",
            "### rt-2026-09-13-one\n\n**Category:** X\n\nSupersedes tf-2026-01-01-gone in prose.\n",
        )]);
        assert!(findings_for(&f).is_empty());
    }
}
