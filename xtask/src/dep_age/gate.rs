//! Changed-dependency cooldown gate -- the `validate` step.
//!
//! Checks only the `(name, version)` pairs newly present in
//! the working-tree lockfiles versus `HEAD`, so it costs
//! nothing (no network) on the common commit that leaves the
//! lockfiles untouched, and fires exactly at the moment a
//! dependency is adopted. A *whole-tree* continuous gate is
//! deliberately avoided -- it would flag every already-locked
//! version on every routine update.
//!
//! Scope caveat: the check covers **every** newly-locked
//! registry dependency, transitive ones included. A
//! lockfile-churning update (`cargo update` / `npm update`)
//! therefore can flag many fresh transitive versions at once;
//! see `RUSTBASE_DEP_AGE_ALLOW` and the supply-chain notes in
//! `CLAUDE.md` for the intended workflow.
//!
//! The shared registry-fetch and date-verdict machinery lives
//! in the parent [`super`] module; this submodule imports it.

use std::collections::HashSet;
use std::fs;
use std::process::Command;

use serde_json::Value;

use super::{
    COOLDOWN_DAYS, Ecosystem, age_in_days, cargo_version_date, fetch_registry,
    npm_version_date, parse_iso_date, today_days,
};
use crate::helpers::workspace_root;

/// Env var listing `name@version` entries exempt from the
/// changed-deps gate (comma-separated). This is the "stated
/// justification" escape hatch: a deliberately-adopted fresh
/// version, or a security fix, is named here so the gate
/// passes while leaving an auditable record of what was waved
/// through.
const ALLOW_ENV: &str = "RUSTBASE_DEP_AGE_ALLOW";

/// Combined outcome of the changed-deps gate, shaped like
/// [`crate::audit::AuditResult`] so `validate` treats the two
/// network gates the same way (fatal error vs non-fatal
/// warnings that degrade an offline run rather than block it).
pub struct DepAgeResult {
    /// Human-readable detail for the step line.
    pub detail: String,
    /// `Some` only when a changed dependency is within the
    /// cooldown (always fatal).
    pub error: Option<String>,
    /// Non-fatal problems (missing baseline, unreachable
    /// registry, unparseable response).
    pub warnings: Vec<String>,
}

/// Per-dependency check outcome.
#[derive(Debug, PartialEq, Eq)]
enum DepOutcome {
    /// Older than the cooldown -- fine.
    Aged,
    /// Within the cooldown window (fatal); carries the reason.
    Fresh(String),
    /// Could not be checked (network / tool / parse) -- a
    /// non-fatal warning, mirroring the audit gate offline.
    Unavailable(String),
    /// Named in the allow env var -- skipped, never fetched.
    Allowed,
}

/// Fold per-dependency outcomes into the combined result.
/// Pure, so the fatal-vs-warning boundary is unit-tested.
fn classify_dep_age(outcomes: &[(String, DepOutcome)]) -> DepAgeResult {
    if outcomes.is_empty() {
        return DepAgeResult {
            detail: "no dependency changes".into(),
            error: None,
            warnings: Vec::new(),
        };
    }
    let mut fresh: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let (mut aged, mut allowed) = (0usize, 0usize);
    for (label, outcome) in outcomes {
        match outcome {
            DepOutcome::Aged => aged += 1,
            DepOutcome::Allowed => allowed += 1,
            DepOutcome::Fresh(why) => fresh.push(format!("{label} ({why})")),
            DepOutcome::Unavailable(reason) => {
                warnings.push(format!("{label}: {reason}"));
            }
        }
    }
    // `warnings.len()` is the count of Unavailable outcomes.
    // Including it keeps the sub-counts reconciling with the
    // "N changed" total in the degraded-network case (where
    // nothing was actually checked).
    let detail = format!(
        "{} changed: {aged} aged, {allowed} allow-listed, {} fresh, \
         {} unchecked",
        outcomes.len(),
        fresh.len(),
        warnings.len(),
    );
    let error = if fresh.is_empty() {
        None
    } else {
        Some(format!(
            "dependencies within the {COOLDOWN_DAYS}-day cooldown: {}\n  \
             adopt only with a stated justification (security fixes \
             exempt); allow via {ALLOW_ENV}=name@version[,...]",
            fresh.join("; ")
        ))
    };
    DepAgeResult {
        detail,
        error,
        warnings,
    }
}

