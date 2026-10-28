//! Which page, if any, the pane shows in place of a conversation, and the
//! frame it draws around one: every page is a view of its own, held by the
//! window, which the pane mounts below its header.

use super::{ChatPane, Page};
use gpui::{Context, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::StyledExt;

impl ChatPane {
    /// Whether the workspace page is what the pane shows.
    pub fn showing_workspace(&self) -> bool {
        match self.page {
            Some(Page::Workspace(_)) => true,
            Some(
                Page::Tasks(_)
                | Page::Issues(_)
                | Page::Workflows(_)
                | Page::Schedules(_)
                | Page::Settings(_),
            )
            | None => false,
        }
    }

    /// Whether the Tasks page is what the pane shows.
    pub fn showing_tasks(&self) -> bool {
        match self.page {
            Some(Page::Tasks(_)) => true,
            Some(
                Page::Workspace(_)
                | Page::Issues(_)
                | Page::Workflows(_)
                | Page::Schedules(_)
                | Page::Settings(_),
            )
            | None => false,
        }
    }

    /// Whether the Issues page is what the pane shows.
    pub fn showing_issues(&self) -> bool {
        match self.page {
            Some(Page::Issues(_)) => true,
            Some(
                Page::Workspace(_)
                | Page::Tasks(_)
                | Page::Workflows(_)
                | Page::Schedules(_)
                | Page::Settings(_),
            )
            | None => false,
        }
    }

    /// Whether the Workflows page is what the pane shows.
    pub fn showing_workflows(&self) -> bool {
        match self.page {
            Some(Page::Workflows(_)) => true,
            Some(
                Page::Workspace(_)
                | Page::Tasks(_)
                | Page::Issues(_)
                | Page::Schedules(_)
                | Page::Settings(_),
            )
            | None => false,
        }
    }

    /// Whether the Schedules page is what the pane shows.
    pub fn showing_schedules(&self) -> bool {
        match self.page {
            Some(Page::Schedules(_)) => true,
            Some(
                Page::Workspace(_)
                | Page::Tasks(_)
                | Page::Issues(_)
                | Page::Workflows(_)
                | Page::Settings(_),
            )
            | None => false,
        }
    }

    /// Show the Schedules page, `view`, as it was left, the way the
    /// Workflows page is shown.
    pub fn show_schedules(
        &mut self,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_issues_page(window, cx);
        self.page = Some(Page::Schedules(view));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    /// Whether Settings is what the pane shows.
    pub fn showing_settings(&self) -> bool {
        match self.page {
            Some(Page::Settings(_)) => true,
            Some(
                Page::Workspace(_)
                | Page::Tasks(_)
                | Page::Issues(_)
                | Page::Workflows(_)
                | Page::Schedules(_),
            )
            | None => false,
        }
    }

    /// Show Settings, `view`, on the page it was left on, its drafts as they
    /// were left, the way the other pages that are views of their own are
    /// shown.
    pub fn show_settings(
        &mut self,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Off another view page, a field there may hold the caret.
        self.leave_issues_page(window, cx);
        self.page = Some(Page::Settings(view));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    /// Show the workspace page, `view`. Leaves the shown session the way
    /// standing on a project does — the draft put down, the session left
    /// running — since the page is somewhere else to be and not a session
    /// closing.
    pub fn show_workspace(
        &mut self,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Off another view page, a field there may hold the caret.
        self.leave_issues_page(window, cx);
        self.page = Some(Page::Workspace(view));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    /// The ids of the conversations open in a session here, which the
    /// workspace page leaves out of those it offers to resume.
    pub fn open_conversations(&self, cx: &gpui::App) -> std::collections::HashSet<String> {
        self.conversations
            .values()
            .filter_map(|conv| conv.session()?.read(cx).chat.session_id.clone())
            .collect()
    }

    /// Show the Tasks page, `view`, the way the Workflows page is shown.
    pub fn show_tasks(&mut self, view: gpui::AnyView, window: &mut Window, cx: &mut Context<Self>) {
        // Off another view page, a field there may hold the caret.
        self.leave_issues_page(window, cx);
        self.page = Some(Page::Tasks(view));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    /// Show the Workflows page, `view`, as it was left, the way the Issues
    /// page is shown.
    pub fn show_workflows(
        &mut self,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Off another view page, a field there may hold the caret.
        self.leave_issues_page(window, cx);
        self.page = Some(Page::Workflows(view));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    /// Show the Issues page, `view`, as it was left. Leaves the shown session
    /// the way the other pages do.
    pub fn show_issues(
        &mut self,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Off another view page, a field there may hold the caret.
        self.leave_issues_page(window, cx);
        self.page = Some(Page::Issues(view));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    /// A page, every one a view of its own, under the pane's header, which
    /// names it as it names every page.
    pub(super) fn view_page(
        &self,
        view: gpui::AnyView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // The Workflows page splits the pane from its top edge to its bottom,
        // so its header floats over the page rather than standing above it:
        // the strip draws no fill, and the page keeps its own content clear
        // of it.
        if self.showing_workflows() {
            return div()
                .relative()
                .size_full()
                .child(div().size_full().v_flex().child(view))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .child(self.header(cx)),
                )
                .into_any_element();
        }
        div()
            .size_full()
            .v_flex()
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().v_flex().child(view))
            .into_any_element()
    }

    /// Take the caret back before a page that is a view of its own leaves
    /// the frame: a field on it may hold it, and a caret in nothing answers
    /// no key.
    pub fn leave_issues_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.showing_issues()
            || self.showing_workflows()
            || self.showing_settings()
            || self.showing_tasks()
            || self.showing_schedules()
        {
            self.focus_handle.clone().focus(window, cx);
        }
    }

    /// Whether a page about more than one project is what the pane shows.
    pub fn showing_page(&self) -> bool {
        self.page.is_some()
    }

    /// The session whose conversation the pane shows: none while a page
    /// covers it, even though the project still has an active session.
    pub fn viewing(&self) -> Option<u64> {
        self.active.filter(|_| self.page.is_none())
    }
}
