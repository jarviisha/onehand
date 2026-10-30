//! The workers' region: a list of the lead on screen's workers, pinned over
//! the picked one's detail.
//!
//! **Drawn by the pane, mounted in a dock panel of its own.** A worker's
//! conversation, its run layout and the list state that paints it are the
//! pane's, as every session's are; the pane never shows a worker of an open
//! lead itself, so each list is still painted in one place only.
//!
//! **Nothing moves by itself.** The list is ordered by urgency when the
//! region opens and when a worker is picked; in between a row that changes
//! state changes in place, and a worker starting to wait picks nothing and
//! opens nothing. The picked worker's detail is its card, its transcript and,
//! only once asked for, a composer.

mod detail;
mod list;

pub use list::{WorkersBack, WorkersDown, WorkersUp, WorkersWrite};

use super::{ChatPane, ChatPaneEvent, SessionSignal};
use crate::chat::composer::{Composer, ComposerEvent};
use crate::control::Worker;
use crate::controls::Named as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, FocusHandle, IntoElement, ParentElement, Pixels, Styled, Subscription,
    Window, div,
};
use gpui::{AppContext as _, Focusable as _};
use gpui_component::input::InputEvent;
use gpui_component::{ActiveTheme as _, IconName, StyledExt as _};
use onehand_core::chat::Link;
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// What the region holds beyond what the conversations do.
pub(super) struct Workers {
    /// The worker whose detail shows.
    picked: Option<u64>,
    /// The order the list was last drawn in.
    order: Vec<u64>,
    /// Whether the next frame orders the list afresh: the region opened, a
    /// worker was picked, or the lead changed.
    refresh: bool,
    /// The lead the list was last drawn for.
    lead: Option<u64>,
    /// The worker whose composer is open, at most one.
    writing: Option<u64>,
    /// Each opened composer, kept while its session is a worker, so a draft
    /// survives the composer closing and the lead on screen changing.
    composers: HashMap<u64, (Entity<Composer>, [Subscription; 2])>,
    /// The list's, for its keys.
    focus: FocusHandle,
    /// The detail's, so a caret anywhere in it (its composer, its card) is
    /// known when the detail goes to another worker.
    detail_focus: FocusHandle,
    /// How tall the detail measured last frame: what its card is bounded
    /// against. Its own height, which the card does not change, unlike the
    /// transcript's under the card.
    detail_height: Rc<Cell<Pixels>>,
    /// Since when each session has stood as it is, by the rail's clock.
    since: HashMap<u64, Instant>,
}

impl Workers {
    pub(super) fn new(focus: FocusHandle, detail_focus: FocusHandle) -> Self {
        Self {
            picked: None,
            order: Vec::new(),
            refresh: true,
            lead: None,
            writing: None,
            composers: HashMap::new(),
            focus,
            detail_focus,
            detail_height: Rc::default(),
            since: HashMap::new(),
        }
    }
}

/// Which of two states is the more urgent, by the signal's own order; no
/// state at all is the least.
pub(super) fn urgency(a: Option<SessionSignal>, b: Option<SessionSignal>) -> Ordering {
    match a == b {
        true => Ordering::Equal,
        false if SessionSignal::most_urgent(a.into_iter().chain(b)) == a => Ordering::Less,
        false => Ordering::Greater,
    }
}

/// The workers in the order the list draws them. Fresh (`refresh`), most
/// urgent first, ties in the order the lead opened them. Otherwise the
/// `frozen` order kept, so a row never moves under a person reading it: a
/// worker gone leaves it, a new one joins its end.
fn ordered(frozen: &[u64], now: &[(u64, Option<SessionSignal>)], refresh: bool) -> Vec<u64> {
    if refresh {
        let mut fresh = now.to_vec();
        fresh.sort_by(|(_, a), (_, b)| urgency(*a, *b));
        return fresh.into_iter().map(|(uid, _)| uid).collect();
    }
    let mut kept: Vec<u64> = frozen
        .iter()
        .copied()
        .filter(|uid| now.iter().any(|(at, _)| at == uid))
        .collect();
    kept.extend(
        now.iter()
            .map(|(uid, _)| *uid)
            .filter(|uid| !frozen.contains(uid)),
    );
    kept
}

