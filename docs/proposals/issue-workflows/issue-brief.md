# Piece 2: an issue written to be worked, and the brief shown before it is

- Status: proposal, wave 1. Part of [the proposal](README.md).
- Contracts it touches: [unattended.md](../../../docs/unattended.md) (*The brief*), the Issues tab's
  new-issue form, the issue picker.

## Goal

Fewer runs that do what the issue says and not what its author meant. Two levers: the issue is
written with the parts an agent needs, and the person sees the brief the agent will get, and can
add to it, before the run starts.

## Today

- A run's brief is `unattended::brief_for`: the issue's title and body word for word, and
  instructions naming the issue and saying nobody is watching. Each step's prompt fills `{brief}`
  in; onehand adds the place, the repository-conventions paragraph and one line per gate.
- The launcher's *Preview* already shows the first prompt as the agent would get it
  (`workflow::first_prompt`). The issue picker (`dialogs::pick_issue`) shows no preview.
- The Issues tab's new-issue form is a title, a body and labels (`issues::Draft`). There are no
  templates.

## Proposal

### Templates for an issue

Three shipped templates, chosen in the new-issue form: **Bug**, **Feature**, **Refactor**. Each is
Markdown with the same four headings, worded for its kind:

```markdown
## Problem
<!-- What is wrong, or missing, and who meets it. A bug: what happens and what should. -->

## Scope
<!-- What may change, and what must not. -->

## Acceptance
<!-- How you will judge the work. One line each. -->

## How to check
<!-- The command, the steps or the screen that shows it works. -->
```

- A template only fills the body in. It adds no field to the issue and no state; an issue written
  without one is worked as it is today.
- A project with its own `.github/ISSUE_TEMPLATE/*.md` offers those instead (front matter read for
  the name and labels, the rest as the body). Decision 3 of [the proposal](README.md).
- A template may carry labels (a bug template the `bug` label), which is how it can choose the
  workflow through a workflow label, with nothing new.

### What an issue lacks, said before it runs

A pure function in core reads an issue's body against **the template it was written from** and
says which of that template's headings are missing or empty. It is advice, never a refusal: the
picker and the issue's detail show *No acceptance written* in the muted ink, and the run still
starts. Onehand does not judge whether acceptance is met: that stays the reviewer's.

How a body is read:

- **Which headings count** are the template's own: the shipped four, or the headings of the
  project template the body matches. A body that matches no known template gets **no advice at
  all**: an issue written its own way, or under other headings, is never told it lacks acceptance.
- **A body matches a template** when it carries at least half of that template's headings.
  Headings are ATX lines (`#` to `######`), matched on their text with case and surrounding space
  ignored; the level does not matter, so a template's `##` and a body's `###` agree.
- **A section is empty** when, between its heading and the next heading of the same or a higher
  level, nothing is left once HTML comments (`<!-- … -->`, the template's hints) and whitespace are
  taken out. A section holding only a sub-heading is empty.
- **Fenced code blocks are skipped** when looking for headings, so a `# comment` in a shell
  sample is not one.

### The brief, shown and added to

The issue picker, once an issue is chosen, shows the same collapsed *Preview* the launcher has:
the workflow's steps and limits, and the first prompt filled with this issue's brief. Under it,
**Instructions for this run**, empty, which goes into the brief's instructions after the ones
`brief_for` writes. It is kept on the task's brief, not on the issue, so a retry carries it and the
issue's text is never rewritten behind its author.

For an unattended run found by its label nothing is shown, since nobody is there; for a body that
matches a template, the missing headings go into the report instead, so the person reading why a run went wrong sees *the issue had
no acceptance* beside it.

### Later, not in this piece

An agent that drafts the missing parts, or splits an issue into smaller ones, as a draft the person
edits and accepts. It needs a session that is not a run, writing into a form rather than a
worktree; worth its own proposal once templates show which parts are usually missing.

## What stays

- The body goes to the agent word for word, under the rules that guard it (only issues you opened
  are found by the search; a picked one shows who opened it).
- The brief is still built in one place, `brief_for`; the added instructions are a second argument,
  not a second builder.
- The commit convention and test commands stay out of the brief; the repository's instructions say
  them.

## Open questions

1. Does a project's template replace the shipped three, or sit beside them? Recommended: replace;
   a project that wrote its own has chosen.
2. Is half of a template's headings the right line for a match? Recommended: start there, and
   count the advice a person dismisses before moving it.

## Done when

- A new issue from each template has the four headings; one without a template is unchanged.
- The picker's preview shows this issue's body inside the first prompt, and what is typed under
  *Instructions for this run* reaches the agent's first prompt and a retry's.
- An issue from a template whose *Acceptance* holds only its hint comment says so in the picker
  and the detail, starts all the same, and an unattended report on it names the missing part.
- An issue written without a template, or under its own headings, gets no advice.
- The body reader has a test per rule above: heading levels, comments, fences, the match line.

## Documents to change when built

- `docs/unattended.md`: *The brief*.
- `DESIGN.md`: the launcher bullet, if the picker's preview reuses it as one control.
- `CONTEXT.md`: *Acceptance*.
