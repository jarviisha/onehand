//! The transcript: one element per [`ChatItem`].
//!
//! Follows the design language's **structure** — the block types, the user's
//! right-hand bubble against the agent's full-width lane —
//! while every colour, radius and size comes from
//! `cx.theme()`. That split is deliberate: the component library's theme is
//! this app's look, so no token table is carried anywhere and nothing here can
//! drift away from the rest of the window.
//!
//! Rendering stays **bounded**. Diffs and command output
//! draw one element per line, so an unbounded result would freeze the frame.
//! The caps below are correctness, not tuning.

use super::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Axis, ClickEvent, Entity, HighlightStyle, InteractiveElement, IntoElement, ParentElement,
    Rems, RenderOnce, ScrollHandle, SharedString, StatefulInteractiveElement, StyleRefinement,
    Styled, Window, div, relative, rems,
};
use gpui_component::Disableable as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::{ScrollableMask, Scrollbar, ScrollbarMode};
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::acp::{
    ElicitKind, PermissionWeight, PlanStatus, ToolContent, ToolKind, ToolStatus,
};
use onehand_core::chat::activity;
use onehand_core::chat::{
    AskItem, ChatItem, Md, NoticeLevel, PermItem, PlanItem, Thought, ToolItem, TranscriptItemId,
    TurnAnswer, UserMsg,
};
use onehand_core::diff::Row as DiffRow;
use std::path::Path;

/// Diff lines drawn per tool card, **shared across all its hunks** — a
/// MultiEdit touching twenty files must not cost twenty times the budget.
const MAX_DIFF_LINES: usize = 200;
/// Lines of a mono output well before the tail is dropped.
const MAX_MONO_LINES: usize = 60;
/// Lines a tool's output well shows before it folds behind "Show N more".
const MONO_FOLD_LINES: usize = 12;
/// Lines of live terminal output kept on screen (the tail).
const MAX_TERM_LINES: usize = 40;
/// Plan entries drawn before the list is truncated.
const MAX_TODO_ITEMS: usize = 50;
/// Attachment rows drawn under a prompt before the rest are counted.
const MAX_ATTACHMENT_ROWS: usize = 8;
/// Consecutive quiet steps below this never become a strip: folding one row
/// behind a disclosure hides it without saving anything.
const MIN_ACTIVITY_RUN: usize = 2;
/// Height a fenced code block in prose is allowed before it scrolls inside
/// itself instead of pushing the rest of the answer off screen.
const MAX_CODE_BLOCK_H: Rems = rems(22.5);
/// Height one machine-detail card may occupy before its contents scroll inside
/// the card instead of lengthening the transcript.
const MAX_TOOL_DETAIL_H: Rems = rems(18.);
/// The size every well of machine text is set at — a tool's output, a diff, a
/// live terminal, and the fenced blocks inside an answer.
///
/// **One size, because they are one thing.** All four are "a machine produced
/// this", and the two that arrived by different routes had drifted apart: the
/// wells this file draws were a step below the meta labels naming them, while
/// the markdown renderer set its blocks from a fixed pixel size of its own. The
/// same command run by the agent therefore read at one size in a tool card and
/// another when quoted back in prose.
const CODE_TEXT: Rems = rems(0.8125);
/// The transcript's own reading size, one step under the app's base.
///
/// A conversation is read the way a page is, not the way a form is filled in:
/// it is long, it is mostly prose, and the eye travels down it rather than
/// stopping at each field. Set at the app's base — a size chosen for labels and
/// controls that have to be hit — a long answer is a wall. One step down fits
/// more of a thought in one glance and leaves the *controls* around it, which
/// are still at base size, reading as the larger things they are.
///
/// Applied where a run is framed, so every block gets it at once, and fed to
/// the markdown renderer's heading base as well: headings scaled off the app's
/// base while the body sat a step under it would print a third-level heading
/// larger than the prose it names for no reason the reader can see.
pub(super) const TEXT: Rems = rems(0.875);
/// The transcript's second voice: the record of how an answer got made.
///
/// A tool card, a plan and a blocking card's secondary line are *about* the
/// work rather than part of it, and size is what tells the two voices apart.
/// That distinction is the one thing the reading size above cost: it used to be
/// a step under the app's base, which the reading size then dropped to as well,
/// so an answer and the tool card beside it came out identical. This is the
/// step put back, under the new reading size rather than under the old one.
///
/// It lands on the same number as the wells of machine text and stays a
/// separate decision from them: a card's header is chrome around output, not
/// output, and the two are free to move apart. Nothing else marks them the
/// same — a well is mono, tinted and padded, a card header is none of those.
const WORK_TEXT: Rems = rems(0.8125);
/// Leading for those wells, as a multiple of the size above.
///
/// Prose is set at the golden ratio, which is right for a paragraph and wrong
/// for a diff: two hundred lines each carrying two thirds of a blank line is a
/// column of half-empty rows the eye cannot track down. Tight enough to read as
/// a block, loose enough to separate the lines.
const CODE_LEADING: f32 = 1.45;
/// The padding those wells carry, whether quoted in prose or drawn as a tool's
/// output.
const WELL_PAD: Rems = rems(0.625);
/// Space between the paragraphs of one answer.
///
/// **Bounded by the space between whole blocks**, which is the rule this had
/// been breaking: at the renderer's own default two paragraphs of one answer
/// stood further apart than the answer stood from the tool card beneath it, so
/// the inside of a turn was louder than the transcript's own rhythm.
const PARAGRAPH_GAP: Rems = rems(0.75);
/// Width of the `IN` / `OUT` / `EDIT` tag column.
///
/// One step of the nesting scale, which is also what the tag has to clear: the
/// longest of the four is four characters at the meta size.
const SECTION_TAG_W: Rems = rems(2.5);
/// Start the `IN` / `OUT` / `EDIT` tag column where the activity label starts:
/// after the row's icon slot and the gap following it.
const ACTIVITY_DETAIL_INSET: Rems = rems(1.5);
/// Width a question's tab label is elided at. Only a *tab* is ever elided.
const ASK_TAB_W: Rems = rems(8.75);
/// The marker a pending plan entry draws, in place of an icon.
const PLAN_DOT: Rems = rems(0.3125);
/// Height an attached image's thumbnail is bounded to under a prompt.
const ATTACHMENT_THUMB_H: Rems = rems(9.);
/// How much of the row a user prompt may take before it wraps.
///
/// The prompt is the one block that shrinks to what was typed: a one-line
/// question stretched edge to edge is indistinguishable from an answer, and
/// telling the two apart at a glance is the whole reason it sits on the other
/// side. Past this it wraps rather than crowding the answer beneath it.
const USER_BUBBLE_MAX: f32 = 0.8;

/// The bounded detail card beneath one leaf activity label.
///
/// Groups are only collections of these leaf rows and never own a fixed-height
/// viewport. The card keeps a persistent scroll handle and a visible thumb
/// whenever its own detail exceeds the cap.
#[derive(IntoElement)]
struct ActivityDetail {
    target: TranscriptItemId,
    children: Vec<gpui::AnyElement>,
}

impl ActivityDetail {
    fn new(target: TranscriptItemId, children: Vec<gpui::AnyElement>) -> Self {
        Self { target, children }
    }
}

impl RenderOnce for ActivityDetail {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let target = fold_key(self.target);
        let state_id = ("activity-detail-scroll-state", target);
        let scroll = window
            .use_keyed_state(state_id, cx, |_, _| ScrollHandle::default())
            .read(cx)
            .clone();

        div()
            .id(("activity-detail-frame", target))
            .relative()
            .w_full()
            .max_h(MAX_TOOL_DETAIL_H)
            .child(
                div()
                    .id(("activity-detail-scroll", target))
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .max_h(MAX_TOOL_DETAIL_H)
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .p_3()
                    // The thumb overlays the viewport; a wider right inset
                    // keeps section content out from underneath it.
                    .pr_6()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .children(self.children),
            )
            // The mask handles vertical wheel input in the capture phase. A
            // bubble listener runs too late inside `gpui::list`: the parent
            // transcript has already consumed the same delta by then.
            .child(
                ScrollableMask::new(Axis::Vertical, &scroll).id(("activity-detail-mask", target)),
            )
            .child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
    }
}

