use super::metrics::{ASK_TAB_W, CARD_PAD, PART_GAP, TEXT_SM};
use super::parts::{
    ActivityRow, BlockingBody, Object, RowMark, activity_row, card_body, card_foot, card_head,
    detail_well, grows, pill, pinned_card,
};
use super::{fold_key, live_index};
use crate::chat::session::ChatSession;
use crate::controls::Quiet as _;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::controls::text_input;
use crate::tokens::{CONTROL_H, LEADING_TIGHT};
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, relative,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::kbd::Kbd;
use gpui_component::{ActiveTheme, Icon, IconName, Selectable as _, Sizable as _, StyledExt};
use onehand_core::acp::ElicitKind;
use onehand_core::chat::{AskItem, TranscriptItemId};

mod keys;
mod record;
use keys::*;
pub(super) use record::user_icon;
use record::*;

// ── the agent asking a question (`AskUserQuestion` / an MCP form) ───────────

pub(in crate::chat) fn ask(
    session: &Entity<ChatSession>,
    a: &AskItem,
    target: TranscriptItemId,
    cx: &App,
) -> impl IntoElement + use<> {
    if let Some(answer) = a.resolved.as_deref() {
        return ask_record(session, a, answer, target, cx);
    }
    let idx = live_index(target);
    let live = idx.is_some();
    let counter = (live && a.req.fields.len() > 1).then(|| {
        format!(
            "Question {} of {}",
            a.active_field() + 1,
            a.req.fields.len()
        )
    });

    // Where the reader is in the form, in words, because the tab strip says
    // which question is open but not how many are left. No mark: the
    // permission card keeps the one mark a pinned card carries.
    pinned_card(live, cx)
        .child(card_head(
            None,
            a.req.message.clone(),
            counter.map(Into::into),
            cx,
        ))
        // A question replayed from the archive carries an rpc id no running
        // adapter issued: showing controls would invite an answer nobody is
        // waiting for. An answered one never reaches here -- it is a row.
        .map(|card| match idx {
            None => card.into_any_element(),
            Some(idx) => ask_form(card, session, a, target, idx, cx),
        })
}

/// The highest row a single digit reaches, and so the last one that is offered
/// a key at all.
///
/// **The handler reads one keystroke, not a typed number.** There is nowhere to
/// hold a half-entered figure and nothing that could say when one had ended, so
/// a row past the ninth has no key and must not be drawn carrying one. Printed
/// anyway, `10` was a hint for a press that cannot be made — and worse than
/// inert on the card that answers as soon as a row is taken, where reaching for
/// it lands on `1` and commits the first choice instead.
const ASK_KEY_ROWS: usize = 9;

/// The key at the right-hand end of a row, and the number that reaches it.
///
/// **A row a keyboard can reach says so on the row**: this is the jump, and a
/// jump has to be to something the user can already see a name for. `None`
/// past the ninth row, which is a row with no key rather than a row with an
/// unusable one.
fn ask_key_hint(n: usize) -> Option<Kbd> {
    if n > ASK_KEY_ROWS {
        return None;
    }
    gpui::Keystroke::parse(&n.to_string()).ok().map(Kbd::new)
}

/// The live form.
///
/// **One question at a time.** A multi-question form renders as a numbered tab
/// strip with only the active field's choices below it: stacking every question
/// made the card taller than the pane and the overflow was lost off the top. A
/// single single-select form is the *quick* shape — clicking a choice answers
/// on the spot, with no Submit to hunt for.
///
/// **The forward button is what moves the form on**, not the pick. A
/// single-select press used to walk itself to the next open question, on the
/// reasoning that answering one question is asking for the next; what that cost
/// once the card grew a footer is the button in it — a form that has already
/// moved on leaves *Next* pointing at a question the user is now looking at, so
/// the one control the footer exists to offer is the one control there is never
/// a moment to press. The tab strip is still how an answer is gone back to.
///
/// **The keys belong to the card and only to the card.** Numbers, the arrows,
/// Enter and Esc are every one of them a key somebody is as likely to be typing
/// into the composer an inch below, so they are taken on the card's own focus
/// handle rather than bound as app actions — a binding would reach over the
/// composer exactly the way the panel shortcuts are meant to, which is the
/// opposite of what a form wants.
/// The four parts of a question card are drawn against the same five facts:
/// which session it belongs to, the item, where it is in the transcript, which
/// question of the form is open, and whether the card is the quick kind that
/// commits on the click itself.
///
/// Held together rather than threaded through four signatures, which is what
/// they were when this was one function: the same five arguments in the same
/// order at every call, and a fifth part added later would have taken them
/// again. Everything else each part needs is derived from these inside it.
struct AskForm<'a> {
    session: &'a Entity<ChatSession>,
    a: &'a AskItem,
    idx: usize,
    /// The question the strip has open, and the one every row below it answers.
    active: usize,
    /// A one-question single-select: it answers on the click and carries no
    /// footer to hunt for.
    quick: bool,
}

