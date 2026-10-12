---
name: ui-review
description: Review checklist for UI changes in crates/app, plugins/builtin and crates/plugin-host. Use before finishing any change that alters what is drawn, and when reviewing someone else's.
---

# UI review

1. **Answer [checklist.md](checklist.md)** for the change: every item yes, no or n/a, each *no*
   with its file and line, each *n/a* with its reason. An item names the document that owns its
   rule; read that rule, not just the question.
2. **Scan the lines the change adds**: `.claude/skills/ui-review/scan.sh` (against the merge-base
   with `main`; pass another base as its argument). Fix every line it prints, or give the reason
   it stays in the report. It cannot tell an inline `#[cfg(test)]` module from code; skip those.
3. **Run the checks**: `cargo test -p onehand --locked` (its source guards included), the tests
   of every other crate the change touched (`cargo test -p <crate> --locked`), `make clippy
   CLIPPY_EXTRA="-- -D warnings"` and `make fmt`. Quote any failure.
4. **Carry the documents** in the same change:
   - what a screen holds, its order, or a rule changed: `DESIGN.md`, or the feature's own
     document (`docs/tasks.md`, `docs/workflows.md`, `docs/unattended.md`);
   - a pattern or a behaviour changed: `docs/ui-patterns.md`;
   - a token or a builder added: `design-tokens/reference.md` or `ui-components/catalog.md`.

   `DESIGN.md` stays an overview of about 250 lines, at most six for a new screen; past that,
   merge or cut before adding. Code never cites a document: a comment gives its reason in its own
   words. A file past about 800 lines of code splits along a seam in the same change.
5. **Report**, then stop: each item that failed and how it was fixed or why it was left; the
   checks run and their results; what the user should look at when they open the app (`make dev`,
   `make showcase`). Build and test, and leave the window to the user.

No line a change adds breaks a rule, and the guards in `crates/app/src/guards.rs` hold the
colour, length, spacing, shadow and type rules across every UI crate.

Done when every checklist item has an answer, the scan prints nothing left unexplained, and every
check passes.
