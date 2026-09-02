# Fresh Reader Findings -- Deferred backlog

Comprehension review findings, from the reviewer that reads the
changed files cold. Newest first.

An entry here is a place where the code did not explain itself
and we chose not to fix it yet. A finding that *was* fixed
leaves no entry -- the comment it produced is the record.

---

### fr-2026-09-02-two-more-files-describe-the-push

**Category:** Files no sweep opened

`bombyx.toml.sample` and `llms.txt` both describe the push as current
behaviour, and `README.md` and `docs/vm-host-setup.md` point readers
at them. `llms.txt` is the file whose name promises a machine can
read it first; it says bombyx "pushes a project's `vagrant/`
directory" and that "the host holds a cache refreshed on every `up`".
It also says `validate` has eight steps where `CLAUDE.md` says nine.

### fr-2026-09-02-host-setup-tells-you-to-write-a-vagrantfile

**Category:** Two documents giving opposite instructions

`docs/vm-host-setup.md`'s "Configuration for each project" says every
project needs "a `vagrant/` directory containing a Vagrantfile" and
that bombyx does not ship one "because the project repository is
meant to be the source of truth". `docs/tutorial.md` says the
opposite in as many words: bombyx renders it, a committed one is read
by nothing, delete it. The same page also says bombyx does not
control the synced folder, which the generated Vagrantfile disables
unconditionally, and names "the jutro VM" with no definition.

### fr-2026-09-02-boundary-claim-unqualified-in-five-places

**Category:** One rule, two strengths

`docs/trust-boundary.md` qualifies "the VM host holds no project
code" once -- the guest's disk image is a file on the host -- and
repeats it unqualified at three other points in the same file, plus
`CLAUDE.md`, `.claude/skills/architect/SKILL.md` and
`crates/bombyx/README.md`. A reader meets the strong form first and
may never reach the qualification.

### fr-2026-09-02-main-narrates-its-own-history

**Category:** Voice

Six comments in `main.rs` explain the code by saying what it used to
be: "It used to say 'thin by design'", "the first cut of this fix",
"An earlier version answered a non-zero `curl` with ...". Two of them
describe a `matches!` "four hundred lines away" that no longer
exists, which costs a search of the file. `CLAUDE.md` rules the shape
out by name.

### fr-2026-09-02-architect-skill-calls-main-thin

**Category:** Canon disagreeing with the code it describes

The architect skill calls `main.rs` "(thin)" and says to keep logic
out of it. `main.rs` opens by refusing that description: "It used to
say 'thin by design', and that is worth not claiming", and the
self-update sequence lives there. The rule the skill wants is "keep
new decisions out", not "it is thin".

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
