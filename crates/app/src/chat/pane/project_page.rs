use super::header::Archives;
use super::page_parts::{page_card, page_head, ruled};
use super::{ChatPane, ChatPaneEvent, ProjectFacts, rel_time};
use crate::chat::conversation::Conversation;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use crate::controls::text_input;
use crate::tokens::PAGE_WIDE;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Div, FocusHandle, InteractiveElement, IntoElement, ParentElement, Rems,
    SharedString, StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::chat::ConvMeta;
use onehand_plugin_host::Tall as _;
use std::path::{Path, PathBuf};

/// How many past conversations the project page lists.
///
/// The page is an entrance, not an archive browser: the newest handful is what
/// "where was I" needs, and a project worked in for months would otherwise draw
/// a column of hundreds for the sake of the two or three anybody came for. What
/// the cap cut off is said on screen rather than silently dropped. The list
/// scrolls as well, because this many rows already outgrows a short window.
const HOME_ROWS: usize = 8;

/// The width the history starts from before it grows into its row; the side
/// column wraps under it where both do not fit.
const HOME_MAIN: Rems = rems(26.);

/// The width the side column starts from beside the history.
const HOME_SIDE: Rems = rems(16.);

/// The slot a conversation's age stands in, the overview row's, so every
/// title starts on one edge.
const AGE_W: Rems = rems(3.);

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
    /// Its check command field, the shell's: `None` for a run's own
    /// worktree, which keeps none.
    pub(super) check_input: Option<gpui::Entity<gpui_component::input::InputState>>,
    /// One focus handle per past conversation the page lists, by place: each
    /// row is a tab stop, so the keyboard reaches its *Delete…* as the pointer
    /// does.
    pub(super) rows: Vec<FocusHandle>,
}

