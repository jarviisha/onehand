use super::header::Archives;
use super::{ChatPane, ChatPaneEvent, ProjectFacts, build_chip, rel_time};
use crate::controls::Short as _;
use crate::page_parts::{count_of, page_card, page_head, ruled};
use crate::tokens::PAGE_PROSE;
use gpui::{
    App, Context, FocusHandle, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::chat::ConvMeta;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub(super) mod facts;
mod host;
mod no_agent;
pub(super) use host::focus_rows as host_rows;
mod offload_box;
mod worktrees_box;

#[cfg(test)]
mod tests;

/// How many past conversations the project page lists.
///
/// The page is an entrance, not an archive browser: the newest handful is what
/// "where was I" needs, and a project worked in for months would otherwise draw
/// a column of hundreds for the sake of the two or three anybody came for. What
/// the cap cut off is said on screen rather than silently dropped. The list
/// scrolls as well, because this many rows already outgrows a short window.
const HOME_ROWS: usize = 8;

/// How much faster than the part beside it a part of a line that gives way
/// first shrinks. Flex shares a shortfall by each part's shrink factor times
/// its width, so a factor this large makes it give way all but whole before
/// the other starts to.
const GIVES_FIRST: f32 = 1e3;

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
    /// The host the folder's sessions run on; `None` for one whose sessions
    /// run here.
    pub(super) host: Option<onehand_core::workspace::HostName>,
    /// The mirror an offloaded project's sessions run in on `host`; `None`
    /// for a host's own folder, whose sessions run in `path`.
    pub(super) mirror: Option<PathBuf>,
    /// Its past conversations, across every agent. `None` while the scan is
    /// still running, which is a different thing from a project that has none:
    /// one is a wait and the other is an answer.
    pub(super) history: Option<Vec<ConvMeta>>,
    /// What the host said about its sessions in the folder, for one on a
    /// host; `None` until it has answered.
    pub(super) listed: host::Listed,
    /// Whether an ask of the host is under way.
    pub(super) listing: bool,
    /// The newest ask of the host, so an older answer landing late is dropped.
    pub(super) asked: u64,
    /// Whether the host answers the folder's cards by itself, as it last
    /// said; `None` until it has, which draws no switch.
    pub(super) bypass: Option<bool>,
    /// What the page's own menu needs to know about the project and cannot
    /// work out: they live in the workspace tree and in a `git status` sweep,
    /// and both are the shell's. Pushed rather than asked for, like everything
    /// else the pane knows about the window, and pushed again whenever one
    /// changes — a menu still offering *Pin to top* on a project pinned a
    /// second ago is worse than one that does not offer it at all.
    pub(super) facts: ProjectFacts,
    /// git's list of the project's worktrees, as its rows; `None` while it is
    /// read.
    pub(super) worktrees: Option<worktrees_box::Read>,
    /// How many sessions run in each folder of the project, the shell's.
    pub(super) worktree_sessions: std::collections::HashMap<PathBuf, usize>,
    /// The worktrees the project records, as the shell last told them: a
    /// change is what reads git's list again. `None` until told.
    pub(super) recorded: Option<Vec<PathBuf>>,
    /// The newest read of git's worktrees asked, so an older answer landing
    /// late is dropped.
    pub(super) worktrees_asked: u64,
    /// The project's worktrees on disk, by folder and branch, as the shell
    /// last told them: what *In <branch>* lists.
    pub(super) on_disk: Vec<(PathBuf, String)>,
    /// One focus handle per past conversation the page lists, by place: each
    /// row is a tab stop, so the keyboard reaches its *Delete…* as the pointer
    /// does.
    pub(super) rows: Vec<FocusHandle>,
    /// One focus handle per session of its host the page lists, by place, so
    /// the keyboard opens one with `Enter` as the pointer does; none on a
    /// page with no host.
    pub(super) host_rows: Vec<FocusHandle>,
}

impl EmptyProject {
    /// The folder its sessions run in on its host: the mirror of an
    /// offloaded project, else its own path.
    pub(super) fn cwd(&self) -> &std::path::Path {
        self.mirror.as_deref().unwrap_or(&self.path)
    }

    /// Whether its folder is on this machine, its sessions running here or
    /// offloaded: what has worktrees.
    pub(super) fn here(&self) -> bool {
        self.host.is_none() || self.mirror.is_some()
    }

