//! The checks that run on this workstation.

use std::path::Path;

use super::text::sanitize;
use super::{
    Finding, Outcome, ProbeResult, Scope, VersionAnswer, cannot_run,
    not_on_path,
};

/// The detail for a local tool, from whatever it printed.
///
/// Keeps the name and version, which for `tar` matters:
/// `bsdtar` and GNU `tar` differ on `--exclude` pattern
/// matching and the push depends on it. Some builds print the
/// banner on stderr -- `ssh -V` always does -- so both streams
/// are considered rather than reporting a pass with nothing in
/// it.
fn tool_banner(result: &ProbeResult) -> String {
    let text = if result.stdout.trim().is_empty() {
        &result.stderr
    } else {
        &result.stdout
    };
    let mut words = text.split_whitespace();
    let Some(name) = words.next() else {
        return "version unknown".to_owned();
    };
    // The first version-looking token, not simply the second
    // word: GNU tar announces itself as "tar (GNU tar) 1.35",
    // where the second word is "(GNU".
    let version =
        words.find(|w| w.chars().any(char::is_numeric) && w.contains('.'));
    // `ssh -V` prints "OpenSSH_for_Windows_9.5p1, LibreSSL
    // 3.8.2", so the comma can land on either token.
    let name = name.trim_end_matches(',');
    let short = match version {
        Some(v) => format!("{name} {}", v.trim_end_matches(',')),
        None => name.to_owned(),
    };
    sanitize(&short)
}

/// Turns a local tool lookup into a finding.
///
/// The binary resolves the program and, when there is a version
/// flag worth asking for, runs it -- and does nothing else. Every
/// decision about what those results *mean* is here, because
/// `src/bin/` is excluded from the coverage gate and a diagnostic
/// whose own reasoning is untested is not much of a diagnostic.
///
/// The four cases it distinguishes:
///
/// - Not on the `PATH` -- a failure, and the whole diagnosis.
/// - Present, with no version to ask for. The directory alone is
///   the useful fact: it is what makes a hijacked binary
///   visible, since `tool::resolve` never searches the working
///   directory but the operator still wants to see where it did
///   look.
/// - Present but the version call would not start. Still a
///   failure, and a different one from absent.
/// - Present, started, and answered unhelpfully -- a *pass*.
///   Telling the operator to install something they already have
///   would be worse than a missing version string.
#[must_use]
pub fn local_tool_finding(
    name: &str,
    resolved: Option<&Path>,
    version: &VersionAnswer,
) -> Finding {
    let Some(path) = resolved else {
        return Finding::new(
            Scope::Local,
            name,
            Outcome::Fail(not_on_path(name)),
        );
    };
    let where_from = path
        .parent()
        .map_or_else(String::new, |p| p.display().to_string());
    let outcome = match version {
        VersionAnswer::NotAsked => Outcome::Pass(where_from),
        VersionAnswer::Answered(result) => {
            Outcome::Pass(format!("{} in {where_from}", tool_banner(result)))
        }
        VersionAnswer::WouldNotStart(err) => {
            Outcome::Fail(cannot_run(&path.display().to_string(), err))
        }
    };
    Finding::new(Scope::Local, name, outcome)
}

