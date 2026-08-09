//! `cargo xtask sync-candidates` -- the deterministic file-delta
//! half of the `/template-sync` workflow.
//!
//! `/template-sync` is already SHA-delta based, but it surfaced
//! template-internal bookkeeping files (CHANGELOG, the feedback
//! file, the diary and review logs, per-issue docs) as sync
//! candidates. Those grow on every commit, so they became pure
//! review noise on every downstream sync. This command runs the
//! `git diff --name-status` delta, drops the never-sync set, and
//! categorizes what remains into a clean candidate table, so the
//! LLM only judges files that could actually be worth applying.

use std::process::Command;

use crate::helpers::{BACKFEED_LEDGER_REL, FEEDBACK_REL, workspace_root};

/// Template-internal paths that must never appear as sync
/// candidates. A trailing `/` marks a directory prefix; every
/// other entry is an exact path. These files are bookkeeping
/// that each project owns independently, so an upstream change
/// to them is never something a downstream should pull.
const NEVER_SYNC: &[&str] = &[
    "CHANGELOG.md",
    FEEDBACK_REL,
    BACKFEED_LEDGER_REL,
    "docs/developer/DIARY.md",
    "docs/developer/redteam-log.md",
    "docs/developer/artisan-log.md",
    "docs/issues/",
];

/// True when `path` is in the never-sync set (exact match, or
/// under a directory-prefix entry).
fn is_excluded(path: &str) -> bool {
    NEVER_SYNC.iter().any(|pat| {
        if pat.ends_with('/') {
            path.starts_with(pat)
        } else {
            path == *pat
        }
    })
}

/// Bucket a repo-relative path into the same categories
/// `/template-sync` presents. First match wins, so the order
/// encodes precedence (a path under `.github/` is
/// Infrastructure even though it is arguably config).
fn categorize(path: &str) -> &'static str {
    const INFRA_PREFIXES: &[&str] = &["xtask/", "scripts/", ".github/"];
    const INFRA_FILES: &[&str] = &[
        "build.ps1",
        "rust-toolchain.toml",
        "rustfmt.toml",
        "clippy.toml",
    ];
    const CLAUDE_PREFIXES: &[&str] = &[".claude/"];
    const BOILERPLATE_PREFIXES: &[&str] = &["crates/", "frontend/", "e2e/"];
    const DOC_PREFIXES: &[&str] = &["docs/"];
    const DOC_FILES: &[&str] = &["README.md", "llms.txt"];
    const PROJECT_FILES: &[&str] =
        &["Cargo.toml", "Cargo.lock", ".gitignore", ".editorconfig"];

    if INFRA_PREFIXES.iter().any(|p| path.starts_with(p))
        || INFRA_FILES.contains(&path)
    {
        "Infrastructure"
    } else if path == "CLAUDE.md"
        || CLAUDE_PREFIXES.iter().any(|p| path.starts_with(p))
    {
        "Claude config"
    } else if BOILERPLATE_PREFIXES.iter().any(|p| path.starts_with(p)) {
        "Boilerplate"
    } else if DOC_PREFIXES.iter().any(|p| path.starts_with(p))
        || DOC_FILES.contains(&path)
    {
        "Docs"
    } else if PROJECT_FILES.contains(&path) {
        "Project config"
    } else {
        "Other"
    }
}

/// Parse `git diff --name-status` output into `(status, path)`
/// rows. Renames (`R###\told\tnew`) and copies (`C###`) map to
/// their destination path (the last tab-separated field);
/// every other status has a single path. Blank lines are
/// dropped. Pure.
fn parse_name_status(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let status = parts.next()?.trim();
            if status.is_empty() {
                return None;
            }
            let path = parts.next_back()?.trim();
            if path.is_empty() {
                return None;
            }
            Some((status.to_string(), path.to_string()))
        })
        .collect()
}

/// A sync candidate surviving the never-sync filter.
#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    status: String,
    category: &'static str,
    path: String,
}

/// Build the candidate list from already-parsed `name-status`
/// rows: drop the never-sync set, categorize, sort by
/// (category, path) for a stable table. Pure.
fn candidates_from_rows(rows: Vec<(String, String)>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = rows
        .into_iter()
        .filter(|(_, p)| !is_excluded(p))
        .map(|(status, path)| Candidate {
            status,
            category: categorize(&path),
            path,
        })
        .collect();
    out.sort_by(|a, b| {
        a.category.cmp(b.category).then_with(|| a.path.cmp(&b.path))
    });
    out
}

/// Convenience wrapper for tests: parse `name-status` output
/// then build the candidate list. Pure.
#[cfg(test)]
fn candidates(output: &str) -> Vec<Candidate> {
    candidates_from_rows(parse_name_status(output))
}

