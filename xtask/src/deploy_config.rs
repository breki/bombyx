//! Shared config loader for deploy and deploy-setup.
//!
//! Reads `<project>/.deploy`, validates all values to
//! prevent shell injection when they're interpolated into
//! SSH command strings, and returns a `DeployConfig`.

use std::fs;
use std::path::Path;

/// Only `/opt/rustbase` is supported — the systemd unit
/// file hardcodes this path.
pub const REQUIRED_DEPLOY_PATH: &str = "/opt/rustbase";

/// Resolved, validated values from `.deploy`. Fields are
/// private so the validation contract from `load()`
/// cannot be bypassed by mutation.
#[derive(Debug, Clone)]
pub struct DeployConfig {
    rpi_host: String,
    deploy_path: String,
    remote: String,
}

impl DeployConfig {
    pub fn rpi_host(&self) -> &str {
        &self.rpi_host
    }

    pub fn deploy_path(&self) -> &str {
        &self.deploy_path
    }

    /// `user@host` string used by ssh/scp.
    pub fn remote(&self) -> &str {
        &self.remote
    }
}

/// Load and validate `<project_root>/.deploy`.
pub fn load(project_root: &Path) -> Result<DeployConfig, String> {
    let path = project_root.join(".deploy");
    let contents = fs::read_to_string(&path).map_err(|e| {
        format!(
            "{} not found or unreadable ({e})\n  \
             copy .deploy.sample to .deploy and configure",
            path.display()
        )
    })?;

    let mut rpi_host = String::new();
    let mut rpi_user = String::new();
    let mut deploy_path = REQUIRED_DEPLOY_PATH.to_owned();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().to_owned();
        match key {
            "rpi_host" => rpi_host = value,
            "rpi_user" => rpi_user = value,
            "deploy_path" => deploy_path = value,
            _ => {}
        }
    }

    validate(&rpi_host, &rpi_user, &deploy_path)?;

    let remote = format!("{rpi_user}@{rpi_host}");
    Ok(DeployConfig {
        rpi_host,
        deploy_path,
        remote,
    })
}

