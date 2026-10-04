use super::header::Archives;
use super::workspace_page::PAGE_COLUMN;
use super::{ChatPane, ChatPaneEvent, ProjectFacts, rel_time};
use crate::chat::conversation::Conversation;
use gpui::{
    App, Context, Div, ElementId, InteractiveElement, IntoElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::{DialogClose, DialogFooter};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt, WindowExt as _};
use onehand_core::chat::ConvMeta;
use std::path::{Path, PathBuf};

/// How many past conversations the project page lists.
///
/// The page is an entrance, not an archive browser: the newest handful is what
/// "where was I" needs, and a project worked in for months would otherwise draw
/// a column of hundreds for the sake of the two or three anybody came for. What
/// the cap cut off is said on screen rather than silently dropped. The list
/// scrolls as well, because this many rows already outgrows a short window.
const HOME_ROWS: usize = 8;

/// The project the pane is standing in while no conversation is showing.
///
/// Pushed by the shell rather than looked up: the workspace tree is the
/// shell's, and a copy of it here would be one more thing to keep in step.
pub(super) struct EmptyProject {
    /// What the project is called, for the page's own title.
    pub(super) label: SharedString,
    /// Where it is, which is what its archived conversations are keyed by --
    /// and what tells one scan's answer from another's.
    pub(super) path: PathBuf,
    /// Its past conversations, across every agent. `None` while the scan is
    /// still running, which is a different thing from a project that has none:
    /// one is a wait and the other is an answer.
    pub(super) history: Option<Vec<ConvMeta>>,
    /// What the page's own menu needs to know about the project and cannot
    /// work out: they live in the workspace tree and in a `git status` sweep,
    /// and both are the shell's. Pushed rather than asked for, like everything
    /// else the pane knows about the window, and pushed again whenever one
    /// changes — a menu still offering *Pin to top* on a project pinned a
    /// second ago is worse than one that does not offer it at all.
    pub(super) facts: ProjectFacts,
}

impl ChatPane {
    /// Point the header's menu at `root`, reading its conversations if it is not
    /// already the one being held.
    pub(super) fn follow_archives(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.archives.as_ref().is_some_and(|held| held.root == root) {
            return;
        }
        self.archives = Some(Archives {
            root: root.to_path_buf(),
            found: None,
        });
        self.scan_archives(cx);
    }

    /// Read the held project's conversations again.
    ///
    /// Called at the two moments the listing on disk actually changes under a
    /// running window: a turn ending, which is when a conversation is written —
    /// so a session's first turn is when it appears here at all — and a session
    /// closing, which is the moment somebody is most likely to want it back.
    /// Neither re-reads the *directory* on the UI thread; both go the same way
    /// the first read did.
    pub(super) fn refresh_archives(&mut self, cx: &mut Context<Self>) {
        if self.archives.is_some() {
            self.scan_archives(cx);
        }
    }

