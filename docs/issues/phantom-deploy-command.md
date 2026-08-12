# phantom-deploy-command

**Status:** Done
**Captured:** 2026-08-10
**Started:** 2026-08-12
**Completed:** 2026-08-12

## Problem

`CLAUDE.md` and `.claude/commands/release.md` described
`cargo xtask deploy` as the thing that gates shipping -- it
"refuses unless `HEAD` is on a matching annotated tag" -- and
`/commit` named it as what `/release` is a prerequisite for.

No such subcommand exists. It went with the template's deploy
subsystem during the CLI-only prune, and the prose describing it
stayed. `.gitignore` still carried `.deploy` for a
`.deploy.sample` that is likewise gone.

Canon describing a gate that cannot run is worse than either
having the gate or not: a reader trusts it, and the next person
to cut a release waits for a check that will never happen.

## Decisions

- **2026-08-12 -- Strip the references rather than implement
  the command.** bombyx is a CLI installed with
  `cargo install`; there is no server to publish to, so a
  deploy step would be a gate on nothing. The template it came
  from ships to a Raspberry Pi, which is where the idea
  belonged.

## Outcome

Removed from `CLAUDE.md` (the "Commits and releases" bullet),
`.claude/commands/commit.md`, and `.claude/commands/release.md`
(three places: the prerequisite line, the "nothing to release"
rationale, and the closing instruction). `.gitignore` lost
`.deploy`.

Two rewrites were needed rather than plain deletions, because
the deleted command was carrying an argument:

- The annotated-tag rule was justified by a lightweight tag
  being "invisible to the deploy guard". The rule is still
  right, so it now stands on what annotated tags actually give
  you -- a date, an author, and being what `git describe` finds.
- `/release`'s refusal to release nothing was justified as
  avoiding "a deploy in disguise". It now says the real reason:
  a version number describes what changed, and bumping it when
  nothing did makes it bookkeeping.

`.claude/commands/template-sync.md` keeps its mention. It uses
`cargo xtask deploy` as the example of a template file
referencing something a derived project has removed -- which is
exactly this bug, described in advance.

`CLAUDE.md` now states plainly that there is no deploy step, so
the absence is recorded rather than merely un-described.

### Follow-up

Closing this outside `/implement` produced the dangling Done
link that `done-links-may-dangle` is about; this file exists so
the link resolves.