    /// Its host and that folder, when its sessions run on one.
    pub(super) fn on_host(&self) -> Option<(&onehand_core::workspace::HostName, &std::path::Path)> {
        Some((self.host.as_ref()?, self.cwd()))
    }
}

/// The focus handles for the page's listed conversations, one per row it can
/// list.
pub(super) fn home_rows(cx: &mut App) -> Vec<FocusHandle> {
    (0..HOME_ROWS)
        .map(|_| cx.focus_handle().tab_stop(true))
        .collect()
}

/// What resuming the past conversation `meta`, listed for the folder
/// `folder`, asks: a session on that folder when it ran there; else (it ran in
/// a folder its project no longer holds) the conversation by its id, which
/// resumes it where it ran and refuses a folder that is gone. Never a session
/// in `folder`: its agent would load the wrong place, and its record of where
/// it ran would be written over.
pub(super) fn resume_of(folder: &Path, meta: &onehand_core::chat::ConvMeta) -> Resume {
    match meta.root == folder {
        true => Resume::Here(SharedString::from(meta.agent.clone()), meta.dir.clone()),
        false => Resume::WhereItRan(meta.session_id.clone()),
    }
}

/// How a past conversation is resumed: see [`resume_of`].
#[derive(Clone)]
pub(super) enum Resume {
    /// On the row's folder, by `agent`, from the archive.
    Here(SharedString, PathBuf),
    /// By its id, where it ran.
    WhereItRan(String),
}

