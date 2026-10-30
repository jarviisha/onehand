//! The text fields every crate draws, and a setting stacked over its field.

use crate::tokens;
use gpui::{
    AnyElement, App, Div, Entity, IntoElement, ParentElement as _, SharedString, Styled as _, div,
};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _};

/// A text field as the library draws it, edged in `input` and ringed on focus,
/// at the theme's corner: the corner [`action`](crate::action) gives a button,
/// so a field and the button beside it are one shape.
///
/// Here rather than in the app for the reason [`action`](crate::action) is: a feature crate draws
/// fields too, and one wrapper is one place for the corner to hold.
pub fn text_input(state: &Entity<InputState>, cx: &App) -> Input {
    Input::new(state).rounded(cx.theme().radius)
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
                .child(
                    div()
                        .text_size(tokens::CHROME_TEXT)
                        .font_medium()
                        .child(title.into()),
                )
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

/// A field of several lines, at the corner [`text_input`] gives a field of
/// one, so the two read as one control in a form.
pub fn text_area(state: &Entity<TextareaState>, cx: &App) -> Textarea {
    Textarea::new(state).rounded(cx.theme().radius)
}
