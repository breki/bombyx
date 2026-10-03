//! Tests reading the shell text of `super::ACCOUNT`.
//!
//! The same kind of test `bootstrap_tests` holds, for the same
//! reason: `templates/account.sh` is shipped to the guest byte for
//! byte, so its text is a contract, and a test here cannot run it.
//! Every assertion that the script *does* something goes through
//! `super::account_code`, which drops the comment lines, so the
//! script's own prose cannot satisfy a needle.
//!
//! This script runs as root, so the lints pin the few things that
//! keep root narrow: nothing from the repository is read, every
//! refusal clears the staged credentials, the sudoers entry is
//! checked before it is installed, root writes nothing into the
//! agent's home, and the hand-over carries the Vagrantfile's
//! variables across.

use super::{ACCOUNT, account_code};

// The body of `refuse`, as code: everything between its opening
// line and the first line that is a lone `}`.
fn refuse_body() -> String {
    let start = ACCOUNT
        .find("\nrefuse() {\n")
        .expect("account.sh defines refuse");
    let body = &ACCOUNT[start..];
    let end = body.find("\n}\n").expect("refuse has a closing brace");
    super::script_code(&body[..end])
}

// `text` with the guest-egress loader's heredoc body cut out.
//
// The loader is a program of its own that systemd runs, so the
// lints about account.sh's control flow -- where it exits, what
// it refuses -- must not read it.
fn without_loader(text: &str) -> String {
    let start = text.find("<<'LOADER'").expect("the loader heredoc");
    let close = "\nLOADER\n";
    let end = start
        + text[start..].find(close).expect("the heredoc's end")
        + close.len();
    format!("{}{}", &text[..start], &text[end..])
}

#[test]
fn the_script_hands_over_through_sudo_with_the_preserve_list() {
    // `sudo` clears the environment by default, so without
    // `--preserve-env` bootstrap.sh would start with none of the
    // variables the Vagrantfile set and refuse at its first check.
    // `exec` keeps root from waiting around for it to finish.
    let code = account_code();
    assert!(
        code.contains(
            "exec sudo -u \"$user\" -H \
             --preserve-env=\"$BOMBYX_PRESERVE_ENV\" -- \"$BOOTSTRAP\""
        ),
        "the hand-over is missing:\n{code}"
    );
}

#[test]
fn every_exit_goes_through_refuse() {
    // `refuse` is what removes the staging directory, so an exit
    // anywhere else would leave the credentials in the login
    // account's home. The loader's own `exit` leaves the loader,
    // not this script, so it is not counted.
    let code = super::script_code(&without_loader(ACCOUNT));
    assert_eq!(
        code.matches("exit").count(),
        1,
        "exactly one exit, inside refuse:\n{code}"
    );
    assert!(refuse_body().contains("exit 1"));
}

#[test]
fn every_refusal_removes_the_staging_directory() {
    assert!(
        refuse_body().contains("rm -rf -- \"$STAGING\""),
        "refuse must remove the staged credentials"
    );
}

#[test]
fn a_refusal_after_the_first_write_removes_what_was_written() {
    // `place` writes the credentials one at a time, and a later
    // step can still refuse. bootstrap.sh never runs after a
    // refusal here, so nothing else would remove a key already
    // written into the agent's home. The removal runs as the
    // agent, for the reason `root_writes_nothing_into_the_agents_home`
    // gives.
    let body = refuse_body();
    for file in super::GUEST_HOME_FILES {
        assert!(
            body.contains(&format!("\"$home/{file}\"")),
            "refuse does not remove {file}:\n{body}"
        );
    }
    assert!(body.contains("sudo -u \"$user\" -- rm -f --"), "{body}");
}

#[test]
fn the_sudoers_entry_is_checked_before_it_is_installed() {
    // A sudoers file with a syntax error stops `sudo` working for
    // every account on the box, Vagrant's own included, and then
    // no later provision could repair it.
    let code = account_code();
    let check = code.find("visudo -cqf").expect("visudo checks the entry");
    let install = code
        .find("install -m 0440 -o root -g root \"$sudoers_tmp\"")
        .expect("the checked file is installed");
    assert!(check < install, "the check must come first");
}