/// Render the candidate table as text: one `status  category
/// path` row per candidate, or a single note when empty. Pure.
fn format_candidates(candidates: &[Candidate]) -> String {
    if candidates.is_empty() {
        return "(no sync candidates after the never-sync filter)".to_string();
    }
    candidates
        .iter()
        .map(|c| format!("{:<3} {:<15} {}", c.status, c.category, c.path))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run `git diff --name-status <last_synced>..template/main`
/// from the workspace root.
fn git_name_status(range: &str) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(workspace_root())
        .args(["diff", "--name-status", range])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git diff {range} failed -- has `template/main` been \
             fetched?\n{stderr}"
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|e| format!("git output was not UTF-8: {e}"))
}

/// `cargo xtask sync-candidates <last-synced>` -- print the
/// template file delta from `<last-synced>` to `template/main`,
/// minus the never-sync set, categorized. The `/template-sync`
/// workflow consumes this instead of scanning the raw diff.
pub fn sync_candidates(last_synced: &str) -> Result<(), String> {
    let range = format!("{last_synced}..template/main");
    let output = git_name_status(&range)?;
    let rows = parse_name_status(&output);
    let total = rows.len();
    let cands = candidates_from_rows(rows);
    eprintln!(
        "sync-candidates: {} candidate(s) after excluding {} never-sync \
         path(s)",
        cands.len(),
        total - cands.len()
    );
    println!("{}", format_candidates(&cands));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_excluded_matches_exact_and_prefix() {
        assert!(is_excluded("CHANGELOG.md"));
        assert!(is_excluded("docs/developer/template-feedback.md"));
        assert!(is_excluded("docs/developer/backfeed-ledger.toml"));
        assert!(is_excluded("docs/issues/some-issue.md")); // prefix
        // Not excluded: other docs, code.
        assert!(!is_excluded("docs/deployment.md"));
        assert!(!is_excluded("docs/developer/architecture.md"));
        assert!(!is_excluded("src/main.rs"));
        assert!(!is_excluded("CHANGELOG.md.bak"));
    }

    #[test]
    fn categorize_buckets_by_path() {
        assert_eq!(categorize("xtask/src/main.rs"), "Infrastructure");
        assert_eq!(categorize("scripts/e2e.sh"), "Infrastructure");
        assert_eq!(categorize(".github/workflows/ci.yml"), "Infrastructure");
        assert_eq!(categorize("build.ps1"), "Infrastructure");
        assert_eq!(categorize("CLAUDE.md"), "Claude config");
        assert_eq!(categorize(".claude/commands/commit.md"), "Claude config");
        assert_eq!(categorize("crates/rustbase/src/lib.rs"), "Boilerplate");
        assert_eq!(categorize("frontend/src/App.svelte"), "Boilerplate");
        assert_eq!(categorize("docs/deployment.md"), "Docs");
        assert_eq!(categorize("README.md"), "Docs");
        assert_eq!(categorize("Cargo.toml"), "Project config");
        assert_eq!(categorize(".gitignore"), "Project config");
        assert_eq!(categorize("some-random-file"), "Other");
    }

    #[test]
    fn parse_name_status_handles_statuses_and_renames() {
        let output = "\
M\tsrc/main.rs
A\tnew_file.rs
D\told_file.rs
R100\tdocs/old.md\tdocs/new.md

";
        let rows = parse_name_status(output);
        assert_eq!(
            rows,
            vec![
                ("M".to_string(), "src/main.rs".to_string()),
                ("A".to_string(), "new_file.rs".to_string()),
                ("D".to_string(), "old_file.rs".to_string()),
                // Rename maps to the destination path.
                ("R100".to_string(), "docs/new.md".to_string()),
            ]
        );
    }

    #[test]
    fn candidates_filters_sorts_and_categorizes() {
        let output = "\
M\tCHANGELOG.md
M\tdocs/issues/foo.md
M\txtask/src/main.rs
A\tdocs/deployment.md
M\tCLAUDE.md
";
        let cands = candidates(output);
        // CHANGELOG.md and docs/issues/foo.md are excluded.
        assert_eq!(cands.len(), 3);
        // Sorted by category: Claude config, Docs, Infrastructure.
        assert_eq!(cands[0].category, "Claude config");
        assert_eq!(cands[0].path, "CLAUDE.md");
        assert_eq!(cands[1].category, "Docs");
        assert_eq!(cands[2].category, "Infrastructure");
    }

    #[test]
    fn format_candidates_notes_empty_set() {
        assert!(format_candidates(&[]).contains("no sync candidates"));
    }

    #[test]
    fn format_candidates_renders_rows() {
        let out = format_candidates(&candidates("M\tCLAUDE.md\n"));
        assert!(out.contains("CLAUDE.md"));
        assert!(out.contains("Claude config"));
        assert!(out.starts_with('M'));
    }
}
