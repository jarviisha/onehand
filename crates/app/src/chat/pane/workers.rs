//! The workers' region: the lead on screen's workers, a block each, stacked.
//!
//! **Drawn by the pane, mounted in a dock panel of its own.** A worker's
//! conversation, its run layout and the list state that paints it are the
//! pane's, as every session's are; the pane never shows a worker of an open
//! lead itself, so each list is still painted in one place only.
//!
//! **One block is open at a time**: the whole transcript, the oldest card it
//! waits on and a compact composer of its own. The others fold to the
//! transcript's tail at a fixed height, and drop it before the open block
//! goes under its minimum; past that the stack scrolls as a whole.

mod block;

use super::{ChatPane, ChatPaneEvent};
use crate::chat::composer::{Composer, ComposerEvent};
use crate::control::Worker;
use crate::controls::Named as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, InteractiveElement, IntoElement, ParentElement, Pixels, Rems,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, rems,
};
use gpui::{AppContext as _, Focusable as _};
use gpui_component::input::InputEvent;
use gpui_component::{ActiveTheme as _, IconName, StyledExt as _};
use onehand_core::chat::Link;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// The open block's least height: about ten rows of transcript and its
/// composer. In rems, so a zoom scales it.
const WORKER_OPEN_MIN: Rems = rems(19.);
/// A folded block's tail: its transcript's last few lines.
const WORKER_TAIL_H: Rems = rems(4.5);

/// What the region holds beyond what the conversations do.
pub(super) struct Workers {
    /// The block open, at most one.
    open: Option<u64>,
    /// Each opened block's composer, kept while its session is a worker, so
    /// a draft survives the block folding and the lead on screen changing.
    composers: HashMap<u64, (Entity<Composer>, [Subscription; 2])>,
    scroll: gpui::ScrollHandle,
    /// How tall the stack measured last frame.
    height: Rc<Cell<Pixels>>,
    /// Whether the folded blocks drew their tails last frame, which the
    /// stack's measured height includes.
    tails: bool,
}

impl Workers {
    pub(super) fn new() -> Self {
        Self {
            open: None,
            composers: HashMap::new(),
            scroll: gpui::ScrollHandle::new(),
            height: Rc::default(),
            tails: true,
        }
    }
}

/// Whether the folded blocks keep their tails: only while the whole stack
/// (heads, cards, the open block at its minimum) fits in `avail` with them.
/// `content` is what the stack measured last frame, its `folded` tails of
/// `tail` each included when `shown`. Without them, and still short, the
/// stack scrolls.
fn tails_fit(avail: f32, content: f32, shown: bool, folded: usize, tail: f32) -> bool {
    let needed = match shown {
        true => content,
        false => content + folded as f32 * tail,
    };
    needed <= avail
}

/// Whether a worker's block draws its body, the composer among it: once its
/// session is up, and not while a first connect is all there is.
fn shows_body(present: bool, link: Link, was_live: bool) -> bool {
    present && (link != Link::Connecting || was_live)
}

impl ChatPane {
    /// The workers of the lead on screen that this pane holds, opening or
    /// open, oldest first.
    pub fn lead_workers(&self) -> Vec<Worker> {
        let Some(lead) = self.active else {
            return Vec::new();
        };
        crate::control::workers_of(lead)
            .into_iter()
            .filter(|worker| self.conversations.contains_key(&worker.uid))
            .collect()
    }

