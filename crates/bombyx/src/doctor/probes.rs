//! The probe list, and reading one probe's result.
//!
//! What `doctor` asks the VM host, in what order, and how a
//! reply becomes an [`Outcome`]. The scripts themselves live in
//! [`crate::remote::probe`]; this module decides which of them
//! run and what their output means.

use super::text::{fail_reason, first_line, sanitize};
use super::{Finding, Outcome, ProbeResult, Scope};
use crate::config::{Config, Provider, Transport};
use crate::remote::{self, RemoteCommand};

/// A check applied to a probe's stdout when a zero exit is not
/// the whole answer.
///
/// `Ok` means the precondition holds; `Err` carries the reason
/// it does not.
pub type Verdict = fn(&str) -> Result<(), String>;

/// A probe to run on the VM host.
#[derive(Debug, Clone)]
pub struct HostProbe {
    /// Label it reports under.
    pub name: &'static str,
    /// The command to run.
    pub command: RemoteCommand,
    /// When this probe fails, the rest are skipped.
    ///
    /// A flag rather than matching on `name`, so renaming a
    /// report column cannot silently disable the cascade.
    pub gates_the_rest: bool,
    /// Applies a verdict the shell could not express.
    ///
    /// `None` means a zero exit is the whole verdict.
    pub verdict: Option<Verdict>,
}

impl HostProbe {
    /// A probe whose exit status is the whole verdict.
    ///
    /// Every probe starts here. The two that need more chain
    /// `.gating()` or `.with_verdict()` on at their definition,
    /// where a reader scanning the list can see it -- there is
    /// no second constructor to look for.
    #[must_use]
    pub fn plain(name: &'static str, command: RemoteCommand) -> Self {
        Self {
            name,
            command,
            gates_the_rest: false,
            verdict: None,
        }
    }

    /// Marks this probe as the one whose failure skips the rest.
    #[must_use]
    pub fn gating(mut self) -> Self {
        self.gates_the_rest = true;
        self
    }

    /// Attaches a check the shell could not express.
    #[must_use]
    pub fn with_verdict(mut self, verdict: Verdict) -> Self {
        self.verdict = Some(verdict);
        self
    }
}

/// The host probes, in order.
///
/// Reachability is first and gates the rest: the probes behind
/// it would each wait on a dead host and teach nothing the first
/// failure did not.
#[must_use]
pub fn host_probes(cfg: &Config) -> Vec<HostProbe> {
    let mut probes = vec![];

    // Running here there is no host to reach, and `sh -c true`
    // would pass unconditionally while reporting nothing.
    // `reachability_finding` supplies the row instead.
    if cfg.transport == Transport::Ssh {
        probes.push(
            HostProbe::plain("ssh", remote::probe::reachable(cfg)).gating(),
        );
    }

    probes.extend([
        HostProbe::plain("login shell", remote::probe::posix_shell(cfg))
            .with_verdict(posix_shell_verdict),
        HostProbe::plain("vagrant", remote::probe::command(cfg, "vagrant")),
        HostProbe::plain(
            "project dir",
            remote::probe::dir_writable(cfg, &cfg.remote_project_dir()),
        ),
    ]);

    // The provider probe greps `vagrant plugin list` for
    // `vagrant-libvirt`, which only answers a question a libvirt
    // project is asking. Hyper-V ships inside Vagrant and has no
    // plugin to find, so sending the probe there reports a
    // failure about a plugin that project never needed.
    //
    // A non-libvirt project gets `provider_finding` instead,
    // which is a skip row rather than an absent one.
    match cfg.vm.provider {
        Provider::Libvirt => probes.push(HostProbe::plain(
            "libvirt provider",
            remote::probe::provider(cfg),
        )),
        Provider::Hyperv => {}
    }
    probes
}

/// The reachability row for a run that has nothing to reach.
///
/// Returns `None` over `ssh`, whose probe is in the list above.
/// Running here it returns a skip, because the row must appear
/// either way: an absent row reads as a check that passed.
pub(crate) fn reachability_finding(cfg: &Config) -> Option<Finding> {
    (cfg.transport == Transport::Local).then(|| {
        Finding::new(
            Scope::Host,
            "ssh",
            Outcome::Skip("not used; the host is this machine".to_owned()),
        )
    })
}

