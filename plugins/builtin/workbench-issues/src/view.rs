//! The Issues mode's own state — which project, what it has kept, what is
//! being read or written — and how it is drawn.
//!
//! The issues themselves are core's (`onehand_core::issues`): the file, the
//! numbering, the rules a draft has to meet, and keeping them in step with a
//! forge. What is here is the half that needs a window: the list, the reading,
//! the form, and when a sync runs.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, Task, Window, div,
};
use gpui_component::WindowExt as _;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::InputState;
use gpui_component::text::TextViewState;
use gpui_component::{StyledExt, h_resizable, resizable_panel};
use onehand_core::connector::{Connector, PullRequest};
use onehand_core::issues::{self, IssueKey, Issues, LocalIssue};
use onehand_core::task::work::{IssueWork, Reading};
use onehand_plugin_host::{Ask, Request, hint, status_line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod detail;
mod file;
mod form;
mod full;
mod list;
mod mentions;
mod page;
mod reads;
mod review;
mod store;
mod work;
use detail::{Doing, issue_view};
use form::{Form, form_view};
use list::Showing;
use store::IssuesFile;

/// How often a project kept in step with its forge is synced while it is the
/// one this mode is on, if nothing else has synced it sooner.
const SYNC_EVERY: Duration = Duration::from_secs(300);

/// The least time between two syncs that nothing asked for. Being shown, and
/// reading the file again, are frequent; the forge does not need to hear about
/// each of them.
const SYNC_GAP: Duration = Duration::from_secs(60);

/// How often the view is drawn again with nothing having changed, so the
/// footer's "synced 3m ago" keeps telling the time. A multiple of it is
/// [`SYNC_EVERY`].
const TICK: Duration = Duration::from_secs(60);

/// The list's width before anybody drags it, and the range a drag may take it
/// through — pixels, because that is the only thing the split accepts. A
/// row's title wraps to two lines, so the floor is what the search box and the
/// filters above the rows need.
const LIST_W: f32 = 240.;
const LIST_MIN: f32 = 160.;
const LIST_MAX: f32 = 420.;

/// How many rows the list draws. A project with more than this many issues is
/// said to have them, and the rest are not built: every row is an element, and
/// a list nobody scrolls to the end of is not worth building to the end.
const LIST_CAP: usize = 500;

/// How many of the files a body names are listed under it. A body that names
/// more is a list of files, and the list says how many it left out.
const FILES_SHOWN: usize = 20;

/// How many entries of an issue's history are drawn under it: the latest ones,
/// since the last thing that happened to it is the one read first.
const HISTORY_SHOWN: usize = 50;

/// How many of an issue's earlier tasks are drawn under it, newest first;
/// the rest are on the Tasks page, and the list says so.
const EARLIER_SHOWN: usize = 5;

/// How old what is read of an issue's pull request may be before the window
/// coming back to the front reads it again.
const READ_AGE: Duration = Duration::from_secs(60);

pub(crate) struct IssuesView {
    root: Option<PathBuf>,
    /// The workspace's storage directory. `None` is a workspace bound to
    /// nothing, which keeps no issues — said on screen rather than offering a
    /// form whose work would be thrown away.
    storage: Option<PathBuf>,
    /// Each project's issues file, and what is open in it.
    roots: HashMap<PathBuf, RootIssues>,
    split: Entity<gpui_component::ResizableState>,
    /// A read or a write that could not be done, as a standing line under the
    /// body. Cleared by the next one that works.
    status: Option<String>,
    /// The connectors a project may be kept in step with, in the order one is
    /// offered them.
    connectors: &'static [&'static dyn Connector],
    /// The timer behind the periodic sync, held for as long as the view.
    _sync_every: Task<()>,
    /// The list's search box, made on its first draw.
    query: Option<Entity<InputState>>,
    /// Which half of the issues the list shows.
    showing: Showing,
    /// The one label the list is narrowed to, if any. Dropped on a switch of
    /// project, whose issues carry labels of their own.
    label: Option<String>,
    /// How the mode asks the Workbench for something — a file opened in the
    /// editor.
    ask: Ask,
    /// The conversations with a live session in this window, by the agent's
    /// session id: an issue whose history names one of them is being worked.
    live: Vec<String>,
    /// The work on every issue this window's projects keep that a task
    /// works, as the app last told it.
    works: Vec<IssueWork>,
    /// The pull request of the work of the issue on screen, as last read.
    pr: Reading<reads::PrAbout, Option<PullRequest>>,
    /// The work moved since the pull request was last read.
    moved: bool,
    /// The window came back to the front since the issue was last drawn.
    returned: bool,
    /// The issue whose work was last drawn, so opening one reads it.
    shown: Option<IssueKey>,
    /// Watches the window coming back to the front, made on the first draw.
    _activation: Option<Subscription>,
    /// The projects a run may be started on.
    offered: Vec<PathBuf>,
    /// What the Issues page holds beside what the tab does: `None` for the
    /// Workbench's Issues mode, which draws one project at the dock's width.
    page: Option<page::PageState>,
}