#[test]
fn no_temporary_file_follows_the_projects_environment() {
    // The project's `[env]` table is in this script's own
    // environment, so a bare `mktemp` would create root's
    // sudoers draft wherever that table points `TMPDIR` -- a
    // directory the agent may own, or one that does not exist
    // yet. An absolute template names the directory itself.
    let code = account_code();
    assert!(
        code.contains("mktemp /etc/sudoers.d/.bombyx-XXXXXX"),
        "the sudoers draft must be created beside its target:\n{code}"
    );
    assert_eq!(code.matches("mktemp").count(), 1, "{code}");
}

#[test]
fn a_vm_set_up_for_another_account_is_refused_before_anything_is_written() {
    // Everything this script writes is under the current
    // `guest_user`, so after a rename nothing would ever remove
    // the old account's sudoers file or its credentials. The
    // check has to come before `useradd`, so a refused run leaves
    // the VM as it found it.
    let code = account_code();
    let check = code
        .find("for granted in /etc/sudoers.d/bombyx-*; do")
        .expect("account.sh looks for another account's grant");
    let create = code
        .find("useradd --create-home")
        .expect("account.sh creates the account");
    assert!(check < create, "the check must come first");
    assert!(
        code.contains("[ \"${granted#/etc/sudoers.d/bombyx-}\" != \"$user\" ]"),
        "{code}"
    );
    // A VM an earlier bombyx built has no such file: its agent
    // was the login account, and the credentials sit in that
    // account's home. Those paths are the tell.
    let older = code
        .find(
            "for left in \"$staging_home/.ssh/bombyx-deploy-key\" \
               \"$staging_home/.bombyx-env\" \
               \"$staging_home/.bombyx-git-credentials\"; do",
        )
        .expect("account.sh looks for an earlier bombyx's credentials");
    assert!(older < create, "that check must come first too");
}

#[test]
fn nothing_from_the_repository_is_read_as_root() {
    // The clone does not exist until bootstrap.sh makes it, as
    // the agent. A root script that named the repository, the
    // project's script or `git` would be one edit away from
    // running the project's code as root. `$HOME` is on the list
    // because a project's `[env]` table can set it.
    let code = account_code();
    for name in [
        "BOMBYX_REPO",
        "BOMBYX_REF",
        "BOMBYX_SCRIPT",
        "git ",
        "$HOME",
    ] {
        assert!(!code.contains(name), "account.sh reads {name}");
    }
}

#[test]
fn root_writes_nothing_into_the_agents_home() {
    // Each credential is written by the agent's account, from a
    // file root opened, so a link the agent left at one of those
    // paths reaches only what the agent could already reach. The
    // one writer is the `sh -c` below, run through `sudo -u`.
    //
    // `umask 077` comes first, so `cat >` creates the file at 0600
    // rather than at the inherited umask's 0644 until `chmod`
    // runs. An account's home is often traversable, and another
    // account holding the file open in that window keeps it.
    let code = account_code();
    assert!(
        code.contains(
            "sudo -u \"$user\" -- sh -c 'umask 077 && \
             mkdir -p -m 700 \"${1%/*}\" && \
             cat >\"$1\" && chmod 600 \"$1\"'"
        ),
        "the credentials must be written as the agent:\n{code}"
    );
    assert_eq!(code.matches(">\"$1\"").count(), 1, "{code}");
    assert!(!code.contains(">\"$home"), "{code}");
    assert!(!code.contains(">\"$3\""), "{code}");
}

// The egress step, as code: from its heading to the hand-over's,
// with the loader cut out, since the loader is not this script.
fn egress_step() -> String {
    let start = ACCOUNT
        .find("\n# 4. A BACKSTOP EGRESS RULE")
        .expect("account.sh has the egress step");
    let end = ACCOUNT
        .find("\n# 5. THE HAND-OVER")
        .expect("the hand-over follows it");
    super::script_code(&without_loader(&ACCOUNT[start..end]))
}