/// What *Write to …* calls a worker: its name up to the first ` · `, so
/// `b · report r1` is written to as `b`.
fn short_name(name: &str) -> &str {
    name.split(" · ").next().unwrap_or(name)
}

/// Whether a worker's detail draws its body: once its session is up, and
/// not while a first connect is all there is.
fn shows_body(present: bool, link: Link, was_live: bool) -> bool {
    present && (link != Link::Connecting || was_live)
}

impl ChatPane {
    /// The workers of the lead on screen that this pane holds, opening or
    /// open, in the order the lead opened them.
    pub fn lead_workers(&self) -> Vec<Worker> {
        let Some(lead) = self.active else {
            return Vec::new();
        };
        crate::control::workers_of(lead)
            .into_iter()
            .filter(|worker| self.conversations.contains_key(&worker.uid))
            .collect()
    }

    /// Ages by the rail's clock, pushed by the shell as they change.
    pub fn set_since(&mut self, since: HashMap<u64, Instant>) {
        self.workers.since = since;
    }

    /// Whole minutes worker `uid` has stood as it is, by the rail's clock.
    pub(super) fn worker_age(&self, uid: u64) -> u64 {
        self.workers
            .since
            .get(&uid)
            .map_or(0, |at| at.elapsed().as_secs() / 60)
    }

    /// The region is coming on screen, or asked for again: its list is
    /// ordered afresh and the most urgent worker picked.
    pub fn workers_shown(&mut self, cx: &mut Context<Self>) {
        self.follow_lead();
        self.workers.refresh = true;
        self.workers.picked = None;
        cx.notify();
    }

    /// The region drawn for the lead on screen from here on: a lead it was
    /// not drawn for starts afresh, no composer open.
    fn follow_lead(&mut self) {
        if self.workers.lead != self.active {
            self.workers.lead = self.active;
            self.workers.refresh = true;
            self.workers.picked = None;
            self.workers.writing = None;
        }
    }

