use super::fold_key;
use super::metrics::{
    DETAIL_INSET, FENCE_LEADING, FENCE_TEXT, FRAME_PAD, HAIR_GAP, OBJECT_TEXT, PARAGRAPH_GAP,
    PART_GAP, ROW_PAD_X, STACK_GAP, TEXT, TEXT_PAD_X, TEXT_PAD_Y, TIGHT_GAP, radius_block,
};
use super::parts::{ActivityRow, Object, RowMark, activity_row, copy_button, copy_turn_button};
use super::strip::group_icon;
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Entity, HighlightStyle, InteractiveElement, IntoElement, ParentElement, Rems,
    StyleRefinement, Styled, Window, div, relative, rems,
};
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::chat::activity;
use onehand_core::chat::{Md, Thought, TranscriptItemId, TurnAnswer};

/// Height a fenced code block in prose is allowed before it scrolls inside
/// itself instead of pushing the rest of the answer off screen.
const MAX_CODE_BLOCK_H: Rems = rems(22.5);

// ── agent answer ────────────────────────────────────────────────────────────

/// One block of the agent's answer, wearing the turn's chrome at its two ends.
///
/// **A turn is one thing said by one speaker, and it is what the footer marks
/// — not this block.** An answer interrupted by three tool calls arrives as
/// four `Agent` items, so a Copy on each would copy a quarter of what it looks
/// like it copies. The model knows which block closes a turn
/// (`Chat::turn_answer`), so the footer goes on the last and Copy takes the
/// whole turn's prose rather than this fragment of it.
pub(super) fn agent(
    session: &Entity<ChatSession>,
    md: &Md,
    target: TranscriptItemId,
    turn: Option<TurnAnswer>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    // Everything the footer needs, resolved before the borrow ends.
    let footer = turn.as_ref().and_then(|t| {
        // Not while the turn is still arriving: a Copy offered mid-stream
        // copies whatever had landed by the click, silently.
        (t.is_last && !t.is_active).then_some(t.elapsed_secs)
    });

    div()
        .v_flex()
        .gap(TIGHT_GAP)
        .w_full()
        .group("turn-footer")
        .child(md_view(session, md, window, cx))
        .children(footer.map(|elapsed| {
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap(PART_GAP)
                .w_full()
                // **The row holds its height whether or not anything is in
                // it.** Appearing under the pointer it would push the whole
                // conversation down by its own height every time the reader
                // crossed the last paragraph of an answer, which is a page
                // that moves while it is being read.
                .h(rems(1.75))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .invisible()
                .group_hover("turn-footer", |row| row.visible())
                // What can be done with the answer, at the end the reading
                // stopped at; what the answer *was*, opposite it. A control and
                // a record are two different offers and the row is read from
                // the left.
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(HAIR_GAP)
                        .flex_none()
                        .child(copy_turn_button(session, target).tooltip("Copy this answer")),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(TEXT_PAD_Y)
                        .min_w_0()
                        .truncate()
                        .children(elapsed.map(|secs| div().child(format!("Processed in {secs}s")))),
                )
        }))
}

/// The parsed markdown for `md`.
///
/// Falls back to the raw source rather than rendering nothing: a block the
/// cache has not seen is a bug, but a *silent* one would be a transcript with
/// a hole in it.
fn md_view(
    session: &Entity<ChatSession>,
    md: &Md,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    match session.read(cx).md_view(md) {
        Some(state) => TextView::new(state)
            .selectable(true)
            .style(prose_style(window, cx))
            // **The language, beside the copy.** A block that does not say what
            // it is leaves a reader guessing from the syntax -- and this
            // floating corner is the only slot the renderer opens, so it is
            // where the label has to go until the block is ours to lay out.
            .code_block_actions(|block, _, cx| {
                div()
                    .h_flex()
                    .items_center()
                    .gap(TIGHT_GAP)
                    .children(block.lang().filter(|l| !l.trim().is_empty()).map(|lang| {
                        div()
                            .flex_none()
                            .pl(TIGHT_GAP)
                            .text_size(OBJECT_TEXT)
                            .text_color(cx.theme().muted_foreground)
                            .child(lang)
                    }))
                    .child(copy_button("copy-code", block.code()))
            })
            .into_any_element(),
        None => div().child(md.source.clone()).into_any_element(),
    }
}

