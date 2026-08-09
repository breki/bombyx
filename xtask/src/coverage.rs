use std::fmt::Write as _;
use std::fs;

use serde::Deserialize;

use crate::helpers::{run_cargo_capture, workspace_root};

/// Minimum line coverage percentage for the workspace
/// as a whole. The validate pipeline fails when the
/// summed `lines.percent` across all non-excluded files
/// drops below this number.
pub const OVERALL_THRESHOLD: f64 = 90.0;

/// Per-module (per-file) coverage floor. A single file
/// dipping below this triggers a failure even when the
/// overall figure is still healthy -- a single uncovered
/// module would otherwise be averaged away.
pub const MODULE_THRESHOLD: f64 = 85.0;

/// Regex passed to `llvm-cov --ignore-filename-regex`.
/// Excludes binary entry points on both Unix and Windows
/// path separators: the direct `src/main.rs` bootstrap
/// wrapper and every module under `src/bin/`. A real CLI
/// is often a multi-file binary (`src/bin/<name>/main.rs`
/// plus sibling command modules) whose outer shell --
/// argument parsing, process output -- is exercised only
/// by `assert_cmd` integration tests as a spawned
/// subprocess, which llvm-cov cannot fully credit to the
/// source. Keep the testable logic in the library crate;
/// the binary shell is the thin entry point.
const IGNORE_REGEX: &str = r"src[/\\](main\.rs$|bin[/\\])";

/// The full `--ignore-filename-regex` for this workspace: the
/// [`IGNORE_REGEX`] baseline merged with any project patterns
/// from `[workspace.metadata.coverage] ignore` in the root
/// `Cargo.toml`. A missing/unreadable manifest degrades to the
/// baseline (coverage never fails over config *parsing*), but a
/// pattern that would match every file is a hard error -- such
/// a pattern silently neuters the gate, which is never intended.
fn ignore_regex() -> Result<String, String> {
    let manifest = fs::read_to_string(workspace_root().join("Cargo.toml"))
        .unwrap_or_default();
    let patterns = validate_ignore_patterns(parse_coverage_ignore(&manifest))?;
    Ok(build_ignore_regex(IGNORE_REGEX, &patterns))
}

/// Merge the baseline regex with project-supplied patterns
/// into one `--ignore-filename-regex` value. Baseline first,
/// user patterns appended in declared order, joined by `|`
/// (top-level alternation). Returns the baseline unchanged
/// when there are no user patterns. Pure.
fn build_ignore_regex(default: &str, user: &[String]) -> String {
    if user.is_empty() {
        return default.to_string();
    }
    let mut out = String::from(default);
    for pattern in user {
        out.push('|');
        out.push_str(pattern);
    }
    out
}

/// Drop blank patterns and reject any that match every file.
/// A blank entry would append a bare `|` to the alternation
/// (matching the empty string -> every path); `.`, `.*`, `.+`
/// match everything outright. Either silently reduces coverage
/// to a vacuous always-pass, so a blank is dropped as a typo
/// and a match-all fails loudly. Pure.
fn validate_ignore_patterns(
    patterns: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut kept = Vec::new();
    for pattern in patterns {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        if matches!(trimmed, "." | ".*" | ".+" | ".*?") {
            return Err(format!(
                "coverage ignore pattern {pattern:?} matches every file \
                 -- refusing a vacuous coverage gate; fix or remove it in \
                 [workspace.metadata.coverage] ignore"
            ));
        }
        kept.push(pattern);
    }
    Ok(kept)
}

/// Extract `[workspace.metadata.coverage]` `ignore = [...]`
/// patterns from a root `Cargo.toml`, in declared order; empty
/// when the section or `ignore` key is absent. Pure -- a
/// dependency-free scan, matching the manifest-parsing
/// convention used by `dep_age::gate` and `deploy_guard` (no
/// `toml` crate).
///
/// The value scan is quote-aware: `]` and `#` inside a quoted
/// pattern are literal (patterns are regex fragments, which
/// routinely contain `]`), and both double-quoted (`"..."`) and
/// single-quoted TOML literal (`'...'`) strings are read -- a
/// literal string is the recommended form for a regex since it
/// needs no backslash doubling. Only the
/// `[workspace.metadata.coverage]` header followed by a
/// line-leading `ignore = [...]` is recognised; the dotted-key
/// (`coverage.ignore = ...`) and inline-table
/// (`coverage = { ignore = ... }`) spellings are not (a
/// deliberate limit of the hand-rolled scan; see CLAUDE.md).
fn parse_coverage_ignore(manifest: &str) -> Vec<String> {
    let body = coverage_section_body(manifest);
    ignore_value_region(&body)
        .map(scan_quoted)
        .unwrap_or_default()
}

