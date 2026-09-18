---
description: Re-put the last decision the assistant raised in prose to you as a structured AskUserQuestion, recommendation first
allowed-tools: AskUserQuestion
---

Take the most recent choice the assistant put in prose -- options,
a recommendation, an open decision it laid out but did not ask
through the tool -- and re-put it as a structured
`AskUserQuestion`, so you click rather than type. It adds no new
analysis; it reshapes what is already on screen into a question.

## When to use it

After the assistant has written a decision or a set of options into
the body of a reply and left it as prose. `/ask` turns that into
buttons.

## Instructions

1. **Find the decision.** Look back to the last relevant message --
   the assistant's own, or a choice a subagent returned -- and take
   the question it raised and the options it named. Do not invent a
   new decision: if the recent transcript raised none, say so and
   stop rather than manufacturing one.

2. **Reshape, do not re-argue.** The options are choices the
   operator has already read, so the labels summarize them and the
   descriptions carry the one-line trade-off. Context the choice
   needs was in the prose already; do not repeat it at length.

3. **Call `AskUserQuestion`.** One to four questions, each with two
   to four options, following `CLAUDE.md` under **Collaboration**:
   - Put the assistant's own recommendation first, label it
     "(Recommended)", and give its one-line reason. If the assistant
     made no recommendation, do not invent one -- present the
     options evenly.
   - Keep the question's lead readable by a non-expert: what the
     decision means, not how it is built. Internal type names,
     paths and API names go in the option descriptions.
   - Add a `preview` to an option only when a concrete artifact -- a
     snippet, a diff, a layout -- helps compare; previews are
     single-select only.
   - Use `multiSelect` when the choices are not mutually exclusive.

## Rules

- **Reshape only.** Add no option the assistant did not raise and
  no recommendation it did not make. If the right answer needs
  analysis that has not happened, do that in prose first -- `/ask`
  is not the place for it.
- **One decision per question.** Independent decisions are separate
  questions in the one call, not options crammed into one.
- **Do not choose.** `/ask` opens the question; the answer is the
  operator's. Once it comes back, act on the selected option as the
  decision, not a suggestion.