/// The provider row for a project that `host_probes` cannot
/// check.
///
/// This returns `None` for libvirt, whose probe is in the list
/// above. For any other provider it returns an
/// [`Outcome::Skip`], because leaving the row out would shrink
/// the report, and an absent row reads as a check that passed.
///
/// bombyx has never driven a Hyper-V host, so there is no probe
/// here to write honestly -- see `Provider::Hyperv`. Saying so
/// in the report is the whole point: the operator learns that
/// this part of their configuration is unverified rather than
/// approved.
#[must_use]
pub(crate) fn provider_finding(cfg: &Config) -> Option<Finding> {
    // Every variant named, so adding a provider is a compile
    // error here rather than a row that silently stops printing.
    match cfg.vm.provider {
        Provider::Libvirt => None,
        p @ Provider::Hyperv => Some(Finding::new(
            Scope::Host,
            "provider",
            Outcome::Skip(format!("not checked for {p}")),
        )),
    }
}

/// The commands `probes` would run, in order.
///
/// The single renderer behind `--dry-run`, used by both `plan`
/// and the binary, so no path can advertise a probe list the
/// live run would not use.
#[must_use]
pub fn probe_commands(probes: &[HostProbe]) -> Vec<RemoteCommand> {
    probes.iter().map(|p| p.command.clone()).collect()
}

/// Every host finding for `cfg`, in report order.
///
/// This runs the probes and appends the rows no probe can
/// produce. Callers get
/// this rather than composing the pieces themselves:
/// `run_probes` and `provider_finding` are `pub(crate)`, so
/// this is the only composition a caller outside the crate can
/// reach. A report with no provider row is the state
/// `provider_finding` exists to prevent, and the visibility is
/// what stops a caller reaching it.
///
/// `run` carries out one probe. It is a parameter so this stays
/// free of process spawning; the binary passes the real one and
/// tests pass a canned verdict.
pub fn host_findings<F>(cfg: &Config, run: F) -> Vec<Finding>
where
    F: FnMut(&HostProbe) -> Outcome,
{
    // The reachability row comes first whichever way it was
    // produced, so the report reads the same on both routes.
    let mut findings: Vec<Finding> =
        reachability_finding(cfg).into_iter().collect();
    findings.extend(run_probes(&host_probes(cfg), run));
    findings.extend(provider_finding(cfg));
    findings
}

/// Confirms the host shell ran a POSIX construct correctly.
///
/// # Errors
///
/// Returns the reason when the expected token is absent.
fn posix_shell_verdict(stdout: &str) -> Result<(), String> {
    if stdout.lines().any(|l| l.trim() == "posix") {
        return Ok(());
    }
    Err("shell did not run a POSIX construct; bombyx sends \
         POSIX sh scripts"
        .to_owned())
}

/// Reads a probe's result into an outcome.
///
/// `verdict` applies a check the shell could not express, and
/// runs only once the command itself succeeded.
#[must_use]
pub fn classify(result: &ProbeResult, verdict: Option<Verdict>) -> Outcome {
    if !result.success {
        return Outcome::Fail(fail_reason(&result.stdout, &result.stderr));
    }
    if let Some(check) = verdict
        && let Err(reason) = check(&result.stdout)
    {
        return Outcome::Fail(sanitize(&reason));
    }
    Outcome::Pass(first_line(&result.stdout))
}