/// One project's issues file, shared with every other view of it, and what
/// this view has open in it.
struct RootIssues {
    file: Entity<IssuesFile>,
    /// Draws this view again whenever the file moves.
    _watch: Subscription,
    selected: Option<u64>,
    /// A new issue or an edit being written: kept across a switch of project
    /// or of page, and asked about before it is dropped.
    form: Option<Form>,
    /// The selected issue's body, parsed — so it is parsed again when the
    /// issue changes and never on a frame when it has not.
    body: Option<Body>,
}

/// An issue's body as drawn: parsed, and the project's files it names.
struct Body {
    /// The issue and the stamp it was parsed at.
    number: u64,
    updated: u64,
    parsed: Entity<TextViewState>,
    /// The files it names that exist, in the order it names them. Empty
    /// until the check, which reads the disk, has come back.
    files: Vec<String>,
}

impl IssuesView {
    /// The view the Workbench's Issues mode draws, or with `page` the one
    /// the Issues page does, across every project of the workspace.
    pub(crate) fn new(
        connectors: &'static [&'static dyn Connector],
        ask: Ask,
        page: bool,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| Self {
            page: page.then(page::PageState::default),
            root: None,
            storage: None,
            roots: HashMap::new(),
            split: cx.new(|_| gpui_component::ResizableState::default()),
            status: None,
            connectors,
            query: None,
            showing: Showing::default(),
            label: None,
            ask,
            live: Vec::new(),
            works: Vec::new(),
            pr: Reading::default(),
            moved: false,
            returned: false,
            shown: None,
            _activation: None,
            offered: Vec::new(),
            _sync_every: cx.spawn(async move |view, cx| {
                let every = (SYNC_EVERY.as_secs() / TICK.as_secs()).max(1);
                for tick in 1u64.. {
                    cx.background_executor().timer(TICK).await;
                    let alive = view.update(cx, |view: &mut Self, cx| {
                        cx.notify();
                        if tick % every == 0 {
                            view.sync(false, cx);
                        }
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            }),
        })
    }

    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        self.label = None;
        // Arriving reads the project's file again: a run may have written it
        // while another project was on screen.
        if let Some(state) = self.roots.get(root) {
            state.file.update(cx, |file, _| file.mark_stale());
        }
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.roots.remove(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    /// Point at another storage directory. Everything read from the old one
    /// is dropped: it was another workspace's file, or none.
    pub(crate) fn set_storage(&mut self, storage: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.storage == storage {
            return;
        }
        self.storage = storage;
        self.roots.clear();
        cx.notify();
    }

    pub(crate) fn set_works(
        &mut self,
        works: &[IssueWork],
        offered: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        // Only the issue on screen moving reads its pull request again.
        let shown = |all: &[IssueWork]| {
            let root = self.root.as_deref()?;
            let number = self.roots.get(root)?.selected?;
            let key = self.key(root, number)?;
            all.iter().find(|work| work.key == key).cloned()
        };
        if shown(&self.works) != shown(works) {
            self.moved = true;
        }
        self.works = works.to_vec();
        self.offered = offered.to_vec();
        cx.notify();
    }

    pub(crate) fn set_live(&mut self, ids: &[String], cx: &mut Context<Self>) {
        self.live = ids.to_vec();
        cx.notify();
    }

    /// Read every file this view holds again the next time it draws it.
    pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
        for state in self.roots.values() {
            state.file.update(cx, |file, _| file.mark_stale());
        }
        cx.notify();
    }

    /// `root`'s entry, made on first use with the file every view of it
    /// shares. `None` for a workspace that keeps nothing.
    fn state_for(&mut self, root: &Path, cx: &mut Context<Self>) -> Option<&mut RootIssues> {
        if !self.roots.contains_key(root) {
            let path = issues::file_for(self.storage.as_deref()?, root);
            let file = IssuesFile::get(root, path, self.connectors, cx);
            let watch = cx.observe(&file, |_, _, cx| cx.notify());
            self.roots.insert(
                root.to_path_buf(),
                RootIssues {
                    file,
                    _watch: watch,
                    selected: None,
                    form: None,
                    body: None,
                },
            );
        }
        self.roots.get_mut(root)
    }

    /// The issues of `root` as last read, if they have been.
    fn issues_of<'a>(&'a self, root: &Path, cx: &'a App) -> Option<&'a Issues> {
        self.roots.get(root)?.file.read(cx).issues.as_ref()
    }

    /// The forge serving the active project, if one does.
    fn forge(&self, cx: &App) -> Option<&'static dyn Connector> {
        let root = self.root.as_deref()?;
        self.roots.get(root)?.file.read(cx).forge
    }