fn validate(
    rpi_host: &str,
    rpi_user: &str,
    deploy_path: &str,
) -> Result<(), String> {
    if rpi_host.is_empty() {
        return Err("rpi_host not set in .deploy".into());
    }
    if rpi_user.is_empty() {
        return Err("rpi_user not set in .deploy".into());
    }
    if deploy_path != REQUIRED_DEPLOY_PATH {
        return Err(format!(
            "deploy_path must be {REQUIRED_DEPLOY_PATH} \
             (the systemd service file hardcodes this path)"
        ));
    }
    if !rpi_host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err("rpi_host contains unsafe characters \
             (only a-z A-Z 0-9 . _ - allowed)"
            .into());
    }
    if !rpi_user
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
    {
        return Err("rpi_user contains unsafe characters \
             (only a-z A-Z 0-9 _ - allowed)"
            .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> (String, String, String) {
        ("malina".into(), "pi".into(), "/opt/rustbase".into())
    }

    #[test]
    fn validates_good_config() {
        let (h, u, p) = ok();
        assert!(validate(&h, &u, &p).is_ok());
    }

    #[test]
    fn rejects_empty_host() {
        let (_, u, p) = ok();
        let err = validate("", &u, &p).unwrap_err();
        assert!(err.contains("rpi_host"));
    }

    #[test]
    fn rejects_relative_deploy_path() {
        let (h, u, _) = ok();
        let err = validate(&h, &u, "opt/rustbase").unwrap_err();
        assert!(err.contains("/opt/rustbase"));
    }

    #[test]
    fn rejects_deploy_path_with_shell_metachar() {
        let (h, u, _) = ok();
        let err = validate(&h, &u, "/opt/$(rm -rf /)").unwrap_err();
        assert!(err.contains("/opt/rustbase"));
    }

    #[test]
    fn rejects_non_canonical_deploy_path() {
        let (h, u, _) = ok();
        let err = validate(&h, &u, "/srv/rustbase").unwrap_err();
        assert!(err.contains("/opt/rustbase"));
    }

    #[test]
    fn rejects_host_with_space() {
        let (_, u, p) = ok();
        let err = validate("bad host", &u, &p).unwrap_err();
        assert!(err.contains("rpi_host"));
    }

    #[test]
    fn rejects_user_with_semicolon() {
        let (h, _, p) = ok();
        let err = validate(&h, "evil;rm", &p).unwrap_err();
        assert!(err.contains("rpi_user"));
    }

    #[test]
    fn rpi_host_error_lists_allowed_chars() {
        let (_, u, p) = ok();
        let err = validate("bad host", &u, &p).unwrap_err();
        assert!(err.contains("a-z"));
    }

    #[test]
    fn rpi_user_error_lists_allowed_chars() {
        let (h, _, p) = ok();
        let err = validate(&h, "evil;rm", &p).unwrap_err();
        assert!(err.contains("a-z"));
    }

    #[test]
    fn load_from_tempdir() {
        let dir = std::env::temp_dir().join(format!(
            "rustbase-xtask-deploy-config-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join(".deploy");
        fs::write(
            &cfg_path,
            "# sample\n\
             rpi_host=malina\n\
             rpi_user=pi\n\
             deploy_path=/opt/rustbase\n",
        )
        .unwrap();

        let cfg = load(&dir).unwrap();
        assert_eq!(cfg.rpi_host(), "malina");
        assert_eq!(cfg.remote(), "pi@malina");
        assert_eq!(cfg.deploy_path(), "/opt/rustbase");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_reports_missing_file() {
        let dir = std::env::temp_dir().join(format!(
            "rustbase-xtask-deploy-missing-{}",
            std::process::id()
        ));
        let err = load(&dir).unwrap_err();
        assert!(err.contains(".deploy"));
    }

    /// Write a throwaway `.deploy` and run `load`.
    fn load_with_contents(contents: &str) -> Result<DeployConfig, String> {
        let dir = std::env::temp_dir().join(format!(
            "rustbase-xtask-deploy-parse-{}-{}",
            std::process::id(),
            contents.len()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".deploy"), contents).unwrap();
        let result = load(&dir);
        fs::remove_dir_all(&dir).ok();
        result
    }

    #[test]
    fn load_ignores_comments_and_blank_lines() {
        let cfg = load_with_contents(
            "# a comment\n\
             \n\
             rpi_host=malina\n\
             # another\n\
             rpi_user=pi\n\
             deploy_path=/opt/rustbase\n",
        )
        .unwrap();
        assert_eq!(cfg.rpi_host(), "malina");
    }

    #[test]
    fn load_trims_value_whitespace() {
        let cfg = load_with_contents(
            "rpi_host=   malina   \n\
             rpi_user=pi\n\
             deploy_path=/opt/rustbase\n",
        )
        .unwrap();
        assert_eq!(cfg.rpi_host(), "malina");
    }

    #[test]
    fn load_ignores_unknown_keys() {
        let cfg = load_with_contents(
            "unused_key=whatever\n\
             rpi_host=malina\n\
             rpi_user=pi\n\
             deploy_path=/opt/rustbase\n",
        )
        .unwrap();
        assert_eq!(cfg.rpi_host(), "malina");
    }

    #[test]
    fn load_ignores_malformed_lines_without_equals() {
        let cfg = load_with_contents(
            "bare_line_with_no_equals_sign\n\
             rpi_host=malina\n\
             rpi_user=pi\n\
             deploy_path=/opt/rustbase\n",
        )
        .unwrap();
        assert_eq!(cfg.rpi_host(), "malina");
    }

    #[test]
    fn load_last_value_wins_for_duplicate_keys() {
        let cfg = load_with_contents(
            "rpi_host=first\n\
             rpi_host=malina\n\
             rpi_user=pi\n\
             deploy_path=/opt/rustbase\n",
        )
        .unwrap();
        assert_eq!(cfg.rpi_host(), "malina");
    }
}
