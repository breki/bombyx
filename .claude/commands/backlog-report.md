---
description: Local HTML report of every open todo and GitHub issue -- merged, grouped, described, and rated for complexity and priority
argument-hint: "[focus] (optional, e.g. 'security only' or 'P1 and P2')"
allowed-tools: Read, Write, Edit, Grep, Glob, PowerShell, Bash(cat:*), Bash(gh issue list:*), Bash(gh issue view:*), Bash(grep:*), Bash(ls:*), Bash(cp:*), Bash(xdg-open:*)
---

Build one local HTML page that lists every open item in
`docs/todo.md` and every open GitHub issue. Each item gets a row with
a short description, a complexity rating and a priority. Items are
grouped by area.

The user's focus: `$ARGUMENTS`. If empty, report everything. A focus
narrows which rows appear. It does not change the scales below.

This is a **reporting** command. It changes nothing in the repo: no
edits to `docs/todo.md`, no issue comments, no labels, no commits.

The page is an `/html-report` page, so every hard rule in
`.claude/commands/html-report.md` applies: it is local only and never
a cloud Artifact, it makes no external requests, the CSP `<meta>`
stays, sizes come from the `--fs-*` scale, both themes work, and it
holds no secrets or machine names. Read that file and
`docs/ai-agents/html-report-template.html` before writing.

## 1. Gather

```bash
cat docs/todo.md
gh issue list --state open --limit 200 --json number,title,labels,body
```

An issue body often states the problem first and the proposed fix
last. For every issue whose body runs past about 1500 characters,
read the rest with `gh issue view <n> --json body` before describing
it. A description written from the first half of an issue misses the
options it asks us to choose between.

## 2. Merge the duplicates

A todo entry and an issue that describe the same work become **one
row carrying both references**. Two signs identify a pair:

- the todo body names the issue ("GitHub issue #27"), or
- the issue title starts with the todo's slug
  (`packer-box -- ...` pairs with `### packer-box`).

A todo that only *mentions* an issue as related (for example
`host-network-isolation` citing #92) is not a pair. Keep both rows.

Count the pairs. The page reports them, and the notes section lists
them, because closing one side of a pair and not the other leaves the
two trackers disagreeing.

## 3. Group

Sort the rows into five to nine areas named by what the work touches,
not by where it is tracked. The areas used so far, as a starting point
rather than a fixed list:

- Network containment
- Credentials and guest hygiene
- CLI correctness and usability
- Provisioning and guest state
- Hosts, platforms and boot speed
- Multi-host visibility
- Documentation accuracy
- Code structure and developer tooling

Give each area a one-sentence introduction saying what its items have
in common. Order the areas by how many P1 and P2 items they hold,
most first. Within an area, sort rows by priority, then complexity.

## 4. Describe and rate each item

For each row, write:

- **Title** -- plain words, not the slug. Replace machine names with
  their role: "the VM host", "a Windows machine".
- **References** -- `#n` for the issue and the slug for the todo.
- **Description** -- one paragraph of three to five sentences: what
  goes wrong or is missing, the mechanism behind it, and the fix or
  the options to choose between. Say when the item is a decision
  rather than a bug. Follow the **Writing** rules in `CLAUDE.md`.
- **Complexity, 1-5:**
  - 1 -- an hour: a one-line or doc fix
  - 2 -- a small change with a test
  - 3 -- a focused change touching a few files, or a host bring-up
  - 4 -- a feature: new module, command or infrastructure
  - 5 -- a redesign
- **Priority, 1-3:**
  - P1 -- containment or a credential is at risk, or the tool claims
    a safety property that does not hold
  - P2 -- real friction, or a decision worth making soon
  - P3 -- nice to have, or waiting on something else
- **Blocked** -- flag an item that cannot start yet, such as work that
  needs a Windows VM host or depends on another open item.

The ratings are our judgement, made from the item text. Nobody has
estimated them, and the page says so under the table.

## 5. Write the page

Copy the template to the scratchpad as `backlog-report.html` and keep
its token block, dark theme and CSP. Set `--wide` to about `1240px`,
because the table needs more width than a reading column. Write the
HTML directly. Do not add a generator script to the repo.

Sections, in order:

1. **Masthead** -- title "Open todos and GitHub issues", the date, and
   a row of stat tiles: distinct items, GitHub issues, todo entries,
   tracked in both, P1 / P2 / P3, blocked.
2. **Quick wins** -- cards for every item rated complexity 1-2 and
   priority 1-2, each showing its title, references and priority.
3. **Backlog** -- one table with the columns Item, Description,
   Complexity and Priority. Put a legend above it that defines both
   scales and the two kinds of reference chip. Each area opens with a
   full-width group row holding its name, item count and
   introduction.
4. **Notes** -- the merged pairs, and a line saying machine names were
   left out on purpose.

Components the template lacks, all sized through `--fs-*`: a group
row on `--surface-2`; reference chips, with an accent fill for `#n`
and an outlined chip for a slug; five dots for complexity, filled in
`--accent`, with the number beside them; a priority pill using
`crit`, `warn` or a neutral style for P1, P2 and P3; and a dashed
"Blocked" pill.

## 6. Verify

Run the two checks from `/html-report` (no external references, no
`px` font sizes). Then check the numbers against the file, not
against memory:

```bash
F=<scratchpad>/backlog-report.html
grep -c '<tr class="grp"' "$F"   # must equal the number of areas
grep -o '<td class="item">' "$F" | wc -l   # must equal "distinct items"
```

Recount the stat tiles the same way, one `grep -o ... | wc -l` per
figure, and fix any tile that disagrees. Finally, grep the page for
every `host` value in the operator's config and for every machine
name that appeared in the issue bodies. Expect zero matches.

## 7. Deliver

Copy the page to the Desktop as `backlog-report.html` and open it.
Keep the filename stable, so a re-run refreshes the same path. If the
file already exists and this session did not create it, ask before
overwriting.

```bash
dest=~/Desktop/backlog-report.html
[ -e "$dest" ] && echo "EXISTS -- ask first"
cp <scratchpad>/backlog-report.html "$dest" && xdg-open "$dest"
```

```powershell
$dest = "$env:USERPROFILE\Desktop\backlog-report.html"
if (Test-Path $dest) { throw "exists -- ask first" }
Copy-Item <scratchpad>\backlog-report.html $dest
Start-Process $dest
```

## 8. Reply

Give the path, the counts (items, issues, todos, pairs), and the P1
items by title. Say that the ratings are judgement. Name anything you
could not read, such as `gh` failing or an issue body you did not
fetch in full.
