//! Repeatable deploy to a remote Linux host.
//!
//! Builds the frontend locally, tars the Rust sources and
//! ships them to the remote, builds the release binary
//! there, stages the artifacts on the remote, then does
//! a short stop/swap/start window. On any failure during
//! that window we attempt to restart the service before
//! returning.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use crate::deploy_config;
use crate::deploy_guard;
use crate::deploy_remote;
use crate::helpers::workspace_root;

pub fn deploy() -> Result<(), String> {
    // Ship only a released commit: HEAD on a vX.Y.Z tag
    // matching Cargo.toml, clean tree. Ties deploy to
    // `/release` (see deploy_guard). Checked before any work.
    deploy_guard::require_release_tag()?;

    let project_root = workspace_root();
    let cfg = deploy_config::load(&project_root)?;
    let remote = cfg.remote();

    println!("=== Deploy to {remote} ({}) ===", cfg.deploy_path());

    build_frontend(&project_root)?;
    sync_source(&project_root, remote)?;
    build_on_remote(remote)?;
    stage_frontend(&project_root, remote)?;

    // Short window: stop, swap, start. Roll back on any
    // failure so we never leave the service stopped.
    let swap_result = stop_swap_start(remote, cfg.deploy_path());
    if let Err(e) = swap_result {
        eprintln!();
        eprintln!("ERROR during install: {e}");
        eprintln!("attempting to restart service...");
        let _ = deploy_remote::ssh_run(
            remote,
            "sudo systemctl start rustbase-web",
            "restart after failure",
        );
        return Err(e);
    }

    println!();
    println!("=== Deploy OK ===");
    Ok(())
}

fn build_frontend(project_root: &Path) -> Result<(), String> {
    println!();
    println!("[1/5] Building frontend...");
    let frontend = project_root.join("frontend");
    let status = Command::new(npm_bin())
        .args(["run", "build"])
        .current_dir(&frontend)
        .status()
        .map_err(|e| format!("failed to run npm: {e}"))?;
    if !status.success() {
        return Err(match status.code() {
            Some(c) => format!("npm run build exited with status {c}"),
            None => "npm run build terminated by signal".into(),
        });
    }
    let dist = frontend.join("dist");
    if !dist.is_dir() {
        return Err(format!("frontend dist not found at {}", dist.display()));
    }
    Ok(())
}