/// Raw text of the `[workspace.metadata.coverage]` section
/// (lines after its header, up to the next section header or
/// EOF). Lines are kept verbatim so the quote-aware value scan
/// sees comments and brackets in context.
fn coverage_section_body(manifest: &str) -> String {
    let mut in_section = false;
    let mut body = String::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let name = trimmed.trim_matches(|c| c == '[' || c == ']').trim();
            in_section = name == "workspace.metadata.coverage";
            continue;
        }
        if in_section {
            body.push_str(line);
            body.push('\n');
        }
    }
    body
}

/// The text after the `=` of a line-leading `ignore` key in
/// `body`, through end of `body` (so a multi-line array is
/// scanned in full). `None` when there is no such key.
fn ignore_value_region(body: &str) -> Option<&str> {
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if let Some(after) = line.trim_start().strip_prefix("ignore")
            && after.trim_start().starts_with('=')
            && let Some(eq) = line.find('=')
        {
            return Some(&body[offset + eq + 1..]);
        }
        offset += line.len();
    }
    None
}

/// Collect the contents of each quoted string in `region`
/// (both `"..."` and `'...'`), stopping at the first `]` that
/// falls outside a quote (the array close). `#` outside a quote
/// begins a comment to end of line. A quote or comment
/// character *inside* a string is captured literally. An
/// unterminated final quote is discarded. Pure.
fn scan_quoted(region: &str) -> Vec<String> {
    enum State {
        Normal,
        Comment,
        Double,
        Single,
    }
    let mut state = State::Normal;
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in region.chars() {
        match state {
            State::Normal => match c {
                '#' => state = State::Comment,
                '"' => state = State::Double,
                '\'' => state = State::Single,
                ']' => break,
                _ => {}
            },
            State::Comment => {
                if c == '\n' {
                    state = State::Normal;
                }
            }
            State::Double => {
                if c == '"' {
                    out.push(std::mem::take(&mut cur));
                    state = State::Normal;
                } else {
                    cur.push(c);
                }
            }
            State::Single => {
                if c == '\'' {
                    out.push(std::mem::take(&mut cur));
                    state = State::Normal;
                } else {
                    cur.push(c);
                }
            }
        }
    }
    out
}

/// Coverage check result for use by validate.
pub struct CoverageResult {
    /// Overall line coverage percentage.
    pub line_pct: f64,
    /// Covered lines.
    pub covered: u64,
    /// Total lines.
    pub total: u64,
    /// Structured failure detail (None = passed).
    pub error: Option<CoverageFailure>,
}

/// Structured coverage failure reason.
///
/// Kept separate from any rendered string so callers
/// can decide how to present it (text for validate,
/// JSON for hypothetical CI annotations, sort/filter
/// for tooling, etc.). Render via [`format_failure`].
pub enum CoverageFailure {
    /// Workspace overall percentage fell below
    /// [`OVERALL_THRESHOLD`].
    Overall { pct: f64, threshold: f64 },
    /// One or more modules fell below
    /// [`MODULE_THRESHOLD`] while the overall figure
    /// was still healthy.
    Modules(Vec<FailingModule>),
}

/// A module that fell below the per-file coverage
/// threshold, along with the line ranges that are
/// uncovered.
pub struct FailingModule {
    /// Short path (`src/`-relative or last segment).
    pub name: String,
    /// Module's line coverage percentage.
    pub pct: f64,
    /// Uncovered line ranges (sorted, merged,
    /// inclusive on both ends).
    pub ranges: Vec<(u64, u64)>,
}

