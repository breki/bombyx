---
description: Restate the reply above, or answer an instruction, in under 40 words
---

Say it in what a reader gets through in ten seconds.

## Usage

- `/short` -- restate the reply immediately above, in under 40
  words.
- `/short <instruction>` -- carry out `$ARGUMENTS`, and answer in
  under 40 words.

## The budget is forty words

Ten seconds of silent reading comes to roughly forty words, and
fewer when the text carries paths and identifiers. Forty is the
number to hold. "Be brief" is a criterion nobody can fail, and a
word count is one a reader can check.

Over budget, cut in this order: the evidence, then the
reasoning, then a qualification the reader has already accepted.
Never cut the answer.

## What survives

The outcome, and whatever the reader has to do next. Name that
next step in words rather than by quoting a path or a command,
for the reason under **It is read aloud** below: the reply
above is where the exact spelling belongs.

Drop tables, headings and code blocks. One or two sentences, or
two short lines. No preamble and no closing summary.

## It is read aloud

A speech synthesizer speaks this text, so write it for the ear
as well as the eye.

It strips every backticked span before speaking. A path or a
command inside the short version is therefore **deleted, not
pronounced**, and a sentence built around one comes out with a
hole in it. Say "the settings file" and leave the path to the
reply above. A commit hash, a line number or a version string
is the same problem heard rather than seen: `1c5360e` read out
is noise.

Prefer whole words to symbols: "about" rather than `~`, "to"
rather than `->`, "and" rather than `&`. Keep sentences short
and end each with a full stop, because the synthesizer splits
on those, and a missing one runs two sentences together into a
single breath. Bold and italics are stripped as well, so
emphasis has to live in the word choice.

None of this loosens the budget. Forty words still holds, and
naming a thing in words rather than in backticks usually costs
one or two.

## What does not bend

**A failure stays a failure.** When the reply above reported a
failing test, a skipped step or a claim nobody verified, the
short version reports it too. Compressing "nine gates pass,
coverage could not run" into "gates pass" produces a false
statement rather than a brief one. Cut the detail and keep the
bad news.

**This command re-derives nothing.** In the first form the reply
above is the whole input: read it, compress it, and stop. Do not
re-run a command to confirm it, and do not add a fact it did not
contain. When no reply precedes the call, say that in one line.

The **Narrate the work as it happens** rule under
**Collaboration** does not apply here, and this is the one place
it does not. The command is its own output, so a sentence
announcing what is about to happen would spend a quarter of the
budget saying nothing.

## Tools

There is no `allowed-tools` line, deliberately. The first form
needs no tools at all. The second needs whatever its instruction
turns out to need, and that cannot be listed in advance.
