//! The project page's *Worktrees*: those its project records, on disk or
//! missing, and the checkouts git lists that no project records.
//!
//! **Which worktrees exist is git's.** The page reads git's list when it
//! opens and whenever the project's record changes; while that read runs, or
//! when it fails, the recorded worktrees are drawn as they are recorded, with
//! a line saying so, never an empty box.

use super::{ChatPane, ChatPaneEvent, EmptyProject};
use crate::chat::pane::WorktreeAct;
use crate::controls::Refuses as _;
use crate::controls::Short as _;
use crate::page_parts::page_card;
use crate::settings::{list_row, list_rows};
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, Context, Div, IntoElement, ParentElement, Styled, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::project::Project;
use onehand_core::project::worktrees::WorktreeRow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::projects::{WORKTREES_SHOWN, more_not_shown};

/// What git's read of the project's worktrees came to: its rows, or why
/// there are none.
pub(in crate::chat::pane) type Read = Result<Vec<WorktreeRow>, String>;

/// One line of the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Line {
    pub(super) branch: String,
    pub(super) folder: PathBuf,
    pub(super) kind: Kind,
    /// The checkout as git lists it, for an offered line: what *Add to this
    /// project* takes in.
    pub(super) listed: Option<onehand_core::worktree::Listed>,
}

/// What a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Recorded and on disk, with how many sessions run in it here.
    Recorded { sessions: usize },
    /// Recorded, and git lists it no more.
    Missing,
    /// Listed by git, recorded by no project.
    Offered,
}

/// The box's lines for `project` as git's list `read` has them (`None`
/// while it is read), `sessions` counting what runs in each folder, and the
/// note said over them: the read under way, or why it failed.
pub(super) fn lines(
    project: &Project,
    read: Option<&Read>,
    sessions: &HashMap<PathBuf, usize>,
) -> (Vec<Line>, Option<String>) {
    // With git unread, or failing, the recorded worktrees as recorded.
    let (rows, note) = match read {
        Some(Ok(rows)) => (Some(rows.clone()), None),
        Some(Err(why)) => (
            None,
            Some(format!("git's worktrees could not be read: {why}")),
        ),
        None => (None, Some("Reading git's worktrees…".to_string())),
    };
    let rows = rows.unwrap_or_else(|| {
        onehand_core::project::worktrees::rows(project, None, &Default::default(), |_| true)
    });
    let lines = rows
        .iter()
        .filter_map(|row| match row {
            // One the record no longer holds, git read before it changed,
            // is not drawn.
            WorktreeRow::Recorded { path, on_disk } => {
                let worktree = project.worktrees.iter().find(|w| w.path == *path)?;
                let kind = match on_disk {
                    true => Kind::Recorded {
                        sessions: sessions.get(&worktree.path).copied().unwrap_or(0),
                    },
                    false => Kind::Missing,
                };
                Some(Line {
                    branch: worktree.branch.clone(),
                    folder: worktree.path.clone(),
                    kind,
                    listed: None,
                })
            }
            WorktreeRow::Offered(listed) => Some(Line {
                branch: listed.branch_or_head(),
                folder: listed.path.clone(),
                kind: Kind::Offered,
                listed: Some(listed.clone()),
            }),
        })
        .collect();
    (lines, note)
}

/// Whether the page draws the box: not for a folder outside git that
/// records no worktree, which has nothing to say but git's refusal.
pub(super) fn shows_box(project: &Project, read: Option<&Read>) -> bool {
    !matches!(read, Some(Err(_))) || !project.worktrees.is_empty()
}

/// What a line says under its branch: its folder, then what runs in it, that
/// it is being removed, or that it is missing.
fn about(line: &Line, removing: bool) -> String {
    let folder = super::host::place(None, &line.folder, std::env::home_dir().as_deref());
    if removing {
        return format!("{folder} · Removing…");
    }
    match line.kind {
        Kind::Recorded { sessions: 0 } => folder,
        Kind::Offered => format!("{folder} · Not in this project"),
        Kind::Recorded { sessions: 1 } => format!("{folder} · 1 session"),
        Kind::Recorded { sessions } => format!("{folder} · {sessions} sessions"),
        Kind::Missing => format!("{folder} · Missing"),
    }
}