/// Render one transcript item.
///
/// `target` addresses fold toggles back into the model, and carries whether the
/// item came from the read-only resumed history or the live tail: history and
/// live items are two collections, so a fold addressed by render position lands
/// in the wrong one.
pub fn item(
    session: &Entity<ChatSession>,
    it: &ChatItem,
    target: TranscriptItemId,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let chat = &session.read(cx).chat;
    // Only an answer has a footer, and only its turn's last one draws it.
    let turn = matches!(it, ChatItem::Agent(_))
        .then(|| chat.turn_answer(target))
        .flatten();
    let body = match it {
        ChatItem::User(u) => user(u, cx).into_any_element(),
        ChatItem::Agent(md) => agent(session, md, target, turn, window, cx).into_any_element(),
        ChatItem::Thought(th) => thought(session, th, target, window, cx).into_any_element(),
        ChatItem::Tool(t) => tool(session, t, target, cx).into_any_element(),
        ChatItem::Plan(p) => plan(session, p, target, cx).into_any_element(),
        ChatItem::Permission(p) => permission(session, p, target, cx).into_any_element(),
        ChatItem::Ask(a) => ask(session, a, target, cx).into_any_element(),
        ChatItem::Notice { text, level } => notice(text, *level, cx).into_any_element(),
    };

    // Width is owned by the pane's run so an activity summary drawn by the
    // pane and the steps rendered here always share the same two edges.
    div().w_full().min_w_0().child(body)
}

// ── user prompt — filled, shrink-to-fit, against the right edge ─────────────

/// What the user asked, on its own side of the column.
///
/// **The side is the label.** Every other block in the transcript is the
/// agent's, so the one thing a reader scanning back through a long
/// conversation is looking for — where they last asked something — is also the
/// only thing that has to be findable without reading. A fill alone said that
/// when the transcript was two blocks long; twenty blocks down it is one more
/// box among boxes, while an edge is still an edge.
fn user(u: &UserMsg, cx: &App) -> impl IntoElement + use<> {
    div().h_flex().w_full().justify_end().child(
        div()
            .v_flex()
            .gap_2()
            .max_w(relative(USER_BUBBLE_MAX))
            .p_3()
            .rounded(cx.theme().radius * 2.)
            .bg(cx.theme().secondary)
            .text_color(cx.theme().secondary_foreground)
            .child(u.text.clone())
            .children(
                u.attachments
                    .iter()
                    .take(MAX_ATTACHMENT_ROWS)
                    .map(|a| attachment(a, cx)),
            )
            .when(u.attachments.len() > MAX_ATTACHMENT_ROWS, |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "+{} more attachment(s)",
                            u.attachments.len() - MAX_ATTACHMENT_ROWS
                        )),
                )
            }),
    )
}

/// One attached file: what it is, and — for a picture — what it looks like.
///
/// **The prompt is what the user wrote plus what they handed over, and "3
/// attachment(s)" is neither.** A count cannot be checked against what was
/// meant to be sent, so the mistake it hides — the wrong screenshot — reads as
/// correct right up until the answer is about the wrong picture. The name can
/// be wrong *visibly*; the thumbnail can be wrong at a glance.
///
/// The image is addressed **by path**, which is what makes it affordable here:
/// gpui loads and caches a path-sourced image off the UI thread, so a row
/// redrawn on every streamed chunk costs a cache lookup rather than a decode.
/// The archive keeps paths and not bytes, so this is also the only form the
/// picture still exists in once the conversation is reopened — and a file that
/// has since moved simply leaves the row as its name.
fn attachment(a: &onehand_core::attachment::AttachmentSnapshot, cx: &App) -> impl IntoElement {
    use onehand_core::attachment::{AttachmentDelivery, AttachmentKind};
    let unavailable = a.delivery == AttachmentDelivery::Unavailable;
    // Nothing to show for a file that was never sent: the thumbnail would say
    // the agent saw this picture.
    let thumbnail = (a.kind == AttachmentKind::Image && !unavailable).then(|| a.path.clone());

    div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    Icon::new(match a.kind {
                        // The registry has no picture glyph; the plain file
                        // mark is the honest stand-in, and for an image the
                        // thumbnail below says it better than an icon could.
                        AttachmentKind::Image => IconName::File,
                        AttachmentKind::File => IconName::File,
                    })
                    .size_3(),
                )
                .child(div().flex_1().min_w_0().truncate().child(a.name.clone()))
                // An attachment the agent never received is the one thing about
                // this row that changes the answer, so it is spelled out.
                .when(unavailable, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_color(crate::theme::status_ink(cx).danger)
                            .child("not sent"),
                    )
                }),
        )
        .children(thumbnail.map(|path| {
            gpui::img(path)
                .max_h(ATTACHMENT_THUMB_H)
                .max_w_full()
                .rounded(cx.theme().radius)
        }))
}

// ── agent answer ────────────────────────────────────────────────────────────

/// One block of the agent's answer, wearing the turn's chrome at its two ends.
///
/// **A turn is one thing said by one speaker, and it is what the footer marks
/// — not this block.** An answer interrupted by three tool calls arrives as
/// four `Agent` items, so a Copy on each would copy a quarter of what it looks
/// like it copies. The model knows which block closes a turn
/// (`Chat::turn_answer`), so the footer goes on the last and Copy takes the
/// whole turn's prose rather than this fragment of it.
fn agent(
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
        .gap_1()
        .w_full()
        .child(md_view(session, md, window, cx))
        .children(footer.map(|elapsed| {
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .children(elapsed.map(|secs| div().child(format!("Processed in {secs}s"))))
                .child(div().flex_1())
                .child(copy_turn_button(session, target).tooltip("Copy this answer"))
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
            .code_block_actions(|block, _, _| copy_button("copy-code", block.code()))
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
/// the block is how the clipped tail stays reachable. (`Md::open_blocks` and `Chat::toggle_code`
/// are still in the model for whenever that trade looks worth making.)
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
        .code_block(
            StyleRefinement::default()
                .max_h(MAX_CODE_BLOCK_H)
                .overflow_hidden()
                .p(WELL_PAD)
                .text_size(CODE_TEXT)
                .line_height(relative(CODE_LEADING)),
        );
    style.heading_base_font_size = window.rem_size() * TEXT.0;
    style
}

/// The shared tinted well for machine detail, in the mono family at the one
/// size all of them share. The enclosing activity detail owns the border and
/// scroll; `IN` / `OUT` wells only distinguish sections inside that card.
///
/// The markdown renderer's fenced blocks receive the shared padding and
/// typography through a style refinement, which is the only way to reach their
/// library-owned container.
fn well(cx: &App) -> gpui::Div {
    div()
        .w_full()
        .p(WELL_PAD)
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(CODE_TEXT)
        .line_height(relative(CODE_LEADING))
}

/// Copy `text` to the clipboard, as a quiet icon button.
///
/// One constructor for both the copies the transcript offers — a fenced block
/// and a whole answer — because they are the same gesture and the only reason
/// they ever looked different was that they were written months apart.
fn copy_button(id: &'static str, text: impl Into<SharedString>) -> Button {
    let text = text.into();
    crate::controls::action(id)
        .ghost()
        .xsmall()
        .icon(Icon::new(IconName::Copy))
        .on_click(move |_, _, cx: &mut App| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
        })
}

/// Copy the whole turn `target` belongs to, gathered **when the button is
/// clicked**.
///
/// The eager form above is right for a fenced block, whose text the caller
/// already holds. A turn's prose is a join of every agent block in it — the
/// length of the answer, built from scratch — and building that on every redraw
/// is the length of the answer per frame, to have it ready in case a button is
/// pressed.
fn copy_turn_button(session: &Entity<ChatSession>, target: TranscriptItemId) -> Button {
    let session = session.clone();
    crate::controls::action("copy-answer")
        .ghost()
        .xsmall()
        .icon(Icon::new(IconName::Copy))
        .on_click(move |_, _, cx: &mut App| {
            let prose = session.read(cx).chat.turn_prose(target);
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(prose));
        })
}

// ── thought — collapsed reasoning, never contains tool calls ────────────────

fn thought(
    session: &Entity<ChatSession>,
    th: &Thought,
    target: TranscriptItemId,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let label = match th.elapsed_secs {
        Some(secs) => format!("Thought for {secs}s"),
        None => "Thinking…".to_string(),
    };

    div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(ghost_row(
            IconName::Info,
            ("thought", fold_key(target)).into(),
            // Built here rather than through the one-label row, which is the
            // shape a *summary* takes: this line names a block, and a block's
            // name carries the weight every other one does. It stays in the
            // quiet size and colour, so the weight reads as a title inside the
            // chrome rather than as a second voice against the answer.
            div()
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .font_semibold()
                .child(label)
                .into_any_element(),
            None,
            Some(th.expanded),
            {
                let session = session.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    session.update(cx, |s, cx| {
                        s.chat.toggle_thought(target);
                        cx.notify();
                    });
                }
            },
            cx,
        ))
        .when(th.expanded, |block| {
            block.child(
                div()
                    .w_full()
                    .p_3()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .child(md_view(session, &th.md, window, cx)),
            )
        })
}

