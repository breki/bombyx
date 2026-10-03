# Editing hazards

Five ways a scripted or anchored edit lands somewhere other than
where it was aimed, or reports success for an edit it never made.
`CLAUDE.md` under **Environment Constraints** holds the rule they
add up to; this file holds the detail, so it does not sit in every
session's context.

## A slurp-mode regex picks the wrong block

A whole-file `perl -0pi -e 's/.../.../'` has no idea which block it
lands in. Two shapes are reliably dangerous:

- **Indentation-carrying formats**, such as YAML, where a
  wrong-block match still parses.
- **Anything next to a `///` block**, where inserting before an
  item silently reassigns the comment above it to the new one.

`sed` and `perl` are fine for flat text and one-line
substitutions. Edit YAML and doc-comment neighbourhoods with
`Edit`.

## A scripted string replace over Rust

The same hazard, whatever language does the replacing: a rename
catches the word in a sentence, a `map_err` closure nests wrongly,
a signature change misses a call site -- and each such edit tends
to sit next to a `///` block. Reach for `Edit` with an anchor
unique in the file, and keep a scripted replace for a substitution
that fits on one line.

## Python's `open()` rewrites line endings

A `.ps1` file checks out with CRLF, because `.gitattributes` sets
`*.ps1 text eol=crlf`. The default `open()` reads it as LF and, on
Linux and macOS, writes LF back, so the tests read other bytes
than a fresh checkout or CI does. Open it with `newline=''` so the
CRLF survives, and check that `file` still prints "with CRLF line
terminators".

## A batched replace reports edits it never made

A script that makes several replacements and writes the file once
at the end can fail an assertion partway: it raises, the write
never runs, and the shell's next `echo ok` prints anyway, because
nothing reads the script's exit status. Write the file after
**each** successful replacement and read it back. A fix is landed
when `grep` or `sed -n` shows it, not when the script that made it
says so.

## An `Edit` anchor that ends mid-line

Ending an `Edit`'s `old_string` partway through a wrapped line
leaves the rest of that line untouched, and the rest fuses onto
the last line of `new_string`, making a line over 80 columns or two
words run together. Extend the anchor to the end of the line and
reflow the whole span in the one edit.

Rust has the same hazard, and rustfmt does not repair a fused
token inside a string. A scripted replace, such as Python's
`str.replace`, has it too: its old and new text also end at a
newline.
