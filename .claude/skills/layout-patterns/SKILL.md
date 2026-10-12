---
name: layout-patterns
description: Layout for pages, strips, boxes, lists, list-and-detail, forms, dialogs and detail views. Use when adding a screen, panel or dialog, or changing where things sit, how wide they run, or when a view splits or stacks.
---

# Layout

The window's regions are `DESIGN.md`'s (*Layout*); the patterns drawn inside them are
`docs/ui-patterns.md`'s (*Layout*). Read the parts for what you build, at the version in the
working tree.

## Building a screen

1. **Pick its pattern** from the table, open the example, and build from its builders.
2. **Measure the container** for every split or stack: a dock or a pane can be narrow in a wide
   window. `list_detail` and the boxes' wrap read the width they are given.
3. **Bound it**: every list has a named cap and says how many it left out; text that can run long
   truncates or wraps, as its neighbours do.
4. **Take widths and heights from tokens**: a page's column from `PAGE_*`, a strip's height from
   `BAR_H`, a row's from `ROW_H` (the `design-tokens` skill).

Done when the screen follows one pattern below, built from that pattern's builders, and every
list on it is capped.

| Building | Pattern | Example |
|---|---|---|
| A page in the agent pane | Page | `tasks_frame` in `crates/app/src/chat/pane/tasks_page.rs` |
| The facts about one thing on a page | Box | `card_box` and `card_title` in `crates/app/src/chat/pane/workspace_page.rs` |
| A bar along the top of a region | Strip | `crates/app/src/workbench/panel/strip.rs` |
| Rows to pick from | List | the Files tree, `plugins/builtin/workbench-files/src/view.rs` |
| A list beside its detail | List and detail | `list_detail` in `crates/plugin-host/src/list_detail.rs`; the Markdown mode |
| Inputs and a submit | Form | `field` in `crates/app/src/settings.rs`; the Workflows form |
| A modal question or form | Dialog | `crates/app/src/dialogs.rs` |
| One thing read in full | Detail | the issue, `plugins/builtin/workbench-issues/src/view/detail.rs` |

## A page, in code

From `tasks_frame`: the header names the page, the body scrolls as one centred column.

```rust
div().size_full().v_flex()
    .child(self.header(cx))
    .child(
        div().id("tasks-page").flex_1().min_h_0().overflow_y_scroll().p_6().child(
            div().h_flex().w_full().justify_center().child(
                div().v_flex().gap_6().w_full().max_w(PAGE_WIDE).text_sm().children(body),
            ),
        ),
    )
```

## Right and wrong

```rust
// Wrong: the window decides, so a narrow dock in a wide window still splits
let side_by_side = window.viewport_size().width > px(900.);
// Right (workbench-markdown/src/view.rs): the width measured on the view decides
let layout = list_detail(self.split.read(cx), &DOC_LIST, self.width.get(), rem);
```

```rust
// Wrong (chat/pane/header.rs): each strip with its own padding
.h(BAR_H).px_4()
// Right: every strip
.h(BAR_H).px_3()
```

```rust
// Wrong: `mx_auto` inside a gpui `list` row, which is its own layout root
row.child(div().max_w(CONTENT_COLUMN).mx_auto().child(body))
// Right
row.child(div().h_flex().justify_center().child(div().w_full().max_w(CONTENT_COLUMN).child(body)))
```

```rust
// Wrong (workflows_page.rs): a page padded its own way, its width a bare number
div().px_4().py_6().child(div().max_w(rems(EDITOR_COLUMN)))
// Right
div().p_6().child(div().max_w(tokens::PAGE_PROSE))
```

A change to what a screen holds or the order it holds it in updates `DESIGN.md` (or the
feature's own document) in the same change; the `ui-review` skill checks it.
