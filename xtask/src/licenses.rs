//! `cargo xtask licenses` -- generates the third-party attribution
//! file shipped in the release archives.
//!
//! bombyx is MIT and builds on a few dozen crates. Every one of them
//! is permissive, but permissive is not obligation-free: MIT and
//! Apache-2.0 both require the licence and copyright notice to travel
//! with a distributed binary, and `serde`, `clap`, `anyhow` and the
//! `windows-sys` tree are all in the shipped binary under one or the
//! other. The release archives previously held only bombyx's own
//! `LICENSE`, so that attribution went unmet the moment binaries
//! started being published.
//!
//! The licence *texts* come from the crate sources already unpacked
//! in the cargo registry, so nothing is downloaded.
//!
//! **The set is what goes into building the binary for one target.**
//! Crates reachable from a *distributed* workspace member (so not
//! `xtask`'s own tree) through *normal* dependencies (so not
//! `assert_cmd`, `predicates` or `difflib`), resolved for *one
//! platform* (so not `r-efi`, a `uefi` crate). Before those three
//! restrictions the file listed every package in the tree.
//!
//! **It is deliberately over-inclusive within that, and the wording
//! has to stay that way.** Two kinds of crate are in the list without
//! being *linked*: proc-macro crates, which run at compile time
//! (`clap_derive`, `serde_derive`, `thiserror-impl` and their `syn` /
//! `quote` / `unicode-ident` closure -- 8 of the 50 on Windows), and
//! optional dependencies the release build does not enable, which
//! `resolve.nodes[].deps` reports with the same `kind: null` as a
//! real edge (`toml` -> `indexmap` -> `hashbrown`, `equivalent`:
//! `cargo tree -e normal` gives 47 crates where this walk gives 50).
//! Pruning either one means reimplementing feature resolution, which
//! fails quietly and in the direction that matters -- an omitted
//! notice. So the file says "goes into building", never "linked
//! into", and an earlier version of it said the wrong one.
//!
//! **A crate shipping no licence text fails the command** rather
//! than being noted in passing. Reporting it was not enough: if the
//! registry sources are absent every crate comes back text-less, and
//! the tool would write a short file announcing that all of them
//! ship no licence and exit 0. Raise `--max-missing` with a reason
//! when a crate genuinely has none.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::helpers::run_cargo_capture;
use graph::{distributed_roots, reachable_normal, workspace_members};

mod graph;

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
    /// Whether this crate shipped no licence terms.
    ///
    /// **Notices do not count, and neither does an empty file.** The
    /// collected set deliberately includes `NOTICE`, `AUTHORS` and
    /// `COPYRIGHT`, because those carry obligations of their own --
    /// but a crate shipping only an `AUTHORS` contributor list has
    /// given us no licence terms to reproduce, and letting that
    /// satisfy the gate would restore exactly the reassuring-but-
    /// empty file the gate exists to prevent. The predicate is
    /// therefore narrower than [`is_license_file`], on purpose.
    #[must_use]
    pub fn text_missing(&self) -> bool {
        !self.texts.iter().any(|(name, body)| {
            is_license_terms(name) && !body.trim().is_empty()
        })
    }
}

/// Whether `name` is a file stating licence *terms*, as opposed to a
/// notice accompanying them. See [`Attribution::text_missing`].
#[must_use]
pub fn is_license_terms(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !has_text_extension(&lower) {
        return false;
    }
    lower.starts_with("license")
        || lower.starts_with("licence")
        || lower.starts_with("copying")
}

/// An **allow-list** of extensions, not a deny-list.
///
/// A deny-list enumerating `rs` and `spdx` silently starts including
/// whatever format nobody has invented yet (`LICENSE.toml`, a `.bk`
/// backup), and the safe default here is the other way round: a
/// spurious text is harmless, a missing one is a compliance gap
/// nothing reports. `None` covers the common bare `LICENSE`.
fn has_text_extension(lower: &str) -> bool {
    let ext = Path::new(lower).extension().and_then(|e| e.to_str());
    matches!(ext, None | Some("txt" | "md" | "html"))
}

