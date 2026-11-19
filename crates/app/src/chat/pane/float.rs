//! A subagent's own transcript, in a float over the pane, and the chip in
//! the composer's row that counts the turn's subagents while any works.
//!
//! **Beside the conversation, never instead of it.** The float is not modal:
//! the conversation behind it still scrolls and takes presses, and the float
//! reads the pane it stands in, not the window, for how much of it to take.

use super::{ChatPane, FLOAT_BESIDE, FLOAT_W, SIDE_MARGIN_NARROW};
use crate::chat::session::ChatSession;
use crate::chat::transcript;
use crate::chat::viewport;
use crate::controls::{Named as _, Quiet as _, Refuses as _, Short as _};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Pixels,
    StatefulInteractiveElement as _, Styled, Window, div, relative,
};
use gpui_component::{ActiveTheme as _, IconName, StyledExt as _};
use onehand_core::chat::Standing;
use onehand_ui::ShortIcon as _;

gpui::actions!(subagent_float, [CloseSubagent]);

/// Which subagent the float shows: the session it is in, and the cards from
/// the one opened from the conversation down to the one on screen, so a
/// nested one has a way back.
pub(super) struct Float {
    pub(super) uid: u64,
    pub(super) stack: Vec<u64>,
    /// Whether the task is shown whole rather than cut at two lines.
    pub(super) task_open: bool,
}

/// How much of the pane's height the float takes at most, so the
/// conversation's tail and the composer stay in sight. Under that it is as
/// tall as what it holds.
const FLOAT_TALL: f32 = 0.6;
/// How many of a subagent's runs the float draws, saying so past it.
const FLOAT_RUNS: usize = 200;
/// How long a task runs before it is cut at two lines with *Show all*.
const TASK_CUT_CHARS: usize = 160;

impl ChatPane {
    /// Show subagent `key` of session `uid` in the float: on top of the one
    /// shown when it is a card inside it, in its place otherwise.
    pub(super) fn open_subagent(&mut self, uid: u64, key: u64, cx: &mut Context<Self>) {
        let inside = self
            .float
            .as_ref()
            .filter(|float| float.uid == uid)
            .and_then(|float| float.stack.last().copied())
            .zip(self.conversations.get(&uid).and_then(|conv| conv.session()))
            .is_some_and(|(top, session)| session.read(cx).chat().card_holds(top, key));
        match (inside, self.float.as_mut()) {
            (true, Some(float)) => float.stack.push(key),
            _ => {
                self.float = Some(Float {
                    uid,
                    stack: vec![key],
                    task_open: false,
                })
            }
        }
        if let Some(float) = self.float.as_mut() {
            float.task_open = false;
        }
        self.float_scroll.set_offset(gpui::Point::default());
        self.focus_float = true;
        cx.notify();
    }