// ── tool call ───────────────────────────────────────────────────────────────

/// A tool step: a compact record once settled, a card while it needs attention.
///
/// **Which shape is not a style choice.** Status is the only input: terminal
/// work (completed or failed) is a quiet row, while pending and running work is
/// a card. A failure remains unmistakable through its danger status; a border
/// around the whole tool adds no further information.
fn tool(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let descriptor = tool_descriptor(t, &session.read(cx).chat.root);
    let quiet = is_settled(t);
    let show_kind = show_tool_kind(t);
    // Mono only when the descriptor *is* the command. An `Execute` step that
    // arrived with a description is being described in prose, and prose set in
    // mono claims to be something that could be typed back into a shell.
    let verbatim = t.call.kind == ToolKind::Execute && t.call.description.is_none();
    // What the agent actually ran, when the header is not already showing it.
    //
    // The header truncates to one line and a command is the one part of a tool
    // step that is worth reading in full — a `find` with four predicates, a
    // `sed` whose expression is the whole point — and the card had nowhere
    // else that carried it. With no `description` the header *is* the command,
    // truncated; with one, the command was not on screen at all.
    let command = (t.call.kind == ToolKind::Execute && !t.call.title.trim().is_empty())
        .then(|| t.call.title.clone());
    let open = t.is_open();
    let has_detail = !t.call.content.is_empty() || command.is_some();

    let sections = if open {
        let mut budget = MAX_DIFF_LINES;
        command
            .map(|cmd| labeled("IN", mono_well(&cmd, MAX_MONO_LINES, cx), cx).into_any_element())
            .into_iter()
            .chain(
                t.call.content.iter().enumerate().map(|(key, content)| {
                    section(session, t, target, key, content, &mut budget, cx)
                }),
            )
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_tool(target);
                cx.notify();
            });
        }
    };

    if quiet {
        return div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(ghost_row(
                tool_icon(t.call.kind),
                ("tool", fold_key(target)).into(),
                // Shrink-to-fit, not fill: this row's chevron belongs beside
                // what it opens. The descriptor still truncates rather than
                // running the row off the column -- it may shrink, it just does
                // not grow.
                div()
                    .h_flex()
                    .gap_2()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .h_flex()
                            // Only text participates in baseline alignment.
                            // Sans labels and mono commands have different font
                            // metrics; glyphs beside them belong to the row's
                            // centred icon grid instead.
                            .items_baseline()
                            .gap_2()
                            .min_w_0()
                            .overflow_hidden()
                            .children(
                                show_kind.then(|| div().flex_none().child(tool_label(t.call.kind))),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .when(verbatim, |d| {
                                        d.font_family(cx.theme().mono_font_family.clone())
                                    })
                                    .child(descriptor),
                            ),
                    )
                    .children(completed_mark(t.call.status, cx))
                    .into_any_element(),
                status_note(t.call.status, cx),
                has_detail.then_some(open),
                toggle,
                cx,
            ))
            .when(open && has_detail, |row| {
                // One card owns everything below this activity's label: its
                // tags, command, output, diffs and status. The card edge starts
                // on the label's axis, and its fixed viewport keeps opening one
                // activity from pushing the following rows out of sight.
                row.child(
                    div()
                        .w_full()
                        .pl(ACTIVITY_DETAIL_INSET)
                        .child(ActivityDetail::new(target, sections)),
                )
            })
            .into_any_element();
    }

    div()
        .v_flex()
        .gap_2()
        .w_full()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                // The same size the quiet shape of this step is drawn at. A
                // tool card is *how* the answer got made, not part of it, and
                // at the prose size it competed with the answer beside it --
                // while the very same step, drawn as a row inside a strip, sat
                // a step below. One step, two sizes, decided by nothing but
                // where it landed.
                .text_size(WORK_TEXT)
                .id(("tool", fold_key(target)))
                .when(has_detail, |header| {
                    header.cursor_pointer().on_click(toggle)
                })
                .child(Icon::new(tool_icon(t.call.kind)).size_4())
                .children(show_kind.then(|| {
                    div()
                        .font_semibold()
                        .flex_none()
                        .child(tool_label(t.call.kind))
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .when(verbatim, |d| {
                            d.font_family(cx.theme().mono_font_family.clone())
                        })
                        .child(descriptor),
                )
                .children(completed_mark(t.call.status, cx))
                .children(status_note(t.call.status, cx))
                .children(has_detail.then(|| {
                    Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size_4()
                })),
        )
        .children(sections)
        .into_any_element()
}

/// The part of a tool row after its semantic action.
///
/// Native file/search tools often repeat the action in both ACP's kind and its
/// title (`Edit Edit package.json`, `Read Read src/lib.rs`). Their structured
/// kind already lets core recover the actual subject, so use that. Execute and
/// unknown tools keep the agent-authored description because it is commonly a
/// useful task-level summary rather than a raw target.
fn tool_descriptor(t: &ToolItem, root: &Path) -> String {
    let presented = activity::presentation(t);
    let descriptor = match t.call.kind {
        ToolKind::Execute | ToolKind::Other => {
            t.call.description.as_deref().unwrap_or(&t.call.title)
        }
        _ if !presented.subject.trim().is_empty() => &presented.subject,
        _ => t.call.description.as_deref().unwrap_or(&t.call.title),
    };
    let descriptor = if show_tool_kind(t) {
        strip_action_prefix(descriptor, tool_label(t.call.kind))
    } else {
        descriptor.trim()
    };
    let descriptor = if matches!(
        t.call.kind,
        ToolKind::Edit | ToolKind::Read | ToolKind::Delete | ToolKind::Move
    ) {
        path_for_display(root, descriptor)
    } else {
        descriptor.to_string()
    };
    header_line(&descriptor)
}

/// A tool header is one line even when the raw command is a heredoc or a small
/// script. The exact newlines remain in the expandable `IN` body; the header is
/// only its compact index entry.
fn header_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn path_for_display(root: &Path, value: &str) -> String {
    let path = Path::new(value);
    path.strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// A human description already names an execute step (`Dump backend routes`,
/// `Rebuild after the fix`). Prefixing every one with `Run` adds no meaning and
/// makes a long audit read like a command log. Raw commands keep the kind label
/// because it is the only prose saying what that machine text is.
fn show_tool_kind(t: &ToolItem) -> bool {
    t.call.kind != ToolKind::Execute
        || t.call
            .description
            .as_deref()
            .is_none_or(|description| description.trim().is_empty())
}

fn is_settled(t: &ToolItem) -> bool {
    matches!(t.call.status, ToolStatus::Completed | ToolStatus::Failed)
}

fn strip_action_prefix<'a>(value: &'a str, action: &str) -> &'a str {
    let value = value.trim();
    let Some(head) = value.get(..action.len()) else {
        return value;
    };
    let Some(rest) = value.get(action.len()..) else {
        return value;
    };
    if head.eq_ignore_ascii_case(action)
        && rest
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || matches!(c, ':' | '—' | '-'))
    {
        rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '—' | '-'))
    } else {
        value
    }
}

