# Supply-chain hygiene

**Consult this file; do not memorise it.** Read it when adding
or bumping a dependency, when a `validate` supply-chain gate
fails, or when cutting a release. `CLAUDE.md` states the rules
that apply on every commit, including the 14-day cooldown; this
file is the how and the why behind them.

Six `cargo xtask` commands guard the dependency tree. Two of them
are about **licences** rather than vulnerabilities, and the split
matters because the two hazards behave differently:

- **`cargo xtask deny`** runs `cargo deny check licenses bans
  sources` against `deny.toml`. It is **offline** -- it reads
  `Cargo.lock` and the metadata already on disk -- which is why it
  runs as `validate` step 4 *and* on every push in CI, where
  `audit` deliberately does not. An advisory can appear overnight
  and fail a pull request that changed nothing; a licence cannot
  change under you that way. A missing `cargo-deny` is an error
  here, not a warning, because there is no network to be down.

  The allow-list is every licence in the current tree and no
  more: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception,
  Unicode-3.0, Unlicense. `LGPL-2.1-or-later` is deliberately
  absent even though `r-efi` offers it, because an SPDX `OR` is
  satisfied by any allowed member -- so that crate resolves to MIT
  and a crate that is *only* copyleft fails the gate.

- **`cargo xtask licenses`** generates `THIRD-PARTY-LICENSES` for
  one target, and the release workflow writes it into every
  archive. This is compliance, not tidiness: MIT and Apache-2.0
  both require the licence and notice to travel with a distributed
  binary, and `serde`, `clap`, `anyhow` and the `windows-sys` tree
  are all in the shipped binary under one or the other. Until this
  existed the archives held only bombyx's own `LICENSE`, so the
  obligation was unmet from the first published binary. Texts come
  from the registry sources already on disk; nothing is
  downloaded.

  **The set is what goes into building the binary for one
  target**, which needs three restrictions: crates reachable from
  a *distributed* workspace member (so not `xtask`'s tree),
  through *normal* dependencies (so not `assert_cmd`,
  `predicates`, `difflib`), resolved for the *one* platform named
  by `--target` (so not `r-efi`). Those three cut the walk from
  87 crates to 50 on `x86_64-pc-windows-msvc`. Pass `--target` from
  the release matrix, or the host triple is used -- and it fails
  rather than guessing one, because a guessed triple resolves
  another platform's set and still exits 0.

  **It says "goes into building", not "links", and that wording is
  deliberate.** Within those three restrictions the set is
  deliberately over-inclusive: proc-macro crates run at compile
  time and are not in the binary (8 of the 50, including the
  `unicode-ident` whose `Unicode-3.0` is the obvious thing to
  cite as the reason this file exists -- it reaches bombyx only
  through `clap_derive`, `serde_derive` and `thiserror-impl`), and
  `resolve.nodes[].deps` reports an optional dependency the build
  never enables with the same `kind: null` as a real edge
  (`cargo tree -e normal` says 47 where the walk says 50). Pruning
  either means reimplementing feature resolution, which fails
  quietly and in the direction that matters. An unnecessary
  attribution costs nothing; a false sentence in a legal document
  does, so the sentence is what gets kept true.

  **A crate shipping no licence terms fails the command**, with
  `--max-missing N` to raise the bar deliberately. Naming them in
  the file was not enough: if the registry sources are absent every
  crate comes back text-less, and the tool would write a short file
  announcing that none of them ship a licence and exit 0. "Terms"
  is narrower than what the tool *collects*: `NOTICE`, `AUTHORS`
  and `COPYRIGHT` are gathered because they carry obligations of
  their own, but a crate shipping only an `AUTHORS` list has given
  us nothing to reproduce, so it does not satisfy the gate. Nor
  does an empty `LICENSE`. The generator runs in every-push CI as
  well as the release, because a gate that first fires after the
  tag exists costs a moved tag.

  It is not committed (`.gitignore`), because it is derived from
  `Cargo.lock` and would drift the moment a dependency moved.

The other four are about vulnerabilities and freshness:

- **`cargo xtask audit`** runs `cargo audit` (RUSTSEC) over
  `Cargo.lock`, failing on any vulnerability (advisory
  *warnings* -- unsound / unmaintained / yanked -- are
  reported, not fatal). It runs late in `validate`, so
  **`validate` needs `cargo-audit` installed
  (`cargo install cargo-audit`) and network access** to the
  advisory DB.

  **Inside `validate` a missing tool or an unreachable
  advisory DB is a printed warning, not a failure**, so an
  offline machine is not blocked. The consequence is worth
  stating plainly: `Validate OK` does **not** mean the
  dependencies were audited. The standalone
  `cargo xtask audit` errors on both instead, and that is
  the spelling a release uses.

  **Releases audit twice, and neither copy is optional.**
  `/release` runs the standalone command as its own step
  after `validate`, which blocks the tag from being created;
  the `gates` job in `.github/workflows/release.yml` runs it
  as well, which blocks the binaries from being published.
  The second one is the copy nobody can skip. Every-push CI
  still leaves audit out on purpose -- an advisory filed
  overnight would fail a pull request that changed nothing.

  **What this does not cover.** An advisory against a
  dependency you have not touched is caught only at the next
  release, because `dep-age-check` looks at *changed* deps by
  design and nothing else watches. There is no provenance
  vetting (no `cargo-vet`), so nothing distinguishes an audited
  crate from one that merely has no advisory yet, and no
  automated update cadence, so a dependency whose vulnerability
  is already fixed upstream sits at the old version until
  someone runs `/update-deps`.
- **`cargo xtask dep-age cargo <package> [version]`**
  reports how many days ago a version was published (on-demand,
  a single package). Add **`--latest-aged`** to instead print
  the **highest** version that has cleared the cooldown
  (selected by version, not publish date) -- the pin target the
  `/update-deps` workflow feeds to `cargo update --precise`.
  The `cargo` argument names the registry; it is the only one
  supported, and is kept so adding a second later needs no
  change to the command line.
- **`cargo xtask dep-age-check`** enforces the cooldown as the
  **first** `validate` step, so a dependency adopted within the
  cooldown fails the gate before the compile steps (Clippy,
  Test, Coverage) build and run its build script. It checks
  **only the dependencies added or version-bumped in the working
  tree versus `HEAD`**, so it fires exactly when a dependency is
  adopted and costs nothing -- no network -- on a commit that
  leaves `Cargo.lock` untouched. A *whole-tree* gate is
  deliberately avoided: it would flag every already-locked
  version on every routine update. Like `audit`, an
  unreachable registry / missing `HEAD` baseline degrades to a
  warning, not a hard failure.
- **`cargo xtask dep-preflight`** is the *pre-compile* twin of
  `dep-age-check`. Where the gate reports a cooldown breach
  *after* the fact, preflight *remediates* it *before* you
  build: it reads the changed Rust crates (same `HEAD` diff as
  the gate) and, for each one still inside the cooldown, pins
  it down to its newest aged version with
  `cargo update --precise`, looping until the whole changed set
  is aged or no aged version fits the resolved requirements.
  Every step touches only the registry index and the lockfile,
  so no crate tarball is fetched and no build script runs until
  the tree is clean. Use it as a front door: `cargo add <dep>`
  (updates the lockfile, no compile) -> `cargo xtask
  dep-preflight` -> `cargo build`. Rust / crates.io only.

**Why both a gate *and* a preflight?** `dep-age-check` is a
*post-resolution* check -- by the time it fails, `cargo` has
already downloaded *and compiled* the fresh crates, running
their build scripts (`cc`, `ring`, ...) on your machine. The
gate protects the committed lockfile (and everyone who builds
from it); it does not protect the build host during the window.
`dep-preflight` closes that host-side gap, but only when you
run it *instead of* going straight to `cargo build` -- it
cannot intercept a bare `cargo build` that resolves and
compiles in one shot. The only thing that protects *every*
invocation automatically is cargo's in-resolver
**`-Zmin-publish-age`** (RFC 3923), which refuses to *select* a
too-new version so it is never fetched or built. That flag's
client side is nightly-only as of now; once it stabilizes on
stable, layer it in front of (or in place of) these xtask
commands. Until then, `dep-age-check` is the CI-enforced gate
(runs on stable) and `dep-preflight` is the opt-in host-side
hardening.

**Dependency-version cooldown.** Do not adopt a dependency
version published fewer than 14 days ago without a stated
justification -- that window is when a compromised or
malicious release is most likely still live. Security fixes
are exempt (the fix's urgency outweighs the cooldown). Check
a candidate before adding it:
`cargo xtask dep-age cargo <crate> <version>`; it exits
non-zero when the version is within the cooldown.

`validate` enforces this automatically for changed deps via
the `dep-age-check` step above. When you *do* adopt a
fresh version with justification (or a security fix), name it
in the **`RUSTBASE_DEP_AGE_ALLOW`** env var
(`name@version`, comma-separated) so the gate passes while
leaving an auditable record of what was waved through --
e.g. `RUSTBASE_DEP_AGE_ALLOW=serde@1.0.999 cargo xtask
validate`.

**`cargo update` interaction.** The gate checks *every*
newly-locked registry dependency, **transitive ones included**
-- so it is a no-op only on commits that leave `Cargo.lock`
untouched, not on every "routine" commit. A lockfile-churning
update (`cargo update`) can bump many transitive crates to
versions published within the cooldown, and the gate
will fail listing all of them. That is intended -- a bulk
update is exactly when a freshly-published (possibly
compromised) transitive release slips in. The recommended
workflow: run the update, then either wait out the cooldown
before committing, or, once you've reviewed the flagged
versions, bulk-approve them with
`RUSTBASE_DEP_AGE_ALLOW=a@1.2.3,b@4.5.6,... cargo xtask
validate`. Prefer scoped updates (`cargo update -p <crate>`)
over a blanket `cargo update` so the flagged set stays small
and reviewable.
