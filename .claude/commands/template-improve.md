---
description: Log feedback for the rustbase template
allowed-tools: Read, Write, AskUserQuestion, Bash(cargo xtask feedback-add:*)
---

Log an observation about the rustbase template in
`docs/developer/template-feedback.md`.

Entry placement, date stamping, ID minting, and dedup are
owned by `cargo xtask feedback-add` -- do **not** hand-edit
the file. Your job is the judgement: the section, a short
title, and the body prose.

## Instructions

1. If the user provided a specific observation, use it.
   Otherwise, use `AskUserQuestion` to ask what they
   noticed.

2. Decide which lifecycle section the entry belongs in.
   The file has three (read the file header for the full
   semantics):

   - **Open divergences** (`--section open`) -- new
     observations about current template state that are
     suboptimal, missing, or differently-shaped than the
     ideal. This is the default for a fresh observation.
   - **Resolved** (`--section resolved`) -- something the
     user has already fixed; the entry records what was
     wrong and how it was closed out.
   - **Suggestions to flow back** (`--section suggestion`)
     -- in a derived project, an idea to push upstream.

   Use `AskUserQuestion` (or infer from context) when the
   section is not obvious. Default to **open**.

3. Choose a short title (a few words -- it drives the entry
   ID slug) and write the body prose, wrapped at 80
   characters. The body should explain the issue, why it
   matters, and the suggested fix; for a **resolved** entry,
   end with a one-line summary of the fix.

4. Add the entry with the deterministic appender. Write the
   body to a temp file, then call:

   ```
   cargo xtask feedback-add --section <open|resolved|suggestion> \
     --title "<short title>" --body-file <tmp>
   ```

   The command mints a `tf-<yyyy-mm-dd>-<slug>` ID, inserts
   the entry at the top of the chosen section (newest
   first), and skips silently if that ID is already present.
   (Body may also be piped on stdin instead of `--body-file`.)

5. Do NOT commit -- the entry is included in the next
   `/commit`. Report the ID the command minted.
