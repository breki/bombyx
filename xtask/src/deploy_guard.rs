//! Release-tag guard for `cargo xtask deploy`.
//!
//! Deploy must ship only a *released* commit: `HEAD` on an
//! annotated `vX.Y.Z` tag matching
//! `crates/rustbase/Cargo.toml`, with a clean working tree.
//! This ties "publish to production" to "cut a release"
//! (`/release`) via the build system rather than human
//! discipline -- an untagged or dirty `HEAD` cannot be
//! deployed by accident.
//!
//! The branchy decision ([`check_release_state`]) and the
//! manifest version parse ([`package_version`]) are pure and
//! unit-tested; the git queries are thin wrappers.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::helpers::workspace_root;

/// Workspace-relative path of the primary crate's manifest --
/// the single source of truth for the release version. A
/// downstream that renames the crate changes this one line
/// instead of hunting every hard-coded `crates/rustbase`
/// occurrence. (The `/release` command doc references the same
/// path in prose; keep the two in step on a rename.)
const PRIMARY_MANIFEST: &str = "crates/rustbase/Cargo.toml";

/// Abort the deploy unless `HEAD` is a clean, tagged release
/// whose tag matches the crate version.
pub fn require_release_tag() -> Result<(), String> {
    let root = workspace_root();
    let version = crate_version(&root)?;
    let clean = working_tree_clean(&root)?;
    let tag = head_release_tag(&root);
    check_release_state(&version, tag.as_deref(), clean)
}

/// Resolve the crate's version, following
/// `version.workspace = true` to the workspace root's
/// `[workspace.package]` version when the crate inherits it
/// (a common Cargo pattern in derived projects).
fn crate_version(root: &Path) -> Result<String, String> {
    let path = root.join(PRIMARY_MANIFEST);
    let manifest = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    match package_version(&manifest) {
        Some(PackageVersion::Literal(v)) => Ok(v),
        Some(PackageVersion::Inherited) => {
            let root_path = root.join("Cargo.toml");
            let root_manifest =
                fs::read_to_string(&root_path).map_err(|e| {
                    format!("cannot read {}: {e}", root_path.display())
                })?;
            workspace_package_version(&root_manifest).ok_or_else(|| {
                format!(
                    "{PRIMARY_MANIFEST} has version.workspace = true but \
                     Cargo.toml has no [workspace.package] version"
                )
            })
        }
        None => Err(format!("no [package] version in {PRIMARY_MANIFEST}")),
    }
}

/// A `[package]` version declaration.
#[derive(Debug, PartialEq, Eq)]
enum PackageVersion {
    /// A literal `version = "X.Y.Z"`.
    Literal(String),
    /// `version.workspace = true` -- inherited from the
    /// workspace root's `[workspace.package]`.
    Inherited,
}