impl AskForm<'_> {
    /// The strip of questions across the top, on a form that has more than one.
    ///
    /// **The number is the point, until the question has an answer.** A strip
    /// of numbered titles says these are things in an order, which is the only
    /// question a reader has about a form partway through. Once answered, a
    /// check takes the number's place, so what is left to do reads as the tabs
    /// still carrying digits. The open tab is the selected one, on the
    /// selected step, every tab at one weight.
    fn tabs(&self, cx: &App) -> gpui::Stateful<gpui::Div> {
        let (session, a, idx, active) = (self.session, self.a, self.idx, self.active);
        div()
            .id(("ask-tabs", idx))
            .h_flex()
            .gap_1()
            .w_full()
            .overflow_x_scroll()
            .children(a.req.fields.iter().enumerate().map(|(f, field)| {
                let session = session.clone();
                let (open, answered) = (f == active, a.field_answered(f));
                let label = field
                    .title
                    .clone()
                    .or_else(|| field.description.clone())
                    .unwrap_or_else(|| format!("Question {}", f + 1));
                crate::controls::action(("ask-tab", f))
                    .quiet(cx)
                    .short()
                    .selected(open)
                    .flex_none()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1p5()
                            .map(|tab| match answered {
                                true => tab.child(Icon::new(IconName::Check).xsmall()),
                                false => tab.child(format!("{}", f + 1)),
                            })
                            // Only a *tab* label is cut short; a choice never
                            // is, and one held to a line keeps the strip one
                            // row tall.
                            .child(
                                div()
                                    .max_w(ASK_TAB_W)
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(label),
                            ),
                    )
                    // Jumping back restores what that question already holds:
                    // every answer is kept per field, so the tab only moves
                    // the view.
                    .on_click(move |_, _, cx: &mut App| {
                        session.update(cx, |s, cx| {
                            if let Some(item) = s.chat.ask_at_mut(idx) {
                                item.go_to(f);
                            }
                            cx.notify();
                        });
                    })
            }))
    }

    /// The open question's choices, one row each.
    fn choice_rows(&self, cx: &App) -> Vec<gpui::AnyElement> {
        let (session, a, idx, active, quick) =
            (self.session, self.a, self.idx, self.active, self.quick);
        let field = a.req.fields.get(active);
        let single = matches!(field.map(|f| &f.kind), Some(ElicitKind::Select(_)));
        let choices = field
            .map(|field| match &field.kind {
                ElicitKind::Select(c) | ElicitKind::MultiSelect(c) => c.clone(),
                ElicitKind::Text => Vec::new(),
            })
            .unwrap_or_default();
        let picked = a.picked.get(active).cloned().unwrap_or_default();
        let cursor = a.cursor_row(active);

        choices
            .into_iter()
            .enumerate()
            .map(|(o, choice)| {
                let session = session.clone();
                let on = picked.contains(&o);
                // The one thing on this card that has to be *read* before anything
                // can happen, so it is set at the size everything else meant to be
                // read is, with its description tight underneath rather than a line
                // away: the two are one answer, and spaced apart the description
                // reads as belonging to whichever row it is nearer.
                let destructive = choice.is_destructive();
                // At the leading of a sentence read once, so a choice of one
                // line sits inside the field's height below it.
                let mut words = div()
                    .v_flex()
                    .gap_0p5()
                    .flex_1()
                    .min_w_0()
                    .line_height(relative(LEADING_TIGHT))
                    .child(
                        div()
                            .w_full()
                            .when(destructive, |label| {
                                label.text_color(crate::theme::status_ink(cx).danger)
                            })
                            .child(choice.label.clone()),
                    );
                if let Some(description) = choice.description.clone() {
                    words = words.child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(crate::theme::meta_ink(cx))
                            .child(description),
                    );
                }
                grows(crate::controls::action(("ask-choice", o)))
                    .quiet(cx)
                    // **The taken choice is the selected step**, as every
                    // pick-one row's is, and a destructive one is named by its
                    // danger word rather than by an edge.
                    .selected(on)
                    // **At rest on a well of its own**, so each choice reads
                    // as one thing apart from the next. Only at rest: on the
                    // taken row the library lays the caller's fill over its
                    // selected step.
                    .when(!on, |row| row.bg(crate::theme::floating_well(cx)))
                    // **As tall as the typed answer at one line**, a form's
                    // field height as the floor: the field is the list's last
                    // row, written in rather than picked. A longer label or a
                    // description grows the row past it.
                    .px_3()
                    .py_1()
                    .w_full()
                    .min_h(CONTROL_H)
                    // The keyboard's own position, which has to show without
                    // being an answer: an arrow press moves it and settles
                    // nothing, so it is the focus ring rather than a fill.
                    .border_1()
                    .border_color(
                        match !on && cursor == Some(onehand_core::chat::AskRow::Choice(o)) {
                            true => cx.theme().ring,
                            false => cx.theme().transparent,
                        },
                    )
                    // **One child, holding the row's own layout.** A `Button`
                    // wraps what it is given in a content box of its own, so a
                    // flex written on the button decides nothing. The mark and
                    // the key sit on the row's middle, so the column of marks
                    // stays straight whatever the agent wrote.
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_3()
                            .w_full()
                            .child(onehand_ui::choice_mark(on, single, cx))
                            .child(words)
                            .children(ask_key_hint(o + 1)),
                    )
                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                        ask_take(
                            &session,
                            idx,
                            active,
                            onehand_core::chat::AskRow::Choice(o),
                            quick,
                            window,
                            cx,
                        );
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>()
    }

    /// The question itself, above the choices that answer it.
    ///
    /// On a multi-question form the tab carries the field's heading and nothing
    /// else said what was actually being asked -- a strip reading "Migration",
    /// "Also generate", "Anything else" over three unexplained options is a
    /// form answered by guessing. The description is the question and the title
    /// is the tab's word for it, so the description leads and the title stands
    /// in where the agent sent only one of the two.
    ///
    /// A single-question form is the exception and prints nothing here: its
    /// question *is* the card's heading, so a second copy under it asks twice.
    fn question(&self) -> Option<String> {
        let field = self.a.req.fields.get(self.active)?;
        (self.a.req.fields.len() > 1)
            .then(|| field.description.clone().or_else(|| field.title.clone()))
            .flatten()
    }

    /// The free-text box, where the question offers one.
    ///
    /// The choices above it are what the agent thought of; this is the answer
    /// it did not, and a form that shows only the first is a question the user
    /// cannot actually answer.
    fn custom_row(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let state = self
            .a
            .has_custom(self.active)
            .then(|| {
                self.session
                    .read(cx)
                    .ask_input(self.idx, self.active)
                    .cloned()
            })
            .flatten()?;
        let key = ask_key_hint(self.a.row_count(self.active));
        // The keyboard's position on the list, marked with the edge a focused
        // field draws: the arrows reach this row without handing it the
        // caret, and a row they reach shows it as the choices above do.
        let cursor = self.a.cursor_row(self.active) == Some(onehand_core::chat::AskRow::Custom);
        Some(
            // A form's field, at the height a one-line choice stands at.
            text_input(&state, cx)
                .short()
                .when(cursor, |field| field.border_color(cx.theme().ring))
                .when_some(key, |field, key| field.suffix(key)),
        )
    }

    /// What closes the card: Skip, then the forward button. The keys are
    /// named in the tooltips; the numbers are on the rows.
    fn footer(&self, cx: &App) -> gpui::Div {
        let (session, idx, active, quick) = (self.session, self.idx, self.active, self.quick);
        // The forward button is about *this* question: a form is walked through
        // one at a time, so arming Submit off an answer three tabs back would
        // offer to send while the question on screen is blank.
        let can_advance = self.a.field_answered(active);
        let last = self.a.is_last(active);

        card_foot(cx)
            .children((!quick).then(|| {
                crate::controls::action(("ask-skip", idx))
                    .quiet(cx)
                    .short()
                    .label("Skip")
                    .tooltip_with_action(
                        "Skip this question",
                        &super::QuestionSkip,
                        Some("QuestionCard"),
                    )
                    .on_click({
                        let session = session.clone();
                        move |_, window: &mut Window, cx: &mut App| {
                            ask_skip(&session, idx, active, window, cx);
                        }
                    })
            }))
            .child(
                crate::controls::action(("ask-submit", idx))
                    .primary()
                    .short()
                    // Nothing is picked yet: refusing, and the pointer would
                    // promise otherwise.
                    .refuses((!can_advance).then_some("Pick or type an answer first"))
                    .map(|button| match last {
                        true => button.label("Submit"),
                        false => button.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .child("Next")
                                .child(Icon::new(IconName::ArrowRight).xsmall()),
                        ),
                    })
                    .on_click({
                        let session = session.clone();
                        move |_, _, cx: &mut App| {
                            ask_advance(&session, idx, active, cx);
                        }
                    }),
            )
    }

    /// The card's own key handling, hung on the focus handle it was built with.
    ///
    /// Separate from the boxes above because it answers a different question:
    /// those say what the form looks like, this says what a keystroke landing
    /// anywhere on it means.
    fn keys(&self, body: gpui::Div, focus: gpui::FocusHandle) -> gpui::Stateful<gpui::Div> {
        let (idx, active, quick) = (self.idx, self.active, self.quick);
        let session = self.session.clone();
        let card = focus.clone();
        body.id(("ask-card", idx))
            .key_context("QuestionCard")
            .track_focus(&focus)
            // **The free-text box takes every key while it has the caret**:
            // every one of these is bound outside a field, so a "1" typed into
            // the box is a "1" rather than a jump to the first choice.
            .on_action({
                let (session, card) = (session.clone(), card.clone());
                move |_: &super::QuestionUp, window, cx| {
                    walk(&session, idx, active, -1, &card, window, cx)
                }
            })
            .on_action({
                let (session, card) = (session.clone(), card.clone());
                move |_: &super::QuestionDown, window, cx| {
                    walk(&session, idx, active, 1, &card, window, cx)
                }
            })
            // **Only where the card itself holds the caret.** A choice row is a
            // library `Button`, and a focused one already turns Enter into its
            // own click -- so answering here as well is two answers, which on a
            // multi-select is the choice toggled on and straight back off. The
            // row that has the caret settles itself; this is the walk's Enter,
            // for the cursor the arrows moved.
            .on_action({
                let (session, card) = (session.clone(), card.clone());
                move |_: &super::QuestionTake, window, cx| {
                    if !card.is_focused(window) {
                        return;
                    }
                    let row = session
                        .read(cx)
                        .chat
                        .ask_at(idx)
                        .and_then(|item| item.cursor_row(active));
                    if let Some(row) = row {
                        ask_take(&session, idx, active, row, quick, window, cx);
                    }
                }
            })
            // Esc passes on this question rather than refusing the whole form:
            // the card is walked one question at a time, and the key that means
            // "not this one" has to mean it at the same scale the Skip button
            // beside it does.
            .on_action({
                let session = session.clone();
                move |_: &super::QuestionSkip, window, cx| {
                    if !quick {
                        ask_skip(&session, idx, active, window, cx);
                    }
                }
            })
            // A number jumps to the row carrying it, the typed answer's box
            // included -- and a number nobody offered does nothing at all, which
            // is `row` refusing rather than rounding to the nearest.
            .on_action(move |pick: &super::QuestionPick, window, cx| {
                let row = session
                    .read(cx)
                    .chat
                    .ask_at(idx)
                    .and_then(|item| item.row(active, pick.index));
                if let Some(row) = row {
                    ask_take(&session, idx, active, row, quick, window, cx);
                }
            })
    }
}