/// Run coverage check and return structured result.
pub fn coverage_check() -> Result<CoverageResult, String> {
    // Stale `.profraw` files from earlier runs inflate the
    // line denominator -- `cargo llvm-cov` merges every
    // profile it finds under `target/llvm-cov-target/`, so
    // leftovers from cross-crate / cross-test-binary builds
    // fold into the totals and a real 98% can read as 70%,
    // indistinguishable from a genuine regression. Clean
    // first so the measurement reflects only this run.
    let clean = run_cargo_capture(&["llvm-cov", "clean", "--workspace"])?;
    if !clean.status.success() {
        let stderr = String::from_utf8_lossy(&clean.stderr);
        return Err(format!("cargo llvm-cov clean failed:\n{stderr}"));
    }

    // Note: omitting `--summary-only` keeps the per-file
    // `segments` array in the JSON output, which we need
    // to compute uncovered line ranges on failure. The
    // extra JSON bulk is small for a project this size.
    let ignore = ignore_regex()?;
    let output = run_cargo_capture(&[
        "llvm-cov",
        "--workspace",
        "--exclude",
        "xtask",
        "--ignore-filename-regex",
        &ignore,
        "--json",
    ])?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo llvm-cov failed:\n{stderr}"));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse coverage JSON: {e}"))?;

    let totals = &json["data"][0]["totals"]["lines"];
    let line_pct = totals["percent"].as_f64().ok_or("missing lines.percent")?;
    let covered = totals["covered"].as_u64().ok_or("missing lines.covered")?;
    let total = totals["count"].as_u64().ok_or("missing lines.count")?;

    let below = scan_failing_modules(&json["data"][0]["files"])?;

    let error = if line_pct < OVERALL_THRESHOLD {
        Some(CoverageFailure::Overall {
            pct: line_pct,
            threshold: OVERALL_THRESHOLD,
        })
    } else if !below.is_empty() {
        Some(CoverageFailure::Modules(below))
    } else {
        None
    };

    Ok(CoverageResult {
        line_pct,
        covered,
        total,
        error,
    })
}

/// Run coverage check with printed output (standalone).
pub fn coverage() -> Result<(), String> {
    let r = coverage_check()?;
    println!("  lines: {}/{} ({:.1}%)", r.covered, r.total, r.line_pct);
    if let Some(failure) = r.error {
        Err(format_failure(&failure))
    } else {
        println!("  coverage OK ({:.1}% >= {OVERALL_THRESHOLD}%)", r.line_pct);
        Ok(())
    }
}

/// Render a [`CoverageFailure`] as a human-readable
/// error string. Lives separately from the data so the
/// validate path and any future presentation can share
/// the same structured failure type.
pub fn format_failure(failure: &CoverageFailure) -> String {
    match failure {
        CoverageFailure::Overall { pct, threshold } => format!(
            "coverage {pct:.1}% is below \
             {threshold}% threshold"
        ),
        CoverageFailure::Modules(modules) => format_module_failures(modules),
    }
}

/// Walk the per-file array and collect modules below
/// [`MODULE_THRESHOLD`] together with their uncovered
/// line ranges. Files with zero counted lines are
/// skipped (no signal to derive a percentage from).
fn scan_failing_modules(
    files: &serde_json::Value,
) -> Result<Vec<FailingModule>, String> {
    let Some(arr) = files.as_array() else {
        return Ok(Vec::new());
    };
    let mut below: Vec<FailingModule> = Vec::new();
    for file in arr {
        let name = file["filename"].as_str().unwrap_or("?");
        let pct = file["summary"]["lines"]["percent"].as_f64().unwrap_or(0.0);
        let count = file["summary"]["lines"]["count"].as_u64().unwrap_or(0);
        if count == 0 || pct >= MODULE_THRESHOLD {
            continue;
        }
        let segments = parse_segments(&file["segments"]).map_err(|e| {
            format!(
                "failed to parse llvm-cov segments for \
                 {name}: {e}"
            )
        })?;
        let ranges = uncovered_ranges(&segments);
        below.push(FailingModule {
            name: shorten_path(name).to_string(),
            pct,
            ranges,
        });
    }
    Ok(below)
}

fn format_module_failures(below: &[FailingModule]) -> String {
    let mut msg = String::from("modules below coverage threshold:");
    for m in below {
        let _ = write!(msg, "\n    {}: {:.1}%", m.name, m.pct);
        if !m.ranges.is_empty() {
            let _ =
                write!(msg, "\n      uncovered: {}", format_ranges(&m.ranges));
        }
    }
    msg
}