/// One `IN` / `OUT` / `EDIT` body section.
fn section(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    key: usize,
    content: &ToolContent,
    diff_budget: &mut usize,
    cx: &App,
) -> gpui::AnyElement {
    match content {
        ToolContent::Text(text) => {
            labeled("OUT", out_well(session, t, target, key, text, cx), cx).into_any_element()
        }
        ToolContent::Diff { path, .. } => labeled(
            "EDIT",
            diff(
                session,
                key,
                path,
                t.diff_rows.get(&key).map(Vec::as_slice).unwrap_or_default(),
                diff_budget,
                cx,
            ),
            cx,
        )
        .into_any_element(),
        ToolContent::Terminal(id) => {
            // The live stream for this terminal, if it is still running; a
            // finished one has already been flattened into a `Text` section by
            // the model at turn end.
            let view = session.read(cx).chat.terminals.get(id);
            // Borrowed, not cloned: the buffer runs to tens of kilobytes and
            // only its last few lines are ever drawn, so copying it whole is
            // the whole output copied per redraw to read the tail of it.
            let body = view.map(|v| v.output.as_str()).unwrap_or_default();
            let exit = view.and_then(|v| v.exited.then_some(v.exit_code));
            labeled(
                "OUT",
                div()
                    .v_flex()
                    .gap_1()
                    .child(mono_well(body, MAX_TERM_LINES, cx))
                    .when_some(exit, |well, code| {
                        let code = code.unwrap_or(0);
                        well.child(
                            div()
                                .text_xs()
                                .text_color(if code == 0 {
                                    crate::theme::status_ink(cx).success
                                } else {
                                    crate::theme::status_ink(cx).danger
                                })
                                .child(format!("exited ({code})")),
                        )
                    }),
                cx,
            )
            .into_any_element()
        }
        // An image result, shown inline rather than described.
        ToolContent::Image(bytes) => labeled(
            "OUT",
            match session.read(cx).image(bytes) {
                Some(handle) => gpui::img(handle)
                    .max_w_full()
                    .rounded(cx.theme().radius)
                    .into_any_element(),
                // Unidentifiable bytes: say so rather than draw a broken frame.
                None => div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("[unrecognized image, {} bytes]", bytes.len()))
                    .into_any_element(),
            },
            cx,
        )
        .into_any_element(),
    }
}

/// A gutter tag plus its body: a fixed-width column naming what the section is
/// (`IN`, `OUT`, `EDIT`, `ERR`) beside the content itself, so a card holding
/// several kinds of output reads as one table rather than as stacked blocks.
fn labeled<E: IntoElement>(tag: &'static str, body: E, cx: &App) -> impl IntoElement + use<E> {
    div()
        .h_flex()
        .items_start()
        .gap_2()
        .w_full()
        .child(
            div()
                .w(SECTION_TAG_W)
                .flex_none()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(tag),
        )
        .child(div().flex_1().min_w_0().child(body))
}

/// A tool's text output: the tail, short by default, opened on request.
///
/// **Two bounds, and they answer different questions.** The fold is about
/// reading — twelve lines of a build log is enough to see what happened, and a
/// card that pours sixty lines into the transcript buries the answer that
/// follows it. The hard cap is about the frame: every line is an element, so
/// the well cannot be allowed to draw whatever the agent happened to `cat`.
/// Opening a well lifts the first bound and never the second, and what is
/// still cut is said on screen.
///
/// The fold state is the model's, keyed by this section's index within the
/// card (`ToolItem::out_open`), so it survives the turn's later updates and
/// the archive.
fn out_well(
    session: &Entity<ChatSession>,
    t: &ToolItem,
    target: TranscriptItemId,
    key: usize,
    blob: &str,
    cx: &App,
) -> impl IntoElement + use<> {
    let open = t.out_open.contains(&key);
    let cap = if open {
        MAX_MONO_LINES
    } else {
        MONO_FOLD_LINES
    };
    let (tail, hidden) = tail_lines(blob, cap);

    div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(mono_lines(tail, hidden, cx))
        // Nothing to reveal and nothing revealed: no control. A "Show 0 more"
        // is a button that has to be clicked to learn it does nothing.
        .when(hidden > 0 || open, |well| {
            well.child(
                // Spelled out rather than folded into one number: an id built
                // by arithmetic on two indices is an id that collides the day
                // one of them grows past whatever multiplier was assumed.
                crate::controls::action(SharedString::from(format!(
                    "out-fold-{}-{key}",
                    fold_key(target)
                )))
                .ghost()
                .xsmall()
                .label(if open {
                    "Show less".to_string()
                } else {
                    format!("Show {hidden} more line(s)")
                })
                .on_click({
                    let session = session.clone();
                    move |_, _, cx: &mut App| {
                        session.update(cx, |s, cx| {
                            s.chat.toggle_tool_output(target, key);
                            cx.notify();
                        });
                    }
                }),
            )
        })
}

/// The last `cap` lines of `blob`, and how many were left off the front.
///
/// **One pass.** Both answers come from the same walk, and the walk is over
/// whatever the agent happened to `cat` — a blob big enough for the count to
/// cost is exactly one big enough that counting it twice per redraw is felt.
fn tail_lines(blob: &str, cap: usize) -> (Vec<SharedString>, usize) {
    let mut tail: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
    let mut hidden = 0usize;
    for line in blob.lines() {
        tail.push_back(line);
        if tail.len() > cap {
            tail.pop_front();
            hidden += 1;
        }
    }
    (
        tail.into_iter()
            .map(|l| SharedString::from(l.to_string()))
            .collect(),
        hidden,
    )
}

/// Mono text, one row per line, capped. The tail is *reported*, never silently
/// dropped — a truncated well that looks complete is worse than a short one.
fn mono_well(blob: &str, cap: usize, cx: &App) -> impl IntoElement + use<> {
    let (lines, hidden) = tail_lines(blob, cap);
    mono_lines(lines, hidden, cx)
}

/// The well itself, once the lines to draw are known.
fn mono_lines(lines: Vec<SharedString>, hidden: usize, cx: &App) -> impl IntoElement + use<> {
    well(cx)
        .v_flex()
        .when(hidden > 0, |well| {
            well.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("… {hidden} earlier line(s) hidden")),
            )
        })
        .children(lines.into_iter().map(|l| div().child(l)))
}

/// A unified diff. `budget` is shared across every hunk of one card.
///
/// `hunks` are the ones core computed when the edit landed — ACP sends whole
/// files with no hunk offsets, so rendering the payload literally spent the
/// budget on the unchanged head of the file before ever reaching the edit.
///
/// `key` distinguishes this section from the card's others for GPUI's element
/// ids; it is the section's index, which is unique within the one parent that
/// scopes them.
fn diff(
    session: &Entity<ChatSession>,
    key: usize,
    path: &str,
    hunks: &[DiffRow],
    budget: &mut usize,
    cx: &App,
) -> impl IntoElement + use<> {
    let status = crate::theme::status_ink(cx);
    let (removed, added) = (status.danger, status.success);
    let context = cx.theme().muted_foreground;
    let display_path = path_for_display(&session.read(cx).chat.root, path);
    let mut rows = Vec::new();

    let mut push = |sign: &'static str, line: &str, color: gpui::Hsla, rows: &mut Vec<_>| {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        rows.push(
            div()
                .text_color(color)
                .child(format!("{sign}{line}"))
                .into_any_element(),
        );
    };

    for line in hunks {
        match line {
            DiffRow::Context(l) => push(" ", l, context, &mut rows),
            DiffRow::Removed(l) => push("-", l, removed, &mut rows),
            DiffRow::Added(l) => push("+", l, added, &mut rows),
            // The elided run is stated, not dropped: an unmarked gap reads as
            // adjacent lines and hides that the file is longer than the card.
            DiffRow::Skipped(n) => push("", &format!("… {n} unchanged"), context, &mut rows),
        }
    }

    well(cx)
        .v_flex()
        .child(
            // The path header is the way into the editor: reviewing a diff and
            // wanting to touch it up is one motion, not two.
            div()
                // Keyed by the section's position in the card, not by the
                // path's *length* -- two diffs whose paths happened to be the
                // same number of characters shared one id.
                .id(("diff-path", key))
                .cursor_pointer()
                .text_color(context)
                .hover(|header| header.text_color(cx.theme().foreground))
                .child(display_path)
                .on_click({
                    // Resolved here, where the session -- and so the project
                    // root -- is known. The Workbench opens whatever path it is
                    // handed, and a relative one there resolves against the
                    // process working directory.
                    let path =
                        onehand_core::parse::resolve_in_root(&session.read(cx).chat.root, path);
                    let session = session.clone();
                    move |_, _, cx: &mut App| {
                        session.update(cx, |_, cx| {
                            cx.emit(super::session::ChatEvent::OpenFile(path.clone()))
                        });
                    }
                }),
        )
        .children(rows)
        .when(*budget == 0, |well| {
            well.child(
                div()
                    .text_color(context)
                    .child(format!("… diff truncated at {MAX_DIFF_LINES} lines")),
            )
        })
}