/// Parse the comma-separated allow list into a set of
/// `name@version` labels. Empty / whitespace entries dropped.
fn parse_allow(spec: Option<&str>) -> HashSet<String> {
    spec.into_iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Extract `(name, version)` pairs from a `Cargo.lock`,
/// **registry-sourced crates only**. A dependency-free scan of
/// the `[[package]]` blocks -- avoids adding a TOML parser for
/// a machine-generated, regular format. Lines inside a
/// `dependencies = [...]` array start with a quote, not
/// `name = `/`version = `, so they are ignored.
///
/// Only crates with a `source = "registry+..."` line are
/// returned: local workspace crates (no `source`, e.g. this
/// project's own crates on a version bump) and git deps
/// (`source = "git+..."`) have no crates.io publish date to
/// check, so including them would spuriously warn on every
/// release commit.
pub(super) fn parse_cargo_lock(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut registry = false;
    let mut flush = |name: &mut Option<String>,
                     version: &mut Option<String>,
                     registry: &mut bool| {
        if let (Some(n), Some(v)) = (name.take(), version.take())
            && *registry
        {
            out.push((n, v));
        }
        *registry = false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            flush(&mut name, &mut version, &mut registry);
        } else if let Some(v) = line.strip_prefix("name = ") {
            name = Some(unquote(v));
        } else if let Some(v) = line.strip_prefix("version = ") {
            version = Some(unquote(v));
        } else if let Some(v) = line.strip_prefix("source = ") {
            registry = unquote(v).starts_with("registry+");
        }
    }
    flush(&mut name, &mut version, &mut registry);
    out
}

/// Strip surrounding double quotes from a TOML string value.
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