    /// The active project's file, if this workspace keeps one.
    fn file(&self) -> Option<(PathBuf, PathBuf)> {
        let root = self.root.clone()?;
        let file = issues::file_for(self.storage.as_deref()?, &root);
        Some((root, file))
    }

    fn state_mut(&mut self) -> Option<&mut RootIssues> {
        let root = self.root.clone()?;
        self.roots.get_mut(&root)
    }

    /// Pick issue `number` of the active project. A draft with changes in it
    /// is asked about before it is dropped; one with none goes quietly.
    fn select(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.unless_drafting(window, cx, move |view, _, cx| {
            if let Some(state) = view.state_mut() {
                state.selected = Some(number);
                state.form = None;
            }
            cx.notify();
        });
    }

    /// Do `then`, unless a draft with changes in it is open in the active
    /// project: then ask, in a modal, whether to drop it first.
    fn unless_drafting(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let changed = self
            .state_mut()
            .and_then(|state| state.form.as_ref())
            .is_some_and(|form| form.changed(cx));
        if !changed {
            then(self, window, cx);
            return;
        }
        let view = cx.entity();
        let then = std::rc::Rc::new(then);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (view, then) = (view.clone(), then.clone());
            alert
                .title("Drop this draft?")
                .description("What is written in the issue form has not been saved.")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Drop draft")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    view.update(cx, |view, cx| {
                        if let Some(state) = view.state_mut() {
                            state.form = None;
                        }
                        then(view, window, cx)
                    });
                    true
                })
        });
    }

    /// Select issue `number` of the active project, asked from outside the
    /// mode. The project's entry is made here if its first read has not
    /// started yet, and the read only fills the file in, so a selection made
    /// before the issues arrive is the one drawn once they do. A draft is
    /// kept: the form stays in front until it is saved or cancelled.
    pub(crate) fn show_issue(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if let Some(state) = self.state_for(&root, cx) {
            state.selected = Some(number);
        }
        cx.notify();
    }

    /// Ask before closing issue `number`: closing is the one way an issue
    /// leaves the work, and on a project kept in step it closes on the forge
    /// too. Reopening is not asked about — it takes nothing away.
    fn confirm_close(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(issue) = self.issues_of(&root, cx).and_then(|kept| kept.get(number)) else {
            return;
        };
        let mut description = match &issue.link {
            Some(link) => format!(
                "\u{201c}{}\u{201d} moves to the closed list here and closes on {} at the next sync. It can be reopened.",
                issue.title, link.connector
            ),
            None => format!(
                "\u{201c}{}\u{201d} moves to the closed list. It can be reopened.",
                issue.title
            ),
        };
        if self.work_active(number) {
            description.push_str(" A run is still working on it; closing does not stop it.");
        }
        // The project is held so a dialog left open across a switch closes the
        // issue it was opened for, never the same number in another project.
        let root = self.root.clone();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (root, view) = (root.clone(), view.clone());
            alert
                .title("Close this issue?")
                .description(description.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Close issue")
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    view.update(cx, |view, cx| {
                        if view.root == root {
                            view.set_open(number, false, cx);
                        }
                    });
                    true
                })
        });
    }

    /// Find issue `number`'s address on its forge, then hand it to `then` —
    /// opening it, copying it. Asked of the forge each time rather than built
    /// here, since the forge is the one that knows where a moved repository
    /// went.
    fn with_url(
        &mut self,
        number: u64,
        then: impl FnOnce(String, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(link) = self
            .issues_of(&root, cx)
            .and_then(|kept| kept.get(number))
            .and_then(|issue| issue.link.clone())
        else {
            return;
        };
        let Some(forge) = self
            .forge(cx)
            .filter(|forge| forge.name() == link.connector)
        else {
            self.status = Some(format!(
                "{} {} cannot be reached from this project",
                link.connector, link.reference
            ));
            cx.notify();
            return;
        };
        cx.spawn(async move |view, cx| {
            let url = cx
                .background_executor()
                .spawn(async move { forge.issue_url_blocking(&root, &link.key) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| match url {
                Ok(url) => then(url, cx),
                Err(why) => {
                    view.status = Some(why);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_open(&mut self, number: u64, open: bool, cx: &mut Context<Self>) {
        let now = issues::now();
        self.change(
            move |kept| kept.set_open(number, open, now).map(|()| Some(number)),
            cx,
        );
    }
}

impl Render for IssuesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(root) = self.root.clone()
            && let Some(state) = self.state_for(&root, cx)
        {
            state.file.update(cx, |file, cx| file.load_if_stale(cx));
        }
        if self._activation.is_none() {
            self._activation = Some(cx.observe_window_activation(window, |view, window, cx| {
                if window.is_window_active() {
                    view.returned = true;
                    cx.notify();
                }
            }));
        }
        let body = self.body(window, cx);
        // Coming back to the front reads only what is on screen then.
        self.returned = false;
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(self.standing(cx).map(|status| status_line(status, cx)))
    }
}

impl IssuesView {
    /// The standing line under the body: what a write or a lookup could not
    /// do, else what reading the active project's file could not.
    fn standing(&self, cx: &App) -> Option<String> {
        self.status.clone().or_else(|| {
            let root = self.root.as_deref()?;
            self.roots.get(root)?.file.read(cx).failed.clone()
        })
    }

    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.page.is_some() {
            return self.page_body(window, cx);
        }
        let Some(root) = self.root.clone() else {
            return hint("No project root", cx);
        };
        if self.storage.is_none() {
            return hint(
                "This workspace keeps nothing until it is bound to a storage folder in Settings",
                cx,
            );
        }
        let Some(issues) = self.issues_of(&root, cx).cloned() else {
            return hint("Reading issues…", cx);
        };

        let list = self.list(&issues, window, cx);
        let detail = self.detail(&root, &issues, window, cx);
        div()
            .flex_1()
            .min_h_0()
            .child(
                h_resizable("issues-split")
                    .with_state(&self.split)
                    .child(
                        // `flex_none`, as the Markdown mode's list is: a panel
                        // in the group grows by default, and a list that grows
                        // takes the room the issue was opened to be read in.
                        resizable_panel()
                            .size(gpui::px(LIST_W))
                            .size_range(gpui::px(LIST_MIN)..gpui::px(LIST_MAX))
                            .flex_none()
                            .child(list),
                    )
                    .child(resizable_panel().child(detail)),
            )
            .into_any_element()
    }

    /// The right-hand side: the form while one is open, else the selected
    /// issue, else a line saying what to do.
    fn detail(
        &mut self,
        root: &Path,
        issues: &Issues,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.roots.get(root) else {
            return hint("Reading issues…", cx);
        };
        if let Some(form) = &state.form {
            let project = self.page.as_ref().and_then(|page| page.label_of(root));
            let templates = state.file.read(cx).templates.clone();
            return form_view(form, project, &templates, cx);
        }
        let Some(issue) = state.selected.and_then(|n| issues.get(n)).cloned() else {
            return hint("Pick an issue, or start a new one", cx);
        };
        // Publishing is offered where it can land: a project kept in step with
        // a forge, on an issue not already there.
        let publish_to = state
            .file
            .read(cx)
            .forge
            .filter(|forge| issues.in_step_with(forge.name()))
            .filter(|_| issue.link.is_none())
            .map(|forge| forge.name());
        let templates = state.file.read(cx).templates.clone();
        let body = self.parsed_body(root, &issue, cx);
        let key = self.key(root, issue.number);
        let work = key
            .as_ref()
            .and_then(|key| self.works.iter().find(|work| work.key == *key))
            .cloned();
        self.read_pr_if_due(root, work.as_ref(), cx);
        self.read_left_if_due(work.as_ref(), cx);
        let review = self.review_block(&issue, key.as_ref(), work.as_ref(), cx);
        let doing = Doing {
            review,
            full: self.page.as_ref().map(page::PageState::full),
            session: working_in(&issue, &self.live).map(str::to_string),
            offered: self.offered.iter().any(|offered| offered == root),
            pr: self.pr_seen(work.as_ref()),
            last: self.pr_value(work.as_ref()),
            templates,
            work,
        };
        issue_view(root, &issue, body, publish_to, doing, window, cx)
    }

    /// Put a request to the Workbench about the project on screen, once this
    /// view is no longer being updated: the Workbench puts what it is asked to
    /// every mode, this one included, and a press inside this view lands while
    /// the view is mid-update.
    fn ask_later(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        put: impl FnOnce(&Ask, &Path, &mut Window, &mut App) + 'static,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let ask = self.ask.clone();
        window.defer(cx, move |window, cx| put(&ask, &root, window, cx));
    }

    /// Start a session on the project on screen, in the checkout it is open
    /// on, with issue `number` as its first message.
    fn work_here(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self
            .root
            .as_deref()
            .and_then(|root| self.issues_of(root, cx)?.get(number))
            .map(issues::work_here_prompt)
        else {
            return;
        };
        self.ask_later(window, cx, move |ask, root, window, cx| {
            let request = Request::WorkIssueHere {
                root,
                number,
                prompt: &prompt,
            };
            ask(&request, window, cx)
        });
    }

    /// Choose a workflow and work issue `number` of the project on screen
    /// with it, on a worktree of its own.
    fn run_workflow(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_later(window, cx, move |ask, root, window, cx| {
            ask(&Request::RunIssueWorkflow { root, number }, window, cx)
        });
    }

    /// Put issue `number` of the project on screen on the Issues page.
    fn open_in_issues(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_later(window, cx, move |ask, root, window, cx| {
            ask(&Request::OpenInIssues { root, number }, window, cx)
        });
    }

    /// Ask the app about task `id`, by `request`.
    fn ask_task(
        &mut self,
        id: String,
        request: fn(&str) -> Request<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(&request(&id), window, cx)
        });
    }

    /// Put task `id` on screen, on the Tasks page.
    fn open_task(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(&Request::OpenTask(&id), window, cx)
        });
    }

    /// Put the conversation the agent named `session` on screen.
    fn open_session(&mut self, session: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(&Request::OpenConversation(&session), window, cx)
        });
    }

    /// Open `path`, relative to the project on screen, in the editor.
    fn open_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.to_string();
        self.ask_later(window, cx, move |ask, root, window, cx| {
            ask(&Request::OpenFile(&root.join(&path)), window, cx)
        });
    }

    /// The selected issue's body as parsed markdown and the files it names,
    /// parsed again only when the issue has changed since the last time.
    ///
    /// Parsed at once as written, then again with the files it names made
    /// links once a check off the UI loop has said which of them exist.
    fn parsed_body(
        &mut self,
        root: &Path,
        issue: &LocalIssue,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<TextViewState>, Vec<String>)> {
        if issue.body.is_empty() {
            return None;
        }
        let state = self.roots.get_mut(root)?;
        if let Some(body) = &state.body
            && body.number == issue.number
            && body.updated == issue.updated
        {
            return Some((body.parsed.clone(), body.files.clone()));
        }
        let parsed = cx.new(|cx| TextViewState::markdown(&issue.body, cx));
        state.body = Some(Body {
            number: issue.number,
            updated: issue.updated,
            parsed: parsed.clone(),
            files: Vec::new(),
        });
        let (number, updated) = (issue.number, issue.updated);
        let (root, text) = (root.to_path_buf(), issue.body.clone());
        cx.spawn(async move |view, cx| {
            let (linked, files) = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move {
                        let found = mentions::mentions(&text);
                        let mut files: Vec<String> = Vec::new();
                        for mention in &found {
                            if !files.contains(&mention.path) && root.join(&mention.path).is_file()
                            {
                                files.push(mention.path.clone());
                            }
                        }
                        (mentions::linked(&text, &found, &files), files)
                    }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(body) = view
                    .roots
                    .get_mut(&root)
                    .and_then(|state| state.body.as_mut())
                else {
                    return;
                };
                if body.number != number || body.updated != updated || files.is_empty() {
                    return;
                }
                body.files = files;
                body.parsed
                    .update(cx, |parsed, cx| parsed.set_text(&linked, cx));
                cx.notify();
            });
        })
        .detach();
        Some((parsed, Vec::new()))
    }
}

/// The live session working `issue`: the latest conversation its history
/// names that is among `live`, the ones with a session in this window.
fn working_in<'a>(issue: &'a LocalIssue, live: &[String]) -> Option<&'a str> {
    issue
        .notes
        .iter()
        .rev()
        .filter_map(|note| note.session.as_deref())
        .find(|session| live.iter().any(|id| id == session))
}

#[cfg(test)]
mod tests;
