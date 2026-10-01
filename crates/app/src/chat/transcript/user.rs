use super::Room;
use super::metrics::{
    BUBBLE_PAD_X, BUBBLE_PAD_Y, BUBBLE_TAIL_GAP, BUTTON_H, HAIR_GAP, MARK_SIZE,
    MAX_ATTACHMENT_ROWS, STACK_GAP, THUMB_H, THUMB_W, TIGHT_GAP, USER_BUBBLE_MAX,
    USER_BUBBLE_MAX_NARROW, radius_block, radius_bubble, radius_control,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, relative,
};
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::chat::UserMsg;

// ── user prompt — filled, shrink-to-fit, against the right edge ─────────────

/// What the user asked, on its own side of the column.
///
/// **The side is the label.** Every other block in the transcript is the
/// agent's, so the one thing a reader scanning back through a long
/// conversation is looking for — where they last asked something — is also the
/// only thing that has to be findable without reading. A fill alone said that
/// when the transcript was two blocks long; twenty blocks down it is one more
/// box among boxes, while an edge is still an edge.
///
/// **What was handed over is not what was typed, so it sits outside the
/// bubble.** A picture inside the fill reads as part of the sentence and is
/// bounded by the sentence's box; a prompt that was nothing but a screenshot
/// drew an empty filled card above it, which says the user sent a blank
/// message. Both keep the right edge, because the edge is what says whose they
/// are, and the files come first — they were handed over before the question
/// was asked about them.
pub(super) fn user(u: &UserMsg, uid: usize, room: Room, cx: &App) -> impl IntoElement + use<> {
    let over = u.attachments.len().saturating_sub(MAX_ATTACHMENT_ROWS);
    let share = match room.narrow {
        true => USER_BUBBLE_MAX_NARROW,
        false => USER_BUBBLE_MAX,
    };

    div()
        .v_flex()
        .items_end()
        // Tighter than the gap between blocks, because these are the parts of
        // one thing said: what was handed over, the sentence asking about it,
        // and whatever is offered underneath.
        .gap(STACK_GAP)
        .w_full()
        .children((!u.attachments.is_empty()).then(|| {
            div()
                // **A strip, not a column.** Handed over together, they were
                // handed over at once, and stacked they pushed the question
                // itself further off the screen with every file added.
                .h_flex()
                .flex_wrap()
                .justify_end()
                .items_end()
                .gap(STACK_GAP)
                .max_w(relative(share))
                .children(
                    u.attachments
                        .iter()
                        .take(MAX_ATTACHMENT_ROWS)
                        .map(|a| attachment(a, cx)),
                )
                .when(over > 0, |list| {
                    list.child(
                        div()
                            .h(THUMB_H)
                            .h_flex()
                            .items_center()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("+{over} more attachment(s)")),
                    )
                })
        }))
        .children((!u.text.trim().is_empty()).then(|| {
            div()
                .id(("prompt", uid))
                .v_flex()
                .items_end()
                // The control belongs to the bubble rather than following it.
                .gap(BUBBLE_TAIL_GAP)
                .max_w(relative(share))
                // **The hover belongs to the whole message, not to the row it
                // reveals.** Put on the row itself it asked the reader to find
                // a transparent strip a few pixels tall before it would show
                // them what was in it -- which is the same as not being there.
                // The wrapper shrinks to the bubble, so the region that answers
                // is the thing somebody is pointing at.
                //
                // It works by cascade rather than by naming a group: text
                // colour inherits, so the row is drawn in whatever this says
                // and the bubble, which sets its own ink, is untouched.
                .text_color(cx.theme().transparent)
                .hover(|turn| turn.text_color(cx.theme().muted_foreground))
                .child(
                    div()
                        .w_full()
                        .py(BUBBLE_PAD_Y)
                        .px(BUBBLE_PAD_X)
                        .rounded(radius_bubble(cx))
                        // **The corner nearest the speaker is the tight one.**
                        // A bubble rounded evenly is a lozenge that could
                        // belong to either side; one corner brought back down
                        // to a control's points the shape at the edge the
                        // prompt came from, which is the whole of what the
                        // right-hand lane is saying.
                        .rounded_br(radius_control(cx))
                        // **The one filled surface in the transcript**, on the
                        // ramp's own step for it. Everything else is an edge on
                        // the reading surface, so a fill means one thing here:
                        // this was typed by the person reading it. The hairline
                        // is only to hold the shape where the two get close.
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary)
                        .text_color(cx.theme().secondary_foreground)
                        .child(prompt_text(&u.text, cx)),
                )
                .child(PromptCopy {
                    key: uid,
                    text: u.text.clone().into(),
                })
        }))
}

/// The control under a prompt: hidden until the turn is pointed at, and
/// answering when it is pressed.
///
/// **An element of its own, because it has to remember two things the frame
/// does not.** Whether it was just pressed — which needs keyed state, and keyed
/// state needs the window — and a timer to take that back a second later.
///
/// **It appears by inheriting, not by being revealed.** The usual way to show a
/// child on hover is to name a group on an ancestor and ask for it by name from
/// the child, which resolves through a registry and has already failed twice in
/// this file. Text colour cascades, so the row is drawn transparent and the
/// hover on its own container turns the ink up: one primitive, no registry, and
/// the space is held either way because the row is always laid out.
#[derive(IntoElement)]
struct PromptCopy {
    key: usize,
    text: SharedString,
}

