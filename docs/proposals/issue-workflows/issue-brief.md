# Piece 2: an issue written to be worked, and the brief shown before it is

- Status: built in step 4 except a project's own templates, which the
  [README](README.md#order-of-work) assigns to step 6. What is built lives in
  [unattended.md](../../unattended.md) (*The brief*, the start form and the report), the Issues
  page bullet of [DESIGN.md](../../../DESIGN.md) and the glossary's *Acceptance*.
- Contracts it touches: the same documents.

## What is left

### A project's own templates (step 6)

A project with its own `.github/ISSUE_TEMPLATE/*.md` offers those instead of the shipped three,
whole: front matter read for the name and labels, the rest as the body. Decision 4 of
[the proposal](README.md). Reading them is a disk read, done off the UI thread; the body reader
(`issues::template::lacking`) already takes any template, so the advice follows with nothing
new.

### Later, not in this piece

An agent that drafts the missing parts, or splits an issue into smaller ones, as a draft the person
edits and accepts. It needs a session that is not a run, writing into a form rather than a
worktree; worth its own proposal once templates show which parts are usually missing.

## Done when

- A project with its own issue templates offers them, not the shipped three, and a body written
  from one is read against it.
- The body reader has a test for a project template replacing the shipped ones.
