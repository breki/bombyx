# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first.

An entry here is a place where the code did not explain itself
and we chose not to fix it yet. A finding that *was* fixed
leaves no entry -- the comment it produced is the record.

---

### fr-2026-09-02-which-trust-boundary-lines-change

**Category:** Instruction a reader cannot act on

"`:45` corrected -- it claims that already" does not say what edit
to make, and points at the half of `:45` that becomes true rather
than the half that goes stale. The status banner at `:10-23` and
the argv listing at `:57-61` describe a push chunk 1 deletes, and
the plan does not say whether they are in scope.

### fr-2026-09-02-project-field-unaccounted

**Category:** Gap in the plan

The document opens a ledger of five values read from the
repository. Chunk 1 accounts for `vagrant_dir`, chunk 2 for
`remote_root`, `[vm]` and `[source]`. `project` is never mentioned
again, and it is load-bearing: `remote_project_dir()` builds the
path `destroy` runs `rm -rf` against from it.

### fr-2026-09-02-chunk-two-has-no-caller

**Category:** Gap in the plan

Chunk 2 changes `Config::load` to take a project name, and chunk 3
introduces the `--project` that supplies one. Each chunk is its own
commit, so chunk 2 as described leaves `main.rs` with no name to
pass and no registry path to read.

### fr-2026-09-02-project-flag-clap-shape

**Category:** Unclear specification

"a required global argument for every `VmCmd` variant" does not
describe a shape clap can build. A `global = true` argument cannot
also be required, `VmCmd` is a flattened enum with no shared field,
and the next sentence says `self-update` must work without it.
Three lines of argv and a note on where the check happens would
settle it.

### fr-2026-09-02-chunk-two-test-inventory-missing

**Category:** Asymmetry that misleads

Chunk 1 gets nine tests named by line, all of them accurate. Chunk
2 gets one sentence, which reads as "nothing else breaks". Chunk 2
deletes the overlay, and `integration_test.rs:224` and `:241` are
built on `bombyx.local.toml`; the second becomes a test of nothing
rather than a failure.

### fr-2026-09-02-rules-versus-statements

**Category:** Two names for one thing

The document calls the boundary's two halves "rules" in the Problem
section and "statements" in the Plan. `docs/trust-boundary.md` uses
only "statements".

### fr-2026-09-02-chunk-used-before-defined

**Category:** Term used before introduction

"chunk 1" and "chunk 2" appear in the Problem section; "Three
chunks, in this order. Each is its own commit." is 80 lines later.
The opening paragraph already lists the three items and could say
they are the three chunks. The same document introduces "the
registry" correctly, which is the pattern to copy.

### fr-2026-09-02-three-phrasings

**Category:** Voice

Three stumbles. "Rust unit tests for the config loading and
lookup." is a verbless fragment of the shape `CLAUDE.md` rules out.
"It names the registry file and the keys the entry needs" reads
"It" as the entry, not the error message. "which reads the
filesystem here" uses "here" for "on the workstation", right after
a file:line reference, where it first reads as "at that line".
