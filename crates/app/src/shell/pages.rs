//! The pages a window shows in a session's place, the workspace's and the
//! tasks', and the projects each lists.

use super::Shell;
use gpui::{App, Context, SharedString, Window};
use gpui_component::dock::DockPlacement;
use onehand_core::gitstat;

impl Shell {
    /// Show the workspace page: what waits and what works across every
    /// session and run, every project, the recent conversations and the open
    /// issues. Leaving it is [`Self::show_active_session`]'s, which every rail
    /// click goes through, and which puts back the Workbench this put away.
    pub fn show_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let projects = self.page_projects();
        self.docks_aside(window, cx);
        let view: gpui::AnyView = self.workspace_page.clone().into();
        self.chat.update(cx, |pane, cx| {
            pane.show_page(crate::chat::pane::PageKind::Workspace, view, window, cx)
        });
        self.workspace_page
            .update(cx, |page, cx| page.show(projects, cx));
        // No session is showing now, so there is no running agent to be
        // behind on anything.
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Show the Tasks page, narrowed to the project at `filter` or not.
    /// Left the way the workspace page is.
    pub fn show_tasks(
        &mut self,
        filter: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects = self.local_page_projects();
        self.docks_aside(window, cx);
        let view: gpui::AnyView = self.tasks_page.clone().into();
        self.chat.update(cx, |pane, cx| {
            pane.show_page(crate::chat::pane::PageKind::Tasks, view, window, cx)
        });
        self.tasks_page
            .update(cx, |page, cx| page.show(projects, filter, window, cx));
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Show the Tasks page with task `id` open in it.
    pub fn show_task(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.show_tasks(None, window, cx);
        let id = id.to_string();
        self.tasks_page
            .update(cx, |page, cx| page.open_task(Some(id), cx));
    }

    /// Show project `project`'s board. Left the way the workspace page is.
    pub(crate) fn show_board(
        &mut self,
        project: onehand_core::project::ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.docks_aside(window, cx);
        let view: gpui::AnyView = self.board_page.clone().into();
        self.chat.update(cx, |pane, cx| {
            pane.show_page(crate::chat::pane::PageKind::Board, view, window, cx)
        });
        self.board_page
            .update(cx, |page, cx| page.show(project, window, cx));
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Show the Workflows page on the workflow of id `id`, its step `step`
    /// open if one is named, for the board of `for_board` if it was asked
    /// for from one.
    pub(crate) fn show_workflow(
        &mut self,
        id: &str,
        step: Option<&str>,
        for_board: Option<onehand_core::project::ProjectId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_workflows(window, cx);
        self.workflows_page.update(cx, |page, cx| {
            page.open_workflow(id, step, for_board, window, cx)
        });
    }

    /// The board of the project at row `root_idx`, if it has one and is on
    /// this machine.
    pub(crate) fn board_of_row(
        &self,
        root_idx: usize,
        cx: &App,
    ) -> Option<onehand_core::project::ProjectId> {
        crate::projects::board_here(self.window.workspace.roots.get(root_idx)?, cx)
    }

    /// Show the board of the project on screen, or say it has none.
    pub(super) fn show_active_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.window.workspace.active_root;
        match self.board_of_row(at, cx) {
            Some(project) => self.show_board(project, window, cx),
            None => {
                let on_host = self.window.workspace.active_root().is_some_and(|root| {
                    root.project
                        .as_ref()
                        .and_then(|id| crate::state::Shared::global(cx).projects.get(id))
                        .is_some_and(|p| {
                            p.board().is_some() && (p.host.is_some() || root.local_path().is_none())
                        })
                });
                let note = crate::toast::info(match on_host {
                    true => "A board works on a project on this machine",
                    false => "This project has no board",
                });
                gpui_component::WindowExt::push_notification(window, note, cx);
            }
        }
    }

    /// Answer what the board page asked.
    pub(super) fn on_board_page(
        &mut self,
        event: &crate::board_page::BoardPageEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::board_page::BoardPageEvent as E;
        match event {
            E::OpenTask(id) => self.show_task(id, window, cx),
            E::Task(event) => self.on_tasks_page(event, window, cx),
            E::ProjectSettings(root) => self.show_project_settings(root.clone(), window, cx),
            E::EditWorkflow { step } => {
                let Some(project) = self.board_page.read(cx).project().cloned() else {
                    return;
                };
                let bound = crate::state::Shared::global(cx)
                    .projects
                    .get(&project)
                    .and_then(|p| p.board())
                    .map(|board| board.workflow.clone());
                if let Some(workflow) = bound {
                    self.show_workflow(&workflow, step.as_deref(), Some(project), window, cx);
                }
            }
        }
    }

    /// Both docks hold one project's things, and a page is about all of
    /// them. Put away, not closed: the terminal's state is filed under its
    /// project and the root on screen let go, so the next arrival is a handover
    /// that restores it, and the Workbench comes back on leaving.
    pub(super) fn docks_aside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page_shown(cx) {
            return;
        }
        let live = self.dock.read(cx).is_dock_open(DockPlacement::Bottom);
        self.terminal_homes.put_away(live);
        self.set_terminal_visible(false, window, cx);
        self.page_hid_workbench = self.workbench_open(cx) || self.stepped_aside;
        self.hide_workbench(window, cx);
    }

