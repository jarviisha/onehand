//! A session's resume picker: which of the agent's past conversations in the
//! project to open it on, or a fresh one.

use super::{ChatPane, rel_time};
use crate::chat::conversation::Conversation;
use crate::controls::Short as _;
use crate::tokens::PAGE_NARROW;
use gpui::{
    App, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::chat::ConvMeta;

/// How many past conversations a session's resume picker offers before the
/// older ones are counted instead.
const RESUME_ROWS: usize = 30;

impl ChatPane {
    /// The past conversations `uid` is choosing between, or nothing if it is
    /// not choosing.
    fn choices_of(&self, uid: u64) -> Vec<ConvMeta> {
        self.conversations
            .get(&uid)
            .and_then(Conversation::choices)
            .unwrap_or_default()
            .to_vec()
    }

    /// The resume picker: past conversations for this root + agent, newest
    /// first, plus the option to start fresh.
    pub(super) fn resume_picker(
        &mut self,
        uid: u64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let past = self.choices_of(uid);
        let older = past.len().saturating_sub(RESUME_ROWS);
        let now = onehand_core::chat::now_secs();

        div()
            .size_full()
            .v_flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .w_full()
                    .max_w(PAGE_NARROW)
                    // Bounded by the panel, for the same reason the project
                    // page's column is: this list is every conversation the
                    // agent has had in the project, and a column taller than
                    // the panel is centred into rows nothing can reach.
                    .max_h_full()
                    .min_h_0()
                    .child(div().font_semibold().child("Resume a conversation"))
                    // The rows are what grows, so the rows are what scrolls. The
                    // heading above and the way out below stay where they are --
                    // a picker whose *Start a new conversation* scrolls off the
                    // bottom is a screen with no way out of it.
                    .child(
                        div()
                            .id("resume-choices")
                            .v_flex()
                            .gap_3()
                            .w_full()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(past.into_iter().take(RESUME_ROWS).enumerate().map(
                                |(i, meta)| {
                                    // The agent is not named here: this picker belongs to a
                                    // session that already has one, and every row in it was
                                    // run by that same agent.
                                    let subtitle = format!(
                                        "{} · {} items",
                                        rel_time(now, meta.updated),
                                        meta.item_count
                                    );
                                    conversation_card(
                                        ("resume", i),
                                        meta.title.clone().into(),
                                        subtitle.into(),
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        move |pane: &mut Self, _, _, cx| {
                                            let meta = pane.choices_of(uid).get(i).cloned();
                                            pane.start(uid, meta, cx);
                                        },
                                    ))
                                },
                            )),
                    )
                    .children((older > 0).then(|| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{older} older not shown"))
                    }))
                    .child(
                        crate::controls::action("resume-fresh")
                            .short()
                            .primary()
                            .label("Start a new conversation")
                            .on_click(cx.listener(move |pane: &mut Self, _, _, cx| {
                                pane.start(uid, None, cx);
                            })),
                    ),
            )
    }
}

/// One archived conversation, as a card that can be picked: its name over
/// the line the caller supplies, the caller hanging its own click on it.
fn conversation_card(
    id: impl Into<ElementId>,
    title: SharedString,
    subtitle: SharedString,
    cx: &App,
) -> Stateful<Div> {
    onehand_ui::row(false, cx)
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .py_1p5()
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .child(div().truncate().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
}
