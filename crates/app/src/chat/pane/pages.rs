//! Which page, if any, the pane shows in place of a conversation, and the
//! frame it draws around one: every page is a view of its own, held by the
//! window, which the pane mounts below its header.

use super::{ChatPane, Page};
use gpui::{Context, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::StyledExt;

/// Which page the pane shows in place of a conversation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// The overview: what waits, what works, every project and its issues.
    Workspace,
    Tasks,
    /// The Issues crate's page.
    Issues,
    Workflows,
    Schedules,
    Settings,
}

impl ChatPane {
    /// Which page the pane shows, or `None` for a conversation.
    pub fn shown_page(&self) -> Option<PageKind> {
        self.page.as_ref().map(|page| page.kind)
    }

    /// Show page `kind`, `view`, as it was left. Leaves the shown session the
    /// way standing on a project does — the draft put down, the session left
    /// running — since the page is somewhere else to be and not a session
    /// closing.
    pub fn show_page(
        &mut self,
        kind: PageKind,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Off another page, a field there may hold the caret.
        self.leave_page(window, cx);
        self.page = Some(Page { kind, view });
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
        if self.shown_page() == Some(PageKind::Workflows) {
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

    /// Take the caret back before a page leaves the frame: a field on it may
    /// hold it, and a caret in nothing answers no key.
    pub fn leave_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let holds_fields = match self.shown_page() {
            Some(
                PageKind::Tasks
                | PageKind::Issues
                | PageKind::Workflows
                | PageKind::Schedules
                | PageKind::Settings,
            ) => true,
            // The overview holds no field, and a conversation keeps its own.
            Some(PageKind::Workspace) | None => false,
        };
        if holds_fields {
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
