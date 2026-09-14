# Prose style: what clear technical writing looks like

This is the positive standard `blue-pencil` edits toward. The
**Voice** section of `CLAUDE.md` lists the faults to remove;
this file describes the shape of good prose to aim for, so the
reviewer has a target, not only a list of things to avoid.

The characteristics below come from studying two bodies of
technical writing that readers consistently find clear on the
first pass: the Rust book and the Python documentation. The
point is not their subject matter or their vocabulary. It is
how their sentences are built. Six habits recur.

## 1. Put a named thing in the subject

The subject of the sentence is the actor: a file, a function, a
command, a value, a person. It is not a placeholder like
"there", "it", or "nothing", and it is not an abstract noun
standing in for one of those.

- Weak: "There is nothing to read at any privilege level."
- Clear: "The guest cannot read the host's name at any
  privilege level."

The clear version names who cannot do what. A reader can
picture the guest; they cannot picture "there".

## 2. Give the subject a concrete verb

The verb names the action the subject performs: reads, writes,
clones, refuses, removes, boots. Avoid a verb that only asserts
that something matters ("is the point", "is what counts"), and
avoid a noun doing the verb's work ("the plan owns
exhaustiveness" instead of "`plan.rs` lists every action").

- Weak: "Two types, and the split is the point."
- Clear: "The module has two types. They are separate because
  each carries a different rule."

## 3. Define a term where it first appears

When a sentence introduces a term the reader may not know,
define it in that same sentence, usually as a short apposition:
"X is a Y that does Z." Do this before any later sentence
relies on the term. A term used first and explained later
forces the reader to hold an unknown in their head until the
explanation arrives.

- Clear: "A deploy key is a single-repository SSH key that the
  guest uses to clone. Because it reaches only one repository,
  a leak from the VM exposes only that repository."

The second sentence can lean on "deploy key" because the first
one settled what it is.

## 4. Explain the mechanism before the conclusion

State how something works, then state what follows from it. A
reader given the mechanism can work out the next case without
being told each one.

- Clear: "`ssh host \"cmd\"` starts a non-interactive shell,
  which skips the startup files that set `PATH`. So a program
  installed only by those files is invisible to that command."

The first sentence is the mechanism; the second is the
consequence. A reader who understands the first can diagnose a
second symptom the document never mentions.

## 5. Make the relationship explicit

Spell out cause and effect, condition and result, with plain
connectives: when, if, so, because, then. A relationship the
reader has to reconstruct from two bare sentences placed side
by side is a relationship stated badly.

- Weak: "The config names the host. Teardown runs `rm -rf`."
- Clear: "Because teardown runs `rm -rf` on whichever host the
  config names, a wrong host deletes the wrong machine's
  directory."

The connective "because" is what turns two facts into one
claim the reader can act on.

## 6. Keep the core sentence short, and lead with the concrete

State the main claim in a short declarative sentence. A longer
sentence is fine when its clauses are ordered one step at a
time, each finishing before the next begins, rather than
holding a subject open across an embedded clause. When a
statement is abstract, put a concrete example right after it.

- Clear: "bombyx sends the file down a pipe, not as an
  argument. Any account on the machine can list a command's
  arguments; none can read another process's pipe."

Two short sentences carry the rule and its reason without
making the reader parse a single long one twice.

## Using this file

`blue-pencil` reads this alongside the **Voice**, **Code
comments** and **Documentation style** sections of `CLAUDE.md`.
When it proposes a rewrite, the rewrite should show these
habits: a named subject, a concrete verb, defined terms, the
mechanism before the conclusion, an explicit relationship, and
a short core sentence. A rewrite that removes a fault but does
not read like the examples above has not finished the job.
