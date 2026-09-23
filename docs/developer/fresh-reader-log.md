# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first. A finding that was fixed leaves
no entry -- the comment it produced is the record.

---

### fr-2026-09-23-firewall-doc-narrates-history

**Category:** history in prose

`docs/vm-host-firewall.md` tells two incidents where it should state
rules: "That has happened here: a host ran a predecessor ruleset for
weeks whose DNS accept was not pinned", and "Earlier versions of this
section tried to give you one sentence...". `docs/todo.md` under
`host-network-isolation` says "since the probe was corrected". A
reader cannot tell whether "predecessor ruleset" means an older script
they might still have loaded. Keep the rule and drop the incident: for
example, `status` does not compare against `show`, so a table from an
older script passes; re-run `apply` after changing the script. Raised
on PR #114; outside that PR's change, so deferred.
