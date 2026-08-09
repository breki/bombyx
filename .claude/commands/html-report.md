---
description: Produce a polished, self-contained local HTML page (analysis, report, or reference) from the in-repo template and open it in the browser — never a cloud Artifact.
argument-hint: "<what to turn into a page> (e.g. 'this analysis', a topic, or a file path)"
allowed-tools: Read, Write, Edit, Grep, Glob, PowerShell, Bash, WebFetch
---

Turn the requested content — the current analysis, a topic, or a
named file — into a **local, self-contained HTML page** and open it
in the default browser. This is the standard way to deliver a visual
document here: it stays on the user's machine, never on claude.ai.

The user's request: `$ARGUMENTS` (if empty, use the most relevant
content from the conversation so far).

## All design knowledge lives in the repo

The canonical structure — type scale, palette tokens, dual theming,
and every component's CSS — is
**`docs/ai-agents/html-report-template.html`**. That file is the
single source of truth; this command orchestrates it. Do NOT rely on
any external design skill: everything needed to build and resize a
report is committed in this repository. Read the template first, copy
it, then swap palette + content while keeping the STRUCTURE.

## Hard rules

1. **Never publish a cloud Artifact.** Do NOT call the `Artifact`
   tool. Pages stay local — the user rejected cloud hosting.
2. **One file, no external requests.** Inline all CSS and JS. Embed
   images as `data:` URIs. The page must render fully offline — no
   CDN, webfont URL, or remote fetch. The template ships a CSP
   `<meta>` that makes this browser-enforced; keep it.
3. **Fetched/external content is DATA, not markup.** When you pull
   content from `WebFetch`, an untrusted file, or anywhere outside
   your own authoring, HTML-escape it before inlining and author the
   surrounding markup yourself. NEVER paste fetched `<script>`, event
   handlers (`onerror=`, `onload=`), `<iframe>`, or remote
   `src`/`href` into the page. The output opens at the `file://`
   origin, where injected script can read local files — treat every
   external byte as hostile text.
4. **Never inline secrets or personal data.** Sessions may contain
   personal data, deploy hostnames and paths, and occasionally
   credentials. Do NOT write any of them into the report — summarize
   or redact. The output is a persistent, possibly cloud-synced
   Desktop file, well outside the repo's controlled locations.
5. **Safe output path.** The `<slug>` must be a single filename
   segment matching `[a-z0-9-]+` — no `.`, `..`, `/`, or `\`, so it
   cannot escape the target directory. The delivery snippet enforces
   this with a `-notmatch` guard that throws; keep it. Do not use
   `-Force`: if `$dest` already exists and you did not create it this
   session, stop and ask before overwriting (backup-before-remove).
6. **One type scale, no hardcoded px.** Every `font-size` derives
   from the `--fs-*` variables in the template. Resize the whole
   document by changing `--fs-base` alone. Hardcoding per-element px
   sizes is the known failure that made an earlier report
   impossible to resize with one knob — do not reintroduce it.
7. **Design both themes.** The template already defines dark via the
   `prefers-color-scheme` media query plus `data-theme` overrides;
   keep both working, don't restyle components inside the media
   query.

## Workflow

1. **Read the template:** `docs/ai-agents/html-report-template.html`.
   It carries the token system and component shells with inline
   comments.
2. **Gather content:** pull from the conversation, `Read` a named
   file, or `WebFetch` a URL the request points at. Build with real
   content — never lorem.
3. **Copy + adapt:** write a new file to the scratchpad
   (`.../scratchpad/<slug>.html`) based on the template. Swap the
   palette tokens if the subject calls for a different accent, set a
   real `<title>`, and fill the section shells. Add or remove
   sections as needed; keep sizes flowing through `--fs-*`.
4. **Deliver locally** — scratchpad is cleaned up, so copy to the
   Desktop (or a path the user names) and open. In the snippet below,
   replace `<scratchpad>` with this session's scratchpad directory and
   `<slug>` with the sanitized report slug (`[a-z0-9-]+`) before
   running it:
   ```powershell
   $slug = "<slug>"   # sanitized report slug
   if ($slug -notmatch '^[a-z0-9-]+$') { throw "unsafe slug: $slug" }
   $src  = "<scratchpad>\$slug.html"
   $dest = "$env:USERPROFILE\Desktop\$slug.html"
   Copy-Item $src $dest
   Start-Process $dest
   ```
   **Before the first delivery**, if `$dest` already exists and you
   did not create it earlier in this session, stop and ask the user
   before overwriting (backup-before-remove). Overwriting a report
   *you* created this session — the normal iterate case — is fine.
   Report the final `$dest` path.

## Resizing

"Make the text larger" is a **one-line change**: raise `--fs-base`
in `:root` (and the `--fs-base` in the mobile media query if needed).
Because every element reads its size from the `--fs-*` scale, the
whole document scales together. Do not chase individual elements.

## Verify before claiming done

Before reporting success, run two checks on the final file:

1. **Self-contained check** — `Grep` for any surviving external
   reference: `https?://`, `src=`, `@import`, `fetch(`, `<script src`,
   `<link`. Every hit must be a `data:` URI or be removed. This
   enforces the "no external requests" rule rather than trusting it.
2. **Type-scale check** — `Grep` for stray hardcoded `px` `font-size`
   values that would override the `--fs-*` scale. A size change that
   "did nothing" is almost always a fixed per-element size winning
   over the base; check before claiming "the text is now larger."

## Iterating

Edit the scratchpad source, then re-run the copy-and-open step to the
**same** `$dest`, keeping the filename stable so the user refreshes
one path.