/// The `[package]` version declaration from a `Cargo.toml`,
/// tracking the current `[section]` so a dependency table's
/// `version` cannot shadow it. Pure. `None` when `[package]`
/// has no `version` key.
fn package_version(manifest: &str) -> Option<PackageVersion> {
    let mut in_package = false;
    for line in manifest.lines() {
        let trimmed = strip_comment(line).trim();
        if let Some(name) = section_name(trimmed) {
            in_package = name == "package";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("version") else {
            continue;
        };
        let rest = rest.trim_start();
        if rest.starts_with(".workspace") {
            return Some(PackageVersion::Inherited);
        }
        if let Some(value) = rest.strip_prefix('=') {
            let v = unquote_value(value);
            if !v.is_empty() {
                return Some(PackageVersion::Literal(v.to_string()));
            }
        }
    }
    None
}

/// The `[workspace.package]` `version` from a root manifest.
/// Pure. `None` when absent.
fn workspace_package_version(manifest: &str) -> Option<String> {
    let mut in_ws_package = false;
    for line in manifest.lines() {
        let trimmed = strip_comment(line).trim();
        if let Some(name) = section_name(trimmed) {
            in_ws_package = name == "workspace.package";
            continue;
        }
        if in_ws_package
            && let Some(rest) = trimmed.strip_prefix("version")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let v = unquote_value(value);
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// The name inside a `[section]` / `[[array.table]]` header,
/// or `None` when the line is not a section header. Requires
/// the trimmed line to both open with `[` and close with `]`
/// so an array *value* continuation line does not read as a
/// header.
fn section_name(trimmed: &str) -> Option<&str> {
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        Some(trimmed.trim_matches(|c| c == '[' || c == ']').trim())
    } else {
        None
    }
}

/// Drop a trailing `# ...` inline comment (neither a section
/// header nor a version value legitimately contains `#`).
fn strip_comment(line: &str) -> &str {
    line.split('#').next().unwrap_or(line)
}

/// Trim a TOML string value: surrounding whitespace, then
/// either single- or double-quote delimiters.
fn unquote_value(value: &str) -> &str {
    value.trim().trim_matches(|c| c == '"' || c == '\'')
}

/// Decide whether the current git state permits a deploy.
/// Pure, so the fail-safe branches are unit-tested.
fn check_release_state(
    version: &str,
    tag: Option<&str>,
    clean: bool,
) -> Result<(), String> {
    if !clean {
        return Err("working tree is not clean -- commit or stash \
                    before deploying; deploy ships a released, \
                    reviewed commit"
            .into());
    }
    let expected = format!("v{version}");
    match tag {
        Some(t) if t == expected => Ok(()),
        Some(t) => Err(format!(
            "HEAD tag {t} does not match crate version {expected} -- \
             run /release to cut {expected}, or check out the matching \
             tag before deploying"
        )),
        None => Err(format!(
            "HEAD is not on a release tag -- run /release to cut \
             {expected} before deploying"
        )),
    }
}

/// `true` when `git status --porcelain` is empty. Run through
/// the process working directory, not `git -C`, to stay
/// coverable by a blanket permission rule.
fn working_tree_clean(root: &Path) -> Result<bool, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["status", "--porcelain"])
        .output()
        .map_err(|e| format!("failed to run git status: {e}"))?;
    if !out.status.success() {
        return Err("git status failed (not a git repository?)".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// The annotated `v*` tag exactly at `HEAD`, or `None` when
/// `HEAD` is not on such a tag (or git cannot run). Uses
/// `--exact-match`, which only considers annotated tags.
fn head_release_tag(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["describe", "--exact-match", "--match", "v*", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!tag.is_empty()).then_some(tag)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literal(v: &str) -> PackageVersion {
        PackageVersion::Literal(v.to_string())
    }

    #[test]
    fn package_version_reads_a_literal() {
        let toml = "[package]\nname = \"rustbase\"\nversion = \"0.15.0\"\n";
        assert_eq!(package_version(toml), Some(literal("0.15.0")));
    }

    #[test]
    fn package_version_detects_workspace_inheritance() {
        // `version.workspace = true` -> Inherited, so the
        // caller falls back to [workspace.package] rather than
        // failing with a misleading "no version" (RT-1).
        let toml = "[package]\nname = \"rustbase\"\nversion.workspace = true\n";
        assert_eq!(package_version(toml), Some(PackageVersion::Inherited));
    }

    #[test]
    fn package_version_is_not_shadowed_by_a_dependency_version() {
        // A dependency's own `version` must not win over the
        // package version, regardless of ordering.
        let toml = "[package]\nversion = \"0.15.0\"\n\n\
                    [dependencies.serde]\nversion = \"1.0.200\"\n";
        assert_eq!(package_version(toml), Some(literal("0.15.0")));
    }

    #[test]
    fn package_version_ignores_a_leading_dependency_table() {
        // Even when a dependency table appears first, only the
        // [package] version is returned.
        let toml = "[dependencies]\nserde = \"1\"\n\n\
                    [package]\nversion = \"2.3.4\"\n";
        assert_eq!(package_version(toml), Some(literal("2.3.4")));
    }

    #[test]
    fn package_version_handles_comment_and_single_quotes() {
        // Inline comment stripped, single-quote delimiters
        // trimmed (RT-2).
        let toml = "[package]\nversion = '0.15.0' # release\n";
        assert_eq!(package_version(toml), Some(literal("0.15.0")));
    }

    #[test]
    fn package_version_survives_a_multiline_array_before_version() {
        // A multi-line array value's closing `]` line must not
        // be read as a section header ending [package] (RT-2).
        let toml = "[package]\nkeywords = [\n  \"a\",\n]\n\
                    version = \"9.9.9\"\n";
        assert_eq!(package_version(toml), Some(literal("9.9.9")));
    }

    #[test]
    fn package_version_none_when_absent() {
        assert_eq!(package_version("[dependencies]\nserde = \"1\"\n"), None);
    }

    #[test]
    fn workspace_package_version_reads_the_value() {
        let toml =
            "[workspace.package]\nversion = \"1.2.3\"\nedition = \"2024\"\n";
        assert_eq!(workspace_package_version(toml).as_deref(), Some("1.2.3"));
    }

    #[test]
    fn workspace_package_version_none_without_the_table() {
        assert_eq!(
            workspace_package_version("[workspace]\nmembers = []\n"),
            None
        );
    }

    #[test]
    fn check_release_state_ok_when_clean_and_tag_matches() {
        assert!(check_release_state("0.15.0", Some("v0.15.0"), true).is_ok());
    }

    #[test]
    fn check_release_state_rejects_dirty_tree() {
        let err =
            check_release_state("0.15.0", Some("v0.15.0"), false).unwrap_err();
        assert!(err.contains("not clean"));
    }

    #[test]
    fn check_release_state_rejects_untagged_head() {
        let err = check_release_state("0.15.0", None, true).unwrap_err();
        assert!(err.contains("not on a release tag"));
        assert!(err.contains("v0.15.0"));
    }

    #[test]
    fn check_release_state_rejects_mismatched_tag() {
        let err =
            check_release_state("0.15.0", Some("v0.14.0"), true).unwrap_err();
        assert!(err.contains("does not match"));
        assert!(err.contains("v0.14.0"));
        assert!(err.contains("v0.15.0"));
    }
}
