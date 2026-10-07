//! Issues mode: the project's own issues, listed on the left, the one being
//! read or written on the right.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity, SharedString};
use onehand_core::connector::Connector;
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::{Path, PathBuf};

mod view;
use view::IssuesView;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(PluginId::new("workbench.issues"), "Issues");

/// The Issues mode: a view and nothing else.
pub struct Mode {
    view: Entity<IssuesView>,
}

impl Mode {
    pub fn new(connectors: &'static [&'static dyn Connector], ask: Ask, cx: &mut App) -> Self {
        Self {
            view: IssuesView::new(connectors, ask, false, cx),
        }
    }
}

impl WorkbenchMode for Mode {
    fn spec(&self) -> WorkbenchModeSpec {
        SPEC
    }

    fn view(&self) -> AnyView {
        self.view.clone().into()
    }

    fn set_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.set_root(root, cx));
    }

    fn forget_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.forget_root(root, cx));
    }

    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        handle(&self.view, request, cx)
    }
}

/// The Issues page: the issues of every project of the workspace in one
/// list, and the one picked beside it in full. The same view as the mode,
/// on the same files, so an edit in one is seen in the other at once.
pub struct Page {
    view: Entity<IssuesView>,
}

impl Page {
    pub fn new(connectors: &'static [&'static dyn Connector], ask: Ask, cx: &mut App) -> Self {
        Self {
            view: IssuesView::new(connectors, ask, true, cx),
        }
    }

    pub fn view(&self) -> AnyView {
        self.view.clone().into()
    }

    /// What the mode is told, told to the page too.
    pub fn handle(&self, request: &Request<'_>, cx: &mut App) -> bool {
        handle(&self.view, request, cx)
    }

    /// The workspace's projects, in rail order, with what each is called.
    pub fn set_projects(&self, projects: Vec<(PathBuf, SharedString)>, cx: &mut App) {
        self.view
            .update(cx, |view, cx| view.set_projects(projects, cx));
    }

    /// Open issue `number` of `root`, its filters as they are.
    pub fn open(&self, root: &Path, number: u64, cx: &mut App) {
        self.view
            .update(cx, |view, cx| view.open_on_page(root, number, cx));
    }

    /// Open the review of issue `number` of `root`, on screen: what its work
    /// waits for approval on.
    pub fn review(&self, root: &Path, number: u64, cx: &mut App) {
        self.view
            .update(cx, |view, cx| view.review_on_page(root, number, cx));
    }

    pub fn forget_root(&self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.forget_root(root, cx));
    }
}

/// What a view of the issues does with a request, the mode's and the page's
/// alike.
fn handle(view: &Entity<IssuesView>, request: &Request<'_>, cx: &mut App) -> bool {
    match request {
        Request::SetStorage(storage) => {
            view.update(cx, |view, cx| {
                view.set_storage(storage.map(Path::to_path_buf), cx)
            });
            true
        }
        // Something other than this view may have written the file — a
        // run leaving a note — so it is read again the next time it is
        // drawn. One small file, so there is nothing to save by waiting
        // longer than that.
        Request::Rescan => {
            view.update(cx, |view, cx| view.mark_stale(cx));
            true
        }
        // Being put on screen reads what the view draws again, and on the
        // page its pull requests too.
        Request::Shown => {
            view.update(cx, |view, cx| view.page_shown(cx));
            true
        }
        Request::LiveConversations(ids) => {
            view.update(cx, |view, cx| view.set_live(ids, cx));
            true
        }
        Request::IssueWork { works, offered } => {
            view.update(cx, |view, cx| view.set_works(works, offered, cx));
            true
        }
        Request::ShowIssue(number) => {
            view.update(cx, |view, cx| view.show_issue(*number, cx));
            true
        }
        // Not an Issues view's to answer: a file to edit, a PTY, the agent,
        // and what a view asks upward rather than is told.
        Request::OpenFile(_)
        | Request::Save
        | Request::Start
        | Request::Reap
        | Request::SetGit(_)
        | Request::SetFontSize(_)
        | Request::AgentStarted(_)
        | Request::RestartAgent
        | Request::WorkIssueHere { .. }
        | Request::OpenConversation(_)
        | Request::RunIssueWorkflow { .. }
        | Request::OpenInIssues { .. }
        | Request::ReviewInIssues { .. }
        | Request::ApproveTask { .. }
        | Request::ReviseTask { .. }
        | Request::OpenTask(_)
        | Request::OpenTaskSession(_)
        | Request::ResumeTask(_)
        | Request::RetryTask(_)
        | Request::StopTask(_) => false,
    }
}