    /// Open worker `uid`'s block, folding the one open, and put the caret in
    /// its composer when `caret`.
    pub fn open_worker(
        &mut self,
        uid: u64,
        caret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workers.open = Some(uid);
        // Only into a composer drawn: one still connecting has none yet.
        if caret && self.worker_ready(uid, cx) {
            let composer = self.worker_composer(uid, window, cx);
            composer.read(cx).state.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    /// Whether worker `uid`'s block draws its body.
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

    /// Let go of every block's composer whose session is no longer a worker
    /// here (closed, or its lead gone), handing the caret to the lead's
    /// composer first when one held it: a focused element leaving the frame
    /// takes the whole keymap with it. Another lead's workers keep theirs,
    /// drafts and all.
    pub fn release_workers(&mut self, cx: &mut Context<Self>) {
        let still = |pane: &Self, uid: u64| {
            pane.conversations.contains_key(&uid) && crate::control::lead_of(uid).is_some()
        };
        if self.workers.open.is_some_and(|uid| !still(self, uid)) {
            self.workers.open = None;
        }
        let shown: Vec<u64> = self
            .workers
            .composers
            .keys()
            .copied()
            .filter(|uid| still(self, *uid))
            .collect();
        let gone: Vec<u64> = self
            .workers
            .composers
            .keys()
            .copied()
            .filter(|uid| !shown.contains(uid))
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

    fn send_to_worker(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session_of(uid).cloned() else {
            return;
        };
        let Some((composer, _)) = self.workers.composers.get(&uid) else {
            return;
        };
        Self::submit_from(&session, &composer.clone(), window, cx);
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

    /// The region: its strip, then a block per worker of the lead on screen.
    pub(crate) fn workers_region(
        &mut self,
        maximized: bool,
        zoom: crate::zoom::Zoom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let workers = self.lead_workers();
        // One block open by default, the first.
        if self
            .workers
            .open
            .is_none_or(|open| !workers.iter().any(|w| w.uid == open))
        {
            self.workers.open = workers.first().map(|worker| worker.uid);
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
        let lead = self
            .active
            .map(|uid| self.worker_name(uid, cx))
            .unwrap_or_default();
        // Measured in the zoomed panel, so the tail is too.
        let tail = f32::from(WORKER_TAIL_H.to_pixels(window.rem_size())) * zoom.factor();
        let avail = f32::from(self.workers.height.get());
        let folded = workers
            .iter()
            .filter(|w| Some(w.uid) != self.workers.open && self.worker_ready(w.uid, cx))
            .count();
        let tails = tails_fit(
            avail,
            avail + f32::from(self.workers.scroll.max_offset().y),
            self.workers.tails,
            folded,
            tail,
        );
        self.workers.tails = tails;
        let blocks: Vec<gpui::AnyElement> = workers
            .iter()
            .map(|worker| self.worker_block(worker, tails, window, cx))
            .collect();
        let measure = self.workers.height.clone();
        let pane = cx.entity().downgrade();
        div()
            .size_full()
            .v_flex()
            // Chrome keeps its size while the blocks under it zoom, as the
            // pane's header does.
            .child(
                zoom.unscale(
                    onehand_ui::strip(cx)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(cx.theme().foreground)
                                .child(format!("Workers of {lead}")),
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
                            .on_click(cx.listener(
                                |_: &mut Self, _, _, cx| {
                                    cx.emit(ChatPaneEvent::MaximizeWorkers);
                                },
                            )),
                        )
                        .child(
                            onehand_ui::strip_control("hide-workers", IconName::Minus, false, cx)
                                .named("Hide the workers")
                                .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                                    cx.emit(ChatPaneEvent::ToggleWorkers);
                                })),
                        ),
                ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        gpui::canvas(
                            move |bounds, _, cx| {
                                let height = bounds.size.height;
                                if measure.replace(height) != height {
                                    let pane = pane.clone();
                                    cx.defer(move |cx| {
                                        let _ = pane.update(cx, |_: &mut Self, cx| cx.notify());
                                    });
                                }
                            },
                            |_, _: (), _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .id("workers-stack")
                            .size_full()
                            .v_flex()
                            .overflow_y_scroll()
                            .track_scroll(&self.workers.scroll)
                            .children(blocks),
                    ),
            )
            .into_any_element()
    }

    /// What a worker, or the lead, is called: as the rail names it.
    fn worker_name(&self, uid: u64, cx: &App) -> String {
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
