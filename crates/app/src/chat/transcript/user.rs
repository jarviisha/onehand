use super::Room;
use super::metrics::{
    BUBBLE_MAX, BUBBLE_TAIL_GAP, HAIR_GAP, STACK_GAP, THUMB_H, THUMB_W, TIGHT_GAP, USER_BUBBLE_MAX,
    USER_BUBBLE_MAX_NARROW,
};
use super::parts::copy_button;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, StyledImage as _, Window, div, relative,
};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::chat::UserMsg;

/// Attachment rows drawn under a prompt before the rest are counted.
const MAX_ATTACHMENT_ROWS: usize = 8;

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
                        .enumerate()
                        .map(|(i, a)| {
                            let id = gpui::ElementId::NamedInteger(
                                format!("plate-{uid}").into(),
                                i as u64,
                            );
                            attachment(a, id, cx)
                        }),
                )
                .when(over > 0, |list| {
                    list.child(
                        div()
                            .h(THUMB_H)
                            .h_flex()
                            .items_center()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match over {
                                1 => "+1 more attachment".to_string(),
                                n => format!("+{n} more attachments"),
                            }),
                    )
                })
        }))
        .children((!u.text.trim().is_empty()).then(|| Prompt {
            key: uid,
            text: u.text.clone().into(),
            share,
        }))
}

/// The bubble and the copy control under it, which shows while the message is
/// pointed at and while the control holds the keyboard's focus.
///
/// **An element of its own, because it has to remember what the frame does
/// not**: whether the message is pointed at, which is keyed state, and keyed
/// state needs the window.
///
/// **The hover belongs to the whole message, not to the control it reveals**,
/// so the region that answers is the thing somebody is pointing at. Hidden, the
/// control keeps its place, so the message does not move as it appears.
#[derive(IntoElement)]
struct Prompt {
    key: usize,
    text: SharedString,
    share: f32,
}

impl RenderOnce for Prompt {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = self.key;
        let hovered = window.use_keyed_state(("prompt-hovered", key), cx, |_, _| false);
        // The button's own focus handle, which the library keys by the button's
        // id under the element that holds it: a control reached by the keyboard
        // shows without the pointer.
        let copy_id = ("prompt-copy", key);
        let focus = window.with_id(("prompt", key), |window| {
            window
                .use_keyed_state(copy_id, cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        });
        let shown = *hovered.read(cx) || focus.is_focused(window);

        div()
            .id(("prompt", key))
            .v_flex()
            .items_end()
            // The control belongs to the bubble rather than following it.
            .gap(BUBBLE_TAIL_GAP)
            .max_w(relative(self.share))
            .on_hover(move |on, _, cx| {
                hovered.update(cx, |hovered, cx| {
                    *hovered = *on;
                    cx.notify();
                });
            })
            .child(
                div()
                    .max_w(BUBBLE_MAX)
                    .px_3()
                    .py_2()
                    // **Even corners, the well's fill and no edge.** The side
                    // it sits on already says whose words these are; the fill
                    // only has to set them off the page.
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().foreground)
                    .child(prompt_text(&self.text, cx)),
            )
            .child(
                copy_button(copy_id, self.text.clone(), "Copy the prompt", cx)
                    .when(!shown, |copy| copy.text_color(cx.theme().transparent)),
            )
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
        // The bubble's step on the well it sits in, so the chip shows on it.
        background_color: Some(cx.theme().secondary),
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
fn attachment(
    a: &onehand_core::attachment::AttachmentSnapshot,
    id: gpui::ElementId,
    cx: &App,
) -> impl IntoElement {
    use onehand_core::attachment::{AttachmentDelivery, AttachmentKind};
    let unavailable = a.delivery == AttachmentDelivery::Unavailable;
    // Nothing to show for a file that was never sent: the thumbnail would say
    // the agent saw this picture.
    let thumbnail = (a.kind == AttachmentKind::Image && !unavailable).then(|| a.path.clone());
    let name = SharedString::from(a.name.clone());
    // An attachment the agent never received is the one thing about this plate
    // that changes the answer, so it is spelled out.
    let shown = plate(
        id,
        thumbnail.clone(),
        name.clone(),
        unavailable.then_some("not sent"),
        cx,
    );
    match thumbnail {
        Some(path) => opens(shown, "Show this image".into(), cx, move |window, cx| {
            super::image_view::open(path.clone(), name.clone(), window, cx);
        }),
        None => shown,
    }
}

/// One attached file as a plate, in the transcript and in the tray of staged
/// files alike, so a picture reads the same before it is sent as after.
///
/// **One plate whatever is on it.** A picture fills it and a file leaves it
/// bare, so a prompt that handed over both is a strip of one kind of thing
/// rather than a picture beside a chip of some other height. The name sits at
/// the foot of the plate: a preview is read as the thing itself, and the name
/// is its caption rather than its label. `note` is a word in the danger ink
/// after the name, where the file did not or cannot go, and the plate's edge
/// takes the same ink.
pub(in crate::chat) fn plate(
    id: impl Into<gpui::ElementId>,
    picture: Option<std::path::PathBuf>,
    name: SharedString,
    note: Option<&'static str>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let danger = crate::theme::status_ink(cx).danger;
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(THUMB_W)
        .h(THUMB_H)
        .overflow_hidden()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(match note {
            Some(_) => danger,
            None => cx.theme().border,
        })
        // A fill of its own, so a bare plate reads as a thing and not a hole
        // in whatever surface it rests on.
        .bg(cx.theme().secondary)
        .child(match picture {
            // Cropped to fill the plate rather than fitted inside it: fitted,
            // a wide screenshot was a sliver in a box of mostly nothing.
            Some(path) => gpui::img(path)
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .into_any_element(),
            // A file other than a picture takes the file mark over its name,
            // so the plate is not read as a preview still loading.
            None => div()
                .size_full()
                .h_flex()
                .items_center()
                .justify_center()
                .child(
                    gpui_component::Icon::new(gpui_component::IconName::File)
                        .text_color(cx.theme().muted_foreground),
                )
                .into_any_element(),
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
                // The page's own fill under the name, so it reads over a
                // picture of any colour, and the name in full ink.
                .py(HAIR_GAP)
                .bg(cx.theme().background)
                // Scaled to the plate, not to the chrome around it.
                .text_size(crate::tokens::MICRO_TEXT)
                .text_color(cx.theme().foreground)
                .child(div().min_w_0().truncate().child(name))
                .children(note.map(|note| div().flex_none().text_color(danger).child(note))),
        )
}

/// A plate that does something when pressed: the pointer, an edge that steps
/// up under it, since a fill would be lost behind a picture, and `tip`.
pub(in crate::chat) fn opens(
    plate: gpui::Stateful<gpui::Div>,
    tip: SharedString,
    cx: &App,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let edge = cx.theme().muted_foreground;
    plate
        .cursor_pointer()
        .hover(move |plate| plate.border_color(edge))
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
        .on_click(move |_, window, cx| on_press(window, cx))
}
