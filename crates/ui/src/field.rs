//! The text fields every crate draws, and a setting stacked over its field.

use crate::tokens;
use gpui::{
    AnyElement, App, Div, Entity, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _,
    div,
};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, StyledExt as _,
};

/// How much of the well's own fill a field's fill keeps.
const FIELD_FILL: f32 = 0.94;

/// A field's fill: the well, a step further toward the ink. The well alone sits
/// level with a floating panel's fill in the light palette, and a field on it
/// vanished; a step past it stands off the page, a floating panel and a well
/// alike, in both palettes by one rule.
fn field_fill(cx: &App) -> Hsla {
    cx.theme()
        .muted
        .mix_oklab(cx.theme().foreground, FIELD_FILL)
}

/// A text field at the theme's corner, on a fill with no edge: the corner
/// [`action`](crate::action) gives a button, so a field and the button beside it are one
/// shape, and the fill alone marks it off the surface it sits on.
///
/// **Focus is the edge alone.** The library marks a focused field twice: its
/// edge turns the ring ink, and a ring is drawn round it, outside its bounds.
/// The field clips what lies outside itself, which takes the ring away and
/// leaves the edge. At rest the edge is drawn clear, so it still has
/// somewhere to show.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: a feature crate draws
/// fields too, and one wrapper is one place for the corner to hold.
pub fn text_input(state: &Entity<InputState>, cx: &App) -> Input {
    Input::new(state)
        .rounded(cx.theme().radius)
        .bg(field_fill(cx))
        .border_color(cx.theme().transparent)
        .overflow_hidden()
}

/// One setting, stacked: its name, a line about it in meta text, and the
/// control under both at the column's full width -- so a field is as wide as
/// what can be typed into it, and a long description wraps above it rather
/// than beside it.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: a feature crate's form
/// asks for settings too, and a second copy is a second place for a field's
/// rhythm to drift.
pub fn field(
    title: impl Into<SharedString>,
    about: Option<AnyElement>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .child(div().text_size(tokens::CHROME_TEXT).child(title.into()))
                .children(about.map(|about| {
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(about)
                })),
        )
        .child(control)
}

/// A search field: a [`text_input`] led by the search mark and ended by the
/// library's clear button, so every search in the app and its feature crates is one
/// shape. The caller sizes it, as it would a field.
pub fn search_input(state: &Entity<InputState>, cx: &App) -> Input {
    text_input(state, cx)
        .prefix(
            Icon::new(IconName::Search)
                .small()
                .text_color(cx.theme().muted_foreground),
        )
        .cleanable(true)
}

/// A field of several lines, on the corner and the fill [`text_input`]
/// gives a field of one: the library draws a textarea on a fill of its own,
/// and beside a single-line field in the same form the two read as different
/// controls.
pub fn text_area(state: &Entity<TextareaState>, cx: &App) -> Textarea {
    Textarea::new(state)
        .rounded(cx.theme().radius)
        .bg(field_fill(cx))
        .border_color(cx.theme().transparent)
        .overflow_hidden()
}
