# discard-leaves-dir

**Status:** Done
**Captured:** 2026-08-10
**Completed:** 2026-08-10

Shipped together with `destroy-project-vm`, because the two
share one decision -- whether tearing down a VM also removes
its directory on the host -- and answering it differently for
the two commands would have been worse than either answer.

See **[destroy-project-vm.md](destroy-project-vm.md)** for the
problem statement, the decisions and the outcome.

In short: `discard` now removes the scratch directory after
`vagrant destroy -f` succeeds, via the shared `tear_down`
helper in `crates/bombyx/src/plan.rs`. That makes the
`README.md` claim that nothing survives a scratch VM true,
where before the directory and its pushed `Vagrantfile`
remained -- one per discarded VM.
