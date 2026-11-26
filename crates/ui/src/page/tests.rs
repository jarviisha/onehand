//! A page's head under strain: a long title in a narrow box and a wide one,
//! at two rem sizes, with none, one and three actions.

use super::page_head;
use crate::Short as _;
use gpui::{
    AnyElement, Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, Styled as _, TestAppContext, VisualTestContext, Window, div, px, rems,
};

/// How far two edges may differ and still be the same.
const SLACK: f32 = 0.5;

struct Head {
    title: String,
    actions: usize,
    width: gpui::Rems,
    rem: Pixels,
}

impl Render for Head {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(self.rem);
        let actions: Vec<AnyElement> = (0..self.actions)
            .map(|i| {
                crate::action(("head-act", i))
                    .debug_selector(move || format!("page-head-act-{i}"))
                    .short()
                    .label("Run check")
                    .into_any_element()
            })
            .collect();
        div()
            .w(self.width)
            .child(page_head(self.title.clone(), None, actions, cx))
    }
}

fn long() -> String {
    "word ".repeat(40)
}

fn unbroken() -> String {
    "x".repeat(200)
}

/// The head drawn with `title` and `actions` in a box `width` wide at `rem`.
fn drawn<'a>(
    title: &str,
    actions: usize,
    width: f32,
    rem: f32,
    cx: &'a mut TestAppContext,
) -> &'a mut VisualTestContext {
    let title = title.to_string();
    let (_, cx) = cx.add_window_view(move |_, _| Head {
        title,
        actions,
        width: rems(width),
        rem: px(rem),
    });
    cx.run_until_parked();
    cx
}

#[track_caller]
fn part(cx: &mut VisualTestContext, sel: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(sel)
        .unwrap_or_else(|| panic!("nothing drawn as {sel}"))
}

fn same(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= SLACK
}

/// The widths and rem sizes a head is drawn at.
const DRAWN: [(f32, f32); 4] = [(20., 16.), (20., 24.), (120., 16.), (120., 24.)];

/// The head's parts: itself, its title and its actions.
fn parts(
    title: &str,
    actions: usize,
    width: f32,
    rem: f32,
    cx: &mut TestAppContext,
) -> [Bounds<Pixels>; 3] {
    let cx = drawn(title, actions, width, rem, cx);
    [
        part(cx, "page-head"),
        part(cx, "page-head-title"),
        part(cx, "page-head-actions"),
    ]
}

#[gpui::test]
fn a_long_page_title_leaves_its_actions_in_place(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for (width, rem) in DRAWN {
        let [_, title, actions] = parts("Notes", 3, width, rem, cx);
        for name in [long(), unbroken()] {
            let [head, named, named_actions] = parts(&name, 3, width, rem, cx);
            let at = format!("at {width} rems, {rem} px, a title of {} chars", name.len());
            assert_eq!(named_actions, actions, "{at}: the actions moved");
            assert!(
                same(named.size.height, title.size.height),
                "{at}: the title took a second line"
            );
            assert!(
                named.right() <= actions.left(),
                "{at}: the title runs into the actions"
            );
            assert!(
                named.left() >= head.left() && named.top() >= head.top(),
                "{at}: the title stands outside the head"
            );
        }
    }
}

/// Every action's bounds, read off the head's actions by their ids.
/// The head, and every action's bounds, read off the head's actions by
/// their ids.
fn each_action(
    n: usize,
    width: f32,
    rem: f32,
    cx: &mut TestAppContext,
) -> (Bounds<Pixels>, Vec<Bounds<Pixels>>) {
    let cx = drawn("Notes", n, width, rem, cx);
    let acts = (0..n)
        .map(|i| {
            part(
                cx,
                ["page-head-act-0", "page-head-act-1", "page-head-act-2"][i],
            )
        })
        .collect();
    (part(cx, "page-head"), acts)
}

#[gpui::test]
fn a_page_heads_actions_share_one_height_and_one_line(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for (width, rem) in DRAWN {
        for n in [1, 3] {
            let (head, acts) = each_action(n, width, rem, cx);
            assert!(
                same(head.size.width, rems(width).to_pixels(px(rem))),
                "at {width} rems, {rem} px the head is not its box's width: {head:?}"
            );
            for act in &acts {
                assert!(
                    same(act.size.height, acts[0].size.height) && same(act.top(), acts[0].top()),
                    "at {width} rems, {rem} px: {acts:?}"
                );
                assert!(
                    act.left() + px(SLACK) >= head.left()
                        && act.right() <= head.right() + px(SLACK),
                    "at {width} rems, {rem} px an action stands outside the head {head:?}: {act:?}"
                );
            }
        }
    }
}

#[gpui::test]
fn a_page_head_with_no_actions_gives_the_title_the_row(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    for (width, rem) in DRAWN {
        let [head, title, _] = parts(&unbroken(), 0, width, rem, cx);
        // The gap the head keeps before its actions, as one action shows it:
        // with none, it and their empty box are all that stand between the
        // title and the head's end.
        let [_, one_title, one_actions] = parts(&unbroken(), 1, width, rem, cx);
        let gap = one_actions.left() - one_title.right();
        assert!(
            same(title.right() + gap, head.right()),
            "at {width} rems, {rem} px: {title:?} leaves the head {head:?} unfilled"
        );
    }
}
