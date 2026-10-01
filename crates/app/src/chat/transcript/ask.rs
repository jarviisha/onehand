use super::metrics::{
    ASK_CHOICE_DOT, ASK_CHOICE_MARK, ASK_CUSTOM_ROW_MIN, ASK_HINT_PAD, ASK_HINT_SIZE, ASK_INSET,
    ASK_MARK_RING, ASK_PROMPT_LEADING, ASK_ROW_MIN, ASK_TAB_H, ASK_TAB_MARK, ASK_TAB_W, BUTTON_H,
    PART_GAP, WORK_TEXT,
};
use super::parts::{
    ActivityRow, BlockingBody, Object, RowMark, activity_row, detail_well, floating_card, grows,
    pill,
};
use super::{fold_key, live_index};
use crate::chat::session::ChatSession;
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, relative,
};
use gpui_component::Disableable as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::acp::ElicitKind;
use onehand_core::chat::{AskItem, TranscriptItemId};

/// The settled record of a question.
fn ask_record(
    session: &Entity<ChatSession>,
    a: &AskItem,
    answer: &str,
    target: TranscriptItemId,
    cx: &App,
) -> gpui::AnyElement {
    let pairs = ask_pairs(a);
    // One question is already said whole by the row; several are a list worth
    // opening, and the count is what says there is one.
    let many = pairs.len() > 1;
    let toggle = {
        let session = session.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            session.update(cx, |s, cx| {
                s.chat.toggle_ask(target);
                cx.notify();
            });
        }
    };

    let row = ActivityRow::new(
        ("ask-record", fold_key(target)).into(),
        RowMark::Done,
        user_icon(),
        "Asked",
    )
    .object(Some(Object::plain(match many {
        // The prompt that introduced the form, with the answers in the
        // column beside it -- a strip of question titles would be the
        // form's own chrome rather than what came of it.
        true => a.req.message.to_string(),
        false => pairs
            .first()
            .map(|(question, _)| question.to_string())
            .unwrap_or_else(|| a.req.message.to_string()),
    })))
    .meta(Some(match many {
        true => pill(
            format!("{} answers", pairs.len()),
            cx.theme().muted_foreground,
            cx,
        )
        .into_any_element(),
        false => pill(answer.to_string(), cx.theme().muted_foreground, cx).into_any_element(),
    }))
    .fold(many.then_some(a.expanded));

    div()
        .v_flex()
        .w_full()
        .min_w_0()
        .child(activity_row(row, toggle, cx))
        .when(many && a.expanded, |block| {
            block.child(detail_well(
                div()
                    .v_flex()
                    .w_full()
                    .min_w_0()
                    // **No rules between the pairs.** They are one answer given
                    // in several parts, not several records -- ruled apart they
                    // read as separate exchanges with the agent, which is the
                    // one thing a form is not.
                    .children(pairs.into_iter().map(|(question, chosen)| {
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap(PART_GAP)
                            .w_full()
                            .min_w_0()
                            .h(BUTTON_H)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(question),
                            )
                            .child(div().flex_none().max_w(relative(0.5)).child(pill(
                                chosen,
                                cx.theme().foreground,
                                cx,
                            )))
                    })),
            ))
        })
        .into_any_element()
}

/// Each question of a settled form paired with what was chosen for it.
///
/// A typed answer and a picked one come back the same shape, because from the
/// far side of the exchange they are the same thing: what the user said. Which
/// of the two it was is carried by weight, not by a different control.
fn ask_pairs(a: &AskItem) -> Vec<(SharedString, SharedString)> {
    a.req
        .fields
        .iter()
        .enumerate()
        .map(|(f, field)| {
            let question = field
                .title
                .clone()
                .or_else(|| field.description.clone())
                .unwrap_or_else(|| format!("Question {}", f + 1));
            let typed = a.custom.get(f).map(String::as_str).unwrap_or("").trim();
            let chosen = match typed.is_empty() {
                false => typed.to_string(),
                true => {
                    let picks = a.picked.get(f).cloned().unwrap_or_default();
                    let choices = match &field.kind {
                        ElicitKind::Select(c) | ElicitKind::MultiSelect(c) => c.as_slice(),
                        ElicitKind::Text => &[],
                    };
                    picks
                        .iter()
                        .filter_map(|&i| choices.get(i).map(|c| c.label.to_string()))
                        .collect::<Vec<_>>()
                        .join(" · ")
                }
            };
            (
                SharedString::from(question),
                SharedString::from(match chosen.is_empty() {
                    true => "skipped".to_string(),
                    false => chosen,
                }),
            )
        })
        .collect()
}