#[test]
fn the_egress_rule_warns_and_never_refuses() {
    // The rule is a backstop the agent can delete with one `sudo`
    // command, so a box without nftables, or a rule that fails to
    // load, must not stop the VM being built. Each failure prints
    // a warning and the provision carries on.
    let step = egress_step();
    assert!(!step.contains("refuse"), "{step}");
    assert!(!step.contains("exit"), "{step}");
    assert!(step.contains("egress_warning"), "{step}");
    for checked in ["command -v nft", "command -v systemctl"] {
        assert!(step.contains(checked), "{checked}: {step}");
    }
}

// The ranges inside `NAME="..."` in `text`, as a sorted list.
fn ranges(text: &str, name: &str) -> Vec<String> {
    let open = format!("{name}=\"");
    let start = text.find(&open).expect("the range list") + open.len();
    let len = text[start..].find('"').expect("a closing quote");
    let mut out: Vec<String> = text[start..start + len]
        .split(',')
        .map(|r| r.trim().to_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn the_egress_rule_blocks_what_the_host_firewall_blocks() {
    // The guest rule backs up scripts/agent-vm-firewall.sh, so the
    // two layers state one policy. A range added to one and not
    // the other would make the backstop disagree with the
    // containment it stands behind.
    let host = include_str!("../../../../scripts/agent-vm-firewall.sh");
    assert_eq!(ranges(ACCOUNT, "blocked4"), ranges(host, "BLOCKED_V4"));
}

#[test]
fn the_egress_rule_loads_before_the_agent_runs() {
    // bootstrap.sh clones the repository and runs the project's
    // script. Loading the rule after the hand-over would leave
    // that first run unfiltered on the guest's side.
    let code = account_code();
    let load = code
        .find("systemctl restart bombyx-guest-egress.service")
        .expect("account.sh loads the rule");
    let hand_over = code.find("exec sudo -u").expect("the hand-over");
    assert!(load < hand_over, "{code}");
    // Scoped to the interfaces that carry a default route, so a
    // container bridge inside the guest keeps working.
    assert!(code.contains("ip -4 route show default"), "{code}");
}

#[test]
fn the_egress_rule_lets_the_session_loading_it_live() {
    // A provision loads the rule inside Vagrant's SSH session. On
    // a guest where conntrack is not yet running, the session
    // predates the tracker, so its next packet counts as a new
    // connection to the gateway, which is a blocked address, and
    // without this accept the provision hangs. The comment above
    // the ruleset in account.sh gives the whole mechanism.
    //
    // nft applies a chain's rules in order, so the accept must sit
    // above `$rules`, where the loader puts the rejects.
    let code = account_code();
    let start = code.find("chain output {").expect("the output chain");
    let chain = &code[start..];
    let accept = chain
        .find("tcp sport 22 accept")
        .expect("sshd's replies are accepted in the output chain");
    let rejects = chain.find("$rules").expect("the rejects");
    assert!(accept < rejects, "{chain}");
}

// The guest-egress loader's own code: its heredoc body, as code.
fn loader_code() -> String {
    let start = ACCOUNT.find("<<'LOADER'").expect("the loader heredoc");
    let body = &ACCOUNT[start..];
    let body = &body[body.find('\n').expect("a body") + 1..];
    let end = body.find("\nLOADER\n").expect("the heredoc's end");
    super::script_code(&body[..end])
}

#[test]
fn a_failed_load_shows_the_loaders_own_error() {
    // The loader runs under systemd, so its stderr goes to the
    // journal, not to the provision output. The warning branch
    // prints the unit's journal so the cause reaches the operator.
    let step = egress_step();
    assert!(
        step.contains("journalctl -u bombyx-guest-egress.service"),
        "{step}"
    );
}

#[test]
fn the_loader_waits_for_a_default_route_and_names_a_missing_ip() {
    // At boot the unit can start before DHCP has finished. The
    // loader waits a bounded time for a default route rather than
    // failing once and never retrying. And a box without `ip` must
    // say so, not report "no default route".
    let code = loader_code();
    assert!(code.contains("command -v ip"), "{code}");
    assert!(code.contains("sleep 1"), "{code}");
}