/// A single llvm-cov segment. The JSON wire form is a
/// 6-element array `[line, col, count, has_count,
/// is_region_entry, is_gap_region]`; the [`Deserialize`]
/// impl below converts it into named fields so the rest
/// of the module can stay readable. A length mismatch
/// (older llvm-cov versions emit 5-element segments)
/// turns into a deserialization error at the boundary
/// rather than silently misclassifying spans.
#[derive(Debug)]
struct Segment {
    line: u64,
    #[allow(dead_code)] // present for completeness / future use
    col: u64,
    count: u64,
    has_count: bool,
    #[allow(dead_code)]
    is_region_entry: bool,
    is_gap: bool,
}

impl Segment {
    fn is_uncovered(&self) -> bool {
        self.has_count && self.count == 0 && !self.is_gap
    }
}

impl<'de> Deserialize<'de> for Segment {
    fn deserialize<D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Self, D::Error> {
        let (line, col, count, has_count, is_region_entry, is_gap) =
            <(u64, u64, u64, bool, bool, bool)>::deserialize(d)?;
        Ok(Segment {
            line,
            col,
            count,
            has_count,
            is_region_entry,
            is_gap,
        })
    }
}

/// Parse the llvm-cov `segments` array. Empty / missing
/// arrays yield an empty Vec rather than an error so
/// files with no recorded segments don't break the
/// pipeline.
fn parse_segments(
    segments: &serde_json::Value,
) -> Result<Vec<Segment>, String> {
    if segments.is_null() {
        return Ok(Vec::new());
    }
    serde_json::from_value::<Vec<Segment>>(segments.clone())
        .map_err(|e| format!("segment shape mismatch: {e}"))
}

/// Compute uncovered line ranges from a parsed segment
/// list. A segment's count applies from its `(line,
/// col)` until the next segment's position; lines
/// covered by an uncovered, non-gap segment are added
/// to the result. Adjacent uncovered ranges are merged.
///
/// The trailing segment (no successor in `windows(2)`)
/// is handled explicitly -- without this, a file whose
/// final span is itself an uncovered region would
/// silently drop those lines from the report.
fn uncovered_ranges(segments: &[Segment]) -> Vec<(u64, u64)> {
    let mut raw: Vec<(u64, u64)> = Vec::new();
    for window in segments.windows(2) {
        let seg = &window[0];
        let next = &window[1];
        if !seg.is_uncovered() {
            continue;
        }
        let end = if next.line > seg.line {
            next.line - 1
        } else {
            seg.line
        };
        raw.push((seg.line, end));
    }
    // Trailing segment: no successor, treat as a
    // single-line entry (best we can do without the
    // file's actual line count). Reports at least the
    // starting line so users know where to look.
    if let Some(last) = segments.last()
        && last.is_uncovered()
    {
        raw.push((last.line, last.line));
    }
    merge_ranges(raw)
}

/// Merge overlapping or adjacent inclusive ranges.
/// Adjacent means `next.0 <= prev.1 + 1` so `(5,7)`
/// and `(8,10)` collapse to `(5,10)`.
fn merge_ranges(mut ranges: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    merged.push(ranges[0]);
    for (s, e) in ranges.into_iter().skip(1) {
        let last = merged.last_mut().expect("just pushed");
        if s <= last.1.saturating_add(1) {
            last.1 = last.1.max(e);
        } else {
            merged.push((s, e));
        }
    }
    merged
}

/// Format ranges as a comma-separated list:
/// `(84, 93), (209, 209)` -> `"84-93, 209"`.
fn format_ranges(ranges: &[(u64, u64)]) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(ranges.len());
    for (s, e) in ranges {
        if s == e {
            parts.push(format!("{s}"));
        } else {
            parts.push(format!("{s}-{e}"));
        }
    }
    parts.join(", ")
}

