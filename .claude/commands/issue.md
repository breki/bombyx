---
description: Work a GitHub issue end to end -- verify it is still real, settle the approach, implement with TDD, review, and open a PR that says what was not verified
argument-hint: "<issue number>"
allowed-tools: Bash(gh issue:*), Bash(gh pr:*), Bash(gh run:*), Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git show:*), Bash(git branch:*), Bash(git checkout:*), Bash(git fetch:*), Bash(git pull:*), Bash(git push:*), Bash(cargo xtask*), Read, Write, Edit, Glob, Grep, Agent, AskUserQuestion, Skill(commit), Skill(review)
---

Work the GitHub issue given as the argument, from reading it to
reporting back on it.

This command does not review and does not commit. `/review`
owns the rounds and `/commit` owns the save-point, and
`CLAUDE.md` under **Reviewing is its own process** says why the
two are separate here.

## Steps

**Order.** Step 8 describes how the review rounds fit around the
commits, and it runs after step 9 has made one. The sequence is
1-7, then 9, 10, then 8 and its rounds, then 11 and 12. It is
numbered where it is read, not where it runs.

### 1. Read the issue, then verify it against the tree

`gh issue view <n> --comments`.

**Do not trust the issue body.** Somebody wrote it at one point
in time and the tree has moved since. Check every factual claim
before planning any work:

- Does the code it describes still look like that? Open the
  file at the line it names.
- Has the work already landed? `git log --oneline -20`, and
  grep for the symbol or the string the issue says is missing.
- Are the numbers still current? Counts, versions and file
  sizes all decay. Re-measure with the command that produces
  them.

The bombyx backlog issues cite a slug in `docs/todo.md` and
often a planning document under `docs/issues/`. Read both. The
issue body is a summary of them and the summary is what goes
stale.

If the issue is already satisfied, close it with the evidence
-- file, line, commit -- and say so. Do not perform the work
again. If it is only partly satisfied, narrow it: edit the
title and body down to what remains, comment saying what you
narrowed and why, and work the remainder.

### 2. Settle the approach before writing code

If the issue names more than one viable route, or you can see
one it does not name, use `AskUserQuestion`. `CLAUDE.md` under
**Collaboration** asks for the concrete artifacts first and a
recommendation among the options, not an even menu.

Read enough of the code to make the question concrete. A
question asked from the issue body alone offers the wrong
options.

Record the decision. When a planning document under
`docs/issues/` owns the item, the decision belongs in that
document; otherwise comment it on the issue. A decision that
lives only in this chat is lost.

### 3. Branch

`git checkout -b <type>/<short-name>` from an up-to-date
`main`. Never work an issue directly on `main`.

### 4. Implement, test first

Red then green, per `CLAUDE.md` under **Test-Driven
Development**. That file also carves out structural additions
and tells you to default to the behaviour-change discipline
when the case is unclear. Read it rather than deciding from
memory which half you are in.

**A regression test must be proven to fail.** Write it, then
break the guard it covers and confirm `cargo xtask test` goes
red, then restore. A test that passes whether or not the
behaviour is present is worse than no test: it reports that
something is checked while nobody is checking it.

**Scope discipline.** Fix what the issue describes. When you
find an adjacent problem, capture it with `/todo` rather than
folding it in, unless leaving it would make your own change
wrong.

### 5. Keep canon in step

`CLAUDE.md` under **After removing a capability, re-grep for
it** covers the prose the compiler cannot find. The places that
describe how bombyx is run:

- `CLAUDE.md`
- `.claude/commands/*.md`, both the prose and the
  `allowed-tools` frontmatter, which is executable rather than
  documentation
- `.claude/agents/*.md`
- `.github/workflows/ci.yml`
- clap `///` help in `crates/bombyx/src`

Grep for the old form across the repo before you finish. Leave
the historical records alone: `docs/developer/DIARY.md`, the
dated sections of `CHANGELOG.md` and past finding entries
record what was true then.

### 6. Verify, and know what you did not verify