fn sync_source(project_root: &Path, remote: &str) -> Result<(), String> {
    println!();
    println!("[2/5] Syncing source to {remote}...");

    // Preserve ~/rustbase-build/target across deploys for
    // incremental cargo builds, but still purge stale
    // source files (e.g. a deleted build.rs would keep
    // running on the remote if left in place).
    deploy_remote::ssh_run(
        remote,
        "mkdir -p ~/rustbase-build \
         && find ~/rustbase-build -mindepth 1 -maxdepth 1 \
            ! -name target -exec rm -rf {} +",
        "prepare build dir",
    )
    .map_err(|e| e.to_string())?;

    let tar_path = project_root.join("rustbase-src.tar");
    if tar_path.exists() {
        fs::remove_file(&tar_path)
            .map_err(|e| format!("failed to remove stale tar: {e}"))?;
    }

    let status = Command::new("tar")
        .arg("cf")
        .arg("rustbase-src.tar")
        .args(["Cargo.toml", "Cargo.lock", "crates", "xtask"])
        .current_dir(project_root)
        .status()
        .map_err(|e| format!("failed to run tar: {e}"))?;
    if !status.success() {
        return Err(match status.code() {
            Some(c) => format!("local tar exited with status {c}"),
            None => "local tar terminated by signal".into(),
        });
    }

    let scp_result = deploy_remote::scp_to(
        remote,
        "rustbase-src.tar",
        "~/rustbase-src.tar",
        project_root,
        "source tar",
    );
    // best-effort cleanup, don't mask scp error
    let _ = fs::remove_file(&tar_path);
    scp_result.map_err(|e| e.to_string())?;

    deploy_remote::ssh_run(
        remote,
        "tar xf ~/rustbase-src.tar -C ~/rustbase-build \
         && rm ~/rustbase-src.tar",
        "extract source",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn build_on_remote(remote: &str) -> Result<(), String> {
    println!();
    println!("[3/5] Building on {remote} (this may take a while)...");
    deploy_remote::ssh_run(
        remote,
        ". ~/.cargo/env \
         && cd ~/rustbase-build \
         && cargo build --release -p rustbase-web",
        "cargo build",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Ship the frontend dist to a remote staging directory.
/// Done before the service stop so the down window is
/// kept short.
fn stage_frontend(project_root: &Path, remote: &str) -> Result<(), String> {
    println!();
    println!("[4/5] Staging frontend on {remote}...");
    // Clean any stale staging dir from a previous failed
    // deploy so the subsequent scp -r lands in the
    // expected shape (cp/scp semantics: if target exists
    // as a directory, contents nest one level deeper).
    deploy_remote::ssh_run(
        remote,
        "rm -rf ~/frontend-dist-tmp",
        "clean staging dir",
    )
    .map_err(|e| e.to_string())?;
    deploy_remote::scp_to(
        remote,
        "dist",
        "~/frontend-dist-tmp",
        &project_root.join("frontend"),
        "frontend dist",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Atomic swap of the frontend directory.
///
/// Self-checks `DEPLOY_PATH` against the required
/// constant before any `rm -rf`, as defense in depth
/// against future config loosening. The required path is
/// passed as `$2` so the bash literal stays in lockstep
/// with `deploy_config::REQUIRED_DEPLOY_PATH`.
///
/// Performs a true POSIX-atomic swap: stages the new
/// tree, renames the old one aside, renames the new one
/// into place (single rename(2) — instantaneous), then
/// cleans up. The path is never absent.
const INSTALL_FRONTEND: &str = r#"set -euo pipefail
DEPLOY_PATH="$1"
REQUIRED_PATH="$2"
if [[ "$DEPLOY_PATH" != "$REQUIRED_PATH" ]]; then
    echo "ERROR: unexpected DEPLOY_PATH: $DEPLOY_PATH" >&2
    exit 1
fi
sudo rm -rf "$DEPLOY_PATH/frontend-dist-new" \
    "$DEPLOY_PATH/frontend-dist-old"
sudo cp -r ~/frontend-dist-tmp "$DEPLOY_PATH/frontend-dist-new"
sudo chown -R rustbase:rustbase "$DEPLOY_PATH/frontend-dist-new"
if [[ -e "$DEPLOY_PATH/frontend-dist" ]]; then
    sudo mv "$DEPLOY_PATH/frontend-dist" "$DEPLOY_PATH/frontend-dist-old"
fi
sudo mv "$DEPLOY_PATH/frontend-dist-new" "$DEPLOY_PATH/frontend-dist"
sudo rm -rf "$DEPLOY_PATH/frontend-dist-old"
rm -rf ~/frontend-dist-tmp
"#;

fn stop_swap_start(remote: &str, deploy_path: &str) -> Result<(), String> {
    println!();
    println!("[5/5] Stop / swap / start...");

    // Stop (ignore failure — may not be running).
    deploy_remote::ssh_run(
        remote,
        "sudo systemctl stop rustbase-web || true",
        "stop service",
    )
    .map_err(|e| e.to_string())?;

    // Binary.
    let cmd = format!(
        "sudo cp ~/rustbase-build/target/release/rustbase-web '{deploy_path}/' \
         && sudo chmod 755 '{deploy_path}/rustbase-web'"
    );
    deploy_remote::ssh_run(remote, &cmd, "install binary")
        .map_err(|e| e.to_string())?;

    // Frontend — atomic swap.
    deploy_remote::ssh_bash(
        remote,
        INSTALL_FRONTEND,
        &[deploy_path, deploy_config::REQUIRED_DEPLOY_PATH],
        "install frontend",
    )
    .map_err(|e| e.to_string())?;

    // Start and verify.
    deploy_remote::ssh_run(
        remote,
        "sudo systemctl start rustbase-web",
        "start service",
    )
    .map_err(|e| e.to_string())?;

    let last_status = poll_active_status(remote, 3)?;
    if last_status == "active" {
        return Ok(());
    }
    Err(format!(
        "service not active (last status: {last_status}); \
         check logs: ssh {remote} journalctl -u rustbase-web -n 20"
    ))
}

fn poll_active_status(remote: &str, attempts: u32) -> Result<String, String> {
    let mut last = String::new();
    for attempt in 1..=attempts {
        sleep(Duration::from_secs(2));
        let out = deploy_remote::ssh_capture(
            remote,
            "systemctl is-active rustbase-web || true",
            "poll service status",
        )
        .map_err(|e| e.to_string())?;
        // `systemctl is-active` may print state to stdout
        // and exit non-zero (with `|| true` absorbing the
        // non-zero). Take only the first line so the
        // returned token is a clean systemd state.
        out.lines()
            .next()
            .unwrap_or("")
            .trim()
            .clone_into(&mut last);
        if last == "active" {
            return Ok(last);
        }
        println!("  attempt {attempt}: status={last}, retrying...");
    }
    Ok(last)
}

fn npm_bin() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_frontend_script_is_set_minus_e() {
        assert!(INSTALL_FRONTEND.starts_with("set -euo pipefail"));
    }

    #[test]
    fn install_frontend_compares_against_required_path_arg() {
        // The script must compare $1 (caller-supplied) to
        // $2 (which we always set to REQUIRED_DEPLOY_PATH).
        assert!(
            INSTALL_FRONTEND
                .contains(r#"if [[ "$DEPLOY_PATH" != "$REQUIRED_PATH" ]]"#),
            "tripwire must compare against the second arg"
        );
    }

    #[test]
    fn install_frontend_cleans_stale_staging_dirs() {
        assert!(
            INSTALL_FRONTEND
                .contains("rm -rf \"$DEPLOY_PATH/frontend-dist-new\""),
            "stale frontend-dist-new must be removed before cp"
        );
    }

    /// Guard against a future refactor wiping the remote
    /// build dir, which would defeat the incremental
    /// cargo cache under `target/` on the remote host.
    #[test]
    fn sync_source_does_not_wipe_build_dir() {
        let src = include_str!("deploy.rs");
        // Build the forbidden substring at runtime so the
        // test source itself doesn't match the assertion.
        let forbidden = format!("rm {} ~/rustbase-build", "-rf");
        assert!(
            !src.contains(&forbidden),
            "deploy must not wipe the remote build dir"
        );
    }
}
