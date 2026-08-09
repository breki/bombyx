use std::path::Path;
use std::process::Command;

use crate::helpers::cargo_bin;

/// Maximum allowed exact duplication percentage
/// (production code only, tests excluded).
pub const THRESHOLD: f64 = 6.0;

/// Result from a duplication check.
pub struct DupesResult {
    /// Summary detail string.
    pub detail: String,
    /// Error message if failed (None = passed).
    pub error: Option<String>,
}

/// Run code duplication check (standalone).
pub fn dupes() -> Result<(), String> {
    let r = dupes_check()?;
    match r.error {
        None => {
            println!("Duplication OK ({})", r.detail);
            Ok(())
        }
        Some(err) => Err(err),
    }
}

/// Run duplication check, return structured result.
///
/// Uses `.status()` (streaming) because `code-dupes`
/// fails to detect source files when output is piped.
pub fn dupes_check() -> Result<DupesResult, String> {
    let src_dirs = discover_src_dirs()?;

    let threshold = format!("{THRESHOLD:.1}");
    for src_dir in &src_dirs {
        let status = Command::new("code-dupes")
            .args([
                "-p",
                src_dir,
                "--exclude-tests",
                "check",
                "--max-exact-percent",
                &threshold,
            ])
            .status()
            .map_err(|e| {
                format!(
                    "failed to run code-dupes: {e}\n  \
                     Install with: \
                     cargo install code-dupes"
                )
            })?;

        if !status.success() {
            return Ok(DupesResult {
                detail: String::new(),
                error: Some(format!(
                    "duplication exceeds \
                     {THRESHOLD}% threshold"
                )),
            });
        }
    }

    Ok(DupesResult {
        detail: format!("<= {THRESHOLD}%"),
        error: None,
    })
}

/// Discover `src/` directories for non-xtask workspace
/// members using `cargo metadata`.
fn discover_src_dirs() -> Result<Vec<String>, String> {
    let output = Command::new(cargo_bin())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .map_err(|e| format!("failed to run cargo metadata: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo metadata failed:\n{stderr}"));
    }

    let meta: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse cargo metadata: {e}"))?;

    let workspace_root = meta["workspace_root"]
        .as_str()
        .ok_or("missing workspace_root in metadata")?;

    let mut src_dirs = Vec::new();
    if let Some(packages) = meta["packages"].as_array() {
        for pkg in packages {
            let name = pkg["name"].as_str().unwrap_or("");
            // Skip xtask -- it's build tooling, not
            // production code.
            if name == "xtask" {
                continue;
            }
            let manifest = pkg["manifest_path"].as_str().unwrap_or("");
            // Derive src/ dir from Cargo.toml path.
            if let Some(pkg_dir) = Path::new(manifest).parent() {
                let src = pkg_dir.join("src");
                if src.is_dir() {
                    src_dirs.push(src.to_string_lossy().into_owned());
                }
            }
        }
    }

    if src_dirs.is_empty() {
        return Err(format!(
            "no src/ directories found in workspace \
             at {workspace_root}"
        ));
    }

    Ok(src_dirs)
}