Run `cargo xtask validate`. It runs all ten gates, and
`CLAUDE.md` under **Definition of Done** lists them in run
order.

**Definition of Done item 3 is not something `validate` can
do.** Anything that changes the commands bombyx emits needs a
real run against the VM host, because `--dry-run` proves the
argv and nothing about whether the remote side accepts it. When
the host is unreachable, say that the claim rests on a dry run.

Then write down what you could not exercise. Another platform,
a provider nobody has a host for, a config you did not break.
An unverified fix is not a failure; presenting it as verified
is.

### 7. Self-review the diff

Read your own diff before it becomes a commit. This is
Definition of Done item 4 and no reviewer replaces it.

### 8. The review rounds

`/review` owns the loop, the snapshot, the reviewers and the
stopping rule, and `.claude/commands/code-reviewers.md` owns
which reviewers run and what each is handed. Do not restate
either here. A second copy drifts, and the parts most expensive
to lose are the ones a summary drops first.

What is specific to working an issue:

**The rounds run after step 9's commit**, and again after the
fix commits from a round land. Step 9 owns committing; this
step owns what happens around it.

**Reviewers read a snapshot of the working tree, and their
fixes stay separate from the change they correct.** Commit a
round's fixes on their own, so a bad fix reverts without taking
the original work with it.

**A fix is new code and nobody has reviewed it.** Review round
one's fixes. This is the step most easily skipped and the one
that pays.

**Verify a finding's premise before acting on it.** The
findings are usually right; the reasoning and the proposed
remedy are not always. `CLAUDE.md` under **Print the variable
before claiming what it holds** binds a review suggestion the
same as anything else.

**Stop when we would not fix anything a round found.** That
rule and the three-round cap are `/review` under **Stop, or go
again**. Capture the substantive findings you are not fixing
with `/todo`, and link them from the PR.

**Three failures in one place is a design signal, not a fourth
patch.** When the same mechanism is wrong in consecutive
rounds, replace the mechanism instead of patching the next
case.

### 9. Commit

Commit through `/commit`. It stages explicitly, writes the
diary entry and the `[Unreleased]` bullet, and adds the
`AI-Generated:` footer. It bumps no version and cuts no tag.
Then push and open the PR, which is step 10.

State in the commit body what was verified and what was not.

`/commit` writes the diary entry, and this is the point in the
work where it should be written: the decisions have settled. An
entry written earlier records decisions that later reverse.

### 10. Keep the PR open from the first push

Open the PR as soon as the first commit is pushed, with
`gh pr create` and `Closes #<n>`. Do not hold it back until the
review rounds finish. CI starts on the first push rather than
at the end, review comments attach to the commit that caused
them, and the work is visible while it is still moving.

Then update the title and the body as the branch grows: after
each review round, and whenever the scope changes. A title that
still describes the first commit misdescribes the branch.

The body carries what a reviewer cannot get from the diff:

- **Why this approach**, and what was rejected
- **What changed**, as a table when the change is structural
- **Deliberate regressions**, named -- anything that got
  narrower, and where that is recorded
- **Verification**: the commands run and their results, and
  whether the VM host was involved
- **Not done**: what is unexercised, unreviewed or deferred
- **Review rounds**: which round you stopped after, and the
  findings carried forward

### 11. Watch CI, and separate new failures from old

`gh run list --branch <branch>` then `gh run watch <id>`.

A red step is not automatically yours. Compare against `main`:
when it was already failing there, say so plainly and point at
the item that owns it, rather than fixing it silently or
letting it read as a regression.

### 12. Report back on the issue

Comment on the issue with the outcome: what was fixed, the
commit, the verification, and what remains unverified. Close it
only once the work is merged, or when step 1 established that
it was already satisfied.

## Rules

- Verify before working. The issue is a claim, not a fact.
- Never work an issue on `main`.
- No behaviour change without a failing test first.
- A regression test nobody has seen fail does not count.
- Re-review round one's fixes; `/review` bounds the rest.
- Never report a gate as passing on the strength of a
  document. Run it.
- Say what you did not verify.