// ── plan / TodoWrite ────────────────────────────────────────────────────────

/// The agent's checklist.
///
/// Folds like every other card, and force-opens while an entry is in progress
/// — a plan is only worth the space it takes while it is being worked through,
/// and a finished twenty-item list between two answers is twenty rows of
/// history nobody is reading. Both rules are the model's (`PlanItem::is_open`),
/// the same pair a tool card follows.
fn plan(
    session: &Entity<ChatSession>,
    p: &PlanItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let open = p.is_open();
    let done = p
        .entries
        .iter()
        .filter(|e| e.status == PlanStatus::Completed)
        .count();
    let rows = p
        .entries
        .iter()
        .take(if open { MAX_TODO_ITEMS } else { 0 })
        .map(|entry| {
            // Pending draws a dot rather than an icon: there is no glyph for
            // "not started" that is not just noise, and the row still needs to
            // occupy the marker column so the contents stay aligned.
            let (mark, color) = match entry.status {
                PlanStatus::Completed => {
                    (Some(IconName::Check), crate::theme::status_ink(cx).success)
                }
                PlanStatus::InProgress => (
                    Some(IconName::Calendar),
                    crate::theme::status_ink(cx).warning,
                ),
                PlanStatus::Pending => (None, cx.theme().muted_foreground),
            };
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w_4()
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .text_color(color)
                        .map(|slot| match mark {
                            Some(icon) => slot.child(Icon::new(icon).size_3()),
                            None => slot.child(div().size(PLAN_DOT).rounded_full().bg(color)),
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        // A struck-through line is finished work: the eye skips
                        // it and lands on what is left, which is the only part
                        // of a plan anyone is reading it for.
                        .when(entry.status == PlanStatus::Completed, |row| {
                            row.line_through().text_color(cx.theme().muted_foreground)
                        })
                        .child(entry.content.clone()),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let hidden = if open {
        p.entries.len().saturating_sub(rows.len())
    } else {
        0
    };
    // The header and checklist are separate regions of the card. The list is
    // intentionally denser inside itself, but it must not pull its first row
    // closer to the title than a tool card pulls detail to its header.
    let body = (!rows.is_empty() || hidden > 0).then(|| {
        div()
            .v_flex()
            .gap_1()
            .children(rows)
            .when(hidden > 0, |list| {
                list.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("+{hidden} more")),
                )
            })
    });

    div()
        .v_flex()
        .gap_2()
        .w_full()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        // A checklist is the agent's working notes, drawn at the size the rest
        // of its working steps are: the entries are read down as a list, not
        // across as prose, and at the answer's size a twenty-item plan is a
        // wall between two paragraphs.
        .text_size(WORK_TEXT)
        .child(
            div()
                .h_flex()
                .gap_2()
                .items_center()
                .w_full()
                .id(("plan", fold_key(target)))
                .cursor_pointer()
                .on_click({
                    let session = session.clone();
                    move |_, _, cx: &mut App| {
                        session.update(cx, |s, cx| {
                            s.chat.toggle_tool(target);
                            cx.notify();
                        });
                    }
                })
                .child(Icon::new(IconName::CircleCheck).size_4())
                .child(div().font_semibold().flex_none().child("Plan"))
                // Collapsed, the header is the whole card, so it has to say
                // what the list said: how much of it is done.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{done}/{} done", p.entries.len())),
                )
                .child(Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })),
        )
        .children(body)
}

// ── permission — blocking; the agent parks until answered ───────────────────

pub(super) fn permission(
    session: &Entity<ChatSession>,
    p: &PermItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let idx = live_index(target);

    div()
        .v_flex()
        .gap_2()
        .w_full()
        .p_3()
        .bg(cx.theme().background)
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .h_flex()
                .gap_2()
                .items_center()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size_4()
                        .text_color(crate::theme::status_ink(cx).warning),
                )
                // Left at the answer's size while every other agent-side card
                // stepped down to the chrome size, and that gap is the point:
                // this and the question below are the only two blocks where
                // nothing at all proceeds until the user acts, so they are the
                // only two allowed to speak as loudly as the conversation.
                .child(div().font_semibold().child("Permission required")),
        )
        .child(mono_well(&p.req.title, MAX_MONO_LINES, cx))
        .map(|card| match (&p.resolved, idx) {
            // Resolved, or living in read-only history where no rpc id is
            // answerable: what is drawn is the record of what was decided, not
            // controls that could still decide it.
            (Some(choice), _) => card.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("✓ {choice}")),
            ),
            (None, None) => card,
            (None, Some(idx)) => card.child(
                div().h_flex().gap_2().justify_end().w_full().children(
                    p.req
                        .options
                        .iter()
                        .enumerate()
                        .map(|(i, option)| {
                            let (id, session) = (option.id.clone(), session.clone());
                            crate::controls::action(("perm", i))
                                // One primary per card, and it is the grant
                                // that expires with this call. "Always allow"
                                // is the same word with a far longer reach:
                                // drawn as loudly as "allow once" it is the
                                // one that gets clicked by muscle memory, and
                                // it is the one that cannot be taken back from
                                // the card. It stays reachable, in the neutral
                                // outline — a decision, not a reflex.
                                .map(|b| match option.weight() {
                                    PermissionWeight::AllowOnce => b.primary(),
                                    PermissionWeight::AllowAlways => b.outline(),
                                    PermissionWeight::Deny => b.ghost(),
                                })
                                .label(option.name.clone())
                                .on_click(move |_, _, cx: &mut App| {
                                    session.update(cx, |s, cx| {
                                        s.chat.answer_permission(idx, &id);
                                        cx.notify();
                                    });
                                })
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                ),
            ),
        })
}

// ── the agent asking a question (`AskUserQuestion` / an MCP form) ───────────

pub(super) fn ask(
    session: &Entity<ChatSession>,
    a: &AskItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    let idx = live_index(target);

    div()
        .v_flex()
        .gap_2()
        .w_full()
        .p_3()
        .bg(cx.theme().background)
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(IconName::Info).size_4())
                .child(div().font_semibold().child(a.req.message.clone())),
        )
        .map(|card| match (&a.resolved, idx) {
            (Some(answer), _) => card.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("✓ {answer}")),
            ),
            // A question replayed from the archive carries an rpc id no running
            // adapter issued: showing controls would invite an answer nobody
            // is waiting for.
            (None, None) => card,
            (None, Some(idx)) => ask_form(card, session, a, idx, cx),
        })
}

