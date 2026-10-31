use super::fold_key;
use super::metrics::{
    CODE_BLOCK_PAD, FOLD_INDENT, HEADING_1, HEADING_2, MAX_CODE_BLOCK_H, PARAGRAPH_GAP, TEXT,
    TEXT_SM, TIGHT_GAP,
};
use super::parts::{accent, copy_button, copy_turn_button, fold_box, fold_line, show_more};
use crate::chat::session::ChatSession;
use crate::tokens::LEADING;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Entity, HighlightStyle, IntoElement, ParentElement,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div, relative,
};
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::chat::{Md, Thought, TranscriptItemId, TurnAnswer};

// ── agent answer ────────────────────────────────────────────────────────────

/// One block of the agent's answer, and the footer closing its turn.
///
/// **A turn is one thing said by one speaker, and it is what the footer marks
/// — not this block.** An answer interrupted by three tool calls arrives as
/// four `Agent` items, so the model says which block closes a turn
/// (`Chat::turn_answer`) and only that one carries the footer.
pub(super) fn agent(
    session: &Entity<ChatSession>,
    md: &Md,
    target: TranscriptItemId,
    turn: Option<TurnAnswer>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    // Not while the turn is still arriving: a Copy offered mid-stream copies
    // whatever had landed by the click, silently.
    let footer = turn
        .as_ref()
        .filter(|t| t.is_last && !t.is_active)
        .map(TurnAnswer::closing_line);

    div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(md_view(session, md, window, cx))
        // **Always drawn, not waiting for the pointer**: a control that
        // appears under it is one nobody finds who was not already reaching
        // for it, and a footer that appears pushes the conversation down by
        // its own height as the reader crosses the answer.
        .children(footer.map(|closing| {
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(copy_turn_button(session, target, cx))
                .children(closing)
        }))
}

/// The parsed markdown for `md`.
///
/// Falls back to the raw source rather than rendering nothing: a block the
/// cache has not seen is a bug, but a *silent* one would be a transcript with
/// a hole in it.
pub(super) fn md_view(
    session: &Entity<ChatSession>,
    md: &Md,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    match session.read(cx).md_view(md) {
        Some(state) => TextView::new(state)
            .selectable(true)
            .style(prose_style(window, cx))
            // **The language, beside the copy**, in the one corner the
            // renderer opens on a block.
            .code_block_actions(|block, _, cx| {
                let code = block.code();
                div()
                    .h_flex()
                    .items_center()
                    .gap(TIGHT_GAP)
                    .children(block.lang().filter(|l| !l.trim().is_empty()).map(|lang| {
                        div()
                            .flex_none()
                            .text_xs()
                            .font_family(cx.theme().font_family.clone())
                            .text_color(cx.theme().muted_foreground)
                            .child(lang)
                    }))
                    .child(copy_button("copy-code", code.clone(), "Copy the code", cx))
                    // What the cap holds back, said and opened whole.
                    .children(super::code_view::held_back(&code).map(|lines| {
                        let lang = block.lang();
                        show_more("code-all", format!("All {lines} lines"), cx)
                            .tooltip("Open the whole block")
                            .on_click(move |_, window, cx| {
                                super::code_view::open(code.clone(), lang.clone(), window, cx)
                            })
                    }))
            })
            // **Copy and Open under every table**: the track scrolls a few
            // columns past the edge, the dialog shows a wide one at once.
            .table_actions(|table, _, cx| {
                let open = table.clone();
                div()
                    .h_flex()
                    .justify_end()
                    .gap(TIGHT_GAP)
                    .child(copy_button(
                        "copy-table",
                        table.markdown.clone(),
                        "Copy the table",
                        cx,
                    ))
                    .child(
                        show_more("table-all", "Open", cx)
                            .tooltip("Open the whole table")
                            .on_click(move |_, window, cx| {
                                super::table_view::open(&open, window, cx)
                            }),
                    )
            })
            .into_any_element(),
        None => div().child(md.source.clone()).into_any_element(),
    }
}

