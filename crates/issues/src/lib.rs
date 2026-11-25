//! Issues mode: the project's own issues, listed on the left, the one being
//! read or written on the right.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity, SharedString, Window};
use onehand_core::connector::Connector;
use onehand_workbench::{Asker, ModeId, Request, WorkbenchMode, WorkbenchModeSpec};
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod view;
use view::IssuesView;
#[cfg(feature = "test-support")]
pub use view::testing;

// The issue list's keys, which the app's keymap binds in the `Issues`
// context the page and the Workbench tab both take: the row above, the row
// below, and the innermost layer open over the list closed.
gpui::actions!(issues_page, [IssuesUp, IssuesDown, IssuesClose]);

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(ModeId::new("workbench.issues"), "Issues");

/// What an Issues view asks of the shell, the Workbench's Issues mode and
/// the Issues page alike: a session or a conversation put on screen, or the
/// Issues page. Issues are the app's own, so these go to the
/// shell directly rather than through what every Workbench mode may answer.
pub enum IssuesEvent {
    /// Work issue `number` of project `root` in the checkout it is open on,
    /// with `prompt` as the new session's first message.
    WorkIssueHere {
        root: PathBuf,
        number: u64,
        prompt: String,
    },
    /// Show the conversation the agent named `session`, live or saved.
    OpenConversation(String),
    /// Show issue `number` of project `root` on the Issues page.
    OpenInIssues { root: PathBuf, number: u64 },
}

/// How an Issues view reaches the shell. Handed to the view when it is
/// built, as its [`Asker`] is, so it is never on screen with nobody to tell.
pub type Tell = Rc<dyn Fn(IssuesEvent, &mut Window, &mut App)>;

/// The Issues mode: a view and nothing else.
pub struct Mode {
    view: Entity<IssuesView>,
}

impl Mode {
    pub fn new(
        connectors: &'static [&'static dyn Connector],
        ask: Asker,
        tell: Tell,
        cx: &mut App,
    ) -> Self {
        Self {
            view: IssuesView::new(connectors, ask, tell, false, cx),
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

    fn set_root(&mut self, root: &Path, _disk: &onehand_workbench::Disk, cx: &mut App) {
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
    pub fn new(
        connectors: &'static [&'static dyn Connector],
        ask: Asker,
        tell: Tell,
        cx: &mut App,
    ) -> Self {
        Self {
            view: IssuesView::new(connectors, ask, tell, true, cx),
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

    pub fn forget_root(&self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.forget_root(root, cx));
    }
}

/// Write `change` into the issues file `file` of the project at `root`, made
/// outside every view of it (a tool, a session's note): through the
/// one drive every view of the file shares, so each shows it as it lands, in
/// order with the views' own changes. `answer` hears what it came to, once,
/// after the update that landed it, whether or not a view holds the file.
pub fn write(
    root: &Path,
    file: PathBuf,
    change: onehand_core::issues::drive::Change,
    connectors: &'static [&'static dyn Connector],
    answer: impl FnOnce(Result<onehand_core::issues::drive::Applied, String>, &mut App) + 'static,
    cx: &mut App,
) {
    view::write_outside(root, file, change, connectors, Box::new(answer), cx);
}

/// One project's issues as every view of them holds them, for a page that
/// reads them with no view of its own (a board's Backlog): read on first
/// hold, and woken by each change that lands, a tool's `create_issue`
/// included, so it never reads the file again to stay in step.
pub struct Held(Entity<view::IssuesFile>);

impl Held {
    /// The issues file `file` of the project at `root`.
    pub fn new(
        root: &Path,
        file: PathBuf,
        connectors: &'static [&'static dyn Connector],
        cx: &mut App,
    ) -> Self {
        Self(view::held(root, file, connectors, cx))
    }

    /// What the file holds as last read: none until the first read lands.
    pub fn kept<'a>(&self, cx: &'a App) -> &'a onehand_core::issues::drive::Kept {
        self.0.read(cx)
    }

    /// Call `on` on the observer each time the file changes.
    pub fn observe<V: 'static>(
        &self,
        on: impl Fn(&mut V, &mut gpui::Context<V>) + 'static,
        cx: &mut gpui::Context<V>,
    ) -> gpui::Subscription {
        cx.observe(&self.0, move |this, _, cx| on(this, cx))
    }
}

/// What a view of the issues does with a request, the mode's and the page's
/// alike.
fn handle(view: &Entity<IssuesView>, request: &Request<'_>, cx: &mut App) -> bool {
    match request {
        Request::IssueFiles(files) => {
            let files = (*files).clone();
            view.update(cx, |view, cx| view.set_files(files, cx));
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
        // Being put on screen reads what the view draws again.
        Request::Shown => {
            view.update(cx, |view, cx| view.page_shown(cx));
            true
        }
        Request::LiveConversations(ids) => {
            view.update(cx, |view, cx| view.set_live(ids, cx));
            true
        }
        Request::ShowIssue(number) => {
            view.update(cx, |view, cx| view.show_issue(*number, cx));
            true
        }
        // Not an Issues view's to answer: a file to save, a PTY, the agent,
        // and the tree's and the editor's facts.
        Request::Save
        | Request::Start
        | Request::Reap
        | Request::SetGit(_)
        | Request::SetFontSize(_)
        | Request::AgentStarted(_)
        | Request::Locked(_)
        | Request::Offloaded(_) => false,
    }
}
