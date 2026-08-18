//! `cargo xtask licenses` -- generates the third-party attribution
//! file shipped in the release archives.
//!
//! bombyx is MIT and its dependency tree runs to around ninety
//! crates. Every one of them is permissive, but permissive is not
//! obligation-free: MIT and Apache-2.0 both require the licence and
//! copyright notice to travel with a distributed binary, and
//! `unicode-ident` carries `Unicode-3.0` through an SPDX `AND`,
//! which is required rather than optional. The release archives
//! previously held only bombyx's own `LICENSE`, so that attribution
//! went unmet the moment binaries started being published.
//!
//! The licence *texts* come from the crate sources already unpacked
//! in the cargo registry, so nothing is downloaded. A crate shipping
//! no licence file is listed with its SPDX expression and called out
//! explicitly rather than omitted, because dropping one silently
//! would make the file look complete when it is not.
//!
//! **The list is the whole dependency tree, not what the binary
//! links.** It includes dev-dependencies, the build tooling's own
//! deps and crates for other targets. Over-attribution breaks no
//! licence, and the rendered header says so -- but pruning to the
//! linked set is the better answer and is recorded as a deferred
//! item rather than guessed at.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::helpers::run_cargo_capture;

/// Default output path, relative to the workspace root.
pub const DEFAULT_OUT: &str = "THIRD-PARTY-LICENSES";

/// One dependency's attribution.
#[derive(Debug, PartialEq, Eq)]
pub struct Attribution {
    /// Crate name.
    pub name: String,
    /// Crate version.
    pub version: String,
    /// SPDX expression from the manifest, if it had one.
    pub license: Option<String>,
    /// Licence texts found beside the crate's manifest.
    pub texts: Vec<(String, String)>,
}

impl Attribution {
    /// Whether this crate shipped no licence text at all.
    #[must_use]
    pub fn text_missing(&self) -> bool {
        self.texts.is_empty()
    }
}

/// Whether `name` is a licence or notice file.
///
/// `NOTICE` counts: Apache-2.0 section 4(d) requires it to be
/// carried, and a crate shipping one is asserting there is something
/// to carry.
#[must_use]
pub fn is_license_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // An **allow-list** of extensions, not a deny-list. A deny-list
    // enumerating `rs` and `spdx` silently starts including whatever
    // format nobody has invented yet (`LICENSE.toml`, a `.bk`
    // backup), and the safe default here is the other way round: a
    // spurious text is harmless, a missing one is a compliance gap
    // nothing reports. `None` covers the common bare `LICENSE`.
    let ext = Path::new(&lower).extension().and_then(|e| e.to_str());
    if !matches!(ext, None | Some("txt" | "md" | "html")) {
        return false;
    }
    // `copyright` and `authors` are here because crates in this tree
    // ship them and they are exactly the notice MIT and Apache-2.0
    // require to travel: `rustix` and `linux-raw-sys` explain their
    // triple licence and the LLVM exception in `COPYRIGHT`, not in
    // `LICENSE-MIT`. Missing them was invisible, because both crates
    // also ship a `LICENSE-*`, so neither ever appeared in the
    // "ships no licence file" list.
    lower.starts_with("license")
        || lower.starts_with("licence")
        || lower.starts_with("copying")
        || lower.starts_with("copyright")
        || lower.starts_with("authors")
        || lower.starts_with("notice")
}

/// Collects the licence texts sitting beside a crate's manifest.
fn texts_beside(manifest: &Path) -> Vec<(String, String)> {
    let Some(dir) = manifest.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .flatten()
        // `path().is_file()` follows symlinks; `e.file_type()` does
        // not, and a crate whose `LICENSE-APACHE` is a symlink to a
        // shared file -- common in workspace-published and vendored
        // trees -- was silently dropped, landing the crate in the
        // "ships no licence file" list where it did not belong.
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !is_license_file(&name) {
                return None;
            }
            // Lossy on purpose: a licence file is text, and a stray
            // byte is no reason to omit an attribution.
            let body = std::fs::read(e.path()).ok()?;
            Some((name, String::from_utf8_lossy(&body).into_owned()))
        })
        .collect();
    // Deterministic, so regenerating produces no diff.
    found.sort();
    found
}