/// The focus handles for the page's listed conversations, one per row it can
/// list.
pub(super) fn home_rows(cx: &mut App) -> Vec<FocusHandle> {
    (0..HOME_ROWS)
        .map(|_| cx.focus_handle().tab_stop(true))
        .collect()
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
        crate::shell::confirm_on(
            cx.entity(),
            crate::shell::Confirm {
                id: "delete-conversation",
                title: format!("Delete “{name}”?").into(),
                description: "It will be removed from disk, with every message and image in it. \
                              This cannot be undone."
                    .into(),
                act: "Delete",
                ..Default::default()
            },
            window,
            cx,
            move |pane, _, cx| pane.delete_conversation(dir.clone(), cx),
        );
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
    /// **Three layers.** The page's own surface carries its head: the
    /// project's name, where it is and what git says, and what it can start.
    /// Each group under it stands on a surface a step up, the history as the
    /// main column and what the project needs and is set to beside it, so
    /// what belongs together reads as one object rather than as text loose on
    /// the page.
    pub(super) fn project_home(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(project) = self.empty.as_ref() else {
            // Not a project with nothing in it -- no project at all. Naming
            // what has to happen first beats an offer that cannot be taken:
            // every session belongs to a root, and there is no root to bind one
            // to.
            return onehand_plugin_host::hint("Add a project to start a session", cx);
        };
        let now = onehand_core::chat::now_secs();
        let (muted, ring) = (cx.theme().muted_foreground, cx.theme().ring);
        let danger = crate::theme::status_ink(cx).danger;
        let muted_line = |text: String| div().text_xs().text_color(muted).child(text);
        // Bounded, and the bound says so below. A project worked in for months
        // has more archives than this page is for.
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
            None => Some("Reading past conversations…"),
            Some(all) if all.is_empty() => Some("No conversations in this project yet"),
            Some(_) => None,
        };

        let root = project.path.clone();
        let checking = crate::task::checking(&root, cx);
        let path = std::env::home_dir()
            .and_then(|home| project.path.strip_prefix(&home).ok())
            .map_or_else(
                || project.path.display().to_string(),
                |rest| format!("~/{}", rest.display()),
            );
        let place = std::iter::once(path)
            .chain(self.git.as_ref().map(ToString::to_string))
            .collect::<Vec<_>>()
            .join(" · ");
        // The head: the project named at the page's title size over where it
        // is, and what it can start at the end.
        let head = page_head(
            project.label.clone(),
            Some(place.into_any_element()),
            project
                .facts
                .check
                .then(|| {
                    crate::controls::action("project-run-check")
                        .tall()
                        .secondary()
                        .label("Run check")
                        .loading(checking)
                        .tooltip("Run the project's check command")
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::RunCheck);
                        }))
                        .into_any_element()
                })
                .into_iter()
                .chain([new_session(cx)])
                .collect(),
            cx,
        );

        let history = page_card(
            "Past conversations",
            project.history.as_ref().map(Vec::len),
            None,
            cx,
        )
        .children(note.map(|note| muted_line(note.to_string())))
        .child(ruled(
            shown
                .into_iter()
                .zip(&project.rows)
                .enumerate()
                .map(|(i, (meta, focus))| {
                    let (agent, archive) =
                        (SharedString::from(meta.agent.clone()), meta.dir.clone());
                    let dir = meta.dir.clone();
                    let name = SharedString::from(meta.title.clone());
                    let group = SharedString::from(format!("home-row-{i}"));
                    // The keyboard on the row shows its *Delete…* as the
                    // pointer does, and either way it takes the place of
                    // the facts it covers.
                    let keyed =
                        window.last_input_was_keyboard() && focus.contains_focused(window, cx);
                    let row = onehand_plugin_host::row(false, cx)
                        .id(("home", i))
                        .relative()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .rounded_none()
                        .group(group.clone())
                        .track_focus(focus)
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .focus_visible(|row| row.border_color(ring))
                        .child(
                            div()
                                .flex_none()
                                .w(AGE_W)
                                .text_xs()
                                .text_color(crate::theme::meta_ink(cx))
                                .child(rel_time(now, meta.updated)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(meta.title.clone()),
                        )
                        // The agent *is* named here, unlike in a session's
                        // own picker: this list crosses every agent that
                        // has worked in the project, and resuming a row
                        // starts a session on the one that held it.
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .when(keyed, |facts| facts.invisible())
                                .group_hover(group.clone(), |facts| facts.invisible())
                                .child(format!(
                                    "{} · {}",
                                    count_of(meta.item_count, "item"),
                                    meta.agent
                                )),
                        )
                        .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::StartSession {
                                agent: Some(agent.clone()),
                                resume: Some(archive.clone()),
                            });
                        }))
                        // A word rather than a glyph: everything else here
                        // can be done again, and a deleted conversation
                        // cannot. Laid over the row's facts rather than
                        // beside them, so the facts reach the edge; the
                        // press stops there, or it also opens the row.
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right_0()
                                .h_flex()
                                .items_center()
                                .child(
                                    onehand_plugin_host::hover_slot(
                                        group,
                                        false,
                                        crate::controls::action(("home-delete", i))
                                            .quiet(cx)
                                            .short()
                                            .text_color(danger)
                                            .label("Delete…")
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
                                    .focus(focus),
                                ),
                        );
                    row.into_any_element()
                }),
            cx,
        ))
        .children((hidden > 0).then(|| muted_line(format!("{hidden} more"))));

        let tasks = tasks_box(&project.path, cx);
        // What the project is set to: its check command, the shell's field.
        let settings = project.check_input.as_ref().map(|input| {
            page_card("Settings", None, None, cx).child(onehand_plugin_host::field(
                "Check command",
                Some("What Run check and a command step naming none run here".into_any_element()),
                text_input(input, cx).tall(),
                cx,
            ))
        });

        // The history leads and what stands beside it follows. Wrapped rather
        // than measured: the side column drops under the history wherever the
        // two bases do not fit the column the pane gives them.
        let body = div()
            .h_flex()
            .flex_wrap()
            .items_start()
            .gap_4()
            .w_full()
            .child(history.flex_grow_1().flex_basis(HOME_MAIN).min_w_0())
            .child(
                div()
                    .v_flex()
                    .gap_4()
                    .flex_grow_1()
                    .flex_basis(HOME_SIDE)
                    .min_w_0()
                    .child(tasks)
                    .children(settings),
            );

        div()
            .size_full()
            .v_flex()
            .child(self.header(cx))
            .child(
                div()
                    .id("project-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_6()
                    .child(
                        div().h_flex().w_full().justify_center().child(
                            div()
                                .v_flex()
                                .gap_6()
                                .w_full()
                                .max_w(PAGE_WIDE)
                                .text_size(crate::tokens::CHROME_TEXT)
                                .child(head)
                                .child(body),
                        ),
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

/// *New session*, the page's one primary, joined to a caret starting one with
/// another agent while more than one is configured.
fn new_session(cx: &mut Context<ChatPane>) -> gpui::AnyElement {
    let agents: Vec<SharedString> = crate::state::Shared::global(cx)
        .agents
        .iter()
        .map(|spec| SharedString::from(spec.name.clone()))
        .collect();
    // At the head's size, as *Run check* beside it is: one row of actions
    // stands at one height.
    let start = crate::controls::action("project-new-session")
        .tall()
        .primary()
        .label("New session")
        .tooltip_with_action(
            "Start a session in this project",
            &crate::shell::NewSession,
            None,
        )
        .on_click(cx.listener(|_: &mut ChatPane, _, _, cx| {
            cx.emit(ChatPaneEvent::StartSession {
                agent: None,
                resume: None,
            });
        }));
    if agents.len() < 2 {
        return start.into_any_element();
    }
    let pane = cx.entity();
    crate::controls::split_action(
        "project-new-session-menu",
        start,
        crate::controls::action("project-new-session-agent")
            .primary()
            .tall()
            .tooltip("Start a session with another agent"),
        cx.theme().background,
        gpui::Anchor::TopRight,
        move |mut menu, _, _| {
            menu = menu.label("With agent");
            for name in &agents {
                let (name, pane) = (name.clone(), pane.clone());
                menu = menu.item(crate::controls::menu_item(name.clone()).on_click(
                    move |_, _, cx: &mut App| {
                        pane.update(cx, |_: &mut ChatPane, cx| {
                            cx.emit(ChatPaneEvent::StartSession {
                                agent: Some(name.clone()),
                                resume: None,
                            });
                        });
                    },
                ));
            }
            menu
        },
    )
    .into_any_element()
}

/// What the tasks of the project at `root` need, run or wait on, a row each
/// leading to the Tasks page narrowed to it.
fn tasks_box(root: &Path, cx: &mut Context<ChatPane>) -> Div {
    use onehand_core::task::Section;
    let (mut attention, mut running, mut queued) = (0, 0, 0);
    for row in crate::task::rows(&[root.to_path_buf()], cx) {
        match row.group.section() {
            Section::NeedsAttention => attention += 1,
            Section::Running => running += 1,
            Section::Queued => queued += 1,
            Section::Finished => {}
        }
    }
    let warning = crate::theme::status_ink(cx).warning;
    let muted = cx.theme().muted_foreground;
    let lines = [
        ("Need attention", attention, Some(warning)),
        ("Running", running, None),
        ("Queued", queued, None),
    ];
    let total = attention + running + queued;
    let root = root.to_path_buf();
    page_card("Tasks", Some(total), None, cx)
        .when(total == 0, |tasks| {
            tasks.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Nothing running here"),
            )
        })
        .children(
            lines
                .into_iter()
                .enumerate()
                .filter(|(_, (_, n, _))| *n > 0)
                .map(|(i, (label, n, ink))| {
                    let root = root.clone();
                    onehand_plugin_host::row(false, cx)
                        .id(("project-tasks", i))
                        .h_flex()
                        .items_center()
                        .justify_between()
                        .when_some(ink, |row, ink| row.text_color(ink))
                        .child(label)
                        .child(div().text_xs().child(n.to_string()))
                        .tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new(
                                "Show these tasks on the Tasks page",
                            )
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                            cx.emit(ChatPaneEvent::ShowTasks(Some(root.clone())));
                        }))
                }),
        )
}
