//! The header's history menu: the past conversations of the project on
//! screen, and going back to choosing one.

use super::header::header_control;
use super::{ChatPane, ChatPaneEvent, rel_time};
use crate::chat::conversation::{Conversation, SessionPhase};
use gpui::{App, Context, IntoElement, ParentElement, SharedString, Styled, Window, div};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::{ActiveTheme, IconName, StyledExt};
use std::path::PathBuf;

impl ChatPane {
    /// Go back to choosing which past conversation this session should run.
    ///
    /// The live one has its name and settings written on the way out: the
    /// transcript is written at the end of every turn, so an idle conversation
    /// is already on disk, but the title and the selector picks are metadata
    /// and leaving without writing them would lose them.
    ///
    /// The adapter stays up until a choice is made. Dropping it here would gain
    /// nothing -- the choice is what decides which conversation to connect to,
    /// and `connect` drops it before spawning the replacement anyway.
    pub(super) fn show_history(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.active else {
            return;
        };
        let Some(conv) = self.conversations.get(&uid) else {
            return;
        };
        if let Some(session) = conv.session() {
            Self::archive_meta_detached(uid, &session.clone(), cx);
        }
        let (root, agent) = (conv.root.clone(), conv.spec.name.clone());
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &root,
                        Some(&agent),
                    )
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // An empty list is not a picker with nothing in it -- it is a
                // session that has never been anywhere else, and putting up a
                // page whose only option is the one already on screen would be
                // a dead end.
                if !past.is_empty() {
                    pane.set_phase(uid, SessionPhase::ChoosingHistory(past));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The way back to a conversation this project has already had — as a
    /// session of its own, beside the one on screen.
    ///
    /// **The gap it fills.** Every other route to an archive costs the session
    /// in front of the user. The project page lists them and mints a session on
    /// the one picked, but it is what the centre of the window shows *instead
    /// of* a conversation, so reaching it meant closing every session in the
    /// project first; and the title menu's own picker leaves nothing running —
    /// it takes the session on screen off its conversation to ask the question.
    /// So this is the one that opens an old conversation and keeps the current
    /// one where it is, as a second row in the rail.
    ///
    /// **A menu on a header button rather than a dialog**, because it is a
    /// short list of one project's own conversations and the header is already
    /// where the things about this pane are. A modal over the conversation to
    /// pick a conversation is a heavier gesture than the choice deserves.
    ///
    /// **Every agent's**, as the project page's list is and for the same reason:
    /// the question is which conversation, and which agent had it is a property
    /// of the answer rather than a filter on the question. The title menu's
    /// picker is the narrow one, because there the agent is already decided.
    ///
    /// **A conversation already open is listed and refused, not hidden.** Two
    /// sessions on one archive both believe the transcript so far is on disk, so
    /// the second one's first turn writes the file back holding only what came
    /// after it. Dropping the row instead would leave the one conversation the
    /// user is most likely to look for missing from the list with nothing said.
    pub(super) fn history_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        // Owned rather than borrowed out of the sessions: the menu is built
        // later, from a closure that outlives this borrow of the pane.
        let open: Vec<String> = self
            .conversations
            .values()
            .filter_map(Conversation::session)
            .filter_map(|session| session.read(cx).chat.session_id.clone())
            .collect();
        let now = onehand_core::chat::now_secs();
        let held = self.archives.as_ref().and_then(|held| held.found.as_ref());
        let rows: Vec<HistoryRow> = held
            .into_iter()
            .flatten()
            .take(HISTORY_ROWS)
            .map(|meta| HistoryRow {
                title: SharedString::from(meta.title.clone()),
                // The age, and the agent only where it is not the one running
                // this session — on a machine with one agent configured, naming
                // it on every row is a column of the same word.
                aside: SharedString::from(format!(
                    "{} · {}",
                    rel_time(now, meta.updated),
                    meta.agent
                )),
                open: open.iter().any(|id| id == &meta.session_id),
                agent: SharedString::from(meta.agent.clone()),
                dir: meta.dir.clone(),
            })
            .collect();
        let hidden = held.map_or(0, |all| all.len().saturating_sub(HISTORY_ROWS));
        // `None` is the read still out and `Some([])` is a project that has
        // never been prompted. One is a wait and the other is an answer, and a
        // menu that gives the second while the first is true tells somebody with
        // a hundred conversations that they have none.
        let standing = match held {
            None => Some(SharedString::from("Reading conversations…")),
            Some(all) if all.is_empty() => {
                Some(SharedString::from("No conversations in this project yet"))
            }
            Some(_) => None,
        };
        let this = cx.entity();

        header_control("history", IconName::GalleryVerticalEnd, cx)
            .tooltip("Open a past conversation in a new session")
            // Anchored to its own right-hand corner: this button sits at the end
            // of the header, and a menu hanging rightwards from it opens off the
            // edge of the window.
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, cx| {
                let muted = cx.theme().muted_foreground;
                // A project is worked in for months and every conversation had
                // in it is a row here, so the list is longer than a menu's
                // height by design. Without this the rows past the bottom are
                // built and drawn with no way to reach them.
                let menu = menu.scrollable(true);
                let menu = match standing.clone() {
                    Some(line) => menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div().text_color(muted).child(line.clone())
                        })
                        .disabled(true),
                    ),
                    None => menu,
                };
                let menu = rows.iter().fold(menu, |menu, row| {
                    let (title, aside, open) = (row.title.clone(), row.aside.clone(), row.open);
                    let draw =
                        move |_: &mut Window, _: &mut App| {
                            div()
                                .h_flex()
                                .items_center()
                                .gap_3()
                                .w_full()
                                .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                                .child(div().flex_none().text_xs().text_color(muted).child(
                                    if open {
                                        SharedString::from("already open")
                                    } else {
                                        aside.clone()
                                    },
                                ))
                        };
                    match open {
                        // Already open keeps the library's own cursor with its
                        // refusal, so the pointer stays a promise a row can
                        // keep.
                        true => menu.item(PopupMenuItem::element(draw).disabled(true)),
                        false => {
                            let item = crate::controls::menu_row(draw);
                            let (start, agent, dir) =
                                (this.clone(), row.agent.clone(), row.dir.clone());
                            menu.item(item.on_click(move |_, _, cx: &mut App| {
                                start.update(cx, |_: &mut Self, cx| {
                                    cx.emit(ChatPaneEvent::StartSession {
                                        agent: Some(agent.clone()),
                                        resume: Some(dir.clone()),
                                    });
                                });
                            }))
                        }
                    }
                });
                // Said out loud rather than left as a list that simply stops: a
                // cut nobody is told about reads as archives that were lost.
                match hidden {
                    0 => menu,
                    n => menu.separator().item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{n} older, not shown"))
                        })
                        .disabled(true),
                    ),
                }
            })
    }
}

/// How many past conversations the header's menu offers before it stops and
/// says so.
///
/// **High enough that it is not the thing deciding what the list shows.** The
/// menu scrolls, so what a reader can reach is not bounded by what fits; this
/// bounds the *work*, because a menu builds every row it holds the moment it
/// opens. It sits far past what anybody scrolls a menu for — the menu's own
/// height shows on the order of a dozen rows at a time — so it is a backstop
/// against a store nothing ever prunes rather than an editorial cut, and the one
/// time it bites it says so.
const HISTORY_ROWS: usize = 200;

/// One row of that menu, prepared before the menu builder runs.
///
/// The builder is an `Fn` that outlives the borrow of the pane these came from,
/// so everything it needs is copied out here rather than read through a handle
/// at the moment it draws.
struct HistoryRow {
    title: SharedString,
    /// How long ago, and which agent had it.
    aside: SharedString,
    /// Whether a session in this window is already on this conversation.
    open: bool,
    agent: SharedString,
    dir: PathBuf,
}