/// The live form.
///
/// **One question at a time.** A multi-question form renders as a tab strip
/// with only the active field's choices below it: stacking every question made
/// the card taller than the pane and the overflow was lost off the top
///. A single single-select form is the *quick* shape —
/// clicking a choice answers on the spot, with no Submit to hunt for.
fn ask_form(
    card: gpui::Div,
    session: &Entity<ChatSession>,
    a: &AskItem,
    idx: usize,
    cx: &App,
) -> gpui::Div {
    let quick = a.is_quick();
    let active = a.active_field();
    let multi_field = a.req.fields.len() > 1;

    let tabs = multi_field.then(|| {
        div()
            .id(("ask-tabs", idx))
            .h_flex()
            .gap_1()
            .w_full()
            .overflow_x_scroll()
            .children(a.req.fields.iter().enumerate().map(|(f, field)| {
                let session = session.clone();
                let label = field
                    .title
                    .clone()
                    .or_else(|| field.description.clone())
                    .unwrap_or_else(|| format!("Question {}", f + 1));
                div()
                    .id(("ask-tab", f))
                    .h_flex()
                    .gap_1()
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .text_xs()
                    .cursor_pointer()
                    // The tick beside a label marks a question as *answered*,
                    // which is a different thing from the one on screen — so
                    // the fill is all that says which tab is open. It is the
                    // ramp's selected step, a clear stage past hover, and the
                    // weight beside it does the rest.
                    .when(f == active, |tab| {
                        tab.bg(cx.theme().accent)
                            .text_color(cx.theme().accent_foreground)
                            .font_semibold()
                    })
                    // Only a *tab* label is elided; a choice never is.
                    .child(div().max_w(ASK_TAB_W).truncate().child(label))
                    .when(a.field_answered(f), |tab| {
                        tab.child(Icon::new(IconName::Check).size_3())
                    })
                    .on_click(move |_, _, cx: &mut App| {
                        session.update(cx, |s, cx| {
                            if let Some(item) = s.chat.ask_at_mut(idx) {
                                item.tab = f;
                            }
                            cx.notify();
                        });
                    })
            }))
    });

    let choices = a
        .req
        .fields
        .get(active)
        .map(|field| match &field.kind {
            ElicitKind::Select(c) | ElicitKind::MultiSelect(c) => c.clone(),
            ElicitKind::Text => Vec::new(),
        })
        .unwrap_or_default();
    let picked = a.picked.get(active).cloned().unwrap_or_default();

    let rows = choices
        .into_iter()
        .enumerate()
        .map(|(o, choice)| {
            let session = session.clone();
            let on = picked.contains(&o);
            div()
                .id(("ask-choice", o))
                .v_flex()
                .gap_0p5()
                .w_full()
                .p_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(if on {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .cursor_pointer()
                // The one thing on this card that has to be *read* before
                // anything can happen, so it is set at the size everything
                // else meant to be read is. It had been a step below the
                // question it answers and a step above its own explanation --
                // three sizes inside one decision.
                .child(div().child(choice.label.clone()))
                // Descriptions are never elided either: an option the user
                // cannot read whole is one they cannot choose.
                .when_some(choice.description.clone(), |row, description| {
                    row.child(
                        div()
                            // A step under the label it explains. The card
                            // itself speaks at the conversation's size, because
                            // nothing proceeds until it is answered -- but the
                            // sentence explaining an option is not the option.
                            .text_size(WORK_TEXT)
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    )
                })
                .on_click(move |_, _, cx: &mut App| {
                    session.update(cx, |s, cx| {
                        if let Some(item) = s.chat.ask_at_mut(idx) {
                            item.toggle(active, o);
                        }
                        // A one-question single-select has nothing left to
                        // decide, so it commits on the click.
                        if quick {
                            s.chat.answer_ask(idx, false);
                        }
                        cx.notify();
                    });
                })
                .into_any_element()
        })
        .collect::<Vec<_>>();

    let can_submit = a.has_answer();

    card.children(tabs)
        .child(div().v_flex().gap_1().w_full().children(rows))
        .when(!quick, |card| {
            card.child(
                div()
                    .h_flex()
                    .gap_2()
                    .justify_end()
                    .w_full()
                    .child(
                        crate::controls::action(("ask-skip", idx))
                            .ghost()
                            .label("Skip")
                            .on_click({
                                let session = session.clone();
                                move |_, _, cx: &mut App| {
                                    session.update(cx, |s, cx| {
                                        s.chat.answer_ask(idx, true);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        crate::controls::action(("ask-submit", idx))
                            .primary()
                            .map(|submit| match can_submit {
                                true => submit,
                                // Nothing is picked yet, so the pointer would
                                // be promising a press that does nothing.
                                false => crate::controls::resting(submit),
                            })
                            .disabled(!can_submit)
                            .label("Submit")
                            .on_click({
                                let session = session.clone();
                                move |_, _, cx: &mut App| {
                                    session.update(cx, |s, cx| {
                                        s.chat.answer_ask(idx, false);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
        })
}

// ── notice ──────────────────────────────────────────────────────────────────

/// A line the session says about itself.
///
/// A remark stays a caption. A **failure does not**: the two loudest things
/// this app can say — the turn errored, the agent is gone and here is the key
/// that brings it back — used to draw at the smallest size in the palest
/// colour the theme has, quieter than the file path in the tool card above
/// them. Something that ends the conversation cannot be whispered, so a
/// failure takes the alert icon, the danger tint and the body size.
///
/// It is **not** one of the muted machine wells, and deliberately so: those say
/// "a machine produced this", and the tint is what carries the meaning here. A
/// failure sits on a wash of its own danger colour, in the body face, which is
/// what stops it reading as one more line of the answer without claiming to be
/// quoted output.
fn notice(text: &str, level: NoticeLevel, cx: &App) -> impl IntoElement + use<> {
    let error = level == NoticeLevel::Error;
    div()
        .h_flex()
        .items_start()
        .gap_2()
        .w_full()
        .when(error, |row| {
            row.p_2()
                .rounded(cx.theme().radius)
                .bg(cx.theme().danger.opacity(0.1))
        })
        .when(!error, |row| row.text_xs())
        .text_color(if error {
            crate::theme::status_ink(cx).danger
        } else {
            cx.theme().muted_foreground
        })
        .when(error, |row| {
            row.child(
                div()
                    .flex_none()
                    .child(Icon::new(IconName::TriangleAlert).size_4()),
            )
        })
        .child(div().flex_1().min_w_0().child(text.to_string()))
}

// ── activity strip ──────────────────────────────────────────────────────────

/// Split the transcript into runs: either a single item, or a stretch of
/// adjacent completed steps that folds into one line.
///
/// Grouping is core's call (`onehand_core::chat::activity`), so the two front
/// ends cannot disagree about semantic sections or summaries. This view adds
/// one presentation rule: attention states never disappear into a strip.
pub fn runs(items: &[(TranscriptItemId, &ChatItem)]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for &(target, item) in items {
        let group = fold_group(item);
        match (group, out.last_mut()) {
            // Same coarse section as the run being built: extend it.
            (Some(g), Some(Run::Activity { group, members })) if *group == g => {
                members.push(target);
            }
            (Some(g), _) => out.push(Run::Activity {
                group: g,
                members: vec![target],
            }),
            (None, _) => out.push(Run::Single(target)),
        }
    }
    // A run of one is just that item; folding it would hide a row and save
    // nothing.
    for run in &mut out {
        if let Run::Activity { members, .. } = run
            && members.len() < MIN_ACTIVITY_RUN
        {
            *run = Run::Single(members[0]);
        }
    }
    out
}

/// Only settled work may collapse into history. Pending and running tools
/// remain standalone cards; completed and failed tools are both terminal
/// records. A thought joins history only after its timer has settled.
fn fold_group(item: &ChatItem) -> Option<activity::ActivityGroup> {
    match item {
        ChatItem::Tool(tool) if is_settled(tool) => activity::group(item),
        ChatItem::Thought(thought) if thought.elapsed_secs.is_some() => activity::group(item),
        _ => None,
    }
}

pub enum Run {
    Single(TranscriptItemId),
    Activity {
        group: activity::ActivityGroup,
        members: Vec<TranscriptItemId>,
    },
}

/// The collapsed header for a run of quiet steps.
pub fn activity_strip(
    group: activity::ActivityGroup,
    summary: String,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    id: gpui::ElementId,
    cx: &App,
) -> gpui::AnyElement {
    let (name, icon) = activity_identity(group);
    ghost_row(
        Icon::empty().path(icon),
        id,
        div()
            .h_flex()
            .items_baseline()
            .gap_2()
            .min_w_0()
            .overflow_hidden()
            .child(div().flex_none().font_semibold().child(name))
            .child(div().min_w_0().truncate().child(summary))
            .into_any_element(),
        None,
        Some(open),
        on_click,
        cx,
    )
}

/// Stable identity for a semantic activity section: its name, and the asset
/// path of its icon.
///
/// The summary changes with every member that joins a run; the name and icon
/// do not. Keeping both on the group lets a reader identify the kind of work
/// before reading its counts.
///
/// A path rather than an icon name because the set these are drawn from is not
/// one enum: the bundled library set has no pencil of any kind, so *Changed*
/// comes from the app's own checked-in assets while its five neighbours do not.
/// The path is what both kinds resolve to anyway.
fn activity_identity(group: activity::ActivityGroup) -> (&'static str, SharedString) {
    use gpui_component::IconNamed as _;
    match group {
        activity::ActivityGroup::Explored => ("Explored", IconName::Search.path()),
        // A pencil on a page. The library's nearest shape, `Replace`, is a
        // find-and-replace mark -- one box swapped for another, which is what
        // this group is *not*: it edited files in place.
        activity::ActivityGroup::Changed => ("Changed", crate::icons::Icon::SquarePen.path()),
        activity::ActivityGroup::Ran => ("Ran", IconName::SquareTerminal.path()),
        activity::ActivityGroup::Verified => ("Verified", IconName::CircleCheck.path()),
        activity::ActivityGroup::Reasoned => ("Reasoned", IconName::Info.path()),
        activity::ActivityGroup::Other => ("Other", IconName::Settings2.path()),
    }
}

/// A semantic, counted summary of the work in one activity run.
///
/// Per-target phrases make the folded form almost as expensive to scan as its
/// expanded rows (`Inspected a · Inspected b · Inspected c`). Counts preserve
/// what happened while letting one glance answer how much happened.
pub fn activity_summary(members: &[&ChatItem]) -> String {
    let mut counts = [0usize; 10];
    let mut reason_secs = 0u64;
    let mut running = false;
    let mut failed = false;
    for item in members {
        let kind = match item {
            ChatItem::Thought(th) => {
                counts[8] += 1;
                if let Some(secs) = th.elapsed_secs {
                    reason_secs += secs;
                } else {
                    running = true;
                }
                continue;
            }
            ChatItem::Tool(tool) => {
                let p = activity::presentation(tool);
                running |= matches!(
                    tool.call.status,
                    ToolStatus::Pending | ToolStatus::InProgress
                );
                failed |= matches!(tool.call.status, ToolStatus::Failed);
                if p.kind == activity::ActivityKind::Change {
                    // One MultiEdit is one tool step but can touch many files;
                    // the summary names files, so count its distinct paths.
                    counts[3] += tool.diff_summary.len().max(1);
                    continue;
                }
                p.kind
            }
            _ => continue,
        };
        let index = match kind {
            activity::ActivityKind::Inspect => 0,
            activity::ActivityKind::Search => 1,
            activity::ActivityKind::Fetch => 2,
            activity::ActivityKind::Change => 3,
            activity::ActivityKind::Test => 4,
            activity::ActivityKind::Check => 5,
            activity::ActivityKind::Build => 6,
            activity::ActivityKind::Run => 7,
            activity::ActivityKind::Reason => 8,
            activity::ActivityKind::Other => 9,
        };
        counts[index] += 1;
    }

    let mut phrases = Vec::new();
    let counted = [
        (0, "Inspected", "file", "files"),
        (1, "Searched", "time", "times"),
        (2, "Fetched", "resource", "resources"),
        (3, "Changed", "file", "files"),
        (4, "Ran", "test", "tests"),
        (5, "Ran", "check", "checks"),
        (6, "Built", "target", "targets"),
        (7, "Ran", "command", "commands"),
        (9, "Used", "tool", "tools"),
    ];
    for (index, action, singular, plural) in counted {
        let count = counts[index];
        if count > 0 {
            phrases.push(format!(
                "{action} {count} {}",
                if count == 1 { singular } else { plural }
            ));
        }
    }
    if counts[8] > 0 {
        phrases.push(if reason_secs > 0 {
            format!("Reasoned {reason_secs}s")
        } else {
            "Reasoned".to_string()
        });
    }

    let mut label = phrases.join(" · ");
    if label.is_empty() {
        label.push_str("Activity");
    }
    if failed {
        label.push_str(" · failed");
    } else if running {
        label.push_str(" · running");
    }
    label
}

// ── shared bits ─────────────────────────────────────────────────────────────

/// A chevroned ghost row: the quiet disclosure shape shared by thoughts and
/// settled tool steps. No card, no border — just an icon, what the row says, an
/// optional note at the right edge and the chevron, so completed history reads
/// as a record rather than a stack of attention boxes.
///
/// `fold` is `None` for a row with nothing behind it: a chevron on a row that
/// cannot open is an invitation to click something that does nothing.
///
/// **Every row of this shape is one size**, and the reason is what a group of
/// them looks like when it opens. A thought used to be drawn a step larger than
/// a strip, on the grounds that it names a block rather than summarising work —
/// but a `Reasoned` strip holds thoughts *and* `Think` tool steps, so an opened
/// one put 14px rows beside 12px ones under a 12px header, with the children
/// louder than the group naming them. Weight still separates a name from a
/// summary; size no longer has to.
fn ghost_row(
    icon: impl Into<Icon>,
    id: gpui::ElementId,
    body: gpui::AnyElement,
    trailing: Option<gpui::AnyElement>,
    fold: Option<bool>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let interactive = fold.is_some();
    div()
        .id(id)
        .when(interactive, |row| row.group("ghost-disclosure"))
        .h_flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .when(interactive, |row| row.cursor_pointer().on_click(on_click))
        // Every non-text affordance occupies the same row slot. Their actual
        // drawings may be smaller, but their centres no longer depend on the
        // SVG's or font's own bounding box.
        .child(
            div()
                .size_4()
                .flex_none()
                .h_flex()
                .justify_center()
                .child(Icon::new(icon).size_4()),
        )
        .child(
            div()
                .min_w_0()
                .flex_shrink(1.)
                .when(interactive, |label| {
                    label.group_hover("ghost-disclosure", |label| {
                        label.text_color(cx.theme().foreground)
                    })
                })
                .child(body),
        )
        // Exceptional state is part of what this label says, not a separate
        // right-hand status column. Its explicit semantic colour survives the
        // label's hover treatment.
        .children(trailing)
        // The chevron belongs to the complete label cluster, so it follows the
        // descriptor/status rather than being pushed to a distant rail.
        .children(fold.map(|open| {
            div().size_4().flex_none().h_flex().justify_center().child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size_3(),
            )
        }))
        .child(div().flex_1())
        .into_any_element()
}

/// Completed work is the common case: a compact check next to its descriptor
/// confirms it without building a bright right-hand column of repeated
/// `done`s. States that still need attention remain words.
fn completed_mark(status: ToolStatus, cx: &App) -> Option<gpui::AnyElement> {
    matches!(status, ToolStatus::Completed).then(|| {
        div()
            .size_4()
            .flex_none()
            .h_flex()
            .justify_center()
            .child(
                Icon::new(IconName::Check)
                    .size_3()
                    .text_color(crate::theme::status_ink(cx).success),
            )
            .into_any_element()
    })
}

fn status_note(status: ToolStatus, cx: &App) -> Option<gpui::AnyElement> {
    let (label, color) = match status {
        ToolStatus::Pending => ("pending", cx.theme().muted_foreground),
        ToolStatus::InProgress => ("running", crate::theme::status_ink(cx).warning),
        ToolStatus::Failed => ("failed", crate::theme::status_ink(cx).danger),
        ToolStatus::Completed => return None,
    };
    Some(
        div()
            .flex_none()
            .text_xs()
            .text_color(color)
            .child(label)
            .into_any_element(),
    )
}

fn tool_icon(kind: ToolKind) -> IconName {
    match kind {
        ToolKind::Execute => IconName::SquareTerminal,
        ToolKind::Edit => IconName::File,
        ToolKind::Read => IconName::File,
        ToolKind::Search => IconName::Search,
        ToolKind::Think => IconName::Undo,
        ToolKind::Fetch => IconName::Globe,
        ToolKind::Delete => IconName::Delete,
        ToolKind::Move => IconName::ArrowRight,
        ToolKind::Other => IconName::Settings2,
    }
}

fn tool_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Execute => "Run",
        ToolKind::Edit => "Edit",
        ToolKind::Read => "Read",
        ToolKind::Search => "Search",
        ToolKind::Think => "Think",
        ToolKind::Fetch => "Fetch",
        ToolKind::Delete => "Delete",
        ToolKind::Move => "Move",
        ToolKind::Other => "Tool",
    }
}

/// A stable element id per item. History and live indices overlap, so the
/// source has to be part of the key or two items share one id.
fn fold_key(target: TranscriptItemId) -> usize {
    match target {
        TranscriptItemId::History(i) => i * 2,
        TranscriptItemId::Live(i) => i * 2 + 1,
    }
}

/// The live-items index, or `None` for a history item.
///
/// Only live items are answerable: a permission replayed from the archive
/// carries an rpc id that no running adapter ever issued.
fn live_index(target: TranscriptItemId) -> Option<usize> {
    match target {
        TranscriptItemId::Live(i) => Some(i),
        TranscriptItemId::History(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onehand_core::acp::{ToolCall, ToolKind, ToolStatus};
    use onehand_core::chat::{Md, Thought};

    fn thought(secs: u64) -> ChatItem {
        ChatItem::Thought(Thought {
            md: Md::parse("…"),
            started: None,
            elapsed_secs: Some(secs),
            expanded: false,
        })
    }

    fn tool(kind: ToolKind, title: &str) -> ChatItem {
        tool_with_status(kind, title, ToolStatus::Completed)
    }

    fn tool_with_status(kind: ToolKind, title: &str, status: ToolStatus) -> ChatItem {
        ChatItem::Tool(ToolItem::new(ToolCall {
            id: title.into(),
            title: title.into(),
            description: None,
            kind,
            status,
            content: Vec::new(),
        }))
    }

    fn targets(items: &[ChatItem]) -> Vec<(TranscriptItemId, &ChatItem)> {
        items
            .iter()
            .enumerate()
            .map(|(i, item)| (TranscriptItemId::Live(i), item))
            .collect()
    }

    #[test]
    fn every_activity_group_has_its_own_name_and_icon() {
        let groups = [
            activity::ActivityGroup::Explored,
            activity::ActivityGroup::Changed,
            activity::ActivityGroup::Ran,
            activity::ActivityGroup::Verified,
            activity::ActivityGroup::Reasoned,
            activity::ActivityGroup::Other,
        ];
        let identities = groups.map(activity_identity);
        let mut names = identities.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        // The identity already carries the asset path, which is the fact this
        // test is about: two groups drawing the same SVG is the collision worth
        // catching, whatever the two names in front of it are -- and the six
        // are no longer even drawn from one enum.
        let mut icons = identities
            .iter()
            .map(|(_, icon)| icon.clone())
            .collect::<Vec<_>>();
        icons.sort_unstable();
        icons.dedup();

        assert_eq!(names.len(), groups.len());
        assert_eq!(icons.len(), groups.len());
    }

    #[test]
    fn a_lone_quiet_step_is_not_folded() {
        // Folding one row behind a disclosure hides it and saves nothing.
        let items = vec![thought(3)];
        let runs = runs(&targets(&items));
        assert!(matches!(runs.as_slice(), [Run::Single(_)]));
    }

    #[test]
    fn adjacent_steps_of_one_section_fold_together() {
        let items = vec![
            tool(ToolKind::Read, "Read src/a.rs"),
            tool(ToolKind::Read, "Read src/b.rs"),
        ];
        let runs = runs(&targets(&items));
        assert!(
            matches!(runs.as_slice(), [Run::Activity { members, .. }] if members.len() == 2),
            "two reads are one Explored run"
        );
    }

    #[test]
    fn only_unsettled_tools_stay_out_of_activity() {
        let items = vec![
            tool_with_status(ToolKind::Read, "Read src/pending.rs", ToolStatus::Pending),
            tool_with_status(ToolKind::Read, "Read src/live-a.rs", ToolStatus::InProgress),
            tool_with_status(ToolKind::Read, "Read src/failed.rs", ToolStatus::Failed),
            tool(ToolKind::Read, "Read src/done-a.rs"),
            tool(ToolKind::Read, "Read src/done-b.rs"),
        ];
        let runs = runs(&targets(&items));
        assert!(matches!(
            runs.as_slice(),
            [
                Run::Single(_),
                Run::Single(_),
                Run::Activity { members, .. }
            ]
                if members.len() == 3
        ));
    }

    #[test]
    fn a_prompt_breaks_a_run() {
        // A user prompt is not activity, so the steps either side of it belong
        // to different turns and must not be folded into one strip.
        let items = vec![
            tool(ToolKind::Read, "Read src/a.rs"),
            tool(ToolKind::Read, "Read src/b.rs"),
            ChatItem::notice("interrupted"),
            tool(ToolKind::Read, "Read src/c.rs"),
        ];
        let runs = runs(&targets(&items));
        assert!(matches!(
            runs.as_slice(),
            [Run::Activity { .. }, Run::Single(_), Run::Single(_)]
        ));
    }

    #[test]
    fn summary_aggregates_repeated_steps() {
        let items = [thought(3), thought(3), thought(4), thought(5), thought(6)];
        let bodies: Vec<&ChatItem> = items.iter().collect();
        assert_eq!(activity_summary(&bodies), "Reasoned 21s");

        let reads = [
            tool(ToolKind::Read, "Read src/a.rs"),
            tool(ToolKind::Read, "Read src/b.rs"),
        ];
        let bodies: Vec<&ChatItem> = reads.iter().collect();
        assert_eq!(activity_summary(&bodies), "Inspected 2 files");

        let failed_reads = [
            tool(ToolKind::Read, "Read src/a.rs"),
            tool_with_status(ToolKind::Read, "Read src/b.rs", ToolStatus::Failed),
        ];
        let bodies: Vec<&ChatItem> = failed_reads.iter().collect();
        assert_eq!(activity_summary(&bodies), "Inspected 2 files · failed");

        let multi_edit = ChatItem::Tool(ToolItem::new(ToolCall {
            id: "multi-edit".into(),
            title: "Edit project config".into(),
            description: None,
            kind: ToolKind::Edit,
            status: ToolStatus::Completed,
            content: vec![
                ToolContent::Diff {
                    path: "package.json".into(),
                    old: Some("{}".into()),
                    new: "{\"type\":\"module\"}".into(),
                },
                ToolContent::Diff {
                    path: "tsconfig.json".into(),
                    old: Some("{}".into()),
                    new: "{\"module\":\"NodeNext\"}".into(),
                },
            ],
        }));
        assert_eq!(activity_summary(&[&multi_edit]), "Changed 2 files");
    }

    #[test]
    fn tool_descriptor_does_not_repeat_its_action() {
        let ChatItem::Tool(edit) = tool(ToolKind::Edit, "Edit src/lib.rs") else {
            unreachable!();
        };
        let root = Path::new("/tmp/project");
        assert_eq!(tool_descriptor(&edit, root), "src/lib.rs");

        let ChatItem::Tool(run) = tool(ToolKind::Execute, "Run cargo test") else {
            unreachable!();
        };
        assert_eq!(tool_descriptor(&run, root), "cargo test");

        let multiline = ToolItem::new(ToolCall {
            id: "script".into(),
            title: "python3 - <<'PY'\nprint('ok')\nPY".into(),
            description: None,
            kind: ToolKind::Execute,
            status: ToolStatus::Completed,
            content: Vec::new(),
        });
        assert_eq!(
            tool_descriptor(&multiline, root),
            "python3 - <<'PY' print('ok') PY"
        );
        assert!(show_tool_kind(&run), "a raw command still needs `Run`");

        let mut described = run;
        described.call.description = Some("Run the regression suite".into());
        assert_eq!(
            tool_descriptor(&described, root),
            "Run the regression suite"
        );
        assert!(
            !show_tool_kind(&described),
            "the human description replaces the redundant kind label"
        );

        let ChatItem::Tool(absolute) = tool(ToolKind::Edit, "/tmp/project/src/main.rs") else {
            unreachable!();
        };
        assert_eq!(tool_descriptor(&absolute, root), "src/main.rs");
    }
}
