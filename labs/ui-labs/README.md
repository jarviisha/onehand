# ui-labs

A place to try UI before it reaches the app: proposed layouts, spacing and
components drawn with gpui-component on static data. Nothing here is the app
and nothing here is binding; an idea that works moves into `crates/app` (and
its contract into `DESIGN.md`) in a pull request of its own.

```bash
make labs       # this crate: the proposed UI
make showcase   # the app as it is, on scratch projects and the Mock UI agent
```

- Outside the workspace (`exclude = ["labs"]` in the root manifest) and built
  by no CI job, so an experiment never breaks a build and the app never
  depends on one.
- Its `Cargo.lock` was seeded from the workspace's so it runs on the app's
  gpui commit. After moving gpui or gpui-component in the workspace, copy the
  root `Cargo.lock` here again.
- `make labs` builds into the workspace's `target/`, so the shared gpui graph
  is compiled once.
- No CI job runs its tests either, so run them by hand before a lab change
  lands: `cargo test --manifest-path labs/ui-labs/Cargo.toml --target-dir target`.
  They include source scans holding every length and colour to `src/tokens.rs`.
- `src/tokens.rs` holds every value the proposal is built from, each with
  the reason for it. `src/main.rs` is the window layout and the theme wiring;
  `src/pages.rs` the overview, Tasks and Issues; `src/composer.rs` every card
  and popup on the composer.

What the proposed UI is and the rules it follows are in [DESIGN.md](DESIGN.md),
including where it departs from the app's own and what is not drawn yet.
