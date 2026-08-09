//! `cargo xtask backfeed-diff` / `backfeed-record` -- the
//! deterministic half of the `/template-backfeed` workflow.
//!
//! `/template-backfeed` pushes fixes a downstream rustbase
//! project logged in its `docs/developer/template-feedback.md`
//! back into this template. Without a watermark, every run
//! re-reads the downstream's entire (thousands-of-lines)
//! feedback file. These two commands move that delta
//! determination out of the LLM:
//!
//! - [`backfeed_diff`] reads the downstream feedback file plus
//!   the ledger watermark for that downstream and prints only
//!   the entries on or after the watermark date. The LLM then
//!   judges only that small candidate set.
//! - [`backfeed_record`] advances the ledger watermark (and
//!   records the downstream commit + run date) once the LLM
//!   has finished evaluating a batch.
//!
//! The ledger (`docs/developer/backfeed-ledger.toml`) is
//! entirely machine-owned, so it is hand-parsed and
//! regenerated rather than pulling in the `toml` crate --
//! mirroring the precedent in `coverage.rs`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::helpers::{
    BACKFEED_LEDGER_REL, FEEDBACK_REL, is_fence, today_iso, workspace_root,
};

/// Canonical header re-emitted on every ledger rewrite so the
/// machine-owned file stays self-documenting.
const LEDGER_HEADER: &str = "\
# Backfeed ledger -- machine-owned by `cargo xtask backfeed-record`.
#
# One table per downstream rustbase-derived project. It records
# how far `/template-backfeed` has already evaluated that
# downstream's `template-feedback.md`, so a run never re-scans
# history it has already judged:
#
#   watermark        newest feedback-entry date already evaluated
#                    (YYYY-MM-DD); `backfeed-diff` shows only
#                    entries on or after this date
#   downstream-head  the downstream commit SHA seen at last run
#                    (provenance; best-effort, read from <ds>/.git)
#   last-run         the date this ledger entry was last updated
#
# Do not hand-edit; `cargo xtask backfeed-record` rewrites it.
";

/// The parsed backfeed ledger: one [`LedgerEntry`] per
/// downstream, keyed by project name.
#[derive(Debug, Default, PartialEq, Eq)]
struct Ledger {
    entries: Vec<LedgerEntry>,
}

/// One downstream's ledger row. Every field but the name is
/// optional so a partially-seeded table (watermark only, as in
/// the shipped seed) parses cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LedgerEntry {
    name: String,
    watermark: Option<String>,
    downstream_head: Option<String>,
    last_run: Option<String>,
}

impl Ledger {
    /// The recorded watermark date for `name`, if any.
    fn watermark(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .and_then(|e| e.watermark.as_deref())
    }

    /// Insert or update `name`'s row. `downstream_head` is only
    /// overwritten when a fresh value is supplied, so a run that
    /// cannot read the downstream `.git` preserves the last
    /// known SHA rather than clobbering it with `None`.
    fn upsert(
        &mut self,
        name: &str,
        watermark: &str,
        head: Option<&str>,
        last_run: &str,
    ) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.name == name) {
            e.watermark = Some(watermark.to_string());
            if head.is_some() {
                e.downstream_head = head.map(str::to_string);
            }
            e.last_run = Some(last_run.to_string());
        } else {
            self.entries.push(LedgerEntry {
                name: name.to_string(),
                watermark: Some(watermark.to_string()),
                downstream_head: head.map(str::to_string),
                last_run: Some(last_run.to_string()),
            });
        }
    }
}

/// Parse a ledger file. Tolerant and pure: blank lines and
/// `#` comments are skipped, `[name]` opens a table, and
/// `key = "value"` sets a known field on the current table.
/// Unknown keys are ignored so the format can grow without
/// breaking older parsers.
fn parse_ledger(text: &str) -> Ledger {
    let mut entries: Vec<LedgerEntry> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(inner) =
            t.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
        {
            entries.push(LedgerEntry {
                name: inner.trim().to_string(),
                watermark: None,
                downstream_head: None,
                last_run: None,
            });
            continue;
        }
        if let Some((key, val)) = t.split_once('=')
            && let Some(e) = entries.last_mut()
        {
            let val = unquote(val.trim());
            match key.trim() {
                "watermark" => e.watermark = Some(val),
                "downstream-head" => e.downstream_head = Some(val),
                "last-run" => e.last_run = Some(val),
                _ => {}
            }
        }
    }
    Ledger { entries }
}