impl RenderOnce for PromptCopy {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = self.key;
        let done = window.use_keyed_state(("prompt-copied", key), cx, |_, _| false);
        let copied = *done.read(cx);
        let text = self.text.clone();

        div()
            .id(("prompt-copy", key))
            .h_flex()
            .items_center()
            .justify_end()
            .gap(TIGHT_GAP)
            .flex_none()
            .h(BUTTON_H)
            .cursor_pointer()
            .text_xs()
            // **No colour of its own until it has something to say.** The ink
            // is the message's, inherited, so the row appears when the message
            // is pointed at rather than when this thin strip is. Once pressed
            // it takes its own, which is also what keeps the answer up after
            // the pointer has gone.
            .when(copied, |row| {
                row.text_color(crate::theme::status_ink(cx).success)
            })
            .child(
                Icon::new(match copied {
                    true => IconName::Check,
                    false => IconName::Copy,
                })
                .size(MARK_SIZE),
            )
            .child(match copied {
                true => "Copied",
                false => "Copy",
            })
            .on_click(move |_, window, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                done.update(cx, |done, cx| {
                    *done = true;
                    cx.notify();
                });
                // Taken back on a timer rather than on the next pointer move:
                // an answer that only clears when the mouse happens to leave is
                // one that is still claiming to have just happened a minute
                // later.
                let done = done.clone();
                window
                    .spawn(cx, async move |cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(1_200))
                            .await;
                        done.update(cx, |done, cx| {
                            *done = false;
                            cx.notify();
                        });
                    })
                    .detach();
            })
    }
}

/// What the user typed, with the spans they fenced in backticks set apart.
///
/// **Drawn as typed, and this is the one exception.** A prompt run through the
/// markdown renderer would turn `**/*.rs` into bold and `# 1` into a heading —
/// the transcript misquoting the person who wrote it — so the text is drawn
/// verbatim. A backtick pair is the one mark a reader means as markup even here,
/// because it is how they say "this is a name, not a word", and losing it makes
/// a path in the middle of a sentence unfindable.
///
/// Only *matched* pairs count. A lone backtick is a backtick, which is what
/// somebody typing about shell quoting meant by it.
fn prompt_text(text: &str, cx: &App) -> gpui::AnyElement {
    let spans = code_spans(text);
    if spans.is_empty() {
        // Nothing to set apart, so nothing pays for the run machinery.
        return div().child(text.to_string()).into_any_element();
    }
    let chip = gpui::HighlightStyle {
        // **A fill, and no corner.** A highlight run paints a rectangle behind
        // its glyphs and there is no radius on it — the rounded chip this wants
        // would need the text laid out by hand. The fill and the face together
        // are still enough to read as one.
        background_color: Some(cx.theme().muted),
        color: Some(cx.theme().secondary_foreground),
        ..Default::default()
    };
    let mono = cx.theme().mono_font_family.clone();
    gpui::StyledText::new(text.to_string())
        .with_highlights(spans.iter().map(|range| (range.clone(), chip)))
        .with_font_family_overrides(spans.into_iter().map(|range| (range, mono.clone())))
        .into_any_element()
}

/// The byte ranges of every matched backtick pair, contents only, in order.
///
/// Returned sorted and non-overlapping because both run APIs require it, and a
/// scan that pairs each opening tick with the next closing one is sorted by
/// construction.
fn code_spans(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut open: Option<usize> = None;
    for (at, c) in text.char_indices() {
        if c != '`' {
            continue;
        }
        match open.take() {
            // An empty pair marks nothing, and a zero-width run is a run the
            // layout has to carry for no glyph.
            Some(from) if at > from => out.push(from..at),
            Some(_) => {}
            None => open = Some(at + 1),
        }
    }
    out
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
        // **One plate whatever is on it.** A picture fills it and a file
        // carries its mark in the middle of it, so a prompt that handed over
        // both is a strip of one kind of thing rather than a picture beside a
        // chip of some other height. The name sits at the foot of the plate: a
        // preview is read as the thing itself, and the name is its caption
        // rather than its label.
        .relative()
        .flex_none()
        .w(THUMB_W)
        .h(THUMB_H)
        .overflow_hidden()
        .rounded(radius_block(cx))
        .border_1()
        .border_color(cx.theme().border)
        .map(|plate| match thumbnail {
            Some(path) => plate.child(gpui::img(path).size_full()),
            // The registry has no picture glyph, so the plain file mark stands
            // in for both kinds; where there is a preview it says what the file
            // is better than any icon could, so the mark is drawn only here.
            None => plate.child(
                div()
                    .size_full()
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::File).size_4()),
            ),
        })
        .child(
            div()
                // Over the foot of the plate rather than under it: the plate is
                // a fixed size, and a caption taking a row of its own would
                // make every attachment two different heights again depending
                // on whether its name wrapped.
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h_flex()
                .items_center()
                .gap(TIGHT_GAP)
                .px(TIGHT_GAP)
                .py(HAIR_GAP)
                .bg(cx.theme().secondary)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(a.name.clone()))
                // An attachment the agent never received is the one thing about
                // this plate that changes the answer, so it is spelled out.
                .when(unavailable, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_color(crate::theme::status_ink(cx).danger)
                            .child("not sent"),
                    )
                }),
        )
}
