---
name: ui-review
description: Review onehand UI for cross-screen consistency, contract compliance and interaction states. Use before finishing visible changes, for UI audits, and when changing the UI contract or its skills.
---

# UI review

Choose the path matching the actual diff. A contract update and a rendered UI update have
different evidence; neither proves the other is complete.

## Contract and skill changes only

1. Check ownership: [DESIGN.md](../../../DESIGN.md) owns visual roles and window structure;
   [UI patterns](../../../docs/ui-patterns.md) owns reusable layouts and behavior. References
   inventory actual tokens and builders. Skills explain how to apply them, not a second system.
   The contract is onehand's own: it names no other product as its model or its source, and no
   rule rests on one.
2. Follow the changed rule through the five skills, their references, this checklist and
   `CLAUDE.md`. Resolve contradictions and broken links. Preserve feature behavior unless the
   user requested changing it. Identify target rules separately from implementation claims.
3. Review realistic cases against the text: a new settings section, a row with secondary text,
   a hovered selected item, a narrow dock and a shared token change. Each should lead to an
   existing pattern or an explicit implementation gap, without inventing a local style.
4. Validate skill frontmatter with the available skill validator and run `git diff --check`.
   Runtime tests are unnecessary for prose-only changes. Report documents changed, validation
   and that no rendered UI was migrated.

## Implementation changes

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

   Keep `DESIGN.md` a compact overview, at most six lines for a new screen; reusable pattern
   detail belongs in `docs/ui-patterns.md`. Code never cites a document: a comment gives its reason in its own
   words. A file past about 800 lines of code splits along a seam in the same change.
5. **Compare peers and report**: name a same-role component or screen used for comparison and
   the shared builder or tokens checked. If a shared default changed, inspect its affected
   callers across app and plugins. Report failures, fixes, remaining gaps and checks run.
   Give the user a focused visual check in `make dev` or `make showcase`: both themes, narrow
   and wide containers, zoom, long text and the applicable interaction states. Build and test;
   do not launch or screenshot the app yourself, and do not claim visual verification.

No line a change adds breaks a rule, and the guards in `crates/app/src/guards.rs` hold the
colour, length, spacing, shadow and type rules across every UI crate.

Done when the applicable path is complete. For implementation, every checklist item has an
answer, the scan has no unexplained findings and required checks pass. A clean scan is not
evidence of visual consistency; any visual checks left to the user are stated explicitly.