    /// Pick worker `uid`, ordering the list afresh, and put the caret on the
    /// list when `caret`. A composer open on another worker closes, keeping
    /// its draft.
    pub fn open_worker(
        &mut self,
        uid: u64,
        caret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pick(uid, window, cx);
        self.workers.refresh = true;
        if caret {
            self.workers.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Pick worker `uid` without ordering the list again. A caret in the
    /// detail of the one picked before goes to the list first: that detail
    /// leaves the frame, and a focused element leaving it takes the keymap.
    fn pick(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.follow_lead();
        if self.workers.picked == Some(uid) {
            return;
        }
        self.close_writing(window, cx);
        if self.workers.detail_focus.contains_focused(window, cx) {
            self.workers.focus.focus(window, cx);
        }
        self.workers.picked = Some(uid);
        cx.notify();
    }

    /// The caret on the list: the keys' way in. The list is drawn by the
    /// next frame, before any key reaches it.
    pub fn focus_workers_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workers.focus.focus(window, cx);
        cx.notify();
    }

    /// Whether worker `uid`'s detail draws its body.
    pub(super) fn worker_ready(&self, uid: u64, cx: &App) -> bool {
        let conv = self.conversations.get(&uid);
        let chat = conv
            .and_then(|conv| conv.session())
            .map(|session| &session.read(cx).chat);
        shows_body(
            chat.is_some(),
            chat.map_or(Link::Connecting, |chat| chat.link),
            conv.is_some_and(|conv| conv.was_live),
        )
    }

    /// Open the picked worker's composer with the caret in it.
    fn write_to(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.worker_ready(uid, cx) {
            return;
        }
        self.workers.writing = Some(uid);
        let composer = self.worker_composer(uid, window, cx);
        composer.read(cx).state.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Close the open composer, its draft kept; a caret in it goes to the
    /// list, which stays drawn.
    fn close_writing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(uid) = self.workers.writing.take() else {
            return;
        };
        if let Some((composer, _)) = self.workers.composers.get(&uid)
            && composer.read(cx).state.focus_handle(cx).is_focused(window)
        {
            self.workers.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Let go of every composer whose session is no longer a worker here
    /// (closed, or its lead gone), handing the caret to the lead's composer
    /// first when one held it: a focused element leaving the frame takes the
    /// whole keymap with it. Another lead's workers keep theirs, drafts and
    /// all.
    pub fn release_workers(&mut self, cx: &mut Context<Self>) {
        let still = |pane: &Self, uid: u64| {
            pane.conversations.contains_key(&uid) && crate::control::lead_of(uid).is_some()
        };
        if self.workers.picked.is_some_and(|uid| !still(self, uid)) {
            self.workers.picked = None;
            // A caret in its detail, a card's say, would leave the frame with
            // it: to the list, which stays.
            let (detail, list, window) = (
                self.workers.detail_focus.clone(),
                self.workers.focus.clone(),
                self.window,
            );
            cx.defer(move |cx| {
                let _ = window.update(cx, |_, window, cx| {
                    if detail.contains_focused(window, cx) {
                        list.focus(window, cx);
                    }
                });
            });
        }
        if self.workers.writing.is_some_and(|uid| !still(self, uid)) {
            self.workers.writing = None;
        }
        let gone: Vec<u64> = self
            .workers
            .composers
            .keys()
            .copied()
            .filter(|uid| !still(self, *uid))
            .collect();
        for uid in gone {
            let Some((composer, _)) = self.workers.composers.remove(&uid) else {
                continue;
            };
            // Held until the check, so its focus is still there to read.
            let window = self.window;
            let pane = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = window.update(cx, |_, window, cx| {
                    if composer.read(cx).state.focus_handle(cx).is_focused(window) {
                        // The lead may have gone with it, leaving no composer
                        // to draw: then the pane itself, which always is.
                        let _ = pane.update(cx, |pane, cx| match pane.session() {
                            Some(_) => pane.reclaim_focus(window, cx),
                            None => pane.focus_handle.clone().focus(window, cx),
                        });
                    }
                });
            });
        }
    }

    /// Worker `uid`'s composer, made on first use.
    fn worker_composer(
        &mut self,
        uid: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Composer> {
        if let Some((composer, _)) = self.workers.composers.get(&uid) {
            return composer.clone();
        }
        let composer = cx.new(|cx| Composer::compact(window, cx));
        let state = composer.read(cx).state.clone();
        let enter = cx.subscribe_in(
            &state,
            window,
            move |pane: &mut Self, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    pane.send_to_worker(uid, window, cx);
                }
            },
        );
        let pressed = cx.subscribe_in(
            &composer,
            window,
            move |pane: &mut Self, _, event: &ComposerEvent, window, cx| match event {
                ComposerEvent::SendPressed => pane.send_to_worker(uid, window, cx),
                ComposerEvent::StopPressed => {
                    if let Some(session) = pane.session_of(uid).cloned()
                        && session.read(cx).chat.busy
                    {
                        Self::cancel(&session, cx);
                    }
                }
                // Neither is drawn on a compact composer: no tray, no branch.
                ComposerEvent::OpenFile(_) | ComposerEvent::Branch(_) => {}
            },
        );
        self.workers
            .composers
            .insert(uid, (composer.clone(), [enter, pressed]));
        composer
    }

    /// Send what the composer holds; once it went, the composer closes and
    /// the transcript has its room back.
    fn send_to_worker(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        let Some((composer, _)) = self.workers.composers.get(&uid) else {
            return;
        };
        let composer = composer.clone();
        Self::submit_from(&session, &composer, window, cx);
        if composer.read(cx).text(cx).is_empty() {
            self.close_writing(window, cx);
        }
    }

    /// The pinned permission of the picked worker answered with `weight`,
    /// for the keys while the caret is in the region; nothing when it waits
    /// on none.
    pub fn answer_worker_pinned(
        &mut self,
        weight: onehand_core::acp::PermissionWeight,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self
            .workers
            .picked
            .and_then(|uid| self.session_of(uid).cloned())
        else {
            return;
        };
        crate::chat::transcript::answer_pinned(&session, weight, cx);
    }