/// Serialize a ledger back to TOML: canonical header, then one
/// table per downstream sorted by name (so rewrites are
/// order-stable regardless of insertion order). Pure.
fn serialize_ledger(ledger: &Ledger) -> String {
    let mut entries: Vec<&LedgerEntry> = ledger.entries.iter().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = String::from(LEDGER_HEADER);
    for e in entries {
        let _ = writeln!(out);
        let _ = writeln!(out, "[{}]", e.name);
        if let Some(w) = &e.watermark {
            let _ = writeln!(out, "watermark = \"{w}\"");
        }
        if let Some(h) = &e.downstream_head {
            let _ = writeln!(out, "downstream-head = \"{h}\"");
        }
        if let Some(l) = &e.last_run {
            let _ = writeln!(out, "last-run = \"{l}\"");
        }
    }
    out
}

/// Strip one layer of surrounding double quotes, if present.
fn unquote(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}

// The two ISO-date helpers below are consumed only by backfeed
// (watermark validation + reading dates out of feedback
// headers), so they live here rather than in `helpers`. That
// keeps a downstream that adopts only the sync/feedback half --
// and deletes this template-repo-only `backfeed` module -- from
// tripping `warnings = deny` on now-dead helper code.

/// True when `s` is a well-formed ISO `YYYY-MM-DD` date:
/// exactly ten chars, `dddd-dd-dd`, with month `01..=12` and
/// day `01..=31`. A shallow calendar check -- it does not
/// reject Feb 30 -- which is enough for lexical watermark
/// comparison (ISO dates sort lexically iff well-formed).
fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: &[u8]| r.iter().all(u8::is_ascii_digit);
    if !digits(&b[0..4]) || !digits(&b[5..7]) || !digits(&b[8..10]) {
        return false;
    }
    let two = |a: u8, c: u8| (a - b'0') * 10 + (c - b'0');
    let month = two(b[5], b[6]);
    let day = two(b[8], b[9]);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// Extract the first well-formed ISO `YYYY-MM-DD` date embedded
/// anywhere in `line`, or `None`. Used to read the date out of
/// heterogeneous feedback headers (`### 2026-07-16 -- title`,
/// `## 2026-07-16 fixed X`). Scans every 10-char window and
/// validates it with [`is_iso_date`]. Windows that do not fall
/// on UTF-8 char boundaries are skipped via the non-panicking
/// `str::get`, so a header with non-ASCII text never panics.
fn extract_iso_date(line: &str) -> Option<String> {
    if line.len() < 10 {
        return None;
    }
    (0..=line.len() - 10)
        .filter_map(|i| line.get(i..i + 10))
        .find(|w| is_iso_date(w))
        .map(str::to_string)
}

/// The markdown heading level (1..=3) of `line` when it is a
/// section/entry header (`# `, `## `, or `### ` after trimming
/// leading whitespace), else `None`. Level 4+ headers stay in
/// an entry body rather than acting as block boundaries.
fn hash_level(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let hashes = t.bytes().take_while(|&c| c == b'#').count();
    if (1..=3).contains(&hashes) && t.as_bytes().get(hashes) == Some(&b' ') {
        Some(hashes)
    } else {
        None
    }
}

/// Per-line boundary levels for `lines`, with fenced code
/// blocks masked out: a line inside a ```` ``` ````/`~~~` fence
/// (and the fence lines themselves) yields `None`, so a `###`
/// that is really a code sample in an entry body is not
/// mistaken for a new entry (RT-4).
fn boundary_levels(lines: &[&str]) -> Vec<Option<usize>> {
    let mut in_fence = false;
    lines
        .iter()
        .map(|l| {
            if is_fence(l) {
                in_fence = !in_fence;
                None
            } else if in_fence {
                None
            } else {
                hash_level(l)
            }
        })
        .collect()
}