    /// Every project as the workspace page lists it, in rail order, those on
    /// a host included.
    pub(super) fn page_projects(&self) -> Vec<crate::page_parts::PageProject> {
        page_projects_in(&self.window.workspace, Reach::Overview)
    }

    /// The projects the Tasks, Issues, Workflows and Schedules pages list:
    /// those on this machine.
    pub(super) fn local_page_projects(&self) -> Vec<crate::page_parts::PageProject> {
        page_projects_in(&self.window.workspace, Reach::Local)
    }

    /// Whether the workspace page is what the centre of the window shows, for
    /// the rail row that leads to it.
    pub fn workspace_shown(&self, cx: &App) -> bool {
        self.shown_page(cx) == Some(crate::chat::pane::PageKind::Workspace)
    }

    /// Whether the Tasks page is what the centre of the window shows.
    pub fn tasks_shown(&self, cx: &App) -> bool {
        self.shown_page(cx) == Some(crate::chat::pane::PageKind::Tasks)
    }

    /// Which page the centre of the window shows, if any.
    pub fn shown_page(&self, cx: &App) -> Option<crate::chat::pane::PageKind> {
        self.chat.read(cx).shown_page()
    }

    /// Whether either page is what the centre of the window shows: no
    /// project is on screen, and neither dock is.
    pub fn page_shown(&self, cx: &App) -> bool {
        self.chat.read(cx).showing_page()
    }

    /// The projects the pages list, as paths.
    pub fn page_roots(&self) -> Vec<std::path::PathBuf> {
        page_roots_in(&self.window.workspace)
    }
}

/// Which projects a page lists.
#[derive(Clone, Copy)]
pub(super) enum Reach {
    /// The workspace page, which shows every project and its sessions.
    Overview,
    /// A page about issues, tasks, workflows or schedules: a folder on a host
    /// has no issue files, runs or checks this machine can reach.
    Local,
}

/// Whether a page of `reach` lists a project. A run's own worktree is not a
/// project anybody chose, and its issues are the project's it was cut from.
fn listed(root: &onehand_core::workspace::ProjectRoot, reach: Reach) -> bool {
    !root.transient
        && match reach {
            Reach::Overview => true,
            Reach::Local => root.runs_here(),
        }
}

/// The projects a page of `reach` lists, in rail order. A host's project has
/// no issue file here: what this machine files under its path is a folder of
/// the same path here. Its `git status` is read over its attach.
pub(super) fn page_projects_in(
    workspace: &onehand_core::workspace::WorkspaceDrive,
    reach: Reach,
) -> Vec<crate::page_parts::PageProject> {
    workspace
        .display_order()
        .into_iter()
        .map(|idx| &workspace.roots[idx])
        .filter(|root| listed(root, reach))
        .map(|root| {
            let local = root.local_path();
            crate::page_parts::PageProject {
                label: SharedString::from(root.label.clone()),
                root: root.path.clone(),
                host: root.on_host().cloned(),
                sessions: root
                    .sessions
                    .iter()
                    .map(|session| (session.uid, SharedString::from(session.title().to_string())))
                    .collect(),
                git: workspace.git_of(root).map(gitstat::GitStatus::label),
                issues: local
                    .and(root.project.as_ref())
                    .map(onehand_core::project::file::issues_file),
            }
        })
        .collect()
}

/// The projects the pages list, as paths.
pub(super) fn page_roots_in(
    workspace: &onehand_core::workspace::Workspace,
) -> Vec<std::path::PathBuf> {
    workspace
        .roots
        .iter()
        .filter(|root| listed(root, Reach::Local))
        .map(|root| root.path.clone())
        .collect()
}