/// The mark on a row that is about the person rather than the agent.
pub(super) fn user_icon() -> SharedString {
    use gpui_component::IconNamed as _;
    IconName::User.path()
}

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

    floating_card(cx)
        .v_flex()
        .p_0()
        // **The card's own padding goes to the rows inside it.** The heading
        // and the tab strip are separated by a rule that has to run edge to
        // edge, and a rule inside a padded box stops short of the corners it is
        // squaring off -- which reads as a line somebody drew rather than as
        // the edge of a region.
        //
        // The corners clip what is inside them for the same reason from the
        // other end: the strip's hairline and the footer's both run the full
        // width, so without it each ends in a square nib a pixel outside the
        // rounded edge above it.
        .overflow_hidden()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .px_4()
                .pt_3()
                .child(Icon::new(IconName::Info).size_4())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_semibold()
                        .child(a.req.message.clone()),
                )
                // Where the reader is in the form, in words, because the tab
                // strip says which question is open but not how many are left:
                // three numbered tabs read as three steps only once you have
                // counted them.
                .children(counter.map(|line| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(line)
                })),
        )
        // A question replayed from the archive carries an rpc id no running
        // adapter issued: showing controls would invite an answer nobody is
        // waiting for. An answered one never reaches here -- it is a row.
        .map(|card| match idx {
            None => card.into_any_element(),
            Some(idx) => ask_form(card, session, a, target, idx, cx),
        })
}

/// Take one row of the question showing at `field`.
///
/// **One function behind the click and the key**, because they are the same
/// gesture: a row aimed at with the pointer and a row walked to with the arrows
/// both end in the same answer, and two copies of "what picking does" is how a
/// number key comes to leave the typed answer sitting in a box the click would
/// have emptied.
fn ask_take(
    session: &Entity<ChatSession>,
    idx: usize,
    field: usize,
    row: onehand_core::chat::AskRow,
    quick: bool,
    window: &mut Window,
    cx: &mut App,
) {
    use onehand_core::chat::AskRow;

    match row {
        // The typed answer is a row of the list, so reaching it is reaching its
        // box: the keyboard hands the caret over rather than answering for the
        // user, which is the one row that cannot be settled by arriving at it.
        AskRow::Custom => {
            let state = session.read(cx).ask_input(idx, field).cloned();
            if let Some(state) = state {
                let handle = state.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                session.update(cx, |s, cx| {
                    if let Some(item) = s.chat.ask_at_mut(idx) {
                        item.cursor = item.row_count(field).saturating_sub(1);
                    }
                    cx.notify();
                });
            }
        }
        AskRow::Choice(option) => session.update(cx, |s, cx| {
            // Picking is the user choosing the agent's wording over their own,
            // so the box goes with it -- model and widget together, since
            // neither clears the other.
            s.clear_ask_input(idx, field, window, cx);
            if let Some(item) = s.chat.ask_at_mut(idx) {
                item.toggle(field, option);
                item.cursor = option;
            }
            // A one-question single-select has nothing left to decide, so it
            // commits on the press.
            if quick {
                s.chat.answer_ask(idx, false);
            }
            cx.notify();
        }),
    }
}

