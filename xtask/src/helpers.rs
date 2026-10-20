use std::path::PathBuf;

use rustbase_xtask::canon::Exempt;

/// Resolve the workspace root (the parent of the xtask
/// crate directory).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate always lives under workspace root")
        .to_path_buf()
}

/// This project's settings for the shared `rustbase-xtask`
/// gates. The name "bombyx" gives the `BOMBYX_DEP_AGE_ALLOW` and
/// `BOMBYX_DEP_AGE_STRICT` variables and the User-Agent the
/// cooldown check sends to the registry. A limit is overridden on
/// the returned value, e.g. `cfg.thresholds.coverage_file = 80.0;`.
/// The canon check's exemptions are here too: paths canon names
/// that are correct to be absent, each with its reason. All three
/// exist in the template and are absent here.
pub fn tasks_config() -> rustbase_xtask::Config {
    let mut cfg =
        rustbase_xtask::Config::for_project("bombyx", workspace_root());
    cfg.canon.absent_on_purpose = vec![
        Exempt::new(
            "scripts/e2e.sh",
            "named only by sentences saying the template shipped it \
             and this project removed it",
        ),
        Exempt::new(
            "crates/rustbase/Cargo.toml",
            "the template's version file, quoted for `git show \
             template/main:<path>` in /template-sync",
        ),
        Exempt::new(
            "docs/developer/backfeed-ledger.toml",
            "/template-backfeed and its ledger live in the template \
             repo only",
        ),
    ];
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canon_exemptions_are_valid_and_name_absent_paths() {
        // An exemption for a path that now exists would hide a
        // finding rather than excuse a correct absence.
        let rules = tasks_config().canon;
        assert_eq!(rules.validate(), Ok(()));
        for e in &rules.absent_on_purpose {
            assert!(
                !workspace_root().join(&e.path).exists(),
                "{} exists; drop its exemption",
                e.path
            );
        }
    }

    #[test]
    fn workspace_root_contains_cargo_toml() {
        let root = workspace_root();
        assert!(
            root.join("Cargo.toml").is_file(),
            "workspace root should contain Cargo.toml: {}",
            root.display()
        );
    }
}