/// Reads `cargo metadata` and returns one entry per third-party
/// crate, sorted by name then version.
///
/// # Errors
///
/// Returns a message when `cargo metadata` cannot be run or parsed.
pub fn collect() -> Result<Vec<Attribution>, String> {
    let out = run_cargo_capture(&[
        "metadata",
        "--format-version",
        "1",
        "--all-features",
    ])?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let json: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("cargo metadata produced invalid JSON: {e}"))?;
    Ok(attributions_from(&json))
}

/// The pure half of [`collect`]: metadata JSON to attributions.
///
/// Workspace members are excluded -- this repository's own code is
/// covered by the `LICENSE` already in every archive. Split out so
/// that exclusion and the ordering are tested without running cargo.
#[must_use]
pub fn attributions_from(json: &Value) -> Vec<Attribution> {
    let members: Vec<&str> = json["workspace_members"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    // Keyed by the package **id**, not by (name, version). Those two
    // are not unique: a `[patch]` or path replacement of a crates.io
    // crate reports the same name and version as the original, and
    // keying on the pair dropped one of them -- silently, and the
    // one likely to be dropped is the fork, whose licence is the one
    // that differs. The id is a `PackageIdSpec` and is unique.
    //
    // Sorted by (name, version) for output, so regenerating produces
    // no diff, with the id breaking ties.
    let mut by_key: BTreeMap<(String, String, String), Attribution> =
        BTreeMap::new();
    for pkg in json["packages"].as_array().into_iter().flatten() {
        let id = pkg["id"].as_str().unwrap_or_default();
        if members.contains(&id) {
            continue;
        }
        let name = pkg["name"].as_str().unwrap_or_default().to_owned();
        let version = pkg["version"].as_str().unwrap_or_default().to_owned();
        let manifest = pkg["manifest_path"].as_str().unwrap_or_default();
        by_key.insert(
            (name.clone(), version.clone(), id.to_owned()),
            Attribution {
                name,
                version,
                license: pkg["license"].as_str().map(str::to_owned),
                texts: texts_beside(Path::new(manifest)),
            },
        );
    }
    by_key.into_values().collect()
}

/// Placeholder for a crate whose manifest stated no licence.
const NO_SPDX: &str = "no SPDX expression";

/// Renders the attribution file.
#[must_use]
pub fn render(crates: &[Attribution]) -> String {
    // Collected as parts and joined once, rather than pushing
    // `format!` results into a growing `String` -- one allocation
    // per part either way, and this keeps every piece visible as a
    // value instead of a side effect.
    let rule = "=".repeat(70);
    let mut parts: Vec<String> = vec![
        "THIRD-PARTY LICENCES\n\n".to_owned(),
        // Says what the list *is*, which is the whole dependency
        // tree. It previously claimed "the binary statically links
        // the crates listed below", which is false: the tree also
        // holds dev-dependencies (`assert_cmd`, `predicates`,
        // `difflib`), the build tooling's own deps, and crates for
        // other targets (`r-efi`). Over-attribution breaks no
        // licence, but the sentence has to be true -- pruning the
        // tree to what is actually linked is the better fix and is
        // recorded as a deferred item rather than guessed at here.
        "bombyx is distributed under the licence in the LICENSE file\n\
         beside this one. Listed below is its full dependency tree,\n\
         including build and test dependencies and crates for other\n\
         platforms, so this covers more than the binary links. Their\n\
         licences and notices follow, as those licences require.\n\n\
         Generated by `cargo xtask licenses` -- do not edit by hand.\n\n"
            .to_owned(),
        format!("{} crates in the dependency tree.\n", crates.len()),
    ];

    let missing: Vec<&Attribution> =
        crates.iter().filter(|c| c.text_missing()).collect();
    if !missing.is_empty() {
        // Named rather than dropped: a reader must be able to see
        // the file is short of a text instead of assuming it whole.
        parts.push(format!(
            "{} of them ship no licence file. Their SPDX expression is\n\
             recorded here and the text has to come from the crate's\n\
             own repository:\n",
            missing.len()
        ));
        parts.extend(missing.iter().map(|c| {
            format!(
                "  {} {} -- {}\n",
                c.name,
                c.version,
                c.license.as_deref().unwrap_or(NO_SPDX)
            )
        }));
    }

    for c in crates {
        parts.push(format!(
            "\n{rule}\n{} {}\nSPDX: {}\n{rule}\n",
            c.name,
            c.version,
            c.license.as_deref().unwrap_or(NO_SPDX)
        ));
        parts.extend(c.texts.iter().map(|(file, body)| {
            format!("\n--- {file} ---\n{}\n", body.trim_end())
        }));
    }
    parts.concat()
}

/// Writes the attribution file to `out`, or [`DEFAULT_OUT`].
///
/// # Errors
///
/// Returns a message when metadata cannot be read or the file cannot
/// be written.
pub fn licenses(out: Option<PathBuf>) -> Result<(), String> {
    let crates = collect()?;
    let body = render(&crates);
    let path = out.unwrap_or_else(|| PathBuf::from(DEFAULT_OUT));
    std::fs::write(&path, body)
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    let missing = crates.iter().filter(|c| c.text_missing()).count();
    println!(
        "Licenses OK ({} crates, {missing} without a bundled text) -> {}",
        crates.len(),
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_licence_file_names() {
        for name in [
            "LICENSE",
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "license.txt",
            "LICENCE",
            "COPYING",
            "NOTICE",
            "notice.md",
        ] {
            assert!(is_license_file(name), "{name} must be recognised");
        }
    }

    #[test]
    fn recognises_the_notice_files_this_tree_actually_ships() {
        // Not hypothetical: `rustix` and `linux-raw-sys` explain
        // their triple licence and the LLVM exception in COPYRIGHT,
        // and `r-efi` ships AUTHORS. Missing them was invisible,
        // because those crates also ship a LICENSE-* and so never
        // appeared in the "ships no licence file" list.
        for name in ["COPYRIGHT", "copyright.md", "AUTHORS", "NOTICE.txt"] {
            assert!(is_license_file(name), "{name} must be recognised");
        }
    }

    #[test]
    fn the_extension_rule_is_an_allow_list() {
        // A deny-list of `rs`/`spdx` silently admits the next format
        // nobody has invented. Anything outside the text-ish set is
        // refused, and a bare name with no extension is accepted.
        for good in ["LICENSE", "LICENSE.txt", "LICENSE.md", "NOTICE.html"] {
            assert!(is_license_file(good), "{good} must be recognised");
        }
        for bad in [
            "license.rs",
            "LICENSE.spdx",
            "LICENSE.toml",
            "LICENSE.json",
            "LICENSE.bk",
        ] {
            assert!(!is_license_file(bad), "{bad} must be rejected");
        }
    }

    #[test]
    fn rejects_names_that_only_look_like_licences() {
        // The family: source files, SPDX metadata, and names that
        // merely contain the word.
        for name in [
            "license.rs",
            "LICENSE.spdx",
            "src.rs",
            "README.md",
            "Cargo.toml",
            "relicense-notes.md",
        ] {
            assert!(!is_license_file(name), "{name} must be rejected");
        }
    }

    /// Minimal `cargo metadata` output: one member, two deps.
    fn metadata() -> Value {
        serde_json::json!({
            "workspace_members": ["path+file:///repo#bombyx@0.3.0"],
            "packages": [
                {
                    "id": "path+file:///repo#bombyx@0.3.0",
                    "name": "bombyx",
                    "version": "0.3.0",
                    "license": "MIT",
                    "manifest_path": "/repo/Cargo.toml"
                },
                {
                    "id": "registry+x#serde@1.0.0",
                    "name": "serde",
                    "version": "1.0.0",
                    "license": "MIT OR Apache-2.0",
                    "manifest_path": "/nonexistent/serde/Cargo.toml"
                },
                {
                    "id": "registry+x#anyhow@1.0.0",
                    "name": "anyhow",
                    "version": "1.0.0",
                    "manifest_path": "/nonexistent/anyhow/Cargo.toml"
                }
            ]
        })
    }

    #[test]
    fn excludes_workspace_members() {
        // Our own code is covered by the LICENSE already in the
        // archive; listing it as a third party would be wrong.
        let got = attributions_from(&metadata());
        assert!(
            !got.iter().any(|c| c.name == "bombyx"),
            "workspace member must be excluded"
        );
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn keeps_both_of_two_packages_sharing_name_and_version() {
        // A `[patch]` or path replacement reports the same name and
        // version as the crate it replaces. Keyed on that pair, one
        // silently overwrote the other -- and the one likely to be
        // lost is the fork, whose licence is the one that differs.
        let json = serde_json::json!({
            "workspace_members": [],
            "packages": [
                {
                    "id": "registry+crates.io#serde@1.0.0",
                    "name": "serde",
                    "version": "1.0.0",
                    "license": "MIT OR Apache-2.0",
                    "manifest_path": "/nonexistent/a/Cargo.toml"
                },
                {
                    "id": "path+file:///forks/serde#serde@1.0.0",
                    "name": "serde",
                    "version": "1.0.0",
                    "license": "GPL-3.0-only",
                    "manifest_path": "/nonexistent/b/Cargo.toml"
                }
            ]
        });
        let got = attributions_from(&json);
        assert_eq!(got.len(), 2, "both packages must survive");
        let licences: Vec<Option<&str>> =
            got.iter().map(|c| c.license.as_deref()).collect();
        assert!(licences.contains(&Some("GPL-3.0-only")), "{licences:?}");
    }

    #[test]
    fn sorts_by_name_so_regenerating_is_a_no_op() {
        let got = attributions_from(&metadata());
        let names: Vec<&str> = got.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["anyhow", "serde"]);
    }

    #[test]
    fn keeps_a_crate_with_no_spdx_expression() {
        // Recorded as unknown rather than skipped.
        let got = attributions_from(&metadata());
        let anyhow = got.iter().find(|c| c.name == "anyhow").unwrap();
        assert_eq!(anyhow.license, None);
    }

    #[test]
    fn reports_crates_that_ship_no_text() {
        // The fixture's paths do not exist, so every crate is
        // text-less -- the case the renderer must not hide.
        let got = attributions_from(&metadata());
        assert!(got.iter().all(Attribution::text_missing));

        let body = render(&got);
        assert!(body.contains("ship no licence file"), "{body}");
        assert!(body.contains("serde 1.0.0"), "{body}");
        assert!(body.contains(NO_SPDX), "{body}");
    }

    #[test]
    fn renders_every_crate_with_its_text() {
        let crates = vec![Attribution {
            name: "demo".to_owned(),
            version: "2.0.0".to_owned(),
            license: Some("MIT".to_owned()),
            texts: vec![(
                "LICENSE-MIT".to_owned(),
                "Permission is hereby granted".to_owned(),
            )],
        }];
        let body = render(&crates);
        assert!(body.contains("demo 2.0.0"), "{body}");
        assert!(body.contains("SPDX: MIT"), "{body}");
        assert!(body.contains("--- LICENSE-MIT ---"), "{body}");
        assert!(body.contains("Permission is hereby granted"), "{body}");
        // With nothing missing, the caveat must not appear at all.
        assert!(!body.contains("ship no licence file"), "{body}");
    }

    #[test]
    fn an_empty_set_still_renders_a_usable_header() {
        let body = render(&[]);
        assert!(body.contains("0 crates in the dependency tree"), "{body}");
        assert!(body.contains("LICENSE file"), "{body}");
    }
}