impl Resume {
    pub(super) fn event(&self) -> ChatPaneEvent {
        match self {
            Resume::Here(agent, archive) => ChatPaneEvent::StartSession {
                agent: Some(agent.clone()),
                resume: Some(archive.clone()),
            },
            Resume::WhereItRan(session) => ChatPaneEvent::OpenConversation(session.clone()),
        }
    }
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
        let scope = crate::projects::scope_of(&path, cx);
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &scope,
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
        let Some(path) = self
            .empty
            .as_ref()
            .filter(|project| project.host.is_none() || project.mirror.is_some())
            .map(|project| project.path.clone())
        else {
            return;
        };
        let scope = crate::projects::scope_of(&path, cx);
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &scope,
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
    /// Offered on the project page and nowhere else. The page can show while
    /// the project's sessions run, so a conversation listed there may be open
    /// in one, here or in another window, and the check below refuses it: a
    /// live conversation deleted underneath its own session would not even
    /// stay deleted -- the session's next turn writes the file again, holding
    /// only what came after, because its mark says the rest is already on disk.
    fn delete_conversation(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let live = crate::chat::is_open(&self.open_everywhere(cx), &dir);
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
    /// project has no conversation on screen, whether or not its sessions run.
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
    /// Each group under it stands on a surface a step up, one under another:
    /// the history, then what the project needs and is set to, so
    /// what belongs together reads as one object rather than as text loose on
    /// the page.
    pub(super) fn project_home(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Ahead of whether there is a project: with no agent nothing can be
        // started anywhere, and that is the first thing to put right.
        if crate::state::Shared::global(cx).settings.agents.is_empty() {
            return no_agent::no_agent(cx);
        }
        let Some(project) = self.empty.as_ref() else {
            // Not a project with nothing in it -- no project at all. Naming
            // what has to happen first beats an offer that cannot be taken:
            // every session belongs to a root, and there is no root to bind one
            // to.
            return onehand_ui::hint("Add a project to start a session", cx);
        };
        let now = onehand_core::chat::now_secs();
        let (muted, ring) = (cx.theme().muted_foreground, cx.theme().ring);
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
        let path = host::place(
            project.host.as_ref().filter(|_| project.mirror.is_none()),
            &project.path,
            std::env::home_dir().as_deref(),
        );
        // On one line: the path gives way first, from its start, so the
        // folder's own name is the last of it to go, and then what git says.
        let place = div()
            .h_flex()
            .min_w_0()
            .child(facts::natural(
                path.clone(),
                div()
                    .debug_selector(|| "project-page-path".into())
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .child(path)
                    .into_any_element(),
                GIVES_FIRST,
            ))
            .children(self.git.as_ref().map(|git| {
                let git = format!(" · {git}");
                facts::natural(
                    git.clone(),
                    div()
                        .debug_selector(|| "project-page-git".into())
                        .truncate()
                        .child(git)
                        .into_any_element(),
                    1.,
                )
            }));
        // The project as the store has it, for its worktrees: none for a
        // folder of no project, or one on a host, which has none. An
        // offloaded folder's worktrees run here or on their own.
        let kept = (project.here())
            .then(|| crate::projects::of(&project.path, cx))
            .flatten();
        // Where the session it starts runs: its own folder, a worktree on
        // disk, or a new one where git can cut it.
        // An offloaded project's sessions run in its mirror, so it offers its
        // own folder alone, as a host project does.
        let worktrees: &[_] = match project.host {
            None => &project.on_disk,
            Some(_) => &[],
        };
        let composer =
            self.page_composer(&project.path, worktrees, project.facts.is_repo, window, cx);
        // The head: the project named as every page is, over where it
        // is, and what it can start at the end.
        let head = page_head(
            project.label.clone(),
            Some(place.into_any_element()),
            kept.as_ref()
                .and_then(|kept| build_chip::build_chip(kept, cx))
                .into_iter()
                .chain(kept.as_ref().and_then(|kept| board_link(kept, cx)))
                .chain(project.facts.check.then(|| {
                    crate::controls::action("project-run-check")
                        .short()
                        .secondary()
                        .label("Run check")
                        .loading(checking)
                        .tooltip("Run the project's check command")
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::RunCheck);
                        }))
                        .into_any_element()
                }))
                .chain(
                    project
                        .host
                        .as_ref()
                        .map(|host| host::refresh(host, project.listing, cx)),
                )
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
                    let resumed = resume_of(&project.path, &meta);
                    let dir = meta.dir.clone();
                    let name = SharedString::from(meta.title.clone());
                    let group = SharedString::from(format!("home-row-{i}"));
                    // Resuming is the row's own press; its ⋯ and a
                    // right-click offer it by name, and the removal.
                    let pane = cx.entity().downgrade();
                    let carry_pane = pane.clone();
                    let resume = {
                        let (pane, resumed) = (pane.clone(), resumed.clone());
                        Rc::new(move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                            let _ = pane.update(cx, |_: &mut Self, cx| cx.emit(resumed.event()));
                        })
                            as Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>
                    };
                    let delete = Rc::new(
                        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                            let (dir, name) = (dir.clone(), name.clone());
                            let _ = pane.update(cx, |pane: &mut Self, cx| {
                                pane.confirm_delete(dir, name, window, cx);
                            });
                        },
                    )
                        as Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;
                    // Carried into a new session from the page's own
                    // composer, with whichever agent is picked there; a
                    // page whose sessions run on a host does not offer it.
                    let carry = project.host.is_none().then(|| {
                        let (pane, meta) = (carry_pane.clone(), meta.clone());
                        Rc::new(
                            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                                let _ = pane.update(cx, |pane: &mut Self, cx| {
                                    pane.continue_on_page(&meta, window, cx);
                                });
                            },
                        )
                            as Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>
                    });
                    let acts: Rc<[onehand_ui::RowAct]> =
                        std::iter::once(onehand_ui::RowAct::new("Resume", resume))
                            .chain(carry.map(|carry| {
                                onehand_ui::RowAct::new("Continue in a new session…", carry)
                            }))
                            .chain([onehand_ui::RowAct::new("Delete…", delete).danger()])
                            .collect();
                    let menu_acts = acts.clone();
                    let row = onehand_ui::row(false, cx)
                        .id(("home", i))
                        .debug_selector(move || format!("home-row-{i}"))
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
                                .w(crate::page_parts::ROW_LEAD_W)
                                .whitespace_nowrap()
                                .text_xs()
                                .text_color(crate::theme::meta_ink(cx))
                                .child(rel_time(now, meta.updated)),
                        )
                        .child(facts::natural(
                            meta.title.clone(),
                            div()
                                .id(("home-title", i))
                                .debug_selector(move || format!("home-title-{i}"))
                                .truncate()
                                .child(meta.title.clone())
                                // Said whole, since the row cuts it first at
                                // a narrow width.
                                .tooltip({
                                    let whole = SharedString::from(meta.title.clone());
                                    onehand_ui::tip(move |window, cx| {
                                        gpui_component::tooltip::Tooltip::new(whole.clone())
                                            .build(window, cx)
                                    })
                                })
                                .into_any_element(),
                            1.,
                        ))
                        // The agent *is* named here, unlike in a session's
                        // own picker: this list crosses every agent that
                        // has worked in the project, and resuming a row
                        // starts a session on the one that held it.
                        // The title is what the row is for, so what stands
                        // beside it gives way first: the facts, cut and then
                        // dropped, then what it continued, and only then the
                        // title.
                        .children(meta.continued.as_ref().map(|earlier| {
                            div()
                                .flex_shrink(GIVES_FIRST)
                                .min_w_0()
                                .max_w(gpui::relative(0.4))
                                .truncate()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Continued from {earlier}"))
                        }))
                        // The count leads: cut short it says nothing, so
                        // the line goes once the count no longer fits.
                        .child(facts::facts(
                            div()
                                .debug_selector(move || format!("home-facts-{i}"))
                                .flex_none()
                                .whitespace_nowrap()
                                .text_xs()
                                .text_color(muted)
                                .child(count_of(meta.item_count, "item"))
                                .into_any_element(),
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(muted)
                                .child(format!(" · {}", meta.agent))
                                .into_any_element(),
                            true,
                        ))
                        .on_click(
                            cx.listener(move |_: &mut Self, _, _, cx| cx.emit(resumed.event())),
                        )
                        // The press on ⋯ stays its own, or it also opens the
                        // row.
                        .child(
                            div()
                                .debug_selector(move || format!("home-more-{i}"))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .child(
                                    onehand_ui::hover_slot(
                                        group,
                                        false,
                                        onehand_ui::row_more(
                                            format!("home-more-{i}"),
                                            "Conversation actions",
                                            acts,
                                            cx,
                                        ),
                                    )
                                    .focus(focus),
                                ),
                        )
                        .context_menu(move |menu, _, cx| {
                            onehand_ui::row_menu_items(menu, &menu_acts, cx)
                        });
                    row.into_any_element()
                }),
            cx,
        ))
        .children(
            (hidden > 0).then(|| {
                muted_line(format!("{hidden} more")).debug_selector(|| "home-hidden".into())
            }),
        );

        // One column at every width: the history leads, and what the project
        // needs and is set to follows under it, so the page reads top to
        // bottom however wide the pane is.
        let body = div()
            .v_flex()
            .gap_4()
            .w_full()
            .children(
                project
                    .host
                    .as_ref()
                    .map(|host| host::host_sessions(host, project, cx)),
            )
            // An offloaded project's conversations from before it went stay
            // readable here, under the host's.
            .children((project.host.is_none() || project.mirror.is_some()).then_some(history))
            .children(
                kept.as_ref()
                    .filter(|kept| worktrees_box::shows_box(kept, project.worktrees.as_ref()))
                    .map(|kept| worktrees_box::worktrees_box(project, kept, cx)),
            )
            .children(
                project
                    .mirror
                    .is_some()
                    .then(|| offload_box::offload_box(&project.path, cx))
                    .flatten(),
            );

        div()
            .size_full()
            .v_flex()
            .child(self.header(cx))
            .child(
                div()
                    .id("project-page")
                    .debug_selector(|| "project-page".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_6()
                    .child(
                        div().h_flex().w_full().justify_center().child(
                            div()
                                .debug_selector(|| "project-page-column".into())
                                .v_flex()
                                .gap_6()
                                .w_full()
                                .max_w(PAGE_PROSE)
                                .text_size(crate::tokens::CHROME_TEXT)
                                .child(head)
                                .child(composer)
                                .child(body),
                        ),
                    ),
            )
            .into_any_element()
    }
}

/// The way to `project`'s board, saying what works on it and what waits in
/// its To do; none for a project with no board.
fn board_link(
    project: &onehand_core::project::Project,
    cx: &mut Context<ChatPane>,
) -> Option<gpui::AnyElement> {
    let board = project.board().filter(|_| project.host.is_none())?;
    let running = crate::task::board_running(&project.id, cx);
    let label = format!("Board · {running} running · {} to do", board.todo.len());
    let id = project.id.clone();
    Some(
        crate::controls::action("project-board")
            .short()
            .secondary()
            .label(label)
            .tooltip("Show the project's board")
            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                cx.emit(ChatPaneEvent::ShowBoard(id.clone()));
            }))
            .into_any_element(),
    )
}
