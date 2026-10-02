//! Tests holding the Vagrantfile, `account.sh` and
//! `bootstrap.sh` to one another.
//!
//! A file staged for the guest is named three times: the
//! Vagrantfile uploads it, `account.sh` moves it into the agent's
//! home, and `bootstrap.sh` reads it there. None of the three can
//! see the others, so a rename in one leaves the guest looking at
//! an empty path. The tests here pin what the three must agree
//! on: the staged paths, the credential paths in the agent's
//! home, the account-name pattern, and the variables `sudo`
//! passes from the Vagrantfile's env hash on to the scripts.
//!
//! A child of `super`, the renderer's own test module, so the
//! config fixtures and `rendered_for` serve both without being
//! widened.

use super::*;

/// Every staged path, as the Vagrantfile writes it.
const STAGED_PATHS: [&str; 4] = [
    BOOTSTRAP_STAGED_PATH,
    DEPLOY_KEY_STAGED_PATH,
    ENV_FILE_STAGED_PATH,
    CREDENTIAL_STAGED_PATH,
];

#[test]
fn the_account_script_reads_every_path_the_vagrantfile_stages() {
    // Two files have to agree on each path and neither can
    // see the other: the Vagrantfile uploads to it and
    // `account.sh` reads it. Over the code, not the raw
    // text, so a name mentioned only in a comment does not
    // count.
    let code = account_code();
    let dir = STAGING_DIR
        .strip_prefix("~/")
        .expect("the staging directory is under the login home");
    assert!(
        code.contains(&format!("/{dir}\"")),
        "account.sh does not name {dir}:\n{code}"
    );
    for path in STAGED_PATHS {
        let name = path
            .strip_prefix(&format!("{STAGING_DIR}/"))
            .expect("every staged path is in the staging directory");
        assert!(
            code.contains(&format!("\"$STAGING/{name}\"")),
            "account.sh never reads the staged {name}"
        );
    }
}

#[test]
fn both_guest_scripts_spell_each_credential_path_the_same_way() {
    // `account.sh` writes each credential into the agent's
    // home and `bootstrap.sh` reads it there. A rename in one
    // file would leave the other looking at an empty path,
    // and the guest would refuse the key as never arriving.
    let account = account_code();
    let bootstrap = bootstrap_code();
    for file in GUEST_HOME_FILES {
        assert!(
            account.contains(&format!("\"$home/{file}\"")),
            "account.sh does not write {file}"
        );
        assert!(
            bootstrap.contains(&format!("/{file}\"")),
            "bootstrap.sh does not read {file}"
        );
    }
}

#[test]
fn both_guest_scripts_refuse_the_same_account_names() {
    // Each script checks the name for itself, because each
    // uses it in a place a shell reads. One pattern in both
    // means a tightening in one cannot miss the other.
    let pattern = "\"\" | [!a-z_]* | *[!a-z0-9_-]*)";
    assert!(account_code().contains(pattern), "account.sh");
    assert!(bootstrap_code().contains(pattern), "bootstrap.sh");
}

#[test]
fn every_upload_lands_in_the_staging_directory() {
    // The login account can write only its own home, and
    // `account.sh` clears exactly this directory once it has
    // moved the files on. An upload anywhere else would be
    // left behind in the login account's home.
    let mut cfg = cfg_with_credential();
    cfg.source.deploy_key =
        Some(DeployKeyPath::parse(KEY).expect("a valid fixture path"));
    let out = rendered_for(&cfg);
    let dests: Vec<&str> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("destination: "))
        .collect();
    assert_eq!(dests.len(), STAGED_PATHS.len(), "{out}");
    for dest in dests {
        assert!(
            dest.starts_with(&format!("\"{STAGING_DIR}/")),
            "{dest} is outside the staging directory"
        );
    }
    for path in STAGED_PATHS {
        assert!(
            path.starts_with(&format!("{STAGING_DIR}/")),
            "{path} is outside the staging directory"
        );
    }
}

#[test]
fn the_preserve_list_names_every_variable_the_hash_sets() {
    // `sudo` drops every variable the list leaves out, so a
    // name missing here reaches the root script and never
    // reaches `bootstrap.sh` or the project's own script.
    // The hash is read back from the rendering rather than
    // from `BOMBYX_ENV_NAMES`, which is the list this checks.
    let out = rendered_for(&cfg_with_env());
    let hash = out
        .split_once("env: {\n")
        .expect("the provisioner has an env hash")
        .1
        .split_once("\n    }")
        .expect("the hash closes")
        .0;
    let mut set: Vec<&str> = hash
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix('"'))
        .filter_map(|l| l.split_once('"').map(|(name, _)| name))
        .filter(|name| *name != PRESERVE_ENV)
        .collect();
    set.sort_unstable();
    let listed = rendered(&out, PRESERVE_ENV);
    let mut names: Vec<&str> = listed.trim_matches('"').split(',').collect();
    names.sort_unstable();
    assert_eq!(names, set);
    assert!(set.contains(&"NODE_MAJOR"), "the fixture's [env]:\n{out}");
}
