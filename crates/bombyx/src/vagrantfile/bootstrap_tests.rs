//! Tests reading the shell text of `super::BOOTSTRAP`.
//!
//! These are separate from `super::tests` because the subject
//! is different. That module renders a Vagrantfile and asserts
//! things about the Ruby it produced. This one reads
//! `templates/bootstrap.sh` as text and asserts things about
//! the shell, which makes it a lint over a file that
//! `include_str!` happens to pull into this crate. The two
//! groups share no fixture and no helper.
//!
//! The split has a practical point: an edit to `bootstrap.sh`
//! is reviewed here rather than in the middle of the
//! renderer's own tests, and the renderer's tests sit next to
//! the code they exercise.
//!
//! **Two tests in `super::tests` span both files**, and stay
//! there because neither half is the subject on its own:
//! `the_bootstrap_script_reads_the_path_the_vagrantfile_writes_to`
//! and `the_bootstrap_script_branches_on_the_announcement`.
//! Each compares `BOOTSTRAP` against a Rust constant, so each
//! catches a rename in *either* file. That is a property of
//! how they assert rather than of where they sit: a needle
//! taken over the raw text can be satisfied by the script's
//! own comments, which is why the announcement test goes
//! through `super::bootstrap_code` and the path test does not
//! need to -- `DEPLOY_KEY_GUEST_PATH` appears in the script's
//! code and nowhere in its prose.
//!
//! Two more stay there for different reasons, and it is worth
//! knowing which is which. In `super::tests`,
//! `the_shell_provisioner_runs_unprivileged` is a rendering
//! test whose companion here is
//! `nothing_in_the_bootstrap_script_asks_for_root`: one asserts
//! the flag is set, the other that no line in the script
//! defeats it. And `points_the_provisioner_at_the_bootstrap_script`
//! is a plain rendering test with no shell half at all.
//!
//! **A test here cannot run the script**, so every assertion is
//! over text. `CLAUDE.md` under **Test-Driven Development**
//! holds why that is accepted for this one file and refused for
//! rendered output: `bootstrap.sh` is the artifact bombyx
//! ships, byte for byte, so its text is a contract. A rendered
//! terminal report is not.

use super::{BOOTSTRAP, bootstrap_code};