/// Reports whether the project still carries a Vagrantfile.
///
/// This probe used to check the opposite. It failed when the
/// project had no Vagrantfile, because bombyx pushed that file
/// and could not boot without it.
///
/// bombyx now generates the Vagrantfile and writes it on the VM
/// host after the push, so an absent one is the ordinary case.
/// A present one is what deserves reporting: it is still
/// archived and shipped, and then overwritten, so an operator
/// editing it sees no effect on the VM and nothing explains
/// why. Reporting it is the cheapest place to say so.
///
/// Three states, one of which still fails. A `vagrant_dir` that
/// names no directory is a typo, and this is the cheapest place
/// to catch it -- otherwise it surfaces as a `tar` failure after
/// the remote directory has been created. The other two both
/// pass: [`Outcome`] has no warning variant, and inventing a
/// failure for a working configuration would be worse than a
/// detail line.
#[must_use]
pub fn vagrantfile_finding(local_dir: &Path) -> Finding {
    let outcome = if !local_dir.is_dir() {
        // The check that survived the inversion. A typo in
        // `vagrant_dir` used to show up as a missing Vagrantfile;
        // now that an absent one is ordinary, the directory
        // itself is what is left to notice, and noticing it here
        // is far cheaper than a `tar` failure after bombyx has
        // created the remote directory.
        // The action first, the path second. The report
        // truncates this column, and a long temp path pushed
        // the only actionable words off the end.
        Outcome::Fail(format!(
            "check `vagrant_dir` -- no directory at {}",
            local_dir.display()
        ))
    } else if local_dir.join("Vagrantfile").is_file() {
        Outcome::Pass(
            "ignored -- bombyx generates its own; this copy is \
             pushed but then replaced on the host, so edits to \
             it have no effect"
                .to_owned(),
        )
    } else {
        Outcome::Pass("generated by bombyx".to_owned())
    };
    Finding::new(Scope::Local, "Vagrantfile", outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(success: bool, stdout: &str, stderr: &str) -> ProbeResult {
        ProbeResult {
            success,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn tool_banner_keeps_the_flavour_and_never_lies() {
        assert_eq!(
            tool_banner(&ran(true, "bsdtar 3.8.4 - libarchive\n", "")),
            "bsdtar 3.8.4"
        );
        // GNU tar's second word is "(GNU", so the version has
        // to be found rather than counted to.
        assert_eq!(
            tool_banner(&ran(true, "tar (GNU tar) 1.35\n", "")),
            "tar 1.35"
        );
        // `ssh -V` prints to stderr and puts a comma after the
        // version.
        assert_eq!(
            tool_banner(&ran(
                true,
                "",
                "OpenSSH_for_Windows_9.5p1, LibreSSL 3.8.2\n"
            )),
            "OpenSSH_for_Windows_9.5p1 3.8.2"
        );
        // A version-less banner still names the tool.
        assert_eq!(tool_banner(&ran(true, "busybox\n", "")), "busybox");
        // Silence must not render as an identified flavour.
        assert_eq!(tool_banner(&ran(true, "", "")), "version unknown");
    }

    #[test]
    fn a_local_tool_absent_from_the_path_is_the_whole_diagnosis() {
        let f = local_tool_finding("tar", None, &VersionAnswer::NotAsked);
        assert_eq!(f.scope, Scope::Local);
        assert_eq!(
            f.outcome,
            Outcome::Fail("tar not found on PATH".to_owned())
        );
    }

    #[test]
    fn a_local_tool_with_no_version_flag_reports_where_it_came_from() {
        // `scp` answers no version flag, so the directory is the
        // useful fact -- it is what makes a hijacked binary
        // visible.
        let f = local_tool_finding(
            "scp",
            Some(Path::new("/usr/bin/scp")),
            &VersionAnswer::NotAsked,
        );
        assert_eq!(f.outcome, Outcome::Pass("/usr/bin".to_owned()));
    }

    #[test]
    fn a_local_tool_reports_its_flavour_and_directory() {
        let f = local_tool_finding(
            "tar",
            Some(Path::new("/usr/bin/tar")),
            &VersionAnswer::Answered(ran(true, "tar (GNU tar) 1.35\n", "")),
        );
        assert_eq!(f.outcome, Outcome::Pass("tar 1.35 in /usr/bin".to_owned()));
    }

    #[test]
    fn a_local_tool_that_answers_unhelpfully_still_passes() {
        // Present but uncooperative is not absent. Telling the
        // operator to install a tool they already have would be
        // worse than a missing version string.
        let f = local_tool_finding(
            "tar",
            Some(Path::new("/usr/bin/tar")),
            &VersionAnswer::Answered(ran(false, "", "")),
        );
        assert_eq!(
            f.outcome,
            Outcome::Pass("version unknown in /usr/bin".to_owned())
        );
    }

    #[test]
    fn a_local_tool_that_will_not_start_is_a_different_failure() {
        let f = local_tool_finding(
            "tar",
            Some(Path::new("/usr/bin/tar")),
            &VersionAnswer::WouldNotStart("Permission denied".to_owned()),
        );
        assert_eq!(
            f.outcome,
            Outcome::Fail(
                "cannot run /usr/bin/tar: Permission denied".to_owned()
            )
        );
    }

    #[test]
    fn vagrantfile_finding_reports_a_project_file_as_ignored() {
        // The meaning inverted when bombyx started generating
        // this file. An absent one is now the ordinary case,
        // and a present one is the surprise worth reporting:
        // it is still pushed to the host and then overwritten,
        // so an operator editing it would see no effect and
        // have nothing telling them why.
        //
        // Both are passes because neither blocks anything, and
        // `Outcome` has no warning variant. The detail carries
        // the news.
        let dir = tempfile::tempdir().unwrap();
        assert!(
            matches!(
                &vagrantfile_finding(dir.path()).outcome,
                Outcome::Pass(d) if d.contains("generated")
            ),
            "{:?}",
            vagrantfile_finding(dir.path()).outcome
        );

        std::fs::write(dir.path().join("Vagrantfile"), "x").unwrap();
        let shadowed = vagrantfile_finding(dir.path()).outcome;
        assert!(
            matches!(&shadowed, Outcome::Pass(d) if d.contains("ignored")),
            "{shadowed:?}"
        );
    }
}