    /// The read itself. Leaves whatever is held in place until the answer lands,
    /// so a refresh does not blank a menu that already had something in it.
    fn scan_archives(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.archives.as_ref().map(|held| held.root.clone()) else {
            return;
        };
        let scan = path.clone();
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &scan,
                        None,
                    )
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // Only for the project it was asked about: reading a directory
                // of archives takes long enough that the user can have moved to
                // another project twice over, and one project's conversations
                // are not an answer about another's.
                let Some(held) = pane.archives.as_mut().filter(|held| held.root == path) else {
                    return;
                };
                held.found = Some(past);
                cx.notify();
            });
        })
        .detach();
    }

    /// Read the project page's list of past conversations, off the UI loop.
    ///
    /// Across every agent, not just the configured default: what the user is
    /// looking for is a conversation they had in this project, and which agent
    /// ran it is a detail of that conversation rather than a filter on the
    /// question. The resume picker inside a session is the narrower one -- there
    /// the agent is already chosen, because the session it belongs to has one.
    pub(super) fn scan_project_history(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.empty.as_ref().map(|project| project.path.clone()) else {
            return;
        };
        let scan = path.clone();
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &scan,
                        None,
                    )
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // Reading a directory of archives takes long enough that the
                // user can have moved on twice over. An answer about a project
                // the pane has left is not this page's list, and adopting it
                // would put one project's conversations under another's name.
                let Some(project) = pane.empty.as_mut().filter(|project| project.path == path)
                else {
                    return;
                };
                project.history = Some(past);
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask before deleting a conversation, and delete only on the answer.
    ///
    /// A modal rather than a control that arms on the first press and acts on
    /// the second. Arming reads as a control that did nothing: the press lands,
    /// the word changes, and a user who has looked away comes back to a row
    /// that is one accidental press from gone with no warning left on screen.
    /// This one names the conversation it is about, cannot be missed, and has
    /// to be answered before anything else in the window can be -- which is the
    /// weight the only irreversible thing this app does should carry.
    ///
    /// **The name is passed in rather than looked up.** The archive list is the
    /// page's, and by the time the answer comes back the page may have been
    /// replaced by another project's; the sentence the user is reading has to
    /// be about the row they pressed.
    fn confirm_delete(
        &mut self,
        dir: PathBuf,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            // Cloned per build: a dialog's builder runs again on every frame it
            // is on screen, so nothing captured here can be consumed by one.
            let (pane, dir, name) = (pane.clone(), dir.clone(), name.clone());
            alert
                // The library's own title and description survive here, unlike
                // on a dialog opened from a trigger: this builder is what the
                // window keeps, so both are rebuilt with the rest of it.
                .title("Delete this conversation?")
                .description(format!(
                    "“{name}” will be removed from disk, with every message and \
                     image in it. This cannot be undone."
                ))
                // Ours rather than the default pair, for the reason every button
                // in this app is ours: the library draws its own with the arrow
                // cursor, and the one dialog that asks before destroying
                // something is the last place to say "this does nothing" with
                // the pointer. Keep is first and plain, Delete last and in the
                // danger tint.
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(
                                crate::controls::action("keep-conversation")
                                    .ghost()
                                    .label("Keep"),
                            ),
                        )
                        .child(
                            crate::controls::action("confirm-delete-conversation")
                                .danger()
                                .label("Delete")
                                .on_click(move |_, window: &mut Window, cx: &mut App| {
                                    window.close_dialog(cx);
                                    let dir = dir.clone();
                                    pane.update(cx, |pane: &mut Self, cx| {
                                        pane.delete_conversation(dir, cx);
                                    });
                                }),
                        ),
                )
        });
    }

    /// Delete an archived conversation, the question already answered.
    ///
    /// Offered on the project page and nowhere else, and that is the guard
    /// doing most of the work rather than a rule anybody has to remember: the
    /// page is what shows when the selected project has **no session on it**,
    /// so the conversations listed there are the ones nothing is writing to.
    /// A live conversation deleted underneath its own session would not even
    /// stay deleted -- the session's next turn writes the file again, holding
    /// only what came after, because its mark says the rest is already on disk.
    /// A session in another *window* is the case the page's own shape does not
    /// cover, so the check below covers it.
    fn delete_conversation(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let store = onehand_core::chat::conversations_dir();
        let live = self
            .conversations
            .values()
            .filter_map(Conversation::session)
            .any(|session| {
                session
                    .read(cx)
                    .chat
                    .session_id
                    .as_deref()
                    .is_some_and(|sid| onehand_core::chat::conv_dir(&store, sid) == dir)
            });
        if live {
            cx.emit(ChatPaneEvent::ConversationNotDeleted(
                "it is open in a session".to_string(),
            ));
            cx.notify();
            return;
        }

        let removing = dir.clone();
        cx.spawn(async move |pane, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { onehand_core::chat::delete(&removing) })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                match done {
                    // Taken off the page here rather than by scanning the
                    // directory again: the row is gone because the thing it
                    // named is gone, and a second read of the store to
                    // discover that is a read that can also answer late.
                    Ok(()) => {
                        if let Some(project) = pane.empty.as_mut()
                            && let Some(history) = project.history.as_mut()
                        {
                            history.retain(|conv| conv.dir != dir);
                        }
                    }
                    Err(e) => cx.emit(ChatPaneEvent::ConversationNotDeleted(e.to_string())),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The past conversations `uid` is choosing between, or nothing if it is
    /// not choosing.
    fn choices_of(&self, uid: u64) -> Vec<ConvMeta> {
        self.conversations
            .get(&uid)
            .and_then(Conversation::choices)
            .unwrap_or_default()
            .to_vec()
    }

    /// The resume picker: past conversations for this root + agent, newest
    /// first, plus the option to start fresh.
    pub(super) fn resume_picker(
        &mut self,
        uid: u64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let past = self.choices_of(uid);
        let now = onehand_core::chat::now_secs();

        div()
            .size_full()
            .v_flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .w_full()
                    .max_w(px(PAGE_COLUMN))
                    // Bounded by the panel, for the same reason the project
                    // page's column is: this list is every conversation the
                    // agent has had in the project, and a column taller than
                    // the panel is centred into rows nothing can reach.
                    .max_h_full()
                    .min_h_0()
                    .child(div().font_semibold().child("Resume a conversation"))
                    // The rows are what grows, so the rows are what scrolls. The
                    // heading above and the way out below stay where they are --
                    // a picker whose *Start a new conversation* scrolls off the
                    // bottom is a screen with no way out of it.
                    .child(
                        div()
                            .id("resume-choices")
                            .v_flex()
                            .gap_3()
                            .w_full()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(past.into_iter().enumerate().map(|(i, meta)| {
                                // The agent is not named here: this picker belongs to a
                                // session that already has one, and every row in it was
                                // run by that same agent.
                                let subtitle = format!(
                                    "{} · {} items",
                                    rel_time(now, meta.updated),
                                    meta.item_count
                                );
                                conversation_card(
                                    ("resume", i),
                                    meta.title.clone().into(),
                                    subtitle.into(),
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |pane: &mut Self, _, _, cx| {
                                        let meta = pane.choices_of(uid).get(i).cloned();
                                        pane.start(uid, meta, cx);
                                    },
                                ))
                            })),
                    )
                    .child(
                        crate::controls::action("resume-fresh")
                            .primary()
                            .label("Start a new conversation")
                            .on_click(cx.listener(move |pane: &mut Self, _, _, cx| {
                                pane.start(uid, None, cx);
                            })),
                    ),
            )
    }

    /// The project page: what the centre of the window shows while the selected
    /// project has no conversation on it.
    ///
    /// This is the state every freshly added project starts in, and the state a
    /// project returns to when its last session is closed -- so it is the first
    /// thing a new user sees, and it used to be one line of grey text saying
    /// *Start a session in X* with nothing to press. Everything a project can be
    /// entered by is here instead: the conversations already had in it, newest
    /// first and across every agent, and the button that starts a fresh one.
    ///
    /// The history is drawn from the same card the resume picker uses, because
    /// it is the same question -- which past conversation -- and one of the two
    /// looking unclickable is how a list stops being read as a list.
    pub(super) fn project_home(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(project) = self.empty.as_ref() else {
            // Not a project with nothing in it -- no project at all. Naming
            // what has to happen first beats an offer that cannot be taken:
            // every session belongs to a root, and there is no root to bind one
            // to.
            return div()
                .size_full()
                .v_flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Add a project to start a session")
                .into_any_element();
        };
        let now = onehand_core::chat::now_secs();
        let muted = cx.theme().muted_foreground;
        let danger = crate::theme::status_ink(cx).danger;
        // Bounded, and the bound says so below. A project worked in for months
        // has more archives than this page is for, and none of this scrolls.
        let shown: Vec<ConvMeta> = project
            .history
            .iter()
            .flatten()
            .take(HOME_ROWS)
            .cloned()
            .collect();
        let hidden = project
            .history
            .as_ref()
            .map_or(0, |all| all.len().saturating_sub(HOME_ROWS));
        // `None` is the scan still running, `Some([])` is a project that has
        // never been prompted. Both draw a line, and they must not draw the
        // same one: telling a user with a hundred conversations that they have
        // none, for the half-second a directory read takes, is worse than
        // saying nothing.
        let note = match &project.history {
            None => Some("Looking for past conversations…"),
            Some(all) if all.is_empty() => Some("No conversations in this project yet."),
            Some(_) => None,
        };

        div()
            .size_full()
            .v_flex()
            // The header stays. It is the panel's only chrome, and everything on
            // it that this page can still answer is about the *project* rather
            // than about a conversation: the file tree, a shell, the way back to
            // a hidden rail. Dropping it here took all three away at exactly the
            // moment there is no conversation to reach them from instead -- and
            // a panel that loses its own chrome between one click and the next
            // reads as one that broke.
            .child(self.header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .child(
                        div()
                            .v_flex()
                            .gap_3()
                            .w_full()
                            .max_w(px(PAGE_COLUMN))
                            // Bounded by the panel it sits in, so the page is
                            // centred while it fits and fills the space when it
                            // does not. Without this the column takes its
                            // content's height whatever that is, and a project
                            // with a full list of archives on a short window
                            // pushed its own rows out through the top and bottom
                            // of the panel -- unreachable, because the centring
                            // spends the overflow at both ends and there is
                            // nothing to scroll.
                            .max_h_full()
                            .min_h_0()
                            // The project's name is *not* repeated here. The
                            // header above says it now, in the same place it
                            // says a conversation's name, and printing it again
                            // two rows lower was one word twice on a page whose
                            // whole job is to offer the few things there are.
                            .child(
                                crate::controls::action("project-new-session")
                                    .primary()
                                    .icon(Icon::new(IconName::Plus))
                                    .label("New session")
                                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                                        cx.emit(ChatPaneEvent::StartSession {
                                            agent: None,
                                            resume: None,
                                        });
                                    })),
                            )
                            // Tasks cut off here, each to resume or let go,
                            // and tasks waiting for their place. Above the
                            // conversations: one is work left half done.
                            .children(unfinished_tasks(&project.path, cx))
                            .children(
                                note.map(|note| div().text_xs().text_color(muted).child(note)),
                            )
                            // The archives are the one part of this page that
                            // grows, so they are the part that scrolls. *New
                            // session* above and the count of what was left out
                            // below stay put: the first is why most people are
                            // on this page, and the second is the page saying a
                            // bound bit, which it cannot do from under the fold.
                            .children((!shown.is_empty()).then(|| {
                                div()
                                    .id("project-history")
                                    .v_flex()
                                    .gap_3()
                                    .w_full()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child("Past conversations"),
                                    )
                                    .children(shown.into_iter().enumerate().map(|(i, meta)| {
                                        // The agent *is* named here, unlike in a session's own
                                        // picker: this list crosses every agent that has worked
                                        // in the project, and resuming a row starts a session on
                                        // the one that held it.
                                        let subtitle = format!(
                                            "{} · {} items · {}",
                                            rel_time(now, meta.updated),
                                            meta.item_count,
                                            meta.agent
                                        );
                                        let (agent, archive) = (
                                            SharedString::from(meta.agent.clone()),
                                            meta.dir.clone(),
                                        );
                                        let dir = meta.dir.clone();
                                        let name = SharedString::from(meta.title.clone());
                                        conversation_card(
                                            ("home", i),
                                            meta.title.clone().into(),
                                            subtitle.into(),
                                            cx,
                                        )
                                        .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                                            cx.emit(ChatPaneEvent::StartSession {
                                                agent: Some(agent.clone()),
                                                resume: Some(archive.clone()),
                                            });
                                        }))
                                        // A word rather than a glyph, and this is the one
                                        // control in the app that earns the distinction:
                                        // everything else it offers can be done again --
                                        // a closed session respawns, a removed project is
                                        // added back -- and a deleted conversation cannot.
                                        //
                                        // Inside the card, so it is plainly about the
                                        // conversation beside it rather than about the row
                                        // it happened to be nearest. That puts one clickable
                                        // inside another, which is what the stop below is
                                        // for: without it the press that asks to delete a
                                        // conversation also opens it.
                                        .child(
                                            crate::controls::action(("home-delete", i))
                                                .ghost()
                                                .small()
                                                .text_color(danger)
                                                .label("Delete")
                                                .tooltip("Delete this conversation")
                                                .on_click(cx.listener(
                                                    move |pane: &mut Self, _, window, cx| {
                                                        cx.stop_propagation();
                                                        pane.confirm_delete(
                                                            dir.clone(),
                                                            name.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        )
                                    }))
                            }))
                            .children((hidden > 0).then(|| {
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{hidden} older not shown"))
                            })),
                    ),
            )
            .into_any_element()
    }
}

