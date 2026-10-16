---
name: layout-patterns
description: Compose onehand screens with consistent hierarchy, alignment and density. Use for pages, panels, lists, forms and dialogs, cross-screen layout cleanup, and container-based split or stack behavior.
---

# Layout

Read [DESIGN.md](../../../DESIGN.md) (*Principles*, *Layout*), [screens.md](../../../docs/screens.md)
for the region being built, and [UI patterns](../../../docs/ui-patterns.md) (*Choose the container*,
*Hierarchy*, *Separation*, *Spacing and density*, then the relevant layout pattern). The window structure and feature content remain their own
contracts; visual cleanup does not authorize removing actions or changing navigation.

## Building a screen

1. **Identify hierarchy**: region identity, local filters, content groups and actions. Choose the
   least decorated container that expresses each relationship, preserving feature-required boxes.
   Separate by the ladder (UI patterns, *Separation*), and give each text its ink by role.
2. **Pick its pattern** from the table, inspect an equivalent screen and build from its builders.
   Compare text axes, action slots and section gaps. Existing code may need migration itself.
3. **Measure the container** for every split or stack: a dock or a pane can be narrow in a wide
   window. `list_detail` and the boxes' wrap read the width they are given.
4. **Bound it**: every list has a named cap and says how many it left out; text that can run long
   truncates or wraps, as its neighbours do.
5. **Apply pattern metrics**: a column page's width from `PAGE_*`, a list page's the pane's, strip from `BAR_H`, single-line row from
   `ROW_H`, gaps from the relationship table. Multi-line content grows; it does not acquire a
   smaller font to fit. Keep compact chrome separate from the reading content's rhythm.
6. **Line it up**: marks down a column in slots of one width on one centre line, labels on one
   edge, numbers and times in fixed-width slots, action groups holding their width.

Done when the screen follows one pattern below, built from that pattern's builders, and every
list on it is capped, with no unexplained difference from an equivalent pattern elsewhere.

| Building | Pattern | Example |
|---|---|---|
| Fields, boxes or one thing read in full, in the agent pane | Column page | `tasks_frame` in `crates/app/src/chat/pane/tasks_page.rs` |
| One long list to scan, filter and pick from, in the agent pane | List page | the Issues page, `plugins/builtin/workbench-issues/src/view/page.rs` |
| The filters under a page's head | Filter bar | `plugins/builtin/workbench-issues/src/view/page/head.rs` |
| Related fields or a subsection | Plain section | `section` in `crates/app/src/settings.rs` |
| The facts about one thing on a page | Box | `card_box` (plugin host); `page_card`, `card_title`, `page_section` and `pair` in `crates/app/src/chat/pane/page_parts.rs` |
| A bar along the top of a region | Strip | `strip` and `strip_control` (plugin host) |
| Rows to pick from | List | the Files tree, `plugins/builtin/workbench-files/src/view.rs` |
| A list beside its detail | List and detail | `list_detail` in `crates/plugin-host/src/list_detail.rs`; the Markdown mode |
| Inputs and a submit | Form | `field` in `crates/app/src/settings.rs`; the Workflows form |
| A modal question or form | Dialog | `crates/app/src/dialogs.rs` |
| One thing read in full | Detail | the issue, `plugins/builtin/workbench-issues/src/view/detail.rs` |

## A column page, in code

From `tasks_frame`: the header names the page, the body scrolls as one centred column.

```rust
div().size_full().v_flex()
    .child(self.header(cx))
    .child(
        div().id("tasks-page").flex_1().min_h_0().overflow_y_scroll().px_8().py_6().child(
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
// Wrong: each strip with its own padding
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
// Wrong: a page padded its own way, its width a bare number
div().px_4().py_6().child(div().max_w(rems(EDITOR_COLUMN)))
// Right
div().px_8().py_6().child(div().max_w(tokens::PAGE_PROSE))
```

```rust
// Wrong: a subsection framed, filled and lifted to look like a group
div().p_3().border_1().border_color(theme.border).bg(theme.muted).shadow(lift(cx)).child(group)
// Right: a heading and space; a box only for an independent summary
div().v_flex().gap_2().child(heading).child(group)
```

A change to what a screen holds or the order it holds it in updates `docs/screens.md` (or the
feature's own document) in the same change; the `ui-review` skill checks it.
