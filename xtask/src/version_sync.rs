//! Rewrites version sentinels in docs from `Cargo.toml`.
//!
//! `docs/quickstart.md`'s install snippet pins a bombyx version to
//! build the release download URL, and `/release` bumps
//! `crates/bombyx/Cargo.toml` without touching the snippet, so it
//! drifts and a reader installs a stale release. This reads the
//! crate version and, in every markdown file carrying a
//! `<!-- version: X -->` sentinel, rewrites that sentinel and any
//! `VERSION=<semver>` line in the same file to match. `/release`
//! runs it during the bump, so the synced file rides in the one
//! bookkeeping commit rather than a new one.
//!
//! The sentinel is the opt-in: a file without it is never touched,
//! so an unrelated `VERSION=` line elsewhere is safe.

use std::fs;
use std::path::{Path, PathBuf};

use crate::helpers::workspace_root;

/// Sync every version sentinel to the crate version, in place.
pub fn version_sync() -> Result<(), String> {
    let root = workspace_root();
    let version = crate_version(&root)?;
    let mut changed = Vec::new();
    for path in candidate_files(&root) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(new) = rewrite_text(&text, &version) {
            fs::write(&path, new)
                .map_err(|e| format!("write {}: {e}", path.display()))?;
            let rel = path.strip_prefix(&root).unwrap_or(&path);
            changed.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    if changed.is_empty() {
        println!("Version sentinels already at {version}");
    } else {
        changed.sort();
        println!("Version-sync: set {version} in {}", changed.join(", "));
    }
    Ok(())
}

/// The bombyx crate version, from `crates/bombyx/Cargo.toml`.
fn crate_version(root: &Path) -> Result<String, String> {
    let manifest = root.join("crates/bombyx/Cargo.toml");
    let text = fs::read_to_string(&manifest)
        .map_err(|e| format!("read {}: {e}", manifest.display()))?;
    parse_package_version(&text).ok_or_else(|| {
        format!("no [package] version in {}", manifest.display())
    })
}

/// The `version` under `[package]` in a Cargo manifest.
///
/// Reads only the top `[package]` table, so a dependency's own
/// `version =` and a `[[bin]]` name below it are never mistaken for
/// the crate version.
fn parse_package_version(toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml.lines() {
        let t = line.trim();
        if let Some(section) = t.strip_prefix('[') {
            in_package = section.starts_with("package]");
            continue;
        }
        if in_package
            && let Some(rest) = t.strip_prefix("version")
            && let Some(val) = rest.trim_start().strip_prefix('=')
        {
            return Some(val.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Files a version sentinel might live in: `README.md` and every
/// markdown file under `docs/`.
fn candidate_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("README.md")];
    collect_md(&root.join("docs"), &mut out);
    out.retain(|p| p.is_file());
    out
}

/// Append every `.md` file under `dir`, recursively.
fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().is_some_and(|x| x == "md") {
            out.push(path);
        }
    }
}

/// Rewrite the version sentinels in one file's text.
///
/// Returns the new text when it changes, or `None` when the file
/// carries no `<!-- version: -->` sentinel or is already in sync.
/// Only in a file that carries the sentinel is a `VERSION=<semver>`
/// line rewritten too, so the sentinel is the file's opt-in.
fn rewrite_text(text: &str, version: &str) -> Option<String> {
    if !text.contains("<!-- version:") {
        return None;
    }
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let rewritten = if trimmed.starts_with("<!-- version:")
            && trimmed.ends_with("-->")
        {
            Some(format!("{indent}<!-- version: {version} -->"))
        } else if trimmed.starts_with("VERSION=") {
            Some(format!("{indent}VERSION={version}"))
        } else {
            None
        };
        match rewritten {
            Some(want) => {
                changed |= want != line;
                out.push_str(&want);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    // `lines()` drops a trailing newline; restore the original end.
    if !text.ends_with('\n') {
        out.pop();
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_package_version_ignoring_others() {
        let toml = "\
[package]
name = \"bombyx\"
version = \"0.6.0\"
edition = \"2024\"

[dependencies]
serde = { version = \"1\" }

[[bin]]
name = \"bombyx\"
";
        assert_eq!(parse_package_version(toml).as_deref(), Some("0.6.0"));
    }

    #[test]
    fn no_package_table_yields_none() {
        assert_eq!(parse_package_version("[dependencies]\nx = \"1\"\n"), None);
    }

    #[test]
    fn rewrites_a_drifted_sentinel_and_version_line() {
        let text = "\
## Install

<!-- version: 0.1.0 -->

```bash
VERSION=0.1.0
```
";
        let out = rewrite_text(text, "0.6.0").expect("should change");
        assert!(out.contains("<!-- version: 0.6.0 -->"));
        assert!(out.contains("VERSION=0.6.0"));
        assert!(!out.contains("0.1.0"));
    }

    #[test]
    fn leaves_an_in_sync_file_unchanged() {
        let text = "<!-- version: 0.6.0 -->\nVERSION=0.6.0\n";
        assert_eq!(rewrite_text(text, "0.6.0"), None);
    }

    #[test]
    fn a_file_without_a_sentinel_is_never_touched() {
        // A bare VERSION= must not be rewritten when no sentinel
        // opts the file in.
        assert_eq!(rewrite_text("VERSION=0.1.0\n", "0.6.0"), None);
    }

    #[test]
    fn preserves_a_missing_trailing_newline() {
        let out = rewrite_text("<!-- version: 0.1.0 -->", "0.6.0").unwrap();
        assert_eq!(out, "<!-- version: 0.6.0 -->");
    }
}
