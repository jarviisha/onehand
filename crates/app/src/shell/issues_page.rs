//! The Issues page as the window holds it: made once per window, so its
//! filters, its selection and its scroll are still there whenever it is
//! picked again, and told what the Workbench's Issues mode is told. What an
//! Issues view tells the shell, the tab's and the page's alike, is answered
//! here; so are the Workbench's own events, since both are the shell's
//! answers.

use super::Shell;
use crate::workbench::WorkbenchEvent;
use gpui::{Context, Focusable as _, SharedString, Window};
use gpui_component::dock::DockPlacement;
use onehand_issues::{IssuesEvent, Page, Tell};
use onehand_workbench::{Ask, Request};
use std::path::Path;
use std::rc::Rc;

impl Shell {
    /// How an Issues view, the tab's or the page's, reaches this window's
    /// shell.
    pub(super) fn issues_tell(cx: &mut Context<Self>) -> Tell {
        let shell = cx.weak_entity();
        Rc::new(move |event, window, cx| {
            let _ = shell.update(cx, |shell: &mut Self, cx| {
                shell.on_issues_event(event, window, cx)
            });
        })
    }

    /// The page, with what it asks routed to this window's shell: a file it
    /// names to the Workbench, everything else to [`Self::on_issues_event`].
    pub(super) fn new_issues_page(tell: Tell, cx: &mut Context<Self>) -> Page {
        let shell = cx.weak_entity();
        let ask: Ask = Rc::new(move |request, window, cx| {
            let _ = shell.update(cx, |shell: &mut Self, cx| {
                if let Request::OpenFile(path) = request {
                    shell.open_page_file(path, window, cx)
                }
            });
        });
        Page::new(crate::features::connectors(), ask, tell, cx)
    }

    /// Answer what the Workbench asked for.
    pub(super) fn on_workbench_event(
        &mut self,
        event: &WorkbenchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use WorkbenchEvent as E;
        match event {
            E::Hide => self.hide_workbench(window, cx),
            E::StepAside => self.step_aside(window, cx),
            E::RestartAgent => self.restart_session(window, cx),
            E::ToggleMaximize => {
                self.toggle_maximize_panel(super::FocusedPanel::Workbench, window, cx);
            }
        }
    }

    /// Answer what an Issues view asked for.
    fn on_issues_event(&mut self, event: IssuesEvent, window: &mut Window, cx: &mut Context<Self>) {
        use IssuesEvent as E;
        match event {
            E::WorkIssueHere {
                root,
                number,
                prompt,
            } => self.work_issue_here(&root, number, &prompt, window, cx),
            E::OpenConversation(session) => self.open_conversation(&session, window, cx),
            E::RunIssueWorkflow { root, number } => {
                if let Some(idx) = self.root_index(&root) {
                    self.begin_pick(idx, Some(number), window, cx);
                }
            }
            E::OpenInIssues { root, number } => self.open_issue_on_page(&root, number, window, cx),
            E::ReviewInIssues { root, number } => {
                self.open_issue_on_page(&root, number, window, cx);
                self.issues_page.review(&root, number, cx);
            }
            // Deferred: the run reaches into its session, and the answer
            // reaches back into this page.
            E::ApproveTask(approval) => {
                cx.defer(move |cx| crate::task::approve(approval, cx));
            }
            E::ReviseTask { approval, note } => {
                cx.defer(move |cx| crate::task::revise(approval, note, cx));
            }
            E::OpenTask(id) => self.show_task(&id, window, cx),
            E::OpenTaskSession(id) => {
                if let Some((uid, at)) = crate::task::session_of(&id, cx) {
                    self.show_session_in(uid, at, window, cx);
                }
            }
            E::ResumeTask(id) => self.resume_task(id, window, cx),
            E::RetryTask(id) => self.begin_retry(id, window, cx),
            E::RetryTaskCurrent(id) => self.begin_retry_current(id, window, cx),
            E::AnswerReview(id) => self.answer_review(id, window, cx),
            E::RemoveWorktree(id) => self.remove_worktree(id, window, cx),
            E::StopTask(id) => {
                cx.defer(move |cx| crate::task::stop_task(&id, cx));
            }
        }
    }