/// Shorten a file path to just the part after `src/`.
fn shorten_path(name: &str) -> &str {
    name.rsplit_once("src\\")
        .or_else(|| name.rsplit_once("src/"))
        .map_or(name, |(_, rest)| rest)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn build_ignore_regex_returns_default_without_user_patterns() {
        assert_eq!(build_ignore_regex(IGNORE_REGEX, &[]), IGNORE_REGEX);
    }

    #[test]
    fn build_ignore_regex_appends_user_patterns_in_order() {
        let user = vec!["src/audio/native.rs".to_string(), "src/x.rs".into()];
        assert_eq!(
            build_ignore_regex("base", &user),
            "base|src/audio/native.rs|src/x.rs"
        );
    }

    #[test]
    fn parse_coverage_ignore_reads_an_inline_array() {
        let toml = "[workspace.metadata.coverage]\n\
                    ignore = [\"src/a.rs\", \"src/b.rs\"]\n";
        assert_eq!(parse_coverage_ignore(toml), ["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn parse_coverage_ignore_reads_a_multiline_array() {
        let toml = "[workspace.metadata.coverage]\nignore = [\n  \
                    \"src/a.rs\",  # a comment\n  \"src/b.rs\",\n]\n";
        assert_eq!(parse_coverage_ignore(toml), ["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn parse_coverage_ignore_keeps_brackets_inside_a_pattern() {
        // Regex char classes contain `]`; a `]` inside a quoted
        // pattern must not terminate a multi-line array (RT-2 /
        // AQ-2). The Rust literals below carry `\\` so the TOML
        // value (and thus the pattern) is `src[/\\]a\.rs`.
        let toml = "[workspace.metadata.coverage]\nignore = [\n  \
                    \"src[/\\\\]a\\.rs\",\n  \"src/b\\.rs\",\n]\n";
        assert_eq!(
            parse_coverage_ignore(toml),
            [r"src[/\\]a\.rs", r"src/b\.rs"]
        );
    }

    #[test]
    fn parse_coverage_ignore_keeps_hash_inside_a_pattern() {
        // `#` inside a quoted pattern is literal, not a comment
        // (RT-3).
        let toml = "[workspace.metadata.coverage]\n\
                    ignore = [\"src/v#2/a.rs\"]\n";
        assert_eq!(parse_coverage_ignore(toml), ["src/v#2/a.rs"]);
    }

    #[test]
    fn parse_coverage_ignore_reads_single_quoted_literals() {
        // TOML literal strings ('...') take the value raw -- the
        // recommended form for a regex (no `\\` doubling).
        let toml = "[workspace.metadata.coverage]\n\
                    ignore = ['src[/\\]a.rs']\n";
        assert_eq!(parse_coverage_ignore(toml), [r"src[/\]a.rs"]);
    }

    #[test]
    fn parse_coverage_ignore_empty_when_section_absent() {
        let toml = "[workspace]\nmembers = []\n\n\
                    [profile.release-fast]\ninherits = \"release\"\n";
        assert!(parse_coverage_ignore(toml).is_empty());
    }

    #[test]
    fn parse_coverage_ignore_empty_when_key_absent() {
        // Section present, no `ignore` key -- and a later
        // section's `ignore`-lookalike must not leak in.
        let toml = "[workspace.metadata.coverage]\nother = 1\n\n\
                    [other.section]\nignore = [\"nope.rs\"]\n";
        assert!(parse_coverage_ignore(toml).is_empty());
    }

    #[test]
    fn validate_ignore_patterns_drops_blanks() {
        let got = validate_ignore_patterns(vec![
            "src/a.rs".into(),
            String::new(),
            "  ".into(),
        ])
        .unwrap();
        assert_eq!(got, ["src/a.rs"]);
    }

    #[test]
    fn validate_ignore_patterns_rejects_match_all() {
        // A match-all pattern would silently neuter the gate
        // (RT-1) -- it must fail loudly.
        for p in [".", ".*", ".+", ".*?"] {
            assert!(
                validate_ignore_patterns(vec![p.to_string()]).is_err(),
                "{p:?} should be rejected"
            );
        }
    }

    #[test]
    fn validate_ignore_patterns_passes_normal_patterns() {
        let got =
            validate_ignore_patterns(vec![r"src/audio/native\.rs".into()])
                .unwrap();
        assert_eq!(got, [r"src/audio/native\.rs"]);
    }

    fn seg(line: u64, count: u64, is_gap: bool) -> Segment {
        Segment {
            line,
            col: 1,
            count,
            has_count: true,
            is_region_entry: count == 0,
            is_gap,
        }
    }

    #[test]
    fn merge_ranges_handles_empty() {
        assert!(merge_ranges(Vec::new()).is_empty());
    }

    #[test]
    fn merge_ranges_combines_adjacent() {
        let out = merge_ranges(vec![(5, 7), (8, 10), (15, 17)]);
        assert_eq!(out, vec![(5, 10), (15, 17)]);
    }

    #[test]
    fn merge_ranges_combines_overlapping() {
        let out = merge_ranges(vec![(5, 10), (7, 12), (20, 22)]);
        assert_eq!(out, vec![(5, 12), (20, 22)]);
    }

    #[test]
    fn merge_ranges_sorts_unsorted_input() {
        let out = merge_ranges(vec![(20, 22), (5, 7), (8, 10)]);
        assert_eq!(out, vec![(5, 10), (20, 22)]);
    }

    #[test]
    fn format_ranges_collapses_single_line() {
        assert_eq!(format_ranges(&[(5, 5), (10, 12)]), "5, 10-12");
        assert_eq!(format_ranges(&[]), "");
    }

    #[test]
    fn uncovered_ranges_extracts_zero_count_spans() {
        let segs = vec![
            seg(10, 0, false),
            seg(20, 5, false),
            seg(30, 0, false),
            // Trailing sentinel: covered, terminates
            // the previous uncovered run at line 39.
            seg(40, 5, false),
        ];
        let ranges = uncovered_ranges(&segs);
        assert_eq!(ranges, vec![(10, 19), (30, 39)]);
    }

    #[test]
    fn uncovered_ranges_includes_trailing_uncovered_segment() {
        // Without explicit trailing handling, windows(2)
        // never yields the final segment as window[0],
        // so the last uncovered span would silently
        // disappear. Cross-confirmed regression fixture.
        let segs = vec![seg(10, 5, false), seg(20, 0, false)];
        let ranges = uncovered_ranges(&segs);
        assert_eq!(ranges, vec![(20, 20)]);
    }

    #[test]
    fn uncovered_ranges_handles_single_uncovered_segment() {
        let segs = vec![seg(7, 0, false)];
        let ranges = uncovered_ranges(&segs);
        assert_eq!(ranges, vec![(7, 7)]);
    }

    #[test]
    fn uncovered_ranges_ignores_gap_regions() {
        let segs = vec![seg(10, 0, true), seg(20, 0, false)];
        // Gap-flagged region skipped; the trailing
        // segment is uncovered and survives.
        let ranges = uncovered_ranges(&segs);
        assert_eq!(ranges, vec![(20, 20)]);
    }

    #[test]
    fn uncovered_ranges_handles_empty() {
        assert!(uncovered_ranges(&[]).is_empty());
    }

    #[test]
    fn parse_segments_accepts_six_element_arrays() {
        let json = json!([[10, 1, 0, true, true, false]]);
        let segs = parse_segments(&json).unwrap();
        assert_eq!(segs.len(), 1);
        assert!(segs[0].is_uncovered());
    }

    #[test]
    fn parse_segments_rejects_wrong_arity() {
        // Five-element array (older llvm-cov, pre-LLVM
        // 12) -- must surface as an error rather than
        // be silently truncated.
        let json = json!([[10, 1, 0, true, true]]);
        let err = parse_segments(&json).unwrap_err();
        assert!(err.contains("shape mismatch"));
    }

    #[test]
    fn parse_segments_empty_or_null() {
        assert!(parse_segments(&serde_json::Value::Null).unwrap().is_empty());
        assert!(parse_segments(&json!([])).unwrap().is_empty());
    }

    #[test]
    fn format_failure_overall() {
        let f = CoverageFailure::Overall {
            pct: 82.3,
            threshold: 90.0,
        };
        assert_eq!(format_failure(&f), "coverage 82.3% is below 90% threshold");
    }

    #[test]
    fn format_failure_modules() {
        let f = CoverageFailure::Modules(vec![
            FailingModule {
                name: "api/routes.rs".into(),
                pct: 72.5,
                ranges: vec![(84, 93), (209, 221)],
            },
            FailingModule {
                name: "api/dto.rs".into(),
                pct: 60.0,
                ranges: Vec::new(),
            },
        ]);
        let out = format_failure(&f);
        assert!(out.contains("api/routes.rs: 72.5%"));
        assert!(out.contains("uncovered: 84-93, 209-221"));
        assert!(out.contains("api/dto.rs: 60.0%"));
        // Module with no ranges must not emit an empty
        // `uncovered:` line.
        assert!(!out.contains("api/dto.rs: 60.0%\n      uncovered:"));
    }
}