/// Split feedback markdown into entry blocks. A block starts at
/// either a level-3 (`###`) header -- the per-entry convention
/// `feedback-add` emits -- or a **dated** level-2 (`##`) header,
/// which is how older downstreams (jutro) date a whole section
/// and list its entries as bullets beneath. A block runs up to
/// the next level-1/2/3 header. The level-1 title and *undated*
/// level-2 section headers (`## Resolved`, `## Open divergences`)
/// are boundaries but never block starts. Headers inside fenced
/// code blocks are ignored.
///
/// Tolerating both shapes is deliberate: backfeed is read-only
/// on the downstream, so the reader must accept whatever format a
/// project historically adopted rather than assume the canonical
/// `###` one.
fn entry_blocks(md: &str) -> Vec<String> {
    let lines: Vec<&str> = md.lines().collect();
    let boundary = boundary_levels(&lines);
    let is_block_start = |i: usize| match boundary[i] {
        Some(3) => true,
        Some(2) => extract_iso_date(lines[i]).is_some(),
        _ => false,
    };
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if is_block_start(i) {
            let start = i;
            i += 1;
            while i < lines.len() && boundary[i].is_none() {
                i += 1;
            }
            blocks.push(lines[start..i].join("\n"));
        } else {
            i += 1;
        }
    }
    blocks
}

/// The feedback entries in scope for a backfeed run.
///
/// With no watermark (a first run / "full history once"
/// bootstrap) every entry is returned. With a watermark, an
/// entry is kept when its header carries an ISO date on or
/// after the watermark (inclusive, so a same-day entry added
/// after the last run is never missed -- the LLM's Resolved
/// cross-reference dedups the ones already applied). An entry
/// whose header carries no date predates the dated-entry
/// convention and is treated as older than any watermark.
fn entries_in_scope(md: &str, watermark: Option<&str>) -> Vec<String> {
    entry_blocks(md)
        .into_iter()
        .filter(|block| {
            let header = block.lines().next().unwrap_or("");
            match (watermark, extract_iso_date(header)) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(wm), Some(d)) => d.as_str() >= wm,
            }
        })
        .collect()
}