    /// Show the Issues page, as it was left. Left the way the other pages
    /// are.
    pub fn show_issues(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tell_issues_page_projects(cx);
        self.docks_aside(window, cx);
        let view = self.issues_page.view();
        self.chat
            .update(cx, |pane, cx| pane.show_issues(view, window, cx));
        self.issues_page.handle(&Request::Shown, cx);
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Show the Workflows page, as it was left, the docks put away as for
    /// the other pages.
    pub fn show_workflows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tell_workflows_page_projects(cx);
        self.docks_aside(window, cx);
        let view: gpui::AnyView = self.workflows_page.clone().into();
        self.chat
            .update(cx, |pane, cx| pane.show_workflows(view, window, cx));
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Show the Schedules page, narrowed to project `only` when one is
    /// named, else as it was left, the docks put away as for the other pages.
    pub fn show_schedules(
        &mut self,
        only: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects = self
            .page_projects()
            .into_iter()
            .map(|project| (project.root, SharedString::from(project.label.to_string())))
            .collect();
        self.schedules_page
            .update(cx, |page, cx| page.set_projects(projects, only, cx));
        self.docks_aside(window, cx);
        let view: gpui::AnyView = self.schedules_page.clone().into();
        self.chat
            .update(cx, |pane, cx| pane.show_schedules(view, window, cx));
        // The page holds the keys while it shows, as Tasks does.
        self.schedules_page.focus_handle(cx).focus(window, cx);
        self.sync_agent_started(cx);
        cx.notify();
    }

    /// Whether the Schedules page is what the centre of the window shows.
    pub fn schedules_shown(&self, cx: &gpui::App) -> bool {
        self.chat.read(cx).showing_schedules()
    }

    /// Whether the Workflows page is what the centre of the window shows.
    pub fn workflows_shown(&self, cx: &gpui::App) -> bool {
        self.chat.read(cx).showing_workflows()
    }

    /// Tell the Workflows page which projects a run may start on, and which
    /// one the rail has selected.
    pub(super) fn tell_workflows_page_projects(&mut self, cx: &mut Context<Self>) {
        let projects = self
            .page_projects()
            .into_iter()
            .map(|project| (project.root, SharedString::from(project.label.to_string())))
            .collect();
        let selected = self
            .window
            .workspace
            .active_root()
            .map(|root| root.path.clone());
        self.workflows_page
            .update(cx, |page, cx| page.set_projects(projects, selected, cx));
    }

    /// Show issue `number` of `root` on the Issues page, its filters as they
    /// are.
    pub fn open_issue_on_page(
        &mut self,
        root: &Path,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_issues(window, cx);
        self.issues_page.open(root, number, cx);
    }

    /// Whether the Issues page is what the centre of the window shows.
    pub fn issues_shown(&self, cx: &gpui::App) -> bool {
        self.chat.read(cx).showing_issues()
    }

    /// Tell the page which projects the workspace has, in rail order.
    pub(super) fn tell_issues_page_projects(&mut self, cx: &mut Context<Self>) {
        let projects = self
            .page_projects()
            .into_iter()
            .map(|project| (project.root, SharedString::from(project.label.to_string())))
            .collect();
        self.issues_page.set_projects(projects, cx);
    }

    /// Tell the page what the Workbench's Issues mode is told.
    pub(super) fn tell_issues_page(&mut self, request: &Request<'_>, cx: &mut Context<Self>) {
        self.issues_page.handle(request, cx);
    }

    /// Open a file an issue on the page names: its project on screen, and the
    /// file in the Workbench.
    fn open_page_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self
            .window
            .workspace
            .roots
            .iter()
            .position(|root| path.starts_with(&root.path))
        else {
            return;
        };
        self.select_root(idx, window, cx);
        self.workbench
            .update(cx, |panel, cx| panel.open_file(path, window, cx));
        self.dock.update(cx, |dock, cx| {
            if !dock.is_dock_open(DockPlacement::Right) {
                dock.toggle_dock(DockPlacement::Right, window, cx);
            }
        });
    }
}
