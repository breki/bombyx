# Output staircases on a Windows console

Fixed 2026-08-18. This is a short record so the link from
`docs/todo.md` resolves; the narrative -- what was measured, which
two theories were wrong, and why each half of the fix sits where it
does -- is the "Output that walked off the right edge of the screen"
entry in `docs/developer/DIARY.md`.

## Symptom

`bombyx status` and `bombyx doctor` rendered as a staircase: each
line beginning at the column where the previous one ended, and
splitting mid-token once it reached the right margin.

## Diagnosis

A missing carriage return, proven by the geometry of the operator's
own output rather than inferred. Line lengths 23, 66, 130 against
leading indents 0, 23, 66 -- every line starting exactly where the
last one stopped.

Ruled out along the way: ANSI colour codes (zero `0x1b` bytes in
either capture) and the hardcoded `LINE_WIDTH: usize = 80` in
`doctor/report.rs`, whose longest row measures exactly 80. The
mid-token splits were the terminal's own margin wrap of text that
had already been pushed rightward.

## The two halves

**The remote's bytes (`status`).** Without a pseudo-terminal the
remote's stdout is a pipe, so its tty layer never translates `\n`
to `\r\n`. Measured against the real host: 206 bytes, six line
feeds, no carriage returns. The same command under `ssh -tt`
returned 220 bytes with six CRs and six LFs, perfectly paired.

Fixed with `remote::Tty`, threaded through `plan` so a dry run
prints the argv the live run uses. Chosen only when both stdin and
stdout are terminals: `ssh -t` needs a local terminal to allocate
against, and a pipe must keep the bytes the remote wrote. Under a
PTY vagrant also colourizes -- the other 8 bytes of that
difference, two `ESC[0m` resets -- which is the second reason the
gate looks at stdout too.

**bombyx's own writes (`doctor`).** The table is rendered locally,
so a PTY cannot explain it. Every command that staircases runs
`ssh` first, and `self-update` -- which spawns children but never
`ssh` -- prints cleanly. The leading explanation is that `ssh.exe`
leaves a console-mode bit set that suppresses the console's
implicit carriage return; `DISABLE_NEWLINE_AUTO_RETURN` is the bit
with that effect. **That cause is unverified**: a redirected stdout
cannot reproduce a console-mode change, so confirming it needs a
real console. An earlier draft named
`ENABLE_VIRTUAL_TERMINAL_PROCESSING`, which does not have that
effect on its own -- a specific, checkable, and probably false
claim, which is worse than the hedge it was dressed up as.

Fixed in the binary (`print_lines`, `eprint_lines`), not the
library. `Report::render` keeps emitting `\n`, which is what keeps
its expected-output tests identical on every platform.
`SetConsoleMode` would be the precise instrument and is
unavailable: production crates are `#[forbid(unsafe_code)]`, with
the scoped exception only for `xtask`.

## Deliberately not done

Only the two messages printed *after* a child has run go through
the new writers -- `doctor`'s table and the failure line in
`execute`. Those are the only positions where the console mode can
already have changed, and a one-line message misplaced by a row is
far less visible than a whole table. The remaining `println!`
calls stay as they are until there is evidence they need otherwise.