/// The box, for a project folder on this machine (offloaded or not) of
/// `project`.
pub(super) fn worktrees_box(
    page: &EmptyProject,
    project: &Project,
    cx: &mut Context<ChatPane>,
) -> Div {
    let (lines, note) = lines(project, page.worktrees.as_ref(), &page.worktree_sessions);
    let muted = cx.theme().muted_foreground;
    let said = |line: String| div().text_xs().text_color(muted).child(line);
    let more = more_not_shown(lines.len());
    let none = lines.is_empty() && matches!(page.worktrees, Some(Ok(_)));
    let emit = |act: WorktreeAct| {
        move |_: &mut ChatPane,
              _: &gpui::ClickEvent,
              _: &mut gpui::Window,
              cx: &mut Context<ChatPane>| { cx.emit(ChatPaneEvent::Worktree(act.clone())) }
    };
    let rows: Vec<AnyElement> = lines
        .iter()
        .take(WORKTREES_SHOWN)
        .enumerate()
        .map(|(idx, line)| {
            let folder = line.folder.clone();
            let removing = crate::state::Shared::global(cx).projects.removing(&folder);
            let control = match line.kind {
                // *Remove worktree…* behind the row's ⋯, as a past
                // conversation's *Delete…* is.
                Kind::Recorded { .. } => {
                    let pane = cx.entity().downgrade();
                    let at = folder.clone();
                    let remove = std::rc::Rc::new(
                        move |_: &gpui::ClickEvent, _: &mut gpui::Window, cx: &mut gpui::App| {
                            let act = WorktreeAct::Remove(at.clone());
                            let _ = pane.update(cx, |_: &mut ChatPane, cx| {
                                cx.emit(ChatPaneEvent::Worktree(act))
                            });
                        },
                    )
                        as std::rc::Rc<
                            dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App),
                        >;
                    let acts: std::rc::Rc<[onehand_ui::RowAct]> = std::rc::Rc::new([
                        onehand_ui::RowAct::new("Remove worktree…", remove).danger(),
                    ]);
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            crate::controls::action(("worktree-session", idx))
                                .short()
                                .secondary()
                                .label("New session")
                                .on_click(cx.listener(emit(WorktreeAct::NewSession(folder)))),
                        )
                        .when(!removing, |row| {
                            row.child(onehand_ui::row_more(
                                format!("worktree-more-{idx}"),
                                "Worktree actions",
                                acts,
                                cx,
                            ))
                        })
                }
                Kind::Missing => div().child(
                    crate::controls::action(("worktree-forget", idx))
                        .short()
                        .secondary()
                        .label("Forget")
                        .tooltip("Its folder is gone; forget it in this project")
                        .on_click(cx.listener(emit(WorktreeAct::Forget(folder)))),
                ),
                Kind::Offered => div().children(line.listed.clone().map(|listed| {
                    crate::controls::action(("worktree-add", idx))
                        .short()
                        .secondary()
                        .label("Add to this project")
                        .on_click(cx.listener(emit(WorktreeAct::Add(listed))))
                })),
            };
            list_row(
                line.branch.clone(),
                Some(about(line, removing).into_any_element()),
                control,
                cx,
            )
            .into_any_element()
        })
        .collect();
    let parent = match &project.worktree_parent {
        Some(parent) => super::host::place(None, parent, std::env::home_dir().as_deref()),
        None => "Beside the repository".to_string(),
    };
    let set = project.worktree_parent.is_some();
    let recorded = project.worktrees.len();
    page_card("Worktrees", Some(recorded), None, cx)
        .children(note.map(said))
        .children((!rows.is_empty()).then(|| list_rows(rows, cx)))
        .children(more.map(said))
        .children(none.then(|| said("No worktrees".into())))
        .child(
            div().h_flex().child(
                crate::controls::action("worktree-new")
                    .short()
                    .secondary()
                    .label("New worktree…")
                    .refuses((!page.facts.is_repo).then_some(
                        "Only a git repository whose sessions run here takes a new worktree",
                    ))
                    .on_click(cx.listener(|_: &mut ChatPane, _, _, cx| {
                        cx.emit(ChatPaneEvent::Project(
                            crate::chat::pane::ProjectAction::Worktree,
                        ));
                    })),
            ),
        )
        .child(onehand_ui::field(
            "Worktree folder",
            Some("Where new worktrees of this project go".into_any_element()),
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().min_w_0().truncate().child(parent))
                .child(
                    crate::controls::action("worktree-folder")
                        .short()
                        .secondary()
                        .label("Change…")
                        .on_click(cx.listener(emit(WorktreeAct::PickFolder))),
                )
                .when(set, |row| {
                    row.child(
                        crate::controls::action("worktree-folder-beside")
                            .short()
                            .secondary()
                            .label("Beside the repository")
                            .on_click(cx.listener(emit(WorktreeAct::ClearFolder))),
                    )
                }),
            cx,
        ))
}

impl ChatPane {
    /// Read git's worktrees of the project on screen again: git was read
    /// again, and a worktree may have come or gone outside onehand.
    pub(crate) fn reread_worktrees(&mut self, cx: &mut Context<Self>) {
        if self
            .empty
            .as_ref()
            .is_some_and(|page| page.recorded.is_some())
        {
            self.scan_worktrees(cx);
        }
    }

    /// Told by the shell the worktrees the project on screen records and how
    /// many sessions run in each of its folders; git's list of them is read
    /// again when the record changed.
    pub(crate) fn set_worktree_facts(
        &mut self,
        recorded: Vec<std::path::PathBuf>,
        sessions: std::collections::HashMap<std::path::PathBuf, usize>,
        on_disk: Vec<(PathBuf, String)>,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.empty.as_mut() else {
            return;
        };
        let changed = project.recorded.as_ref() != Some(&recorded);
        if changed || project.worktree_sessions != sessions || project.on_disk != on_disk {
            project.recorded = Some(recorded);
            project.worktree_sessions = sessions;
            project.on_disk = on_disk;
            cx.notify();
        }
        if changed {
            self.scan_worktrees(cx);
        }
    }

    /// Read git's worktrees of the project on screen, a project on this
    /// machine, off the UI loop: its rows, kept while the page is still of
    /// that project.
    fn scan_worktrees(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .empty
            .as_ref()
            .filter(|project| project.here())
            .map(|project| project.path.clone())
        else {
            return;
        };
        let Some(project) = crate::projects::of(&path, cx) else {
            return;
        };
        let others = (*crate::state::Shared::global(cx).projects).clone();
        // Only the newest read is kept: two can be under way, and land in
        // any order.
        let asked = self.empty.as_mut().map_or(0, |page| {
            page.worktrees_asked += 1;
            page.worktrees_asked
        });
        cx.spawn(async move |pane, cx| {
            let at = path.clone();
            let read = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::worktree::list_blocking(&at).map(|listed| {
                        onehand_core::project::worktrees::rows(
                            &project,
                            Some(&listed),
                            &others,
                            Path::is_dir,
                        )
                    })
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // An answer about a project the page has left is not its own.
                if let Some(page) = (pane.empty.as_mut())
                    .filter(|page| page.path == path && page.worktrees_asked == asked)
                {
                    page.worktrees = Some(read);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