/// Derive the ledger key for a downstream from its path (the
/// final path component, e.g. `../jutro` -> `jutro`), unless an
/// explicit `--name` override is supplied. The override is
/// needed for worktree-style layouts where the checkout lives
/// at `<project>/<branch>` (e.g. `../ledgerstone/main`), whose
/// basename `main` is a poor, collision-prone ledger key.
fn downstream_name(path: &str, name_override: Option<&str>) -> String {
    if let Some(name) = name_override {
        return name.to_string();
    }
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

/// A plausible git object id: 7..=64 lowercase/uppercase hex
/// chars (SHA-1 or SHA-256, possibly abbreviated). Used to
/// reject anything that is not a commit id before it is
/// recorded to the committed ledger (RT-2).
fn is_hex_sha(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Best-effort read of a downstream's current commit SHA from
/// its `.git/HEAD` (and the loose ref it points at). `None`
/// when the path is not a plain git checkout (packed refs, a
/// `gitdir:` file, permission error) -- provenance is a
/// nice-to-have, never a hard failure.
///
/// The `ref:` target is confined to `refs/` with no `..`
/// component so a hostile downstream `.git/HEAD` cannot make
/// this read an arbitrary file via path traversal, and the
/// resolved value must look like a git object id before it is
/// accepted (RT-2) -- otherwise the traversed file's first
/// line would be written verbatim into the committed ledger.
fn read_git_head(ds_path: &str) -> Option<String> {
    let git = Path::new(ds_path).join(".git");
    let head = fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let value = if let Some(refpath) = head.strip_prefix("ref: ") {
        let refpath = refpath.trim();
        if !refpath.starts_with("refs/")
            || refpath.split('/').any(|c| c == "..")
        {
            return None;
        }
        fs::read_to_string(git.join(refpath)).ok()?
    } else {
        head.to_string()
    };
    let value = value.trim();
    is_hex_sha(value).then(|| value.to_string())
}

/// Absolute path of the ledger file in this template.
fn ledger_path() -> PathBuf {
    workspace_root().join(BACKFEED_LEDGER_REL)
}

/// Load the ledger, degrading a missing/unreadable file to an
/// empty ledger (a fresh template has no ledger yet).
fn load_ledger() -> Ledger {
    match fs::read_to_string(ledger_path()) {
        Ok(text) => parse_ledger(&text),
        Err(_) => Ledger::default(),
    }
}

/// `cargo xtask backfeed-diff <downstream-path>` -- print the
/// downstream feedback entries on or after this downstream's
/// ledger watermark (entries to stdout, a one-line summary to
/// stderr).
pub fn backfeed_diff(
    downstream_path: &str,
    name_override: Option<&str>,
) -> Result<(), String> {
    let feedback_path = Path::new(downstream_path).join(FEEDBACK_REL);
    let md = fs::read_to_string(&feedback_path)
        .map_err(|e| format!("cannot read {}: {e}", feedback_path.display()))?;
    let name = downstream_name(downstream_path, name_override);
    let watermark = load_ledger().watermark(&name).map(str::to_string);
    let blocks = entries_in_scope(&md, watermark.as_deref());

    match &watermark {
        Some(wm) => eprintln!(
            "backfeed-diff: {name}: {} on/after watermark {wm}",
            plural(blocks.len())
        ),
        None => eprintln!(
            "backfeed-diff: {name}: no ledger watermark -- full history \
             ({})",
            plural(blocks.len())
        ),
    }
    for b in &blocks {
        println!("{b}\n");
    }
    Ok(())
}

/// `cargo xtask backfeed-record <downstream-path> --watermark
/// <YYYY-MM-DD> [--head <sha>]` -- advance the ledger watermark
/// for a downstream. `--head` defaults to the downstream's
/// current `.git` HEAD; `last-run` is stamped with today.
pub fn backfeed_record(
    downstream_path: &str,
    watermark: &str,
    head: Option<&str>,
    name_override: Option<&str>,
) -> Result<(), String> {
    if !is_iso_date(watermark) {
        return Err(format!(
            "--watermark {watermark:?} is not a YYYY-MM-DD date"
        ));
    }
    let name = downstream_name(downstream_path, name_override);
    let head = head
        .map(str::to_string)
        .or_else(|| read_git_head(downstream_path));

    let mut ledger = load_ledger();
    ledger.upsert(&name, watermark, head.as_deref(), &today_iso());
    fs::write(ledger_path(), serialize_ledger(&ledger))
        .map_err(|e| format!("cannot write ledger: {e}"))?;

    match &head {
        Some(h) => eprintln!(
            "backfeed-record: {name}: watermark={watermark}, head={h}"
        ),
        None => {
            eprintln!("backfeed-record: {name}: watermark={watermark}");
        }
    }
    Ok(())
}

/// `N entry` / `N entries` for a count.
fn plural(n: usize) -> String {
    if n == 1 {
        "1 entry".to_string()
    } else {
        format!("{n} entries")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        name: &str,
        watermark: Option<&str>,
        head: Option<&str>,
        last_run: Option<&str>,
    ) -> LedgerEntry {
        LedgerEntry {
            name: name.to_string(),
            watermark: watermark.map(str::to_string),
            downstream_head: head.map(str::to_string),
            last_run: last_run.map(str::to_string),
        }
    }

    #[test]
    fn parse_ledger_reads_tables_and_ignores_unknowns() {
        let text = "\
# a comment
[jutro]
watermark = \"2026-07-14\"
future-key = \"ignored\"

[clockdump]
watermark = \"2026-07-15\"
downstream-head = \"abc123\"
last-run = \"2026-07-16\"
";
        let ledger = parse_ledger(text);
        assert_eq!(ledger.watermark("jutro"), Some("2026-07-14"));
        assert_eq!(ledger.watermark("clockdump"), Some("2026-07-15"));
        assert_eq!(
            ledger.entries[1].downstream_head.as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn ledger_round_trips_through_serialize() {
        // Entries already in sorted (serialize) order.
        let ledger = Ledger {
            entries: vec![
                entry("clockdump", Some("2026-07-15"), None, None),
                entry(
                    "jutro",
                    Some("2026-07-14"),
                    Some("deadbeef"),
                    Some("2026-07-16"),
                ),
            ],
        };
        assert_eq!(parse_ledger(&serialize_ledger(&ledger)), ledger);
    }

    #[test]
    fn serialize_sorts_entries_by_name() {
        let ledger = Ledger {
            entries: vec![
                entry("zeta", Some("2026-01-01"), None, None),
                entry("alpha", Some("2026-01-02"), None, None),
            ],
        };
        let text = serialize_ledger(&ledger);
        let alpha = text.find("[alpha]").unwrap();
        let zeta = text.find("[zeta]").unwrap();
        assert!(alpha < zeta);
    }

    #[test]
    fn upsert_updates_existing_and_preserves_head_when_absent() {
        let mut ledger = Ledger::default();
        ledger.upsert("jutro", "2026-07-14", Some("sha1"), "2026-07-16");
        // Second run advances watermark but reads no new head.
        ledger.upsert("jutro", "2026-07-20", None, "2026-07-20");
        assert_eq!(ledger.entries.len(), 1);
        let e = &ledger.entries[0];
        assert_eq!(e.watermark.as_deref(), Some("2026-07-20"));
        // Prior head preserved rather than clobbered with None.
        assert_eq!(e.downstream_head.as_deref(), Some("sha1"));
        assert_eq!(e.last_run.as_deref(), Some("2026-07-20"));
    }

    #[test]
    fn upsert_appends_new_downstream() {
        let mut ledger = Ledger::default();
        ledger.upsert("jutro", "2026-07-14", None, "2026-07-16");
        ledger.upsert("clockdump", "2026-07-15", None, "2026-07-16");
        assert_eq!(ledger.entries.len(), 2);
    }

    #[test]
    fn hash_level_classifies_headers() {
        assert_eq!(hash_level("# Title"), Some(1));
        assert_eq!(hash_level("## Section"), Some(2));
        assert_eq!(hash_level("### Entry"), Some(3));
        assert_eq!(hash_level("#### Sub"), None); // body, not a boundary
        assert_eq!(hash_level("no hash"), None);
        assert_eq!(hash_level("###no-space"), None);
    }

    const SAMPLE: &str = "\
# Template feedback

## Open divergences

### 2026-07-16 -- newest thing

Body of newest.

#### a sub-heading stays in the body

more body

### undated topic entry

Body with no date.

## Resolved

### 2026-07-10 -- older thing

Body of older.
";

    // A downstream that predates the per-entry `###` convention
    // dates a whole `##` section and lists entries as bullets
    // beneath it (jutro's format). Each dated `##` section is one
    // block; the level-1 title and undated `##` sections are not.
    const SAMPLE_L2: &str = "\
# Template feedback

## 2026-07-23

- **First entry.** Body of first.
- **Second entry.** Body of second.

## 2026-07-14

- **Older entry.** Body.

## 2026-07-10

- **Oldest entry.** Body.
";

    #[test]
    fn entry_blocks_splits_on_dated_level_two_sections() {
        let blocks = entry_blocks(SAMPLE_L2);
        assert_eq!(blocks.len(), 3);
        assert!(blocks[0].starts_with("## 2026-07-23"));
        assert!(blocks[0].contains("First entry"));
        assert!(blocks[0].contains("Second entry"));
        assert!(blocks[1].starts_with("## 2026-07-14"));
        assert!(blocks[2].starts_with("## 2026-07-10"));
    }

    #[test]
    fn entries_in_scope_cuts_dated_level_two_by_watermark() {
        let scoped = entries_in_scope(SAMPLE_L2, Some("2026-07-14"));
        assert_eq!(scoped.len(), 2);
        assert!(scoped[0].starts_with("## 2026-07-23"));
        assert!(scoped[1].starts_with("## 2026-07-14"));
    }

    #[test]
    fn entry_blocks_splits_on_level_three_only() {
        let blocks = entry_blocks(SAMPLE);
        assert_eq!(blocks.len(), 3);
        assert!(blocks[0].starts_with("### 2026-07-16"));
        // The level-4 sub-heading is captured inside the block.
        assert!(blocks[0].contains("#### a sub-heading"));
        assert!(blocks[1].starts_with("### undated topic"));
        assert!(blocks[2].starts_with("### 2026-07-10"));
    }

    #[test]
    fn entry_blocks_ignores_headers_inside_code_fences() {
        // A `###` line inside a fenced code sample in an entry
        // body must not split off a spurious entry (RT-4).
        let md = "\
## Open divergences

### 2026-07-16 -- entry about markdown

Here is an example header syntax:

```
### Not A Real Entry
## Also not a section
```

Back to prose.
";
        let blocks = entry_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("### Not A Real Entry"));
        assert!(blocks[0].contains("Back to prose."));
    }

    #[test]
    fn is_hex_sha_accepts_object_ids_and_rejects_paths() {
        assert!(is_hex_sha("deadbeef"));
        assert!(is_hex_sha("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"));
        assert!(!is_hex_sha("abc")); // too short
        assert!(!is_hex_sha("refs/heads/main")); // a ref, not a sha
        assert!(!is_hex_sha("../../etc/passwd")); // traversal payload
        assert!(!is_hex_sha("")); // empty
    }

    #[test]
    fn entries_in_scope_full_history_when_no_watermark() {
        assert_eq!(entries_in_scope(SAMPLE, None).len(), 3);
    }

    #[test]
    fn entries_in_scope_cuts_by_watermark_inclusive() {
        // Watermark on the newest entry's date: it is kept
        // (inclusive), the older dated entry and the undated
        // entry are dropped.
        let scoped = entries_in_scope(SAMPLE, Some("2026-07-16"));
        assert_eq!(scoped.len(), 1);
        assert!(scoped[0].starts_with("### 2026-07-16"));
    }

    #[test]
    fn entries_in_scope_drops_undated_on_watermark_run() {
        let scoped = entries_in_scope(SAMPLE, Some("2026-07-10"));
        // Both dated entries kept; undated one dropped.
        assert_eq!(scoped.len(), 2);
        assert!(scoped.iter().all(|b| extract_iso_date(b).is_some()));
    }

    #[test]
    fn downstream_name_takes_final_component() {
        assert_eq!(downstream_name("../jutro", None), "jutro");
        assert_eq!(downstream_name("../jutro/", None), "jutro");
        assert_eq!(downstream_name("/abs/path/clockdump", None), "clockdump");
    }

    #[test]
    fn downstream_name_override_wins_over_basename() {
        // A worktree layout basenames to the branch ("main");
        // the override supplies the real project key.
        assert_eq!(
            downstream_name("../ledgerstone/main", Some("ledgerstone")),
            "ledgerstone"
        );
    }

    #[test]
    fn is_iso_date_accepts_valid_and_rejects_junk() {
        assert!(is_iso_date("2026-07-16"));
        assert!(is_iso_date("1999-12-31"));
        assert!(!is_iso_date("2026-13-01")); // month 13
        assert!(!is_iso_date("2026-07-32")); // day 32
        assert!(!is_iso_date("2026-7-16")); // not zero-padded
        assert!(!is_iso_date("2026/07/16")); // wrong separator
        assert!(!is_iso_date("not a date"));
        assert!(!is_iso_date("2026-07-16 "));
    }

    #[test]
    fn extract_iso_date_finds_embedded_date() {
        assert_eq!(
            extract_iso_date("### 2026-07-16 -- some title"),
            Some("2026-07-16".to_string())
        );
        assert_eq!(extract_iso_date("## Open divergences"), None);
        assert_eq!(
            extract_iso_date("### fixed on 2026-01-02!"),
            Some("2026-01-02".to_string())
        );
        assert_eq!(extract_iso_date("short"), None);
    }

    #[test]
    fn extract_iso_date_handles_non_ascii_without_panic() {
        // Multibyte chars before/around the window must not
        // panic on a non-char-boundary slice (RT-1).
        assert_eq!(extract_iso_date("### Café UI regression"), None);
        assert_eq!(
            extract_iso_date("### café then 2026-07-16"),
            Some("2026-07-16".to_string())
        );
        assert_eq!(extract_iso_date("— em-dash lead —"), None);
    }

    #[test]
    fn plural_agrees_with_count() {
        assert_eq!(plural(1), "1 entry");
        assert_eq!(plural(0), "0 entries");
        assert_eq!(plural(5), "5 entries");
    }
}
