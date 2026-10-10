//! The live question card: the open question's choices, the box for an answer
//! nobody offered, the strip of questions on a form with several, and the keys
//! that answer it.

use super::pinned::{CARD_INSET, Footer, Keys, Pinned, key_cap, pinned_card};
use crate::chat::session::ChatSession;
use crate::chat::transcript::parts::BlockingBody;
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Entity, InteractiveElement, IntoElement, ParentElement, Rems, SharedString,
    StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::radio::Radio;
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, StyledExt,
};
use onehand_core::acp::ElicitKind;
use onehand_core::chat::{AskItem, AskRow, TranscriptItemId};

/// A tab never grows past this; a longer question's title truncates, and one
/// line long it never runs into the choices below.
const TAB_MAX_W: Rems = rems(8.75);

/// The highest row a single digit reaches, and so the last one offered a key.
///
/// The handler reads one keystroke, not a typed number, so a row past the
/// ninth has no key and must not be drawn carrying one: reaching for `10` on
/// the card that answers on the press lands on `1`.
const ASK_KEY_ROWS: usize = 9;

/// The question card, live or replayed.
pub(super) fn ask_card(
    session: &Entity<ChatSession>,
    a: &AskItem,
    target: TranscriptItemId,
    idx: Option<usize>,
    cx: &App,
) -> gpui::AnyElement {
    let agent = session.read(cx).chat.agent.clone();
    let active = a.active_field();
    let multi = matches!(
        a.req.fields.get(active).map(|f| &f.kind),
        Some(ElicitKind::MultiSelect(_))
    );
    let meta = match multi {
        true => format!("{agent} asks · pick any"),
        false => format!("{agent} asks"),
    };
    // A question replayed from the archive carries an rpc id no running
    // adapter issued: controls would invite an answer nobody is waiting for.
    let Some(idx) = idx else {
        return pinned_card(
            Pinned {
                icon: Icon::new(IconName::Info),
                title: a.req.message.clone().into(),
                meta: meta.into(),
                body: Vec::new(),
                footer: None,
            },
            cx,
        )
        .into_any_element();
    };
    let form = AskForm {
        session,
        a,
        idx,
        active,
        quick: a.is_quick(),
    };
    // The quick card commits on a click and carries no footer to hunt for --
    // but typing is not a click, so the footer appears the moment there are
    // words with no other way out.
    let typed = a.custom.get(active).is_some_and(|c| !c.trim().is_empty());
    let rows = form.choice_rows(cx);
    let mut body: Vec<gpui::AnyElement> = Vec::new();
    if a.req.fields.len() > 1 {
        body.push(form.tabs(cx).into_any_element());
    }
    if let Some(line) = form.description() {
        body.push(
            div()
                .w_full()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(line)
                .into_any_element(),
        );
    }
    // Only the choices scroll: the strip and the box below are controls, and
    // stay on the card beside the footer.
    if !rows.is_empty() {
        body.push(
            BlockingBody {
                target,
                children: rows,
                inset: CARD_INSET,
            }
            .into_any_element(),
        );
    }
    body.extend(form.custom_row(cx).map(IntoElement::into_any_element));

    let card = pinned_card(
        Pinned {
            icon: Icon::new(IconName::Info),
            title: a.req.message.clone().into(),
            meta: meta.into(),
            body,
            footer: (!form.quick || typed).then(|| form.footer()),
        },
        cx,
    );
    // No handle means no card on screen to hold the keys yet: the boxes and
    // the handle are built in the same pass, so this is only the first frame.
    match session.read(cx).ask_focus(idx).cloned() {
        Some(focus) => form.keys(card, focus).into_any_element(),
        None => card.into_any_element(),
    }
}

/// Take one row of the question showing at `field`: one function behind the
/// click and the key, because they are the same gesture.
fn ask_take(
    session: &Entity<ChatSession>,
    idx: usize,
    field: usize,
    row: AskRow,
    quick: bool,
    window: &mut Window,
    cx: &mut App,
) {
    match row {
        // The typed answer is a row of the list, so reaching it is reaching its
        // box: the caret is handed over rather than an answer given.
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
            // Picking is choosing the agent's wording over one's own, so the
            // box goes with it -- model and widget together.
            s.clear_ask_input(idx, field, window, cx);
            if let Some(item) = s.chat.ask_at_mut(idx) {
                item.toggle(field, option);
                item.cursor = option;
            }
            // A one-question single-select has nothing left to decide.
            if quick {
                s.chat.answer_ask(idx, false);
            }
            cx.notify();
        }),
    }
}

