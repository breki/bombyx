//! One-time remote-host provisioning.
//!
//! Creates the `rustbase` system user and directory tree,
//! then installs and enables the systemd unit. Idempotent:
//! safe to re-run; existing user/dirs/service are skipped.

use std::fs;
use std::path::Path;

use crate::deploy_config;
use crate::deploy_remote;
use crate::helpers::workspace_root;

pub fn deploy_setup() -> Result<(), String> {
    let project_root = workspace_root();
    let cfg = deploy_config::load(&project_root)?;
    let remote = cfg.remote();
    let service_file = project_root.join("deploy").join("rustbase-web.service");

    println!("=== Setup rustbase-web on {remote} ===");
    println!("  deploy_path: {}", cfg.deploy_path());
    println!();

    create_user_and_dirs(remote, cfg.deploy_path())?;
    install_service(remote, &service_file)?;
    verify(remote, cfg.deploy_path())?;
    print_final_message(&service_file, cfg.rpi_host());
    Ok(())
}

const SETUP_USER: &str = r#"set -euo pipefail
DEPLOY_PATH="$1"

if ! id -u rustbase &>/dev/null; then
    sudo useradd --system --shell /usr/sbin/nologin \
        --home-dir "$DEPLOY_PATH" rustbase
    echo "  Created 'rustbase' system user"
else
    echo "  User 'rustbase' already exists"
fi

sudo mkdir -p "$DEPLOY_PATH/frontend-dist"
sudo chown -R rustbase:rustbase "$DEPLOY_PATH"
sudo chmod 750 "$DEPLOY_PATH"
echo "  Directories ready"
"#;

fn create_user_and_dirs(remote: &str, deploy_path: &str) -> Result<(), String> {
    println!("[1/3] Creating rustbase user and directories...");
    deploy_remote::ssh_bash(remote, SETUP_USER, &[deploy_path], "setup user")
        .map_err(|e| e.to_string())?;
    Ok(())
}

const INSTALL_SERVICE: &str = r#"set -euo pipefail
sudo cp ~/rustbase-web.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable rustbase-web
rm ~/rustbase-web.service
echo "  Service installed and enabled"
"#;

fn install_service(remote: &str, service_file: &Path) -> Result<(), String> {
    println!();
    println!("[2/3] Installing systemd service...");
    if !service_file.is_file() {
        return Err(format!(
            "service file not found at {}\n  \
             expected: deploy/rustbase-web.service",
            service_file.display()
        ));
    }
    let parent = service_file.parent().ok_or_else(|| {
        format!(
            "service file path has no parent: {}",
            service_file.display()
        )
    })?;
    let name = service_file
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            format!(
                "service file name is not utf-8: {}",
                service_file.display()
            )
        })?;
    deploy_remote::scp_to(
        remote,
        name,
        "~/rustbase-web.service",
        parent,
        "service file",
    )
    .map_err(|e| e.to_string())?;
    deploy_remote::ssh_bash(remote, INSTALL_SERVICE, &[], "install service")
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Intentionally does NOT `set -e`: we want to report every
// checked item even when earlier ones are missing.
const VERIFY: &str = r#"DEPLOY_PATH="$1"
echo "  User:        $(id rustbase 2>/dev/null || echo 'MISSING')"
echo "  Deploy dir:  $(sudo ls -ld "$DEPLOY_PATH" 2>/dev/null \
    | awk '{print $1, $3, $4}' || echo 'MISSING')"
echo "  Service:     $(systemctl is-enabled rustbase-web 2>/dev/null \
    || echo 'not installed')"
"#;

fn verify(remote: &str, deploy_path: &str) -> Result<(), String> {
    println!();
    println!("[3/3] Verifying setup...");
    deploy_remote::ssh_bash(remote, VERIFY, &[deploy_path], "verify")
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Print the final banner. Infallible — a failure to
/// re-read the service file at this point is purely
/// cosmetic (the remote is fully provisioned), so we fall
/// through to a "?" placeholder rather than fail the
/// command.
fn print_final_message(service_file: &Path, rpi_host: &str) {
    let port = fs::read_to_string(service_file)
        .ok()
        .and_then(|c| parse_port(&c))
        .map_or_else(|| "?".into(), |p| p.to_string());
    println!();
    println!("=== Setup complete ===");
    println!();
    println!("Next steps:");
    println!("  1. Run: cargo xtask deploy");
    println!("  2. Access: http://{rpi_host}:{port}");
}

/// Extract a `--port N` or `--port=N` value as `u16`.
/// Returns `None` for missing, malformed, or
/// out-of-range values.
fn parse_port(service_contents: &str) -> Option<u16> {
    let flag = "--port";
    let idx = service_contents.find(flag)?;
    let rest = &service_contents[idx + flag.len()..];
    let digits: String = rest
        .chars()
        .skip_while(|c| c.is_whitespace() || *c == '=')
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_port_space_form() {
        let svc = "[Service]\n\
                   ExecStart=/opt/rustbase/rustbase-web \
                   --port 9200 --bind 0.0.0.0\n";
        assert_eq!(parse_port(svc), Some(9200));
    }

    #[test]
    fn parse_port_equals_form() {
        let svc = "ExecStart=... --port=9200 --bind=...";
        assert_eq!(parse_port(svc), Some(9200));
    }

    #[test]
    fn parse_port_missing() {
        assert_eq!(parse_port("no port here"), None);
    }

    #[test]
    fn parse_port_trailing_garbage_truncated() {
        let svc = "--port 9200abc";
        assert_eq!(parse_port(svc), Some(9200));
    }

    #[test]
    fn parse_port_out_of_range() {
        let svc = "--port 99999";
        assert_eq!(parse_port(svc), None);
    }

    #[test]
    fn scripts_all_have_set_minus_e() {
        for (name, script) in [
            ("SETUP_USER", SETUP_USER),
            ("INSTALL_SERVICE", INSTALL_SERVICE),
        ] {
            assert!(
                script.contains("set -euo pipefail"),
                "{name} missing set -euo pipefail"
            );
        }
    }
}