/// Pass on the question showing at `field`, and settle the form if it was the
/// last one.
///
/// What is sent then is whatever the *other* questions hold: a form skipped
/// through to the end with nothing filled in is a refusal, and one with answers
/// above the skipped question is those answers. Declining a form that carries
/// work already done would throw it away at the last press.
fn ask_skip(
    session: &Entity<ChatSession>,
    idx: usize,
    field: usize,
    window: &mut Window,
    cx: &mut App,
) {
    session.update(cx, |s, cx| {
        s.clear_ask_input(idx, field, window, cx);
        let done = s
            .chat
            .ask_at_mut(idx)
            .is_none_or(|item| item.skip_field(field));
        if done {
            let answered = s.chat.ask_at_mut(idx).is_some_and(|item| item.has_answer());
            s.chat.answer_ask(idx, !answered);
        }
        cx.notify();
    });
}

/// Move the form on from `field`, or submit it where that was the last
/// question.
fn ask_advance(session: &Entity<ChatSession>, idx: usize, field: usize, cx: &mut App) {
    session.update(cx, |s, cx| {
        let last = s.chat.ask_at_mut(idx).is_none_or(|item| {
            let last = item.is_last(field);
            if !last {
                item.go_to(field + 1);
            }
            last
        });
        if last {
            s.chat.answer_ask(idx, false);
        }
        cx.notify();
    });
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

/// The key hint at the right-hand end of a row, and the number that reaches it.
///
/// **A row a keyboard can reach says so on the row.** The hints at the foot of
/// the card name the arrows and Enter, which is the walk; this is the jump, and
/// a jump has to be to something the user can already see a name for. It is
/// held off shrinking because it is two characters at most and the words beside
/// it are the agent's -- a paragraph of description would otherwise squeeze the
/// one part of the row that is fixed-length.
///
/// `None` past the ninth row, which is a row with no key rather than a row with
/// an unusable one: nothing is drawn, and the words beside it take the space.
fn ask_key_hint(n: usize, cx: &App) -> Option<impl IntoElement + use<>> {
    if n > ASK_KEY_ROWS {
        return None;
    }
    Some(ask_key_mark(n, cx))
}

fn ask_key_mark(n: usize, cx: &App) -> impl IntoElement + use<> {
    div()
        .flex_none()
        .min_w(ASK_HINT_SIZE)
        .h(ASK_HINT_SIZE)
        .px(ASK_HINT_PAD)
        .h_flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(format!("{n}"))
}

/// One numbered step of the tab strip.
///
/// **The number is the point, until the question has an answer.** A strip of
/// titles says these are three things; a strip of *numbered* titles says they
/// are three things in an order, with a first and a last -- which is the only
/// question a reader has about a form they are partway through. Once a question
/// is answered its number has done that job and the tick replaces it: what is
/// left to do is then readable as the tabs still carrying digits, without
/// counting anything. The two never show together, because one 18px circle
/// holds one mark.
fn ask_tab_mark(index: usize, active: bool, answered: bool, cx: &App) -> impl IntoElement + use<> {
    let (fill, ink) = match (active, answered) {
        // Open: the strongest of the three, because it is the one the choices
        // below belong to.
        (true, _) => (cx.theme().primary, cx.theme().primary_foreground),
        // Answered and not open: filled, quietly, so what is left to do is
        // readable as the ones that are *not* filled.
        (false, true) => (cx.theme().accent, cx.theme().accent_foreground),
        (false, false) => (cx.theme().transparent, cx.theme().muted_foreground),
    };
    div()
        .flex_none()
        .size(ASK_TAB_MARK)
        .rounded_full()
        .bg(fill)
        .when(!active && !answered, |mark| {
            mark.border_1().border_color(cx.theme().border)
        })
        .h_flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(ink)
        .map(|mark| match answered {
            true => mark.child(Icon::new(IconName::Check).size_3()),
            false => mark.child(format!("{}", index + 1)),
        })
}

/// The mark at the head of a choice row.
///
/// Round for a single-select and square for a multi-select, which is the one
/// convention this app inherits rather than invents: a reader who has met a
/// form before already knows that a circle means *instead of* and a box means
/// *as well as*, and nothing else on the row says it. Drawn from two divs
/// rather than an icon because the bundled set has neither shape as a control
/// -- its `circle-check` is an outcome, not a thing waiting to be chosen.
///
/// **Its ring is ink and not the hairline**, which is the difference between a
/// control and an edge. Drawn in the hairline it vanished the moment the row
/// was hovered: a ghost button's hover fill is derived from the same step of
/// the ramp the hairline sits on, so in the dark palette the two land within a
/// shade of each other and the ring is painted onto its own background. A mark
/// that disappears under the pointer disappears exactly when it is being aimed
/// at.
fn ask_choice_mark(on: bool, single: bool, cx: &App) -> impl IntoElement + use<> {
    div()
        .flex_none()
        .size(ASK_CHOICE_MARK)
        .h_flex()
        .items_center()
        .justify_center()
        // Half a pixel over the hairline everything else on the card is drawn
        // with, which is the whole of the difference between an edge and a
        // control: this ring is the thing being aimed at, and at one pixel it
        // reads as the seam of the row rather than as the mark inside it.
        .border(ASK_MARK_RING)
        .border_color(match on {
            true => cx.theme().primary,
            false => cx.theme().muted_foreground,
        })
        .map(|mark| match single {
            true => mark.rounded_full(),
            false => mark.rounded(cx.theme().radius),
        })
        .when(on, |mark| {
            mark.child(
                div()
                    .size(ASK_CHOICE_DOT)
                    .bg(cx.theme().primary)
                    .map(|dot| match single {
                        true => dot.rounded_full(),
                        false => dot.rounded_sm(),
                    }),
            )
        })
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
    fn tabs(&self, cx: &App) -> gpui::Stateful<gpui::Div> {
        let (session, a, idx, active) = (self.session, self.a, self.idx, self.active);
        div()
            .id(("ask-tabs", idx))
            .h_flex()
            .gap_1()
            .w_full()
            .px_3()
            .pt_2()
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
                    .ghost()
                    .small()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .h(ASK_TAB_H)
                    .px_2p5()
                    .rounded_none()
                    // **Underlined rather than filled.** A filled tab in a
                    // strip that already carries a filled number in each of its
                    // own tabs is two fills arguing about which one means
                    // "here"; the rule under the open tab lands on the strip's
                    // own hairline and reads as the one continuing into the
                    // body below it.
                    .border_b_2()
                    .border_color(match open {
                        true => cx.theme().primary,
                        false => cx.theme().transparent,
                    })
                    .child(ask_tab_mark(f, open, answered, cx))
                    .child(
                        div()
                            // Only a *tab* label is cut short; a choice never
                            // is. It is held to one line as well as to a width:
                            // a title long enough to wrap turns the strip three
                            // rows tall while the row height stays at one, so
                            // the second and third lines are drawn over the
                            // choices below.
                            .max_w(ASK_TAB_W)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .truncate()
                            .text_sm()
                            .when(open, |label| {
                                label.font_semibold().text_color(cx.theme().foreground)
                            })
                            .when(!open, |label| label.text_color(cx.theme().muted_foreground))
                            .child(label),
                    )
                    // Jumping back restores what that question already holds,
                    // which costs nothing to arrange: every answer is kept per
                    // field, so the tab only has to move the view.
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
                let mut words = div().v_flex().gap_0p5().flex_1().min_w_0().child(
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
                            .text_size(WORK_TEXT)
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    );
                }
                grows(crate::controls::action(("ask-choice", o)))
                    .ghost()
                    .px_3()
                    .py_2()
                    .w_full()
                    .min_h(ASK_ROW_MIN)
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    // **A taken choice is named by its border, and a destructive
                    // one by the danger step of that same border.** Taking it is
                    // still one press, so the tint is not a refusal -- it is the
                    // one row on the card that cannot be pressed a second time to
                    // undo, and the only place to say so is the row itself.
                    .border_color(match (on, destructive) {
                        (true, true) => crate::theme::status_ink(cx).danger,
                        (true, false) => cx.theme().primary,
                        // The keyboard's own position, which has to be visible
                        // without being an answer: an arrow press moves this and
                        // settles nothing, so it is the hairline lifted to full
                        // ink rather than a third colour.
                        (false, _) if cursor == Some(onehand_core::chat::AskRow::Choice(o)) => {
                            cx.theme().ring
                        }
                        (false, _) => cx.theme().border,
                    })
                    // **One child, holding the row's own layout.** A `Button` wraps
                    // whatever the call site gives it in a content box of the
                    // library's own -- centred, with the gap its `Size` chose -- so
                    // a flex written out here lands on a box with exactly one thing
                    // in it and decides nothing at all. That is why the mark sat a
                    // quarter-rem from the words it belongs to while this said
                    // `gap_2p5`, and why aligning it to the top of a two-line
                    // choice did nothing either.
                    // **The mark and the hint sit on the row's middle, not on its
                    // first line.** Both are one shape against a text column that
                    // is one line or three depending on what the agent wrote, so
                    // pinned to the top they line up with the label on a
                    // described choice and with nothing at all on a bare one --
                    // the column of marks down the left comes out ragged for a
                    // reason that is about the wording rather than about the
                    // control.
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_3()
                            .w_full()
                            .child(ask_choice_mark(on, single, cx))
                            .child(words)
                            .children(ask_key_hint(o + 1, cx)),
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
    fn custom_row(&self, cx: &App) -> Option<gpui::Div> {
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
        let cursor = self.a.cursor_row(self.active);
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_3()
                .w_full()
                .px_3()
                .min_h(ASK_CUSTOM_ROW_MIN)
                .rounded(cx.theme().radius_lg)
                .border_1()
                // **Dashed** rather than solid, which is the one thing that
                // separates it from the rows above without taking it out of
                // the list: the agent's wording is fixed and this line is not
                // yet written.
                .border_dashed()
                .border_color(match cursor == Some(onehand_core::chat::AskRow::Custom) {
                    true => cx.theme().ring,
                    false => cx.theme().border,
                })
                .child(
                    Icon::new(crate::icons::Icon::SquarePen)
                        .size_4()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground),
                )
                // The row is the border, so the field inside it draws none: two
                // rings around one input read as two inputs, and the inner one
                // lands a hair inside the outer with the gap between them
                // reading as a mistake.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&state).appearance(false)),
                )
                .children(ask_key_hint(self.a.row_count(self.active), cx)),
        )
    }

    /// The strip that closes the card: what the keyboard can do, then Skip and
    /// the forward button.
    fn footer(&self, cx: &App) -> gpui::Div {
        let (session, idx, active, quick) = (self.session, self.idx, self.active, self.quick);
        // The forward button is about *this* question: a form is walked through
        // one at a time, so arming Submit off an answer three tabs back would
        // offer to send while the question on screen is blank.
        let can_advance = self.a.field_answered(active);
        let last = self.a.is_last(active);

        div()
            // **A strip of its own, over a rule that spans the card.** The
            // buttons here end the block everything is waiting on, and left
            // inside the body's padding they read as the last row of the
            // list rather than as what closes it. The rule is the same
            // pixel the tab strip's is, for the same reason: it is where a
            // region ends.
            .child(div().w_full().h_px().bg(cx.theme().border))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .px_4()
                    .py_2p5()
                    // What the keyboard can do, said where the keyboard's
                    // work ends. The numbers are on the rows themselves --
                    // this is the walk, which has nowhere else to be named.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(match quick {
                                true => "Enter choose",
                                false => "↑↓ move · Enter choose · Esc skip",
                            }),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_none()
                            .children((!quick).then(|| {
                                crate::controls::action(("ask-skip", idx))
                                    .secondary()
                                    .small()
                                    .label("Skip")
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
                                    .small()
                                    .map(|submit| match can_advance {
                                        true => submit,
                                        // Nothing is picked yet, so the
                                        // pointer would be promising a
                                        // press that does nothing.
                                        false => crate::controls::resting(submit),
                                    })
                                    .disabled(!can_advance)
                                    .label(match last {
                                        false => "Next →",
                                        true => "Submit",
                                    })
                                    .on_click({
                                        let session = session.clone();
                                        move |_, _, cx: &mut App| {
                                            ask_advance(&session, idx, active, cx);
                                        }
                                    }),
                            ),
                    ),
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
            .track_focus(&focus)
            .on_key_down(move |event, window, cx| {
                let keystroke = &event.keystroke;
                // A modified key is somebody else's: Ctrl+1 switches sessions and
                // Shift+Enter is a newline in whatever holds the caret.
                if keystroke.modifiers.modified() {
                    return;
                }
                let session = session.clone();
                // **The free-text box takes every key while it has the caret.** It
                // is inside the card, so a key press there reaches this listener on
                // its way out -- and the answers this card is shortest about are
                // digits, which is exactly what somebody writing their own answer
                // types. Unguarded, a "1" in that box jumps to the first choice and
                // empties the line being written.
                let typing = session
                    .read(cx)
                    .ask_input(idx, active)
                    .map(|state| state.read(cx).focus_handle(cx))
                    .is_some_and(|handle| handle.is_focused(window));
                if typing {
                    return;
                }
                match keystroke.key.as_str() {
                    "up" | "down" => {
                        let delta = if keystroke.key == "up" { -1 } else { 1 };
                        // The walk takes the caret back off whichever row was last
                        // clicked, or the cursor and the focus point at two
                        // different rows and Enter answers the one the eye is not
                        // on.
                        window.focus(&card, cx);
                        session.update(cx, |s, cx| {
                            if let Some(item) = s.chat.ask_at_mut(idx) {
                                item.move_cursor(active, delta);
                            }
                            cx.notify();
                        });
                    }
                    // **Only where the card itself holds the caret.** A choice row
                    // is a library `Button`, and a focused one already turns Enter
                    // into its own click -- so answering here as well is two
                    // answers, which on a multi-select is the choice toggled on and
                    // straight back off. The row that has the caret settles itself;
                    // this is the walk's Enter, for the cursor the arrows moved.
                    "enter" if card.is_focused(window) => {
                        let row = session
                            .read(cx)
                            .chat
                            .ask_at(idx)
                            .and_then(|item| item.cursor_row(active));
                        if let Some(row) = row {
                            ask_take(&session, idx, active, row, quick, window, cx);
                        }
                    }
                    "enter" => {}
                    // Esc passes on this question rather than refusing the whole
                    // form: the card is walked one question at a time, and the key
                    // that means "not this one" has to mean it at the same scale
                    // the Skip button beside it does.
                    "escape" if !quick => ask_skip(&session, idx, active, window, cx),
                    // A number jumps to the row carrying it, the typed answer's box
                    // included -- and a number nobody offered does nothing at all,
                    // which is `row` refusing rather than rounding to the nearest.
                    key => {
                        let Some(n) = key.parse::<usize>().ok().filter(|&n| n >= 1) else {
                            return;
                        };
                        let row = session
                            .read(cx)
                            .chat
                            .ask_at(idx)
                            .and_then(|item| item.row(active, n - 1));
                        if let Some(row) = row {
                            ask_take(&session, idx, active, row, quick, window, cx);
                        }
                    }
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
        .children(multi_field.then(|| form.tabs(cx)))
        // The strip's rule spans the card and not just the tabs: the tabs are a
        // label on the body below them, and it is the body that has an edge.
        .when(multi_field, |card| {
            card.child(div().w_full().h_px().bg(cx.theme().border))
        })
        .child(
            div()
                .v_flex()
                .gap_3()
                .w_full()
                .px(ASK_INSET)
                .pt_3p5()
                .pb_4()
                .children(form.question().map(|line| {
                    div()
                        .w_full()
                        .text_sm()
                        .line_height(relative(ASK_PROMPT_LEADING))
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
                    inset: ASK_INSET,
                }))
                .children(form.custom_row(cx)),
        )
        // The rule over the footer is the footer's own, drawn as its first
        // child. Added again here it came out twice, two pixels apart, which
        // reads as a border that failed rather than as an edge — and the
        // permission card beside it draws one.
        .when(!form.quick || typed, |card| card.child(form.footer(cx)));

    // No handle means no card on screen to hold the keys -- the boxes and the
    // handle are built in the same pass, so this is only ever the frame a
    // question first appears in.
    match session.read(cx).ask_focus(idx).cloned() {
        Some(focus) => form.keys(body, focus).into_any_element(),
        None => body.into_any_element(),
    }
}