    /// The header's workers button: present only while the lead on screen
    /// has workers, filled while the region is open, and marked in the
    /// warning ink while one of them waits on the person.
    pub(super) fn workers_control(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let workers = self.lead_workers();
        if workers.is_empty() {
            return None;
        }
        let waits = workers
            .iter()
            .any(|worker| crate::rail::needs_attention(self.signal(worker.uid, cx)));
        let open = self.docks_open.workers;
        let warning = crate::theme::status_ink(cx).warning;
        Some(
            div()
                .relative()
                .flex_none()
                .child(
                    onehand_ui::strip_control("workers", crate::icons::Icon::Users, open, cx)
                        .named(match waits {
                            true => "A worker waits on you — show or hide the workers",
                            false => "Show or hide the workers",
                        })
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::ToggleWorkers);
                        })),
                )
                .when(waits && !open, |control| {
                    control.child(
                        div()
                            .absolute()
                            .top_0p5()
                            .right_0p5()
                            .size(super::header::LIVE_DOT)
                            .rounded_full()
                            .bg(warning),
                    )
                })
                .into_any_element(),
        )
    }

    /// The region: its strip, the list, and the picked worker's detail.
    pub(crate) fn workers_region(
        &mut self,
        maximized: bool,
        zoom: crate::zoom::Zoom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let workers = self.lead_workers();
        // Another lead's workers are ordered afresh, its most urgent picked.
        self.follow_lead();
        let now: Vec<(u64, Option<SessionSignal>)> = workers
            .iter()
            .map(|worker| (worker.uid, self.signal(worker.uid, cx)))
            .collect();
        self.workers.order = ordered(&self.workers.order, &now, self.workers.refresh);
        self.workers.refresh = false;
        if self
            .workers
            .picked
            .is_none_or(|picked| !self.workers.order.contains(&picked))
        {
            self.workers.picked = self.workers.order.first().copied();
        }
        // On screen here, in the window in front, a worker's finished turn
        // has been seen: it never shows in the pane, which is what clears it
        // for any other session.
        let mut seen = false;
        if window.is_window_active() {
            for worker in &workers {
                if let Some(conv) = self.conversations.get_mut(&worker.uid) {
                    seen |= std::mem::take(&mut conv.unseen);
                }
            }
        }
        if seen {
            cx.notify();
        }
        let list = self.workers_list(&workers, window, cx);
        let detail = self
            .workers
            .picked
            .and_then(|uid| workers.iter().find(|worker| worker.uid == uid))
            .map(|worker| self.worker_detail(worker, window, cx));
        div()
            .size_full()
            .v_flex()
            // Chrome keeps its size while the list and the detail zoom, as
            // the pane's header does.
            .child(zoom.unscale(self.workers_strip(workers.len(), maximized, cx)))
            .child(list)
            .children(detail)
            .into_any_element()
    }

    /// The strip: how many workers, maximize and hide.
    fn workers_strip(
        &self,
        count: usize,
        maximized: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        onehand_ui::strip(cx)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(cx.theme().foreground)
                    .child(format!("Workers · {count}")),
            )
            .child(
                onehand_ui::strip_control(
                    "maximize-workers",
                    match maximized {
                        true => IconName::Minimize,
                        false => IconName::Maximize,
                    },
                    false,
                    cx,
                )
                .named_with_action(
                    match maximized {
                        true => "Back to the dock",
                        false => "Fill the window",
                    },
                    &crate::shell::ToggleMaximize,
                    None,
                )
                .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                    cx.emit(ChatPaneEvent::MaximizeWorkers);
                })),
            )
            .child(
                onehand_ui::strip_control("hide-workers", IconName::Minus, false, cx)
                    .named("Hide the workers")
                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                        cx.emit(ChatPaneEvent::ToggleWorkers);
                    })),
            )
    }

    /// What a worker, or the lead, is called: as the rail names it.
    pub(super) fn worker_name(&self, uid: u64, cx: &App) -> String {
        let agent = self
            .conversations
            .get(&uid)
            .map(|conv| conv.spec.name.clone())
            .unwrap_or_default();
        let title = self.title_for(uid, cx);
        crate::rail::session_label(title.as_deref(), &agent).to_string()
    }
}

#[cfg(test)]
mod tests;