/// Runs `probes` in order, skipping the rest once a gating
/// probe fails.
///
/// `run` performs one probe. Keeping it a parameter is what
/// makes the cascade testable without a host: the tests pass a
/// closure returning canned outcomes.
///
/// A skipped probe names the gate that stopped it, read from the
/// gate itself. Hardcoding the reason meant renaming the gating
/// probe left the report explaining the skip in terms of a
/// column that no longer existed.
pub(crate) fn run_probes<F>(probes: &[HostProbe], mut run: F) -> Vec<Finding>
where
    F: FnMut(&HostProbe) -> Outcome,
{
    let mut blocked_by: Option<&str> = None;
    let mut findings = Vec::with_capacity(probes.len());
    for probe in probes {
        if let Some(gate) = blocked_by {
            findings.push(Finding::new(
                Scope::Host,
                probe.name,
                Outcome::Skip(format!("no {gate}")),
            ));
            continue;
        }
        let outcome = run(probe);
        if probe.gates_the_rest && matches!(outcome, Outcome::Fail(_)) {
            blocked_by = Some(probe.name);
        }
        findings.push(Finding::new(Scope::Host, probe.name, outcome));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::for_tests()
    }

    /// The shared config, with `provider` replaced.
    fn cfg_with(provider: Provider) -> Config {
        let mut c = cfg();
        c.vm.provider = provider;
        c
    }

    /// The shared config, running on the machine it names.
    fn local_cfg() -> Config {
        Config {
            transport: Transport::Local,
            ..cfg()
        }
    }

    #[test]
    fn the_reachability_row_is_a_skip_when_there_is_nothing_to_reach() {
        // `sh -c true` would pass every time and say nothing.
        // The row still has to appear: an absent one reads as a
        // check that passed, which is the same trap
        // `provider_finding` exists to close.
        let findings =
            host_findings(&local_cfg(), |_| Outcome::Pass(String::new()));
        let ssh = findings.first().expect("a first row");
        assert_eq!(ssh.name, "ssh");
        let Outcome::Skip(reason) = &ssh.outcome else {
            panic!("expected a skip, got {:?}", ssh.outcome);
        };
        assert!(reason.contains("this machine"), "{reason}");
    }

    #[test]
    fn the_local_probes_all_run_here() {
        for p in host_probes(&local_cfg()) {
            assert_eq!(p.command.program, "sh", "{}", p.name);
        }
    }

    #[test]
    fn host_findings_carries_the_provider_row_for_every_provider() {
        // The row is the point: a caller that assembled the
        // report by hand could leave it out, and an absent row
        // reads as a check that passed.
        let has = |c: &Config, row: &str| {
            host_findings(c, |_| Outcome::Pass(String::new()))
                .iter()
                .any(|f| f.name == row)
        };
        assert!(has(&cfg_with(Provider::Libvirt), "libvirt provider"));
        assert!(has(&cfg_with(Provider::Hyperv), "provider"));
    }

    #[test]
    fn a_non_libvirt_provider_gets_a_skip_row_not_silence() {
        // Dropping the row would shrink the report, and a reader
        // compares what they see against the documented output.
        // An absent row reads as "checked and fine"; a skip says
        // bombyx did not look.
        assert_eq!(provider_finding(&cfg_with(Provider::Libvirt)), None);

        let f = provider_finding(&cfg_with(Provider::Hyperv))
            .expect("a non-libvirt provider must still get a row");
        assert_eq!(f.scope, Scope::Host);
        match &f.outcome {
            Outcome::Skip(why) => assert!(why.contains("hyperv"), "{why}"),
            other => panic!("must not pass or fail: {other:?}"),
        }
    }

    #[test]
    fn the_libvirt_probe_is_only_sent_for_a_libvirt_project() {
        // The probe greps `vagrant plugin list` for
        // `vagrant-libvirt`. Hyper-V is built into Vagrant and
        // has no such plugin, so on a Hyper-V project the row
        // fails on a host where every VM command works -- and a
        // preflight whose red rows do not predict `up` is one
        // operators learn to ignore.
        let names = |c: &Config| {
            host_probes(c).iter().map(|p| p.name).collect::<Vec<_>>()
        };
        let row = "libvirt provider";
        assert!(names(&cfg_with(Provider::Libvirt)).contains(&row));
        assert!(!names(&cfg_with(Provider::Hyperv)).contains(&row));
    }

    fn ran(success: bool, stdout: &str, stderr: &str) -> ProbeResult {
        ProbeResult {
            success,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn classify_reads_a_zero_exit_as_a_pass_with_detail() {
        assert_eq!(
            classify(&ran(true, "/usr/bin/vagrant\n", ""), None),
            Outcome::Pass("/usr/bin/vagrant".to_owned())
        );
    }

    #[test]
    fn classify_passes_with_no_detail_when_output_is_empty() {
        assert_eq!(
            classify(&ran(true, "  \n\n", ""), None),
            Outcome::Pass(String::new())
        );
    }

    #[test]
    fn classify_prefers_the_last_stderr_line() {
        // OpenSSH prints the server Banner and host-key notices
        // before the real error, so the first line is the least
        // useful one.
        let stderr = "*** AUTHORIZED USE ONLY ***\n\
                      Warning: Permanently added 'h'\n\
                      Permission denied (publickey).\n";
        assert_eq!(
            classify(&ran(false, "", stderr), None),
            Outcome::Fail("Permission denied (publickey).".to_owned())
        );
    }

    #[test]
    fn classify_sanitizes_the_detail_it_stores() {
        // Two modules, two guarantees, so two tests. The
        // rendered line is `report`'s guarantee; what `classify`
        // puts in the `Outcome` is this module's. A host
        // emitting cursor escapes could otherwise have them
        // travel in a `Finding` to any consumer that reads the
        // outcome programmatically rather than rendering it.
        let evil = "\x1b[2A\x1b[Kspoofed";
        assert_eq!(
            classify(&ran(true, evil, ""), None),
            Outcome::Pass("?[2A?[Kspoofed".to_owned())
        );
    }

    #[test]
    fn classify_falls_back_when_a_failure_is_silent() {
        assert_eq!(
            classify(&ran(false, "", ""), None),
            Outcome::Fail("not found".to_owned())
        );
    }

    #[test]
    fn classify_applies_a_verdict_a_zero_exit_cannot() {
        // The whole point: `echo` succeeds whatever it prints,
        // so the exit code alone would pass a fish shell.
        let out = classify(
            &ran(true, "/usr/bin/fish\n", ""),
            Some(posix_shell_verdict),
        );
        assert!(matches!(out, Outcome::Fail(_)), "{out:?}");
        assert_eq!(
            classify(&ran(true, "posix\n", ""), Some(posix_shell_verdict)),
            Outcome::Pass("posix".to_owned())
        );
    }

    #[test]
    fn posix_shell_verdict_rejects_silence() {
        // An unset variable or a shell that printed nothing must
        // not pass.
        assert!(posix_shell_verdict("").is_err());
        assert!(posix_shell_verdict("\n\n").is_err());
        assert!(posix_shell_verdict("posix").is_ok());
    }

    #[test]
    fn run_probes_skips_the_rest_once_the_gate_fails() {
        let probes = host_probes(&cfg());
        let findings = run_probes(&probes, |p| {
            if p.gates_the_rest {
                Outcome::Fail("unreachable".to_owned())
            } else {
                panic!("must not run {} after the gate failed", p.name);
            }
        });
        assert_eq!(findings.len(), probes.len());
        assert!(matches!(findings[0].outcome, Outcome::Fail(_)));
        assert!(
            findings[1..]
                .iter()
                .all(|f| matches!(f.outcome, Outcome::Skip(_))),
            "{findings:?}"
        );
    }

    #[test]
    fn a_skip_names_the_gate_that_caused_it() {
        // Read from the gating probe, so renaming a report
        // column cannot leave the report explaining the skip in
        // terms of a column that no longer exists.
        let probes = vec![
            HostProbe::plain("uplink", remote::probe::reachable(&cfg()))
                .gating(),
            HostProbe::plain("tar", remote::probe::command(&cfg(), "tar")),
        ];
        let findings =
            run_probes(&probes, |_| Outcome::Fail("unreachable".to_owned()));
        assert_eq!(findings[1].outcome, Outcome::Skip("no uplink".to_owned()));
    }

    #[test]
    fn run_probes_runs_everything_when_the_gate_passes() {
        let probes = host_probes(&cfg());
        let mut ran = 0;
        let findings = run_probes(&probes, |_| {
            ran += 1;
            Outcome::Pass(String::new())
        });
        assert_eq!(ran, probes.len());
        assert!(
            findings
                .iter()
                .all(|f| matches!(f.outcome, Outcome::Pass(_)))
        );
    }

    #[test]
    fn exactly_one_probe_gates_the_rest_and_it_is_first() {
        let probes = host_probes(&cfg());
        assert!(probes[0].gates_the_rest, "reachability must gate");
        assert_eq!(probes[0].name, "ssh");
        assert_eq!(
            probes.iter().filter(|p| p.gates_the_rest).count(),
            1,
            "a second gate would skip probes that could still run"
        );
    }

    #[test]
    fn probe_labels_are_unique_and_non_empty() {
        // Labels key the report columns.
        let probes = host_probes(&cfg());
        let mut names: Vec<&str> = probes.iter().map(|p| p.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        assert!(names.iter().all(|n| !n.is_empty()));
    }

    #[test]
    fn no_probe_changes_the_host() {
        // Asserted here rather than in `readonly`, which owns the
        // judge: this is a property of the probe *list*, so a
        // change to the list should fail a test in the file that
        // defines it. `readonly` then depends on nothing at all,
        // in production or test code.
        for p in host_probes(&cfg()) {
            let script = p.command.args.last().unwrap();
            assert_eq!(
                super::super::mutating_token(script),
                None,
                "{script:?}"
            );
        }
    }

    #[test]
    fn probe_commands_preserves_the_probe_order() {
        let probes = host_probes(&cfg());
        let cmds = probe_commands(&probes);
        assert_eq!(cmds.len(), probes.len());
        for (cmd, probe) in cmds.iter().zip(probes.iter()) {
            assert_eq!(*cmd, probe.command);
        }
    }
}