/// Extract `(name, version)` pairs from an npm lockfile
/// (`lockfileVersion` 2/3 `packages` map), **registry-sourced
/// packages only**. The map is keyed by install path
/// (`node_modules/<name>`, `node_modules/a/node_modules/b`);
/// the name is the segment after the final `node_modules/`.
///
/// Only entries whose `resolved` is an `http(s)` tarball URL
/// are returned -- the root package (empty key), workspace
/// links (`link: true`, relative `resolved`) and `file:` deps
/// have no npm-registry publish date to check. An unparseable
/// / older-format lockfile yields an empty list (nothing to
/// check).
fn parse_npm_lock(text: &str) -> Vec<(String, String)> {
    let Ok(json) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(packages) = json["packages"].as_object() else {
        return Vec::new();
    };
    packages
        .iter()
        .filter(|(key, _)| !key.is_empty())
        .filter(|(_, val)| {
            val["resolved"]
                .as_str()
                .is_some_and(|r| r.starts_with("http"))
        })
        .filter_map(|(key, val)| {
            let name = key.rsplit("node_modules/").next().unwrap_or(key);
            val["version"]
                .as_str()
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect()
}

/// `(name, version)` pairs present in `new` but not in `old`
/// -- both freshly added packages and version bumps of an
/// existing package (the bumped tuple is simply not in `old`).
pub(super) fn new_locked_versions(
    old: &[(String, String)],
    new: &[(String, String)],
) -> Vec<(String, String)> {
    let old_set: HashSet<&(String, String)> = old.iter().collect();
    new.iter()
        .filter(|pair| !old_set.contains(*pair))
        .cloned()
        .collect()
}

/// `git show HEAD:<rel>` from the workspace root. `None` when
/// the path is absent at `HEAD` or git can't run (no commit
/// yet, not a repo) -- the caller degrades that to a warning,
/// never a hard failure. Run via the process working directory
/// (`current_dir`), not the `git -C` flag, to keep the command
/// coverable by a blanket permission rule.
pub(super) fn git_show(rel: &str) -> Option<String> {
    let output = Command::new("git")
        .current_dir(workspace_root())
        .args(["show", &format!("HEAD:{rel}")])
        .output()
        .ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

/// Network check of one dependency version against the
/// cooldown. Thin wrapper over the shared fetch + date +
/// verdict helpers; the branchy logic lives in those
/// (already unit-tested) pieces.
fn check_one(eco: Ecosystem, name: &str, version: &str) -> DepOutcome {
    let json = match fetch_registry(eco, name) {
        Ok(j) => j,
        Err(e) => return DepOutcome::Unavailable(e),
    };
    let dated = match eco {
        Ecosystem::Npm => npm_version_date(&json, Some(version)),
        Ecosystem::Cargo => cargo_version_date(&json, Some(version)),
    };
    let published = match dated {
        Ok((_, p)) => p,
        Err(e) => return DepOutcome::Unavailable(e),
    };
    match parse_iso_date(&published) {
        Ok(days) => {
            let age = age_in_days(days, today_days());
            if age < COOLDOWN_DAYS {
                DepOutcome::Fresh(format!("published {age}d ago"))
            } else {
                DepOutcome::Aged
            }
        }
        Err(e) => DepOutcome::Unavailable(e),
    }
}

/// Diff one lockfile against `HEAD` and append an outcome per
/// newly-adopted dependency. A missing current file means the
/// ecosystem is absent (skip); a missing `HEAD` baseline with
/// the file present is surfaced as a warning (can't diff).
fn collect_changes(
    eco: Ecosystem,
    rel: &str,
    parser: fn(&str) -> Vec<(String, String)>,
    allow: &HashSet<String>,
    outcomes: &mut Vec<(String, DepOutcome)>,
    warnings: &mut Vec<String>,
) {
    let Ok(current_text) = fs::read_to_string(workspace_root().join(rel))
    else {
        return; // no such lockfile -> ecosystem absent
    };
    let Some(baseline_text) = git_show(rel) else {
        warnings.push(format!(
            "{rel}: no HEAD baseline to diff against -- skipping cooldown \
             check for this lockfile"
        ));
        return;
    };
    let mut changed =
        new_locked_versions(&parser(&baseline_text), &parser(&current_text));
    changed.sort();
    for (name, version) in changed {
        let label = format!("{name}@{version}");
        let outcome = if allow.contains(&label) {
            DepOutcome::Allowed
        } else {
            check_one(eco, &name, &version)
        };
        outcomes.push((label, outcome));
    }
}

/// Run the changed-dependency cooldown gate: check only the
/// `(name, version)` pairs newly present in the working-tree
/// lockfiles versus `HEAD`. Free (no network) when the
/// lockfiles are unchanged.
pub fn check_changed_deps() -> DepAgeResult {
    let allow = parse_allow(std::env::var(ALLOW_ENV).ok().as_deref());
    let mut outcomes: Vec<(String, DepOutcome)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    collect_changes(
        Ecosystem::Cargo,
        "Cargo.lock",
        parse_cargo_lock,
        &allow,
        &mut outcomes,
        &mut warnings,
    );
    collect_changes(
        Ecosystem::Npm,
        "frontend/package-lock.json",
        parse_npm_lock,
        &allow,
        &mut outcomes,
        &mut warnings,
    );

    let mut result = classify_dep_age(&outcomes);
    // Baseline/parse warnings precede the per-dep warnings the
    // classifier collected.
    warnings.extend(result.warnings);
    result.warnings = warnings;
    result
}

/// Standalone `cargo xtask dep-age-check`. Like the audit
/// command, this errors when the check could not complete
/// (a human who ran it explicitly wants to know), whereas the
/// `validate` step degrades that to a warning.
pub fn dep_age_check() -> Result<(), String> {
    let r = check_changed_deps();
    if let Some(err) = r.error {
        eprintln!("  {}", r.detail);
        return Err(err);
    }
    for w in &r.warnings {
        eprintln!("  warning: {w}");
    }
    if r.warnings.is_empty() {
        println!("Dep-age OK ({})", r.detail);
        Ok(())
    } else {
        Err("dep-age check could not complete (see warnings above)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cargo_lock_registry_only_ignoring_locals_and_deps() {
        let lock = r#"
version = 4

[[package]]
name = "aho-corasick"
version = "1.1.2"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"
dependencies = [
 "memchr",
]

[[package]]
name = "rustbase"
version = "0.13.0"
dependencies = [
 "aho-corasick",
]
"#;
        // Registry crate kept; the local workspace crate
        // (no `source`, e.g. this project on a version bump),
        // the `version = 4` header, and the quoted dependency
        // line are all excluded.
        assert_eq!(
            parse_cargo_lock(lock),
            vec![("aho-corasick".into(), "1.1.2".into())]
        );
    }

    #[test]
    fn parse_npm_lock_registry_only_skipping_root_and_links() {
        let lock = r#"{"lockfileVersion":3,"packages":{
            "":{"name":"frontend","version":"0.0.0"},
            "node_modules/vite":{"version":"8.0.1",
                "resolved":"https://registry.npmjs.org/vite/-/vite-8.0.1.tgz"},
            "node_modules/vite/node_modules/esbuild":{"version":"0.24.0",
                "resolved":"https://registry.npmjs.org/esbuild/-/esbuild-0.24.0.tgz"},
            "node_modules/local":{"version":"1.0.0","link":true,
                "resolved":"../local"}
        }}"#;
        let mut pkgs = parse_npm_lock(lock);
        pkgs.sort();
        // Root ("") and the local link (relative `resolved`)
        // dropped; nested name is the final path segment.
        assert_eq!(
            pkgs,
            vec![
                ("esbuild".into(), "0.24.0".into()),
                ("vite".into(), "8.0.1".into()),
            ]
        );
    }

    #[test]
    fn parse_npm_lock_bad_json_is_empty() {
        assert!(parse_npm_lock("not json").is_empty());
        assert!(parse_npm_lock("{}").is_empty());
    }

    #[test]
    fn new_locked_versions_detects_adds_and_bumps() {
        let old = vec![
            ("serde".to_string(), "1.0.100".to_string()),
            ("tokio".to_string(), "1.35.0".to_string()),
        ];
        let new = vec![
            ("serde".to_string(), "1.0.100".to_string()), // unchanged
            ("tokio".to_string(), "1.36.0".to_string()),  // bumped
            ("anyhow".to_string(), "1.0.80".to_string()), // added
        ];
        let mut got = new_locked_versions(&old, &new);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("anyhow".into(), "1.0.80".into()),
                ("tokio".into(), "1.36.0".into()),
            ]
        );
    }

    #[test]
    fn parse_allow_splits_trims_and_handles_none() {
        let a = parse_allow(Some(" serde@1.0.0 , anyhow@1.0.80 ,"));
        assert_eq!(a.len(), 2);
        assert!(a.contains("serde@1.0.0"));
        assert!(a.contains("anyhow@1.0.80"));
        assert!(parse_allow(None).is_empty());
        assert!(parse_allow(Some("")).is_empty());
    }

    #[test]
    fn classify_dep_age_empty_is_clean() {
        let r = classify_dep_age(&[]);
        assert!(r.error.is_none());
        assert!(r.warnings.is_empty());
        assert_eq!(r.detail, "no dependency changes");
    }

    #[test]
    fn classify_dep_age_fresh_is_fatal() {
        let r = classify_dep_age(&[
            ("a@1.0.0".into(), DepOutcome::Aged),
            (
                "b@2.0.0".into(),
                DepOutcome::Fresh("published 2d ago".into()),
            ),
        ]);
        let err = r.error.unwrap();
        assert!(err.contains("b@2.0.0"));
        assert!(err.contains("published 2d ago"));
        assert!(r.detail.contains("1 fresh"));
        assert!(r.detail.contains("1 aged"));
    }

    #[test]
    fn classify_dep_age_unavailable_is_warning_not_fatal() {
        let r = classify_dep_age(&[(
            "a@1.0.0".into(),
            DepOutcome::Unavailable("network down".into()),
        )]);
        assert!(r.error.is_none());
        assert_eq!(r.warnings, vec!["a@1.0.0: network down".to_string()]);
        // The detail sub-counts reconcile with "1 changed".
        assert!(r.detail.contains("1 changed"));
        assert!(r.detail.contains("1 unchecked"));
    }

    #[test]
    fn classify_dep_age_allowed_counts_but_fresh_still_fatal() {
        let r = classify_dep_age(&[
            ("a@1.0.0".into(), DepOutcome::Allowed),
            (
                "b@2.0.0".into(),
                DepOutcome::Fresh("published 1d ago".into()),
            ),
        ]);
        // The allow-listed one is not fatal; the fresh one is.
        assert!(r.error.is_some());
        assert!(r.detail.contains("1 allow-listed"));
    }
}