/// `n` of `noun`, with the noun's plural where `n` is not one.
pub(super) fn count_of(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

/// How many unfinished tasks the page lists before it says how many
/// more there are.
const UNFINISHED_ROWS: usize = 5;

/// The unfinished tasks of the project at `root`: what each was asked to do
/// and where it stands. One cut off offers *Dismiss* and *Resume*; one
/// waiting for its place says so and offers *Stop*.
fn unfinished_tasks(root: &Path, cx: &mut Context<ChatPane>) -> Option<gpui::AnyElement> {
    let tasks = crate::task::listed_in(root, cx);
    if tasks.is_empty() {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let hidden = tasks.len().saturating_sub(UNFINISHED_ROWS);
    let rows: Vec<_> = tasks
        .into_iter()
        .take(UNFINISHED_ROWS)
        .enumerate()
        .map(|(i, task)| {
            let (id, resume_id) = (task.id.clone(), task.id.clone());
            let said = match (task.queued, task.begun) {
                (true, true) => format!("{} · resumes at {}", task.name, task.step),
                (true, false) => format!("{} · starts at {}", task.name, task.step),
                (false, _) => format!("{} · stopped at {}", task.name, task.step),
            };
            let call_off = match task.begun {
                true => "Call off this resume; the task stays as it was",
                false => "Call off this start; the task does not run",
            };
            let row = div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .p_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .v_flex()
                        .gap_0p5()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(task.title))
                        .child(div().text_xs().text_color(muted).truncate().child(said)),
                );
            match task.queued {
                true => row
                    .child(div().text_xs().text_color(muted).child("Queued"))
                    .child(
                        crate::controls::action(("task-stop-queued", i))
                            .ghost()
                            .small()
                            .label("Stop")
                            .tooltip(call_off)
                            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                                cx.emit(ChatPaneEvent::StopQueuedTask(id.clone()));
                            })),
                    ),
                false => row
                    .child(
                        crate::controls::action(("task-dismiss", i))
                            .ghost()
                            .small()
                            .label("Dismiss")
                            .tooltip("Let this task go; it is kept as history and its work stays")
                            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                                cx.emit(ChatPaneEvent::DismissTask(id.clone()));
                            })),
                    )
                    .child(
                        crate::controls::action(("task-resume", i))
                            .small()
                            .label("Resume")
                            .tooltip("Carry on from that step in a new session")
                            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                                cx.emit(ChatPaneEvent::ResumeTask(resume_id.clone()));
                            })),
                    ),
            }
        })
        .collect();
    Some(
        div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(div().text_xs().text_color(muted).child("Unfinished tasks"))
            .children(rows)
            .children((hidden > 0).then(|| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{hidden} more not shown"))
            }))
            .into_any_element(),
    )
}

/// One archived conversation, as a card that can be picked.
///
/// Shared by a session's resume picker and the project page, because they ask
/// the same question from two places: the caller supplies the subtitle, which
/// is the only part that differs, and hangs its own click on the result. Two
/// hand-written card styles is how one of them ends up not looking clickable.
///
/// **A row, not a column, so a caller can add its own control at the end.** The
/// name and the line under it are one column inside it, taking the width that
/// is left; anything a caller hangs on afterwards sits at the right-hand edge,
/// inside the card's own border rather than out beside it. The project page's
/// delete is the reason -- a control that acts on one conversation belongs
/// within the card naming it, and the same shape holds for the picker, which
/// simply adds nothing.
fn conversation_card(
    id: impl Into<ElementId>,
    title: SharedString,
    subtitle: SharedString,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .cursor_pointer()
        // Hover is the fill alone, and the row's hairline stays what it was.
        // The tint these rows carried before was under a twentieth of a step
        // off the card in the light palette -- no feedback at all on a list
        // whose whole purpose is picking one row out of several -- but that was
        // the palette's fault, not the fill's, and the ramp answers it.
        .hover(|row| row.bg(cx.theme().list_hover))
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .flex_1()
                .min_w_0()
                .child(div().truncate().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
}
