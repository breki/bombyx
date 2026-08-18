//! Which package ids the distributed binary actually links.
//!
//! Split from the parent module because this is the part most likely
//! to change again: cargo has already renamed package ids once, and
//! feature resolution and `dep_kinds` are both still moving. Reading
//! `LICENSE-APACHE` off disk has nothing to do with any of that.
//!
//! Everything here is a pure function of the `cargo metadata` JSON,
//! so the whole set of exclusions is tested without running cargo.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// Workspace members that are actually distributed.
///
/// `publish = false` in a manifest is the honest signal that a crate
/// is not shipped. `xtask` is the case that matters here: it is a
/// workspace member and a binary, so neither "is a member" nor
/// "builds a bin" separates it, and its `clap` and `serde_json` are
/// in nobody's release archive.
///
/// The test is an **empty** `publish` array, not the absence of one.
/// Cargo reports `publish = false` as `[]` and an unrestricted crate
/// as `null`, but `publish = ["some-registry"]` is a third case and
/// it means the crate *is* distributed. Requiring `null` dropped it
/// as a root, and with one publishable member that yields an empty
/// attribution set from a perfectly ordinary manifest.
pub(super) fn distributed_roots(json: &Value) -> Vec<&str> {
    let members = workspace_members(json);
    json["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|pkg| {
            let id = pkg["id"].as_str().unwrap_or_default();
            members.contains(&id) && !publish_disabled(&pkg["publish"])
        })
        .filter_map(|pkg| pkg["id"].as_str())
        .collect()
}

/// Whether a package's `publish` field says "do not distribute me".
fn publish_disabled(publish: &Value) -> bool {
    publish.as_array().is_some_and(Vec::is_empty)
}

/// The workspace members' package ids.
///
/// One place for the `workspace_members` JSON shape, because two
/// callers need it and the shape has already changed once: cargo
/// moved these ids from `name version (source)` to a
/// `PackageIdSpec` URL. A format change should mean one edit.
pub(super) fn workspace_members(json: &Value) -> Vec<&str> {
    json["workspace_members"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Package ids reachable from `roots` through **normal** dependencies.
///
/// Walks `resolve.nodes`, following only a dep whose `dep_kinds`
/// carries an entry with `kind: null`. A `kind` of `"dev"` or
/// `"build"` is not in the binary, and the attribution file used to
/// list them: `assert_cmd`, `predicates` and `difflib` were all
/// named as crates the binary links, which `cargo tree -i difflib
/// -e normal` flatly contradicts.
///
/// Platform filtering is not done here -- `--filter-platform` on the
/// `cargo metadata` call already prunes `resolve` to one target, so
/// doing it twice would mean reimplementing cfg evaluation.
pub(super) fn reachable_normal<'a>(
    json: &'a Value,
    roots: &[&'a str],
) -> BTreeSet<&'a str> {
    let nodes: BTreeMap<&str, &Value> = json["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n["id"].as_str().map(|id| (id, n)))
        .collect();

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: Vec<&str> = roots.to_vec();
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(id) else {
            continue;
        };
        for dep in node["deps"].as_array().into_iter().flatten() {
            let normal = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if !normal {
                continue;
            }
            if let Some(pkg) = dep["pkg"].as_str() {
                queue.push(pkg);
            }
        }
    }
    seen
}