/// Prose styling for a transcript answer.
///
/// The height cap on code blocks is the bounded-rendering concern arriving by
/// a different route. The model can bound a long block by *folding* it, keyed
/// by fence-open order — but `TextViewStyle::code_block` is one style for
/// every block, so per-block fold state has nowhere to live — reaching it would mean
/// replacing gpui-component's code-block renderer through a custom block
/// parser, trading away its syntax highlighting to get a chevron. Capping the
/// height keeps the answer readable, which is what the fold was for; Copy on
/// the block is how the clipped tail stays reachable. The model carries no
/// fold state for it to key against.
///
/// **The rest of this is the renderer's defaults being wrong for a chat.**
/// `TextView` is a document renderer: its headings are scaled off a base of its
/// own choosing, and its code blocks are set from `Theme::mono_font_size`, an
/// absolute pixel value. Left alone that gives an answer whose `####` prints
/// *smaller* than the paragraph it names, whose `#` prints as a document title
/// inside a chat message, and whose code blocks are the one thing on screen
/// that per-panel zoom cannot reach — because zoom works by overriding the rem
/// base, and a pixel size is exactly what ignores it.
///
/// Both are reachable from here. The heading base is taken from the *current*
/// rem size, which is the zoomed one inside a zoomed panel, so headings scale
/// with the prose they belong to; the code block's size is written in rems, and
/// lands because the refinement is applied after the renderer's own text size.
fn prose_style(window: &Window, cx: &App) -> TextViewStyle {
    // Two steps up and one, then nothing: an answer's headings are section
    // marks inside one message, not the top of a document. Below the third
    // level, weight alone carries the hierarchy -- which is also what stops a
    // deep heading printing smaller than its own body text.
    let mut style = TextViewStyle::default()
        .paragraph_gap(PARAGRAPH_GAP)
        .heading_font_size(|level, base| match level {
            1 => base * 1.5,
            2 => base * 1.25,
            _ => base,
        })
        // Keep inline code distinct without putting a full-line-height square
        // behind it. `Some(transparent)` is intentional: `None` makes TextView
        // restore its accent-background fallback.
        //
        // **Colour alone, not colour and weight.** Mono is what would normally
        // mark this and the renderer cannot reach it -- inline code is styled
        // through a highlight that carries colour, weight, slant and background
        // and no font family -- so one substitute channel is chosen rather than
        // stacking two. Weight was the one dropped: a sentence naming five
        // symbols came out patched with semibold runs that read as the
        // markdown's own bold, which is a distinction prose actually uses.
        .inline_code(HighlightStyle {
            color: Some(crate::theme::hue_ink(cx.theme().blue, cx)),
            background_color: Some(cx.theme().transparent),
            ..HighlightStyle::default()
        })
        // The fenced block drawn as the wells around it are: the same corner,
        // the same edge, the same two insets. The renderer's own is a filled
        // box with no border at the control radius, which put a quoted command
        // in an answer and the identical command in the tool card below it in
        // two different boxes.
        .code_block(
            StyleRefinement::default()
                .max_h(MAX_CODE_BLOCK_H)
                .overflow_hidden()
                .py(FRAME_PAD)
                .px(TEXT_PAD_X)
                .rounded(radius_block(cx))
                .border_1()
                .border_color(cx.theme().border)
                // **Nothing behind it, which is the point.** The renderer fills
                // a fenced block with the well step, and the user's own bubble
                // is filled too -- so a quotation and a thing somebody said
                // came out as the same object at a glance. A filled surface in
                // the transcript now means one thing only: this was typed by
                // the person reading it. Everything else is an edge on the
                // reading surface, which is the language the activity block
                // already speaks.
                .bg(cx.theme().transparent)
                .text_size(FENCE_TEXT)
                .line_height(relative(FENCE_LEADING)),
        )
        // A table is read across, so its cells are wider than they are tall:
        // padding a cell evenly leaves the columns crowded and the rows loose,
        // which is the one way round a table must not be.
        .table_cell(StyleRefinement::default().py(STACK_GAP).px(TEXT_PAD_Y));
    style.heading_base_font_size = window.rem_size() * TEXT.0;
    style
}

// ── thought — collapsed reasoning, never contains tool calls ────────────────

pub(super) fn thought(
    session: &Entity<ChatSession>,
    th: &Thought,
    target: TranscriptItemId,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    // **One row, whether it is still thinking or has finished.** The verb and
    // the mark change; nothing moves. It used to be a bare line at one height
    // when it stood alone and a row at another inside a group, so the same
    // block was two shapes depending on what happened to be next to it.
    let (verb, summary, mark) = match th.elapsed_secs {
        Some(secs) => ("Reasoned", format!("{secs}s"), RowMark::Done),
        None => ("Reasoning", String::new(), RowMark::Running),
    };
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_thought(target);
                cx.notify();
            });
        }
    };

    let row = ActivityRow::new(
        ("thought", fold_key(target)).into(),
        mark,
        group_icon(activity::ActivityGroup::Reasoned),
        verb,
    )
    .object((!summary.is_empty()).then(|| Object::plain(summary)))
    .fold(Some(th.expanded));

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .when(th.expanded, |block| {
            block.child(
                div()
                    .w_full()
                    .min_w_0()
                    .pl(DETAIL_INSET)
                    .pr(ROW_PAD_X)
                    .pb(FRAME_PAD)
                    .text_color(cx.theme().muted_foreground)
                    .child(md_view(session, &th.md, window, cx)),
            )
        })
}
