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
/// gates. A derived project changes the name here (and with it
/// the `<NAME>_DEP_AGE_ALLOW` and `<NAME>_DEP_AGE_STRICT` variables
/// and the user agent), and
/// overrides a limit on the returned value, e.g.
/// `cfg.thresholds.coverage_file = 80.0;`. The canon check's
/// exemptions are here too: paths canon names that are correct to
/// be absent, each with its reason. All three exist in the
/// template and are absent here.
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
            "crates/rustbase/",
            "upstream template paths, quoted for `git show \
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
    fn the_canon_exemptions_are_valid_and_cover_the_template_paths() {
        let rules = tasks_config().canon;
        assert_eq!(rules.validate(), Ok(()));
        let paths: Vec<&str> = rules
            .absent_on_purpose
            .iter()
            .map(|e| e.path.as_str())
            .collect();
        for p in [
            "scripts/e2e.sh",
            "crates/rustbase/",
            "docs/developer/backfeed-ledger.toml",
        ] {
            assert!(paths.contains(&p), "{p} not exempt: {paths:?}");
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
