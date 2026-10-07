//! The Issues page as the window holds it: made once per window, so its
//! filters, its selection and its scroll are still there whenever it is
//! picked again; told what the Workbench's Issues mode is told; and answered
//! the way that mode is. The answer to the Workbench's own events is here
//! too, since the page's are the same events.

use super::Shell;
use crate::workbench::{WorkbenchEvent, panel::issue_event};
use gpui::{Context, SharedString, Window};
use gpui_component::dock::DockPlacement;
use onehand_plugin_host::{Ask, Request};
use onehand_workbench_issues::Page;
use std::path::Path;
use std::rc::Rc;

impl Shell {
    /// The page, with what it asks routed to this window's shell.
    pub(super) fn new_issues_page(cx: &mut Context<Self>) -> Page {
        let shell = cx.weak_entity();
        let ask: Ask = Rc::new(move |request, window, cx| {
            let _ = shell.update(cx, |shell: &mut Self, cx| match request {
                Request::OpenFile(path) => shell.open_page_file(path, window, cx),
                other => {
                    if let Some(event) = issue_event(other) {
                        shell.on_workbench_event(&event, window, cx)
                    }
                }
            });
        });
        Page::new(crate::plugins::connectors(), ask, cx)
    }

    /// Answer what an Issues view or the Workbench asked for.
    pub(super) fn on_workbench_event(
        &mut self,
        event: &WorkbenchEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use WorkbenchEvent as E;
        match event {
            E::Hide => self.hide_workbench(window, cx),
            E::RestartAgent => self.restart_session(window, cx),
            E::WorkIssueHere {
                root,
                number,
                prompt,
            } => self.work_issue_here(root, *number, prompt, window, cx),
            E::OpenConversation(session) => self.open_conversation(session, window, cx),
            E::RunIssueWorkflow { root, number } => {
                if let Some(idx) = self.root_index(root) {
                    self.begin_pick(idx, Some(*number), window, cx);
                }
            }
            E::OpenInIssues { root, number } => self.open_issue_on_page(root, *number, window, cx),
            E::ReviewInIssues { root, number } => {
                self.open_issue_on_page(root, *number, window, cx);
                self.issues_page.review(root, *number, cx);
            }
            // Deferred: the run reaches into its session, and the answer
            // reaches back into this page.
            E::ApproveTask { task, at } => {
                let (task, at) = (task.clone(), at.clone());
                cx.defer(move |cx| crate::task::approve(&task, at, cx));
            }
            E::ReviseTask { task, at, note } => {
                let (task, at, note) = (task.clone(), at.clone(), note.clone());
                cx.defer(move |cx| crate::task::revise(&task, at, note, cx));
            }
            E::OpenTask(id) => self.show_task(id, window, cx),
            E::OpenTaskSession(id) => {
                if let Some((uid, at)) = crate::task::session_of(id, cx) {
                    self.show_session_in(uid, at, window, cx);
                }
            }
            E::ResumeTask(id) => self.resume_task(id.clone(), window, cx),
            E::RetryTask(id) => self.begin_retry(id.clone(), window, cx),
            E::StopTask(id) => {
                let id = id.clone();
                cx.defer(move |cx| crate::task::stop_task(&id, cx));
            }
            E::ToggleMaximize => {
                self.toggle_maximize_panel(super::FocusedPanel::Workbench, window, cx);
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
            if !dock.is_dock_open(DockPlacement::Right, cx) {
                dock.toggle_dock(DockPlacement::Right, window, cx);
            }
        });
    }
}