/// The question card: the strip of questions, the open one's choices, the box
/// for an answer nobody offered, and what closes it.
///
/// Assembly only. Each part is built by [`AskForm`], because a card that draws
/// four separable regions in one function is one where a change to any of them
/// is read against the other three.
fn ask_form(
    card: gpui::Div,
    session: &Entity<ChatSession>,
    a: &AskItem,
    target: TranscriptItemId,
    idx: usize,
    cx: &App,
) -> gpui::AnyElement {
    let form = AskForm {
        session,
        a,
        idx,
        active: a.active_field(),
        quick: a.is_quick(),
    };
    let multi_field = a.req.fields.len() > 1;
    let rows = form.choice_rows(cx);
    // The quick card commits on a click and deliberately carries no footer to
    // hunt for -- but typing is not a click, so the footer appears the moment
    // there are words with no other way out. Skip is not added with it:
    // refusing the whole question is a thing that card has never offered, and
    // writing an answer is not the moment to start.
    let typed = a
        .custom
        .get(form.active)
        .is_some_and(|c| !c.trim().is_empty());

    let body = card
        .child(
            card_body()
                .gap_3()
                .children(multi_field.then(|| form.tabs(cx)))
                .children(form.question().map(|line| {
                    div()
                        .w_full()
                        .text_size(TEXT_SM)
                        .line_height(relative(LEADING_TIGHT))
                        .child(line)
                }))
                // Only the choices scroll. The tab strip is how the *other*
                // questions are reached and the box below is where an answer
                // nobody offered is written, so both are controls and both stay
                // on the card beside the footer rather than inside the region
                // that can be scrolled away from.
                .children((!rows.is_empty()).then_some(BlockingBody {
                    target,
                    children: rows,
                    inset: CARD_PAD,
                }))
                .children(form.custom_row(cx)),
        )
        .when(!form.quick || typed, |card| card.child(form.footer(cx)));

    // No handle means no card on screen to hold the keys -- the boxes and the
    // handle are built in the same pass, so this is only ever the frame a
    // question first appears in.
    match session.read(cx).ask_focus(idx).cloned() {
        Some(focus) => form.keys(body, focus).into_any_element(),
        None => body.into_any_element(),
    }
}