/// Prose styling for a transcript answer.
///
/// The height cap on code blocks is the bounded-rendering concern arriving by
/// a different route: `TextViewStyle::code_block` is one style for every block,
/// so per-block fold state has nowhere to live. Capping the height keeps the
/// answer readable; the corner's show-more opens a capped block whole, since it
/// cannot scroll inside the list.
///
/// **The rest is the renderer's defaults being wrong for a chat.** Its headings
/// are scaled off a base of its own choosing, and its code blocks are set from
/// an absolute pixel size that per-panel zoom cannot reach. The heading base is
/// taken from the *current* rem size, which is the zoomed one inside a zoomed
/// panel, and the code block's size is written in rems.
pub(super) fn prose_style(window: &Window, cx: &App) -> TextViewStyle {
    // Two steps up and one, then weight alone: an answer's headings are
    // section marks inside one message, not the top of a document.
    let mut style = TextViewStyle::default()
        .paragraph_gap(PARAGRAPH_GAP)
        .heading_font_size(|level, base| match level {
            1 => base * HEADING_1,
            2 => base * HEADING_2,
            _ => base,
        })
        // Keep inline code distinct without putting a full-line-height square
        // behind it. `Some(transparent)` is intentional: `None` makes TextView
        // restore its filled fallback. Colour alone marks it, because the
        // highlight carries no font family and weight already means bold.
        .inline_code(HighlightStyle {
            color: Some(crate::theme::hue_ink(cx.theme().blue, cx)),
            background_color: Some(cx.theme().transparent),
            ..HighlightStyle::default()
        })
        // A fenced block sits in the same well as a tool's output: the sunk
        // fill, no edge, the code size.
        .code_block(
            StyleRefinement::default()
                .max_h(MAX_CODE_BLOCK_H)
                .overflow_hidden()
                .p(CODE_BLOCK_PAD)
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .border_0()
                .text_size(crate::tokens::CODE_TEXT)
                .line_height(relative(LEADING)),
        )
        // A cell's sides stay the renderer's: it measures each column with
        // that padding, so a wider one breaks a short word ("change / d").
        .table_cell(StyleRefinement::default().py_1p5())
        .table(sideways());
    style.heading_base_font_size = window.rem_size() * TEXT.0;
    style
}

/// A table wider than the column scrolls sideways in its own track rather
/// than squeezing every column to fit: the renderer's scroll layout measures
/// each column by its text and stops shrinking at a floor. Only the sideways
/// axis is taken, so the vertical wheel still reaches the conversation.
fn sideways() -> StyleRefinement {
    let mut table = StyleRefinement::default();
    table.overflow.x = Some(gpui::Overflow::Scroll);
    table
}

// ── thought — the agent's reasoning, a block of its own ─────────────────────

/// *Thought for Ns* once done, or a spinner and *Thinking…* in the accent
/// while it runs; opened, the reasoning in the quiet ink.
pub(super) fn thought(
    session: &Entity<ChatSession>,
    th: &Thought,
    target: TranscriptItemId,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let open = th.is_open();
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_thought(target);
                cx.notify();
            });
        }
    };
    let line = fold_line(("thought", fold_key(target)), open, th.is_running(), cx).on_click(toggle);
    let line = match (th.is_running(), th.elapsed_secs) {
        (true, _) => line.text_color(accent(cx)).child("Thinking…"),
        (false, Some(secs)) => line.child(format!("Thought for {}", onehand_core::duration(secs))),
        (false, None) => line.child("Thought"),
    };

    fold_box().child(line).when(open, |block| {
        block.child(
            div()
                .w_full()
                .min_w_0()
                .pl(FOLD_INDENT)
                .text_size(TEXT_SM)
                // What the agent did, a step under what it said: the
                // middle ink, readable at length.
                .text_color(crate::theme::meta_ink(cx))
                .child(md_view(session, &th.md, window, cx)),
        )
    })
}