/// Pass on the question showing at `field`, and settle the form if it was the
/// last one: with nothing filled in that is a refusal, and with answers above
/// it those answers.
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

/// Move the form on from `field`, or submit it where that was the last question.
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

/// The key cap at a row's end, where a single digit reaches it.
fn row_key(n: usize) -> Option<gpui_component::kbd::Kbd> {
    (n <= ASK_KEY_ROWS)
        .then(|| key_cap(&n.to_string()))
        .flatten()
}

/// The parts of a live question card, drawn against the same five facts.
///
/// **One question at a time.** A form with several draws a strip of them and
/// only the open one's choices. **The forward button is what moves the form
/// on**, not the pick, so *Next* never points at a question already left. **The
/// keys belong to the card**, taken on its own focus handle, since numbers, the
/// arrows, Enter and Esc are all keys somebody is as likely to be typing into
/// the composer an inch below.
struct AskForm<'a> {
    session: &'a Entity<ChatSession>,
    a: &'a AskItem,
    idx: usize,
    /// The question the strip has open, and the one every row answers.
    active: usize,
    /// A one-question single-select: it answers on the click.
    quick: bool,
}

impl AskForm<'_> {
    /// The strip of questions, on a form that has more than one: a ghost tab
    /// each, the open one selected and an answered one ticked, then where the
    /// reader is in words.
    fn tabs(&self, cx: &App) -> gpui::Stateful<gpui::Div> {
        let (session, a, idx, active) = (self.session, self.a, self.idx, self.active);
        div()
            .id(("ask-tabs", idx))
            .h_flex()
            .items_center()
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
                crate::controls::action(("ask-tab", f))
                    .ghost()
                    .small()
                    .flex_none()
                    .selected(f == active)
                    .when(a.field_answered(f), |tab| {
                        tab.icon(Icon::new(IconName::Check).text_color(cx.theme().muted_foreground))
                    })
                    // A child and not the label, so a long title truncates
                    // inside the tab's bound instead of running past it.
                    .child(div().max_w(TAB_MAX_W).truncate().child(label))
                    // Every answer is kept per field, so jumping back only moves
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
            .child(
                div()
                    .flex_none()
                    .pl_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} of {}", active + 1, a.req.fields.len())),
            )
    }

    /// What the open question says beyond its title, said once.
    ///
    /// The card's title is the form's prompt, so a single question's own
    /// description goes here only where it says something the title does not;
    /// on a form with several, the open question's words lead, its tab's title
    /// standing in where the agent sent nothing else.
    fn description(&self) -> Option<SharedString> {
        let field = self.a.req.fields.get(self.active)?;
        let line = match self.a.req.fields.len() > 1 {
            true => field.description.clone().or_else(|| field.title.clone()),
            false => field.description.clone(),
        }?;
        (line.trim() != self.a.req.message.trim()).then(|| line.into())
    }

    /// The open question's choices: the library's radio or checkbox, the label
    /// and the agent's words about it, and the key cap that reaches the row.
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
        let (chosen, hover) = (cx.theme().accent, cx.theme().list_hover);

        choices
            .into_iter()
            .enumerate()
            .map(|(o, choice)| {
                let session = session.clone();
                let on = picked.contains(&o);
                // The keyboard's position, visible without being an answer.
                let walked = !on && cursor == Some(AskRow::Choice(o));
                // A destructive choice says so on its own words: taking it is
                // still one press, and the row is the only place to say it.
                let destructive = choice.is_destructive();
                div()
                    .id(("ask-choice", o))
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .when(on, |row| row.bg(chosen))
                    .when(walked, |row| row.bg(hover))
                    .when(!on, |row| row.hover(|row| row.bg(hover)))
                    .child(match single {
                        true => Radio::new(("ask-radio", o)).checked(on).into_any_element(),
                        false => Checkbox::new(("ask-check", o))
                            .checked(on)
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .w_full()
                                    .text_color(match destructive {
                                        true => crate::theme::status_ink(cx).danger,
                                        false => cx.theme().foreground,
                                    })
                                    .child(choice.label.clone()),
                            )
                            .children(choice.description.clone().map(|description| {
                                div()
                                    .w_full()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(description)
                            })),
                    )
                    .children(row_key(o + 1))
                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                        ask_take(&session, idx, active, AskRow::Choice(o), quick, window, cx);
                    })
                    .into_any_element()
            })
            .collect()
    }

    /// The free-text box, where the question offers one: an *Other* row beside
    /// the choices, or the whole answer on a question that has none.
    fn custom_row(&self, cx: &App) -> Option<gpui::Stateful<gpui::Div>> {
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
        let field = self.a.req.fields.get(self.active)?;
        let typed = self
            .a
            .custom
            .get(self.active)
            .is_some_and(|c| !c.trim().is_empty());
        let walked = self.a.cursor_row(self.active) == Some(AskRow::Custom);
        let mark = match &field.kind {
            ElicitKind::Select(_) => {
                Some(Radio::new("ask-other").checked(typed).into_any_element())
            }
            ElicitKind::MultiSelect(_) => {
                Some(Checkbox::new("ask-other").checked(typed).into_any_element())
            }
            ElicitKind::Text => None,
        };
        let has_mark = mark.is_some();
        let (session, idx, active, quick) =
            (self.session.clone(), self.idx, self.active, self.quick);
        Some(
            div()
                .id(("ask-other-row", idx))
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .when(has_mark, |row| row.px_2().py_1().rounded(cx.theme().radius))
                .when(walked && has_mark, |row| row.bg(cx.theme().list_hover))
                .children(mark)
                .child(div().flex_1().min_w_0().child(Input::new(&state).small()))
                .children(row_key(self.a.row_count(self.active)).filter(|_| has_mark))
                .on_click(move |_, window: &mut Window, cx: &mut App| {
                    ask_take(&session, idx, active, AskRow::Custom, quick, window, cx);
                }),
        )
    }

    /// The footer: the keys at the left, then Skip and the forward button.
    fn footer(&self) -> Footer {
        let (session, idx, active, quick) = (self.session, self.idx, self.active, self.quick);
        // The forward button is about *this* question: arming Submit off an
        // answer three tabs back would offer to send a blank one.
        let can_advance = self.a.field_answered(active);
        let last = self.a.is_last(active);
        let keys: Keys = match quick {
            true => &[(&["enter"], "choose")],
            false => &[
                (&["up", "down"], "move"),
                (&["enter"], "choose"),
                (&["escape"], "skip"),
            ],
        };
        let skip = (!quick).then(|| {
            let session = session.clone();
            crate::controls::action(("ask-skip", idx))
                .ghost()
                .small()
                .label("Skip")
                .on_click(move |_, window: &mut Window, cx: &mut App| {
                    ask_skip(&session, idx, active, window, cx);
                })
                .into_any_element()
        });
        let forward = {
            let session = session.clone();
            crate::controls::action(("ask-submit", idx))
                .primary()
                .small()
                .map(|submit| match can_advance {
                    true => submit,
                    // Nothing is picked yet, so no pointer promising a press.
                    false => crate::controls::resting(submit),
                })
                .disabled(!can_advance)
                .label(match last {
                    false => "Next",
                    true => "Submit",
                })
                .on_click(move |_, _, cx: &mut App| {
                    ask_advance(&session, idx, active, cx);
                })
                .into_any_element()
        };
        Footer {
            keys,
            actions: skip.into_iter().chain(Some(forward)).collect(),
        }
    }

    /// The card's own key handling, hung on the focus handle it was built with.
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
                // **The free-text box takes every key while it has the caret**,
                // or a "1" typed into it jumps to the first choice.
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
                        window.focus(&card, cx);
                        session.update(cx, |s, cx| {
                            if let Some(item) = s.chat.ask_at_mut(idx) {
                                item.move_cursor(active, delta);
                            }
                            cx.notify();
                        });
                    }
                    // Only where the card itself holds the caret: a focused
                    // button turns Enter into its own click, and answering here
                    // as well would be two answers.
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
                    // Esc passes on this question, at the scale Skip does.
                    "escape" if !quick => ask_skip(&session, idx, active, window, cx),
                    // A number jumps to the row carrying it; a number nobody
                    // offered does nothing at all.
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