    /// Put the float away, the keys handed back to the composer first: a
    /// focused element leaving the frame takes the whole keymap with it.
    fn close_subagent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reclaim_focus(window, cx);
        self.float = None;
        cx.notify();
    }

    /// Show `key` in place of the card on screen, among the ones beside it.
    fn step_subagent(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(float) = self.float.as_mut() {
            if let Some(top) = float.stack.last_mut() {
                *top = key;
            }
            float.task_open = false;
        }
        self.float_scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    /// Back to the card the one on screen was opened from.
    fn back_subagent(&mut self, cx: &mut Context<Self>) {
        if let Some(float) = self.float.as_mut() {
            if float.stack.len() > 1 {
                float.stack.pop();
            }
            float.task_open = false;
        }
        self.float_scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    /// The float, over the pane whose list is `list_w` wide; `None` while it
    /// is put away or shows another session's subagent.
    pub(super) fn subagent_float(
        &mut self,
        session: &Entity<ChatSession>,
        list_w: Pixels,
        room: transcript::Room,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let float = self
            .float
            .as_ref()
            .filter(|float| Some(float.uid) == self.active)?;
        let key = *float.stack.last()?;
        let nested = float.stack.len() > 1;
        let task_open = float.task_open;
        // Only once the card is found: focus given to a float that is not
        // drawn would take the keymap with it.
        if session.read(cx).chat().card(key).is_some() && std::mem::take(&mut self.focus_float) {
            self.float_focus.clone().focus(window, cx);
        }
        let read = session.read(cx);
        let chat = read.chat();
        let card = chat.card(key)?;
        // A nested card steps among the cards beside it in its own.
        let beside = chat.subagents_beside(key);
        let at = beside.iter().position(|seen| *seen == key).unwrap_or(0);
        let previous = at.checked_sub(1).map(|ix| beside[ix]);
        let next = beside.get(at + 1).copied();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let standing = chat.standing(card);
        let (word, ink) = (
            card.state_word(standing),
            transcript::standing_ink(standing, cx),
        );
        let facts = transcript::subagent_facts(card);
        let task = card.task.clone();
        let name = card.name.clone();

        let runs = viewport::card_runs(
            chat,
            key,
            |anchor, default| read.activity_is_open(anchor, default),
            |target, default| read.item_is_open(target, default),
        );
        // The latest runs, where a working subagent's work is, and the count of
        // the earlier ones above them.
        let over = runs.len().saturating_sub(FLOAT_RUNS);
        let body: Vec<gpui::AnyElement> = runs
            .iter()
            .skip(over)
            .map(|run| match &run.strip {
                Some(strip) => self.cluster_element(run, strip, session, &room, window, cx),
                None => div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .min_w_0()
                    .children(run.members.iter().filter_map(|&target| {
                        viewport::item(chat, target).map(|item| {
                            transcript::item(session, item, target, room.clone(), window, cx)
                                .into_any_element()
                        })
                    }))
                    .into_any_element(),
            })
            .collect();

        let icon = |id: &'static str, icon: IconName, name: &'static str| {
            crate::controls::action(id)
                .quiet_icon(false, cx)
                .short_icon()
                .icon(icon)
                .named(name)
        };
        // The name on a line of its own; under it how it stands, its steps
        // and time, and at that line's end the ways to the others and out.
        let head = div()
            .v_flex()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(name),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_none().text_color(ink).child(word))
                    .child(div().flex_none().child("·"))
                    .child(div().min_w_0().truncate().child(facts))
                    .child(div().flex_1())
                    .when(nested, |row| {
                        row.child(
                            icon("subagent-back", IconName::ArrowLeft, "Back")
                                .on_click(cx.listener(|this, _, _, cx| this.back_subagent(cx))),
                        )
                    })
                    .when(beside.len() > 1, |row| {
                        row.child(div().flex_none().child(format!(
                            "{} of {}",
                            at + 1,
                            beside.len()
                        )))
                        .child(
                            icon(
                                "subagent-previous",
                                IconName::ChevronLeft,
                                "Previous subagent",
                            )
                            .refuses(previous.is_none().then_some("This is the first"))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(key) = previous {
                                        this.step_subagent(key, cx);
                                    }
                                },
                            )),
                        )
                        .child(
                            icon("subagent-next", IconName::ChevronRight, "Next subagent")
                                .refuses(next.is_none().then_some("This is the last"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(key) = next {
                                        this.step_subagent(key, cx);
                                    }
                                })),
                        )
                    })
                    .child(icon("subagent-close", IconName::Close, "Close").on_click(
                        cx.listener(|this, _, window, cx| this.close_subagent(window, cx)),
                    )),
            );

        let task = task.map(|task| {
            let cut = task.lines().count() > 2 || task.chars().count() > TASK_CUT_CHARS;
            div()
                .v_flex()
                .gap_1()
                .px_3()
                .pt_2()
                .text_xs()
                .text_color(muted)
                .child(
                    div()
                        .min_w_0()
                        .when(!task_open, |text| text.line_clamp(2))
                        .child(task),
                )
                .when(cut, |column| {
                    column.child(
                        div().h_flex().child(
                            crate::controls::action("subagent-task")
                                .quiet(cx)
                                .short()
                                .label(match task_open {
                                    true => "Show less",
                                    false => "Show all",
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(float) = this.float.as_mut() {
                                        float.task_open = !float.task_open;
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                })
        });

        let rem = window.rem_size();
        let wide = list_w >= FLOAT_BESIDE.to_pixels(rem);
        let inset = SIDE_MARGIN_NARROW;
        Some(
            div()
                .id("subagent-float")
                .key_context("SubagentFloat")
                .track_focus(&self.float_focus)
                .on_action(cx.listener(|this, _: &CloseSubagent, window, cx| {
                    this.close_subagent(window, cx)
                }))
                .absolute()
                .top(inset)
                .right(inset)
                .max_h(relative(FLOAT_TALL))
                .when(wide, |float| float.w(FLOAT_W))
                .when(!wide, |float| float.left(inset))
                .v_flex()
                .overflow_hidden()
                .occlude()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .shadow_lg()
                .text_size(transcript::TEXT_SM)
                .child(head)
                .children(task)
                .child(
                    // As tall as what it holds, and giving way to the cap: a
                    // body stretched to fill the float stood a column of
                    // empty surface under a subagent of three lines.
                    div()
                        .id("subagent-float-body")
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.float_scroll)
                        .v_flex()
                        .gap_2()
                        .p_3()
                        .when(over > 0, |body| {
                            body.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{over} earlier rows not shown")),
                            )
                        })
                        .children(body),
                )
                .into_any_element(),
        )
    }

    /// The subagents' chip in the composer's row while any of the turn's
    /// subagents works: a count per standing, most urgent first, in marks.
    /// Pressed, the float opens on the most urgent.
    pub(super) fn subagent_chip(
        &self,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let chat = session.read(cx).chat();
        let uid = chat.uid;
        let stood = chat.turn_subagents();
        let live = stood
            .iter()
            .any(|(_, standing)| matches!(standing, Standing::Waiting | Standing::Running));
        if !live {
            return None;
        }
        let (urgent, _) = *stood.iter().min_by_key(|(_, standing)| *standing)?;
        let counts = chat.turn_subagent_counts();
        let said: Vec<String> = counts
            .iter()
            .map(|(standing, n)| format!("{n} {}", standing.word()))
            .collect();
        let name = format!(
            "{}: {}. Open the most urgent",
            match stood.len() {
                1 => "1 subagent".to_string(),
                n => format!("{n} subagents"),
            },
            said.join(", ")
        );
        Some(
            crate::chat::composer::status_chip(
                "subagent-chip",
                gpui_component::Icon::new(crate::icons::Icon::CornerDownRight),
                counts
                    .into_iter()
                    .map(|(standing, n)| (standing_signal(standing), n))
                    .collect(),
                name,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_subagent(uid, urgent, cx)))
            .into_any_element(),
        )
    }
}

/// A subagent's standing in the mark the rail gives a session in the like
/// state, so the composer's row and the rail read one set of marks: waiting
/// on the person is the hand, failed the alert, running the dot, done the
/// check, stopped the bare ring.
fn standing_signal(standing: Standing) -> Option<super::SessionSignal> {
    use super::SessionSignal;
    match standing {
        Standing::Waiting => Some(SessionSignal::AwaitingUser),
        Standing::Failed => Some(SessionSignal::Failed),
        Standing::Running => Some(SessionSignal::Busy),
        Standing::Done => Some(SessionSignal::UnseenTurn),
        Standing::Stopped => None,
    }
}
