---
description: Work a GitHub issue end to end -- verify it is still real, settle the approach, implement with TDD, review, and open a PR that says what was not verified
argument-hint: "<issue number>"
allowed-tools: Bash(gh issue:*), Bash(gh pr:*), Bash(gh run:*), Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git show:*), Bash(git branch:*), Bash(git checkout:*), Bash(git fetch:*), Bash(git pull:*), Bash(git push:*), Bash(cargo xtask*), Bash(wc:*), Bash(grep:*), Read, Write, Edit, Glob, Grep, Agent, AskUserQuestion, Skill(commit), Skill(review), Skill(todo)
---

Work the GitHub issue given as the argument, from reading it to
reporting back on it.

Committing happens through `/commit`, and a review is offered
rather than assumed -- step 8 says who decides. This command
reimplements neither, and `CLAUDE.md` under **Reviewing is its
own process** says why they are separate here.

## Steps

**Order.** The steps run in the order they are numbered.

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
  sizes all decay. Re-measure rather than repeating the
  figure: `Grep` counts matches, `Read` gives line numbers,
  and `wc -l` and `grep -c` are granted for the rest.

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

`/review` owns the loop, the snapshot, the reviewers, the
stopping rule and what happens to a finding nobody fixes.
`.claude/commands/code-reviewers.md` owns which reviewers run
and what each is handed. Read those rather than working from a
summary here.

**The operator decides whether a review happens.**
`CLAUDE.md` under **Reviewing is its own process** leaves the
choice to them: offer it, and spawn the reviewers only when
asked.

Three things are specific to working an issue.

**The rounds run before step 9's commit.** `/review` snapshots
`git diff HEAD`, so it wants the work still in the tree.
Committing first hands the reviewers an empty diff, and they
will report a clean sheet on a change nobody read.

**A round after the PR is open reviews the fixes, not the
branch.** When a comment asks for one, make the edits it asks
for and leave them in the tree, then run `/review` -- it still
snapshots `git diff HEAD`, so what it reads is those edits and
nothing else. Commit them on their own rather than amending, so
a bad fix reverts without taking the original work with it.

**The PR body must list the findings nobody fixed.** `/review`
logs the deferred ones under `docs/developer/`, which a reader
of the branch will not think to open. A finding it *declined*
is logged nowhere, and `target/review-<n>.findings` is not
committed, so copy that one into the PR body while the round's
report is still in front of you. Do not send either kind to
`/todo`; that is for a problem you found yourself while
implementing.

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
`gh pr create` and `Closes #<n>`. CI then starts on the first
push rather than at the end, and the work is visible while it
is still moving.

Then update the title and the body as the branch grows:
whenever the scope changes, and after a round that a PR comment
asked for. A title that still describes the first commit
misdescribes the branch.

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
commit, the verification, and what remains unverified.

**The operator merges the PR, so this command ends here.**
Closing the issue waits on that merge, and so does moving its
entry in `docs/todo.md` to `## Done` -- `cargo xtask todo done`
is the command for it and nothing calls it on your behalf. Say
in the report that both are outstanding. The one exception is
an issue step 1 established was already satisfied: close that
one there and then, with the evidence.

## Rules

- Verify before working. The issue is a claim, not a fact.
- Never work an issue on `main`.
- No behaviour change without a failing test first.
- A regression test nobody has seen fail does not count.
- Offer the review before the commit; `/review` owns the
  loop.
- Never report a gate as passing on the strength of a
  document. Run it.
- Say what you did not verify.