/// Whether `name` is a licence or notice file.
///
/// `NOTICE` counts: Apache-2.0 section 4(d) requires it to be
/// carried, and a crate shipping one is asserting there is something
/// to carry.
#[must_use]
pub fn is_license_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if !has_text_extension(&lower) {
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
pub fn collect(target: &str) -> Result<Vec<Attribution>, String> {
    let out = run_cargo_capture(&[
        "metadata",
        "--format-version",
        "1",
        // Default features, because that is what
        // `cargo build --release --locked` in the release workflow
        // builds. `--all-features` was used here first and kept
        // optional dependencies the release never enables, which
        // over-attributes -- harmless legally, but it makes the
        // header sentence ("linked into the binary in this archive")
        // false, and that sentence is the point of the file.
        // `--locked` so the attribution describes the dependency set
        // the binary was built from. Without it this call can
        // re-resolve and rewrite `Cargo.lock` *after* a
        // `cargo build --locked`, and the file would then describe a
        // set the binary never used.
        "--locked",
        // One platform, matching the archive being packaged. Without
        // it the list carries crates for other targets -- `r-efi` is
        // a `uefi` crate and was being attributed on Windows.
        "--filter-platform",
        target,
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
/// Keeps the crates the distributed binary actually links: reachable
/// from a publishable workspace member through normal dependencies,
/// minus the members themselves, whose code the `LICENSE` in every
/// archive already covers. Split out so the walk, the exclusions and
/// the ordering are tested without running cargo.
#[must_use]
pub fn attributions_from(json: &Value) -> Vec<Attribution> {
    let members = workspace_members(json);
    let roots = distributed_roots(json);
    let keep = reachable_normal(json, &roots);

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
        if members.contains(&id) || !keep.contains(id) {
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
        // "go into building", not "are linked into". The set holds
        // proc-macro crates and optional dependencies the build does
        // not enable, and pruning those needs feature resolution --
        // see the module docs. An earlier version of this file did
        // claim "linked", which was a false statement in a legal
        // document. Over-inclusion is not, so the wording is what
        // gets kept honest, not the set.
        "bombyx is distributed under the licence in the LICENSE file\n\
         beside this one. The crates listed below go into building the\n\
         binary in this archive -- its normal dependencies, for this\n\
         platform, excluding anything used only to test it. Some run\n\
         at compile time rather than shipping inside the binary; they\n\
         are listed anyway, because an unnecessary attribution costs\n\
         nothing and a missing one is the failure that matters. Their\n\
         licences and notices follow, as those licences require.\n\n\
         Generated by `cargo xtask licenses` -- do not edit by hand.\n\n"
            .to_owned(),
        format!("{} crates go into building this binary.\n", crates.len()),
    ];

    let missing = missing_texts(crates);
    if !missing.is_empty() {
        // Named rather than dropped: a reader must be able to see
        // the file is short of a text instead of assuming it whole.
        parts.push(format!(
            "{} of them ship no licence terms. Their SPDX expression is\n\
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
pub fn licenses(
    out: Option<&Path>,
    target: Option<&str>,
    max_missing: usize,
) -> Result<(), String> {
    let target = match target {
        Some(t) => t.to_owned(),
        None => host_triple()?,
    };
    let crates = collect(&target)?;

    check_complete(&crates, max_missing)?;

    let body = render(&crates);
    let path =
        out.map_or_else(|| PathBuf::from(DEFAULT_OUT), Path::to_path_buf);
    std::fs::write(&path, body)
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    println!(
        "Licenses OK ({} crates for {target}) -> {}",
        crates.len(),
        path.display()
    );
    Ok(())
}

/// Refuses an attribution set that is missing licence texts.
///
/// **This is the gate, and it is the point.** Nothing used to fail
/// when a crate shipped no licence text -- the count was printed and
/// the file written anyway. So if the registry sources were absent (a
/// vendored build, or a container whose `CARGO_HOME` differs between
/// the build and packaging steps) every crate came back text-less and
/// the tool wrote a short file announcing that all of them ship no
/// licence, then exited 0. That shipped, and it is worse than having
/// no file at all: it documents that the obligation was considered
/// and then not met.
///
/// An empty set is refused separately, because zero missing out of
/// zero crates satisfies any threshold.
///
/// # Errors
///
/// Returns a message naming the crates when more than `max_missing`
/// of them carry no text.
fn check_complete(
    crates: &[Attribution],
    max_missing: usize,
) -> Result<(), String> {
    if crates.is_empty() {
        return Err("no crates found: cargo metadata returned no \
                    reachable dependencies. Either the `resolve` \
                    graph is absent (`--no-deps` produces that) or \
                    no workspace member is publishable, and neither \
                    can be right here"
            .to_owned());
    }
    let missing = missing_texts(crates);
    if missing.len() <= max_missing {
        return Ok(());
    }
    let names: Vec<String> = missing
        .iter()
        .map(|c| format!("{} {}", c.name, c.version))
        .collect();
    Err(format!(
        "{} of {} crates ship no licence text, more than the {} \
         allowed: {}.\nIf that is correct, raise --max-missing and say \
         why. If it is every crate, the registry sources are missing \
         rather than the licences.",
        missing.len(),
        crates.len(),
        max_missing,
        names.join(", ")
    ))
}

/// The crates carrying no licence text.
///
/// Shared by [`render`], which names them in the file, and
/// [`check_complete`], which fails on too many of them -- so the two
/// can never disagree about which crates those are.
fn missing_texts(crates: &[Attribution]) -> Vec<&Attribution> {
    crates.iter().filter(|c| c.text_missing()).collect()
}

/// The target triple this build is running on.
///
/// Read from `rustc -vV` rather than assembled from
/// `std::env::consts`, which cannot distinguish `-msvc` from `-gnu`
/// and so would hand `--filter-platform` a triple rustc does not
/// know.
///
/// It fails rather than guessing a default. A guess here is not a
/// degraded result but a wrong one: a Windows archive would carry
/// the crates a Linux binary links, and the command would exit 0
/// saying so.
fn host_triple() -> Result<String, String> {
    let out = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| {
            format!("cannot run rustc to find the host triple: {e}")
        })?;
    let text = String::from_utf8(out.stdout)
        .map_err(|e| format!("rustc -vV produced non-UTF-8 output: {e}"))?;
    host_from_vv(&text).map(str::to_owned).ok_or_else(|| {
        "rustc -vV printed no `host:` line, so the target is \
         unknown; pass --target explicitly"
            .to_owned()
    })
}

/// Picks the `host:` line out of `rustc -vV` output.
///
/// Split from [`host_triple`] so the parsing is testable without
/// spawning `rustc`, which is the half that can be wrong.
fn host_from_vv(text: &str) -> Option<&str> {
    text.lines().find_map(|l| l.strip_prefix("host: "))
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
    fn a_notice_alone_does_not_satisfy_the_gate() {
        // `AUTHORS` and `NOTICE` are collected, because they carry
        // obligations -- but they state no terms. A crate shipping
        // only those used to pass a `--max-missing 0` run, and the
        // archive then held an attribution block with no licence in
        // it, which is the reassuring-but-empty file the gate exists
        // to prevent.
        let only_notice = Attribution {
            name: "demo".to_owned(),
            version: "1.0.0".to_owned(),
            license: Some("MIT".to_owned()),
            texts: vec![("AUTHORS".to_owned(), "Someone".to_owned())],
        };
        assert!(only_notice.text_missing());
    }

    #[test]
    fn an_empty_licence_file_does_not_satisfy_the_gate() {
        // A zero-byte or whitespace-only `LICENSE` is a name, not
        // terms. Nothing checked the body before.
        let blank = Attribution {
            name: "demo".to_owned(),
            version: "1.0.0".to_owned(),
            license: Some("MIT".to_owned()),
            texts: vec![("LICENSE".to_owned(), "  \n".to_owned())],
        };
        assert!(blank.text_missing());
    }

    #[test]
    fn real_terms_satisfy_the_gate_even_beside_a_notice() {
        let both = Attribution {
            name: "demo".to_owned(),
            version: "1.0.0".to_owned(),
            license: Some("MIT".to_owned()),
            texts: vec![
                ("AUTHORS".to_owned(), "Someone".to_owned()),
                ("LICENSE-MIT".to_owned(), "Permission is...".to_owned()),
            ],
        };
        assert!(!both.text_missing());
    }

    #[test]
    fn only_terms_files_state_terms() {
        for name in ["LICENSE", "LICENCE.md", "COPYING", "license.txt"] {
            assert!(is_license_terms(name), "{name} states terms");
        }
        for name in ["NOTICE", "AUTHORS", "COPYRIGHT.md"] {
            assert!(!is_license_terms(name), "{name} is a notice");
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

    /// A metadata fixture with the shapes that matter.
    ///
    /// Two workspace members -- one publishable, one not -- plus a
    /// normal dep, a dev-only dep, and a crate reachable only from
    /// the unpublished member. Every exclusion the walk makes has a
    /// representative here, because each was a real defect: the file
    /// once listed all five and said the binary linked them.
    fn metadata() -> Value {
        serde_json::json!({
            "workspace_members": [
                "path+file:///repo#bombyx@0.3.0",
                "path+file:///repo/xtask#xtask@0.1.0"
            ],
            "packages": [
                {
                    "id": "path+file:///repo#bombyx@0.3.0",
                    "name": "bombyx",
                    "version": "0.3.0",
                    "license": "MIT",
                    "publish": null,
                    "manifest_path": "/repo/Cargo.toml"
                },
                {
                    "id": "path+file:///repo/xtask#xtask@0.1.0",
                    "name": "xtask",
                    "version": "0.1.0",
                    "license": "MIT",
                    "publish": [],
                    "manifest_path": "/repo/xtask/Cargo.toml"
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
                },
                {
                    "id": "registry+x#difflib@0.4.0",
                    "name": "difflib",
                    "version": "0.4.0",
                    "license": "MIT",
                    "manifest_path": "/nonexistent/difflib/Cargo.toml"
                },
                {
                    "id": "registry+x#clap@4.0.0",
                    "name": "clap",
                    "version": "4.0.0",
                    "license": "MIT",
                    "manifest_path": "/nonexistent/clap/Cargo.toml"
                },
                {
                    "id": "registry+x#serde_json@1.0.0",
                    "name": "serde_json",
                    "version": "1.0.0",
                    "license": "MIT",
                    "manifest_path": "/nonexistent/serde_json/Cargo.toml"
                }
            ],
            "resolve": {
                "nodes": [
                    {
                        "id": "path+file:///repo#bombyx@0.3.0",
                        "deps": [
                            {
                                "pkg": "registry+x#serde@1.0.0",
                                "dep_kinds": [{"kind": null}]
                            },
                            {
                                "pkg": "registry+x#difflib@0.4.0",
                                "dep_kinds": [{"kind": "dev"}]
                            },
                            {
                                "pkg": "registry+x#serde_json@1.0.0",
                                "dep_kinds": [
                                    {"kind": "dev"},
                                    {"kind": null}
                                ]
                            }
                        ]
                    },
                    {
                        "id": "path+file:///repo/xtask#xtask@0.1.0",
                        "deps": [
                            {
                                "pkg": "registry+x#clap@4.0.0",
                                "dep_kinds": [{"kind": null}]
                            }
                        ]
                    },
                    {
                        "id": "registry+x#serde@1.0.0",
                        "deps": [
                            {
                                "pkg": "registry+x#anyhow@1.0.0",
                                "dep_kinds": [{"kind": null}]
                            }
                        ]
                    }
                ]
            }
        })
    }

    /// The crate names the walk keeps, for the tests below.
    fn kept() -> Vec<String> {
        attributions_from(&metadata())
            .into_iter()
            .map(|c| c.name)
            .collect()
    }

    #[test]
    fn keeps_only_what_the_binary_links() {
        // One assertion per exclusion, because each was a defect that
        // shipped: `bombyx` is our own code and covered by the
        // archive's LICENSE, `xtask` is not distributed at all,
        // `clap` is reachable only through it, and `difflib` is a
        // dev-dependency. `serde` is a normal dep, `anyhow` is
        // transitively normal, and `serde_json` is both dev and
        // normal, so all three stay.
        let names = kept();
        assert_eq!(names, vec!["anyhow", "serde", "serde_json"], "{names:?}");
    }

    #[test]
    fn keeps_a_dep_that_is_both_dev_and_normal() {
        // `dep_kinds` is a list, and one crate can arrive on several
        // edges at once: `serde_json` is used in tests *and* linked
        // into the binary. So the test is `any(kind: null)`, not
        // "the first kind" or "every kind" -- either of those would
        // drop it and under-attribute a crate that really does ship.
        assert!(kept().contains(&"serde_json".to_owned()));
    }

    #[test]
    fn an_absent_resolve_graph_keeps_nothing() {
        // `cargo metadata --no-deps` emits no `resolve`, and older
        // cargo emitted `null`. Neither is a set to attribute, and
        // the empty result is what `check_complete` refuses -- this
        // asserts the walk does not instead fall back to every
        // package in `packages`, which is where the over-attribution
        // came from in the first place.
        let mut json = metadata();
        json.as_object_mut().unwrap().remove("resolve");
        assert!(attributions_from(&json).is_empty());
    }

    #[test]
    fn reads_the_host_triple_out_of_rustc_vv() {
        let vv = "rustc 1.90.0 (abc 2026-01-01)\n\
                  binary: rustc\n\
                  host: x86_64-pc-windows-msvc\n\
                  release: 1.90.0\n";
        assert_eq!(host_from_vv(vv), Some("x86_64-pc-windows-msvc"));
    }

    #[test]
    fn no_host_line_is_no_triple() {
        // Not a default. A guessed triple resolves another
        // platform's dependency set and the command still exits 0.
        assert_eq!(host_from_vv("rustc 1.90.0\nbinary: rustc\n"), None);
    }

    #[test]
    fn excludes_workspace_members() {
        let names = kept();
        assert!(!names.contains(&"bombyx".to_owned()), "{names:?}");
        assert!(!names.contains(&"xtask".to_owned()), "{names:?}");
    }

    #[test]
    fn excludes_a_dev_dependency() {
        // `difflib` reaches the tree only through a `kind: "dev"`
        // edge. It was attributed as linked into the binary, which
        // `cargo tree -i difflib -e normal` contradicts.
        assert!(!kept().contains(&"difflib".to_owned()));
    }

    #[test]
    fn excludes_the_build_tooling_tree() {
        // `clap` is a normal dependency -- of `xtask`, which carries
        // `publish = false` and ships to nobody. Being a workspace
        // member and a binary, neither of those facts separates it;
        // `publish` does.
        assert!(!kept().contains(&"clap".to_owned()));
    }

    #[test]
    fn a_named_registry_still_counts_as_distributed() {
        // `publish` has three states, not two: `null` (any registry),
        // `[]` (`publish = false`), and a non-empty list naming
        // registries. Requiring `null` treated the third as
        // undistributed, so a member published to a private registry
        // stopped being a root -- and with one such member that is an
        // empty attribution set from an ordinary manifest.
        let mut json = metadata();
        json["packages"][0]["publish"] =
            serde_json::json!(["internal-registry"]);
        let names: Vec<String> = attributions_from(&json)
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert!(names.contains(&"serde".to_owned()), "{names:?}");
    }

    #[test]
    fn keeps_a_transitive_normal_dependency() {
        // `anyhow` is not a direct dependency of anything in the
        // workspace; it arrives through `serde`. A walk that only
        // looked one level deep would drop it and under-attribute.
        assert!(kept().contains(&"anyhow".to_owned()));
    }

    #[test]
    fn keeps_both_of_two_packages_sharing_name_and_version() {
        // A `[patch]` or path replacement reports the same name and
        // version as the crate it replaces. Keyed on that pair, one
        // silently overwrote the other -- and the one likely to be
        // lost is the fork, whose licence is the one that differs.
        let json = serde_json::json!({
            "workspace_members": ["path+file:///repo#bombyx@0.3.0"],
            "packages": [
                {
                    "id": "path+file:///repo#bombyx@0.3.0",
                    "name": "bombyx",
                    "version": "0.3.0",
                    "publish": null,
                    "manifest_path": "/repo/Cargo.toml"
                },
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
            ],
            // Both reachable as normal deps, so the walk keeps both
            // and the (name, version) collision is the only thing
            // that could drop one.
            "resolve": {
                "nodes": [{
                    "id": "path+file:///repo#bombyx@0.3.0",
                    "deps": [
                        {
                            "pkg": "registry+crates.io#serde@1.0.0",
                            "dep_kinds": [{"kind": null}]
                        },
                        {
                            "pkg": "path+file:///forks/serde#serde@1.0.0",
                            "dep_kinds": [{"kind": null}]
                        }
                    ]
                }]
            }
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
        assert_eq!(names, vec!["anyhow", "serde", "serde_json"]);
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
        assert!(body.contains("ship no licence terms"), "{body}");
        assert!(body.contains("serde 1.0.0"), "{body}");
        assert!(body.contains(NO_SPDX), "{body}");
    }

    /// An attribution with or without a bundled text.
    fn attribution(name: &str, with_text: bool) -> Attribution {
        Attribution {
            name: name.to_owned(),
            version: "1.0.0".to_owned(),
            license: Some("MIT".to_owned()),
            texts: if with_text {
                vec![("LICENSE".to_owned(), "text".to_owned())]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn a_complete_set_passes() {
        let crates = vec![attribution("a", true), attribution("b", true)];
        assert_eq!(check_complete(&crates, 0), Ok(()));
    }

    #[test]
    fn a_missing_text_fails_and_names_the_crate() {
        let crates = vec![attribution("a", true), attribution("b", false)];
        let err = check_complete(&crates, 0).unwrap_err();
        assert!(err.contains("b 1.0.0"), "{err}");
        assert!(err.contains("--max-missing"), "{err}");
    }

    #[test]
    fn an_allowance_permits_exactly_that_many() {
        let crates = vec![attribution("a", false), attribution("b", false)];
        assert_eq!(check_complete(&crates, 2), Ok(()));
        assert!(check_complete(&crates, 1).is_err());
    }

    #[test]
    fn every_crate_text_less_is_the_registry_not_the_licences() {
        // The failure this gate exists for: absent registry sources
        // make every crate look unlicensed, and the old code wrote
        // that out and exited 0.
        let crates: Vec<Attribution> = (0..50)
            .map(|i| attribution(&format!("c{i}"), false))
            .collect();
        let err = check_complete(&crates, 0).unwrap_err();
        assert!(err.contains("50 of 50"), "{err}");
        assert!(err.contains("registry sources are missing"), "{err}");
    }

    #[test]
    fn an_empty_set_is_refused_however_high_the_allowance() {
        // Zero missing out of zero satisfies any threshold, so the
        // emptiness has to be its own check.
        let err = check_complete(&[], 99).unwrap_err();
        assert!(err.contains("no crates found"), "{err}");
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
        assert!(!body.contains("ship no licence terms"), "{body}");
    }

    #[test]
    fn an_empty_set_still_renders_a_usable_header() {
        let body = render(&[]);
        assert!(body.contains("0 crates go into building"), "{body}");
        assert!(body.contains("LICENSE file"), "{body}");
    }
}