// `BOOTSTRAP` with line continuations joined and every
// whitespace run collapsed to one space.
//
// Needed because a command in that file may be wrapped across
// lines, so the text a reader sees as one command is not a
// contiguous substring -- the same wrap trap `CLAUDE.md` warns
// about for grepping canon prose.
//
// A plain comment, not `///`: this whole file is `cfg(test)`,
// which neither rustdoc pass compiles, so a doc link here
// would never be resolved or checked.
fn flat_bootstrap() -> String {
    BOOTSTRAP
        .replace("\\\n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// `BOOTSTRAP` as lines, each with its continuations joined and
// its whitespace collapsed.
//
// `flat_bootstrap` answers "does this text appear anywhere";
// this one answers "what does each command look like", which is
// what a per-line invariant needs. `super::bootstrap_code`
// answers "does the script actually do this", by dropping the
// comment lines first.
fn flat_bootstrap_lines() -> Vec<String> {
    BOOTSTRAP
        .replace("\\\n", " ")
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn every_variable_is_declared_before_it_is_expanded() {
    // `set -u` makes expanding an unset variable fatal, so
    // a refusal that removes the deploy key before its path
    // is declared aborts with "unbound variable" -- the
    // message never prints and the key stays. The other
    // text tests here cannot catch it, because they compare
    // offsets of things that are present rather than asking
    // whether the shell can run them.
    //
    // Checked for the two the script declares itself.
    // `readonly` lines are the declarations; anything of
    // the form `$NAME` or `"$NAME"` before one is a use.
    let flat = flat_bootstrap_lines();
    for name in ["DEPLOY_KEY", "CLONE_DIR"] {
        let decl = flat
            .iter()
            .position(|l| l.starts_with(&format!("readonly {name}=")))
            .unwrap_or_else(|| panic!("{name} must be declared"));
        let first_use = flat.iter().position(|l| {
            !l.starts_with('#') && l.contains(&format!("${name}"))
        });
        if let Some(u) = first_use {
            // The two lines quoted, not their indices.
            // `flat_bootstrap_lines` joins each `\`-continuation
            // into the line above, so an index into it is not a
            // file line number.
            assert!(
                decl <= u,
                "${name} is used before it is declared.\n  \
                 use:  {}\n  decl: {}",
                flat[u],
                flat[decl]
            );
        }
    }
}

#[test]
fn every_refusal_clears_the_uploaded_key() {
    // Vagrant uploads the key before this script starts, so
    // a refusal that exits without removing it leaves a
    // credential at whatever mode `scp` gave it, in a guest
    // that never provisioned.
    //
    // The rule is structural rather than stated:
    // `refuse` removes the key and exits, and no `exit` or
    // `return` is allowed outside it. The checks on
    // `$HOME` sit above the key block and refuse through
    // it, which `an_unusable_home_is_refused_by_name`
    // pins.
    let mut inside = false;
    for line in flat_bootstrap_lines() {
        if line.starts_with("refuse() {") {
            inside = true;
            continue;
        }
        if inside && line == "}" {
            inside = false;
            continue;
        }
        // Any `exit`, not `exit 1`, and `return` with it. A
        // refusal spelled `exit 2`, a bare `exit` or a
        // `return 1` out of a helper skips the removal exactly
        // as `exit 1` would, and the narrower needle could not
        // see any of them.
        let leaves = line
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == "exit" || w == "return");
        if line.starts_with('#') || !leaves {
            continue;
        }
        assert!(
            inside,
            "a refusal that does not clear the key: {line}\n\
             Call `refuse \"message\"` instead of exiting."
        );
    }
    assert!(
        flat_bootstrap().contains("rm -f \"$DEPLOY_KEY\""),
        "refuse must remove the uploaded key"
    );
}

#[test]
fn every_command_on_the_key_reports_its_own_failure() {
    // The agent does the key removal, and `rm` gives up on
    // a file in a directory it cannot write -- which the
    // project's own script can arrange, because it has
    // `sudo` and this guest to itself.
    //
    // Under `set -e` an unguarded failure aborts the script
    // *inside* `refuse`, before either `echo`. So the
    // operator gets a bare `rm: Permission denied` naming
    // no part of bombyx, and the credential stays in a
    // guest that never provisioned -- the one invariant
    // this file is arranged around.
    //
    // Per line, because all three commands act on the same
    // path, one line at a time.
    for line in flat_bootstrap_lines() {
        if line.starts_with('#') || !line.contains("$DEPLOY_KEY") {
            continue;
        }
        // INVERTED ON PURPOSE: every line naming the key is
        // examined, and the shapes that only read it are
        // allowed. Listing the verbs instead -- `rm`, `chmod`,
        // `install` -- leaves the next one unexamined, and
        // `mv`, `cp`, `ln -sf`, `truncate` and `shred` all
        // reach the same file.
        //
        // The criterion for an allowance is that the line does
        // not touch the file at that path: it prints the path,
        // declares it, or reads it into a variable.
        //
        // EVERY ENTRY IS ANCHORED. An allowance matched
        // anywhere in the line exempts the whole line, so
        // `mv "$DEPLOY_KEY" /tmp/k; GIT_SSH_COMMAND=x` walked
        // through the unanchored version -- measured.
        let only_reads = [
            "echo ",
            "refuse ",
            "readonly ",
            "key_note=",
            "export GIT_SSH_COMMAND=",
        ]
        .iter()
        .any(|allowed| line.starts_with(*allowed));
        if only_reads {
            continue;
        }
        // `if `, and no `|| true` alternative. Silencing
        // one of these commands produces the outcome this
        // guard exists to prevent, and for the removal it
        // is worse than the abort: the script would carry
        // on and print "any uploaded deploy key has been
        // removed from this guest" over a key that is still
        // there.
        assert!(
            line.starts_with("if "),
            "this command on the key can abort without reporting:\n  \
             {line}\n\
             Test its status: `if ! ... ; then refuse \"...\"; fi`. \
             Do not silence it -- a key left behind quietly is \
             what this guard is for."
        );
    }
}

#[test]
fn the_bootstrap_script_points_ssh_at_the_key_and_nothing_else() {
    // `IdentitiesOnly=yes` alone is not enough: it does not
    // exclude identities named in an `ssh_config`, so
    // `-F /dev/null` is what makes "only this key" true.
    //
    // Whole clauses over a comment-stripped view. All three
    // of those words appear in the comment above the code that
    // builds them, so a bare needle over the raw text is
    // satisfied by the prose and says nothing about the
    // code -- and the subject here is a credential scoped to a
    // single identity.
    let code = bootstrap_code();
    for clause in [
        "ssh_opts=\"-o IdentitiesOnly=yes -F /dev/null\"",
        "export GIT_SSH_COMMAND=\"ssh -i $DEPLOY_KEY $ssh_opts\"",
    ] {
        assert!(code.contains(clause), "not in the script: {clause}");
    }
}

#[test]
fn only_one_user_ever_verifies_the_git_host() {
    // One account clones and pushes, so `ssh` reads that
    // account's own `~/.ssh/known_hosts`. Naming a file
    // here would point the clone at a path the agent can
    // rewrite.
    assert!(
        !BOOTSTRAP.contains("UserKnownHostsFile"),
        "the clone must use the agent's own known_hosts"
    );
}

#[test]
fn the_clone_sits_in_the_home_the_provisioner_was_given() {
    // `$HOME` is the account's own home because the
    // provisioner is unprivileged, so the shell that runs
    // `bootstrap.sh` was started with it already set.
    //
    // The derivation is pinned as a literal. Asserting only
    // that `$HOME` appears somewhere would pass with
    // `CLONE_DIR=/srv/project` written underneath a comment
    // that mentions it.
    let flat = flat_bootstrap();
    assert!(
        flat.contains("readonly CLONE_DIR=\"$HOME/project\""),
        "the clone must sit in the account's own home"
    );
}

#[test]
fn an_unusable_home_is_refused_by_name() {
    // A project's `[env]` table can set `HOME`, so the
    // value is not guaranteed sound, and each shape below
    // otherwise reaches a bare `git` error naming no part
    // of bombyx. Each check is asserted as a literal
    // rather than as "a `case` exists": changing `/?*)` to
    // `?*)` re-admits every relative home, which is the
    // defect this family was raised for.
    let flat = flat_bootstrap();

    // Unset or empty. `${HOME:-}` and not `$HOME`, because
    // `set -u` aborts with "unbound variable" before
    // `refuse` can clear the uploaded key.
    assert!(
        flat.contains("if [ -z \"${HOME:-}\" ]; then"),
        "an unset HOME must be refused before set -u aborts"
    );
    // Relative, `.`, `..` and `/` itself. A relative home
    // puts the clone wherever the provisioner started, and
    // `/` gives `//project` under a root-owned parent.
    assert!(
        flat.contains("case \"$HOME\" in /?*) ;;"),
        "the shape check must refuse a relative home"
    );
    // Named but absent. `/nonexistent` is what
    // `useradd -M` writes.
    assert!(
        flat.contains("if [ ! -d \"$HOME\" ]; then"),
        "the home must be required to exist"
    );
    // Writable *and* searchable: a directory at mode 0600
    // passes `test -w` while `mkdir` in it fails.
    assert!(
        flat.contains("if [ ! -w \"$HOME\" ] || [ ! -x \"$HOME\" ]; then"),
        "the home must be writable and searchable"
    );

    // Owned by this account. `HOME=/tmp` passes every
    // check above -- set, absolute, present, and mode 1777
    // gives both bits -- and the clone would then sit in a
    // world-writable directory, `.git/config` and the
    // `core.sshCommand` naming the deploy key with it.
    assert!(
        flat.contains("if [ ! -O \"$HOME\" ]; then"),
        "the home must be owned by the account cloning into it"
    );

    // And the unset check comes first. Every other check
    // expands `$HOME` bare, so under `set -u` an unset
    // `HOME` reaching one of them aborts the script with
    // "unbound variable" -- before `refuse` can remove the
    // uploaded deploy key, which then stays in a guest that
    // never provisioned.
    //
    // `every_refusal_clears_the_uploaded_key` cannot see
    // this: it compares the offsets of `exit 1` and the
    // removal, and an abort is neither.
    let lines = flat_bootstrap_lines();
    let guard = lines
        .iter()
        .position(|l| l.contains("[ -z \"${HOME:-}\" ]"))
        .expect("the unset check must be there");
    // Any spelling of a bare expansion. `$HOME` unquoted,
    // `"$HOME"` quoted and `${HOME}` braced all abort under
    // `set -u`, and this script writes more than one of them.
    // `${HOME:-` is the guarded form, so it is what the first
    // bare use is looked for *after*.
    let first_bare = lines
        .iter()
        .position(|l| {
            !l.starts_with('#')
                && l.contains("$HOME")
                && !l.contains("${HOME:-")
        })
        .expect("HOME must be used");
    assert!(
        guard < first_bare,
        "$HOME is expanded before it is checked for being \
         unset.\n  expanded: {}\n  checked:  {}",
        lines[first_bare],
        lines[guard]
    );
}

#[test]
fn a_fetch_or_checkout_that_cannot_finish_says_so() {
    // Both fail on a tracked file inside a directory the
    // agent cannot write. `git checkout --force` exits 1
    // with "unable to unlink old ... Permission denied"
    // *after* printing "Switched to branch" -- measured --
    // so the worktree is half-changed and `set -e` then
    // aborts with nothing naming bombyx.
    //
    // Checked here rather than by sweeping the tree for
    // foreign ownership beforehand. A sweep refuses
    // provisions over build output no git command touches,
    // cannot report itself when it meets an unsearchable
    // directory, and a symlink at the clone skips it.
    let flat = flat_bootstrap();
    for needle in [
        "if ! git -C \"$CLONE_DIR\" fetch",
        "if ! git -C \"$CLONE_DIR\" checkout",
    ] {
        assert!(flat.contains(needle), "unchecked: {needle}");
    }
    assert!(
        flat.contains("could not update the clone"),
        "a failure must name the path and say what to do"
    );
}

#[test]
fn a_reclone_over_a_leftover_directory_says_so() {
    // A discard that failed part-way may already have
    // deleted `.git`, so the next provision skips the
    // mismatch branch entirely -- the message that
    // diagnosed it is unreachable -- and `git clone` dies
    // with "destination path already exists and is not an
    // empty directory". Measured.
    let flat = flat_bootstrap();
    assert!(
        flat.contains("is not empty"),
        "a non-empty clone directory must be refused by name"
    );
}

#[test]
fn a_discard_that_cannot_finish_says_so() {
    // `rm -rf` fails on a directory inside the clone the
    // agent cannot write, and GNU `rm` does not chmod its
    // way in. Measured: exit 1, "Permission denied", and a
    // partly deleted tree. Nothing normalises such content,
    // deliberately: a root `chown -R` on a tree the agent
    // owns is the escalation this arrangement removes.
    //
    // A project script running one `sudo` step inside its
    // own checkout is enough to reach it -- and
    // `docs/tutorial.md` documents `sudo` as available. So
    // the status is tested and the operator is told what to
    // remove, rather than `set -e` aborting after the
    // "discarding the clone" message has already printed.
    let flat = flat_bootstrap();
    assert!(
        flat.contains("if ! rm -rf \"$CLONE_DIR\""),
        "the discard's failure must be caught"
    );
    assert!(
        flat.contains("could not remove"),
        "a failed discard must name the path and say what to do"
    );
}

#[test]
fn the_clone_is_pinned_from_the_config_not_the_environment() {
    // `GIT_SSH_COMMAND` is inherited from the environment. The
    // deploy-key banner near the top of `bootstrap.sh` states
    // the rule: whether a key was configured must never be
    // re-derived from inside the guest, because an export in
    // `/etc/profile.d` reaches a login shell provisioner.
    // Deciding the `core.sshCommand` write from that variable
    // would let the guest re-pin the clone to a key the
    // operator had just removed.
    //
    // `--replace-all`, because a plain set refuses with
    // "cannot overwrite multiple values" and exits 5 when
    // the key already has two -- measured -- which `set -e`
    // would turn into an aborted provision after the clone
    // and fetch had already run.
    let flat = flat_bootstrap();
    assert!(
        flat.contains("config --replace-all core.sshCommand"),
        "the pin must use --replace-all"
    );
    assert!(
        !flat.contains("if [ -n \"${GIT_SSH_COMMAND:-}\" ]"),
        "the write must not be gated on the inherited value"
    );
    assert!(
        flat.contains("if [ \"${BOMBYX_DEPLOY_KEY:-}\" = 1 ]"),
        "the authoritative flag must be what decides"
    );
}

#[test]
fn removing_the_key_unpins_the_clone_from_it() {
    // The `else` branch of `bootstrap.sh`'s deploy-key block
    // handles a key nothing points at. This is the other
    // direction: a pointer to a key that is gone.
    // `core.sshCommand` names the deleted identity, and
    // `IdentitiesOnly=yes` with
    // `-F /dev/null` stops git falling back to one the
    // agent does hold -- so every fetch and push fails with
    // an ssh error naming nothing about bombyx.
    // `--unset-all`, not `--unset`: measured, `--unset`
    // against two values warns, exits 5 and removes
    // nothing -- indistinguishable from the exit 5 that
    // means "there was nothing to unset".
    let flat = flat_bootstrap();
    assert!(
        flat.contains("config --unset-all core.sshCommand"),
        "the clone keeps pointing at a key that is gone"
    );
    // And it has to run after the clone exists, since
    // there is no config file to unset anything from
    // before that.
    let clone = flat
        .find("git clone --depth 1")
        .expect("the clone must be there");
    let unset = flat
        .find("config --unset-all core.sshCommand")
        .expect("the unset must be there");
    assert!(clone < unset, "the clone must exist before the unset");
}

#[test]
fn the_deploy_key_ends_up_readable_by_the_agent() {
    // The agent has to push with this key: `bootstrap.sh`
    // says committing in the guest does not survive a
    // provision and pushing is what does. A root-owned key
    // the agent cannot read makes that impossible, so the
    // key is left at 0600 where the provisioner put it
    // rather than moved out of reach.
    //
    // It is owned by the agent because Vagrant's file
    // provisioner uploads as that user -- not because
    // bombyx chowns it. There is therefore no chown to
    // assert. `docs/trust-boundary.md` under **What this
    // costs** states what the mode does and does not buy.
    assert!(
        BOOTSTRAP.contains("chmod 600 \"$DEPLOY_KEY\""),
        "the key must end up at 0600"
    );
    // The two negatives forbid the shapes that would move
    // the key out of the agent's reach: an `install` that
    // places it as another user, and an ownership flag on
    // the same operation. Either one leaves the agent
    // unable to push.
    for gone in ["-o root", "install -m 600"] {
        assert!(!BOOTSTRAP.contains(gone), "{gone} is back");
    }
}

#[test]
fn nothing_in_the_bootstrap_script_asks_for_root() {
    // `super::tests` holds one half of this arrangement, as
    // `the_shell_provisioner_runs_unprivileged`, and this is
    // the other. A line here that raises privilege puts root
    // back inside a tree the agent owns, and the rendered flag
    // would go on saying the script is unprivileged.
    //
    // Root in that tree is a measured escalation rather than
    // a theoretical one. git trusts the uid in `SUDO_UID` as
    // well as root's own (see `safe.directory` in
    // git-config(1)), so a `post-checkout` hook the agent
    // planted was seen running with `uid=0`.
    //
    // Comments are skipped: they name `sudo` where they
    // describe what a project's own script may do, and that
    // is a true statement about a different script.
    //
    // The list enumerates the family, and there is no
    // allowance list to add a name to: a command that
    // changes the effective user has no business in this
    // file at all. `setpriv` and `sg` are here because
    // `setpriv --reuid 0` ships in util-linux beside
    // `runuser`, and `sg`/`newgrp` do the same for a group.
    // A new one has to be added here by hand, which is the
    // trade -- a test cannot parse shell.
    const RAISERS: [&str; 8] = [
        "runuser", "sudo", "su", "pkexec", "doas", "setpriv", "sg", "newgrp",
    ];

    for line in flat_bootstrap_lines() {
        if line.starts_with('#') {
            continue;
        }
        // The word, not the letters, so `issue` and
        // `status` are left alone while `/usr/sbin/runuser`
        // still counts. `.` and `/` count as part of a word
        // so that `${a%.git}` is not read as the command
        // `git`, while `/usr/bin/git` still is.
        let word_char =
            |c: char| c.is_alphanumeric() || c == '_' || c == '/' || c == '.';
        for word in line.split(|c: char| !word_char(c)) {
            // The last path component, so `/usr/sbin/runuser`
            // counts and `/usr/bin/sg` does. Comparing the
            // whole word would miss both.
            let command = word.rsplit('/').next().unwrap_or(word);
            assert!(
                !RAISERS.contains(&command),
                "this line raises privilege:\n  {line}\n\
                 The provisioner is unprivileged and every \
                 command here acts on the agent's own home. \
                 A project needing root calls `sudo` from \
                 its own script."
            );
        }
        // Both spellings of root's own home. `~root` expands
        // to it without the string `/root` appearing, so a
        // check on the path alone would let `~root/.ssh`
        // through.
        for reach in ["/root", "~root"] {
            assert!(
                !line.contains(reach),
                "this line reaches root's own home:\n  {line}\n\
                 The provisioner is unprivileged, so nothing \
                 here can write there."
            );
        }
    }
}

#[test]
fn the_clone_is_told_which_key_to_push_with() {
    // Otherwise the agent has a key it may read and no
    // reason to know where it is. Recording it on the
    // clone means a plain `git push` in the guest works
    // without the project's script arranging anything.
    assert!(
        flat_bootstrap().contains("config --replace-all core.sshCommand"),
        "the clone must record the ssh command"
    );
}

#[test]
fn the_bootstrap_script_deletes_a_key_no_upload_replaced() {
    // Removing `deploy_key` from the config has to remove
    // the credential from the guest, not leave one behind
    // that nothing points at.
    assert!(
        BOOTSTRAP.contains("rm -f \"$DEPLOY_KEY\""),
        "the stale-key removal is gone"
    );
}

#[test]
fn the_project_script_runs_as_the_agent_not_as_root() {
    // The whole provisioner is unprivileged, so this
    // `exec` changes no privilege. What it does decide is
    // what the process becomes: `exec` replaces this
    // script rather than starting a second process beside
    // it, so the project's script inherits the process and
    // its exit status is what Vagrant sees.
    // `docs/architecture.md` under **Who runs the
    // project's script** holds the argument.
    //
    // The needle is the whole `exec` line, because a call
    // that started the script as a child would still
    // contain the path somewhere in the file.
    assert!(
        BOOTSTRAP.contains("exec -- \"$script_real\""),
        "the hand-over must exec the project's script"
    );
    // And the bit `exec` needs is set first. `chmod +x` is
    // the one metadata change bombyx makes inside the
    // clone, so nothing else asserts it.
    assert!(
        BOOTSTRAP.contains("chmod +x \"$script_real\""),
        "the script must be made executable before the exec"
    );
}

#[test]
fn the_bootstrap_script_guards_every_variable_it_needs() {
    // Without the `:?` guards an unset variable clones into
    // an empty path.
    for guard in ["BOMBYX_REPO:?", "BOMBYX_REF:?", "BOMBYX_SCRIPT:?"] {
        assert!(BOOTSTRAP.contains(guard), "{guard} missing");
    }
    assert!(BOOTSTRAP.contains("set -euo pipefail"));
}
