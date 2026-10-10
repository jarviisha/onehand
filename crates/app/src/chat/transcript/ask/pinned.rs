//! The shape every card pinned above the composer shares: what it asks and who
//! asks it, the body, then a footer of answers.

use gpui::{AnyElement, App, Keystroke, ParentElement, Rems, SharedString, Styled, div, rems};
use gpui_component::kbd::Kbd;
use gpui_component::{ActiveTheme, Icon, Sizable as _, StyledExt};

/// The card's inset: its `p_3`, named because the scrolling body has to reach
/// back through the right-hand side of it to put its thumb on the card's edge.
pub(in crate::chat::transcript) const CARD_INSET: Rems = rems(0.75);

/// A pinned card's parts.
pub(in crate::chat::transcript) struct Pinned {
    /// The kind of thing asked, in the ink of what waits on the person.
    pub(in crate::chat::transcript) icon: Icon,
    /// The question itself.
    pub(in crate::chat::transcript) title: SharedString,
    /// Who asks, on a line of its own so a long question keeps the width.
    pub(in crate::chat::transcript) meta: SharedString,
    pub(in crate::chat::transcript) body: Vec<AnyElement>,
    /// `None` on a card that takes no answer: one replayed from the archive,
    /// or the quick question that answers on the click.
    pub(in crate::chat::transcript) footer: Option<Footer>,
}

/// The answers, at the footer's right, the primary last.
pub(in crate::chat::transcript) struct Footer {
    pub(in crate::chat::transcript) actions: Vec<AnyElement>,
}

/// A pinned card: the floating fill, a control's edge and the app's lift, since
/// it floats over the transcript.
///
/// **The buttons wrap before a label is cut**: a truncated *Always allow* is a
/// grant nobody can read the reach of.
pub(in crate::chat::transcript) fn pinned_card(card: Pinned, cx: &App) -> gpui::Div {
    let warning = crate::theme::status_ink(cx).warning;
    div()
        .v_flex()
        .w_full()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().input)
        // Opaque: the transcript runs underneath, and text showing through a
        // box that is asking a question is the one place that cannot afford it.
        .bg(cx.theme().popover.alpha(1.))
        .shadow(crate::theme::lift(cx))
        // The footer's rule runs edge to edge, so the corners clip it.
        .overflow_hidden()
        .child(
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .p(CARD_INSET)
                .child(
                    div()
                        .h_flex()
                        .items_start()
                        .gap_2()
                        .w_full()
                        .child(card.icon.small().flex_none().text_color(warning))
                        .child(
                            div()
                                .v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .font_medium()
                                        .text_color(cx.theme().foreground)
                                        .child(card.title),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(card.meta),
                                ),
                        ),
                )
                .children(card.body),
        )
        .children(card.footer.map(|footer| {
            div()
                .h_flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .w_full()
                .px(CARD_INSET)
                .py_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .justify_end()
                .children(footer.actions)
        }))
}

/// One key as the library draws a key cap.
pub(in crate::chat::transcript) fn key_cap(key: &str) -> Option<Kbd> {
    Keystroke::parse(key).ok().map(Kbd::new)
}
