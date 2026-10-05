//! The Issues mode's own state — which project, what it has kept, what is
//! being read or written — and how it is drawn.
//!
//! The issues themselves are core's (`onehand_core::issues`): the file, the
//! numbering, the rules a draft has to meet, and keeping them in step with a
//! forge. What is here is the half that needs a window: the list, the reading,
//! the form, and when a sync runs.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Task, Window, div,
};
use gpui_component::WindowExt as _;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{InputState, TextareaState};
use gpui_component::text::TextViewState;
use gpui_component::{StyledExt, h_resizable, resizable_panel};
use onehand_core::connector::{self, Connector};
use onehand_core::issues::{self, Draft, Issues, LocalIssue, sync};
use onehand_plugin_host::{Ask, IssueRun, Request, hint, status_line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod detail;
mod list;
mod mentions;
use detail::{Doing, form_view, issue_view};
use list::Showing;

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

/// How many of the tasks working an issue are drawn under it, the working
/// ones first; the rest are on the Tasks page, and the list says so.
const RUNS_SHOWN: usize = 5;

pub(crate) struct IssuesView {
    root: Option<PathBuf>,
    /// The workspace's storage directory. `None` is a workspace bound to
    /// nothing, which keeps no issues — said on screen rather than offering a
    /// form whose work would be thrown away.
    storage: Option<PathBuf>,
    /// What each project has, as last read, and what is open in it.
    roots: HashMap<PathBuf, RootIssues>,
    /// Whether the active project's file needs reading again before it is next
    /// drawn. Read when drawn rather than when marked, so a workspace of a
    /// dozen projects does not read a dozen files for a mode nobody opened.
    stale: bool,
    split: Entity<gpui_component::ResizableState>,
    /// A read or a write that could not be done, as a standing line under the
    /// body. Cleared by the next one that works.
    status: Option<String>,
    /// The read in flight, held so that starting another drops it.
    _load: Option<Task<()>>,
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
    /// Every task working an issue this window's projects keep, as the app
    /// last told it.
    runs: Vec<IssueRun>,
    /// The projects a run may be started on.
    offered: Vec<PathBuf>,
}

/// One project's issues and what is open among them.
#[derive(Default)]
struct RootIssues {
    /// `None` until the first read lands.
    issues: Option<Issues>,
    selected: Option<u64>,
    form: Option<Form>,
    /// The selected issue's body, parsed — so it is parsed again when the
    /// issue changes and never on a frame when it has not.
    body: Option<Body>,
    /// The forge that serves this project, if one does — found when its
    /// issues are read, since asking reads the project's git remote.
    forge: Option<&'static dyn Connector>,
    /// A sync is on its way; a second is not started beside it.
    syncing: bool,
    /// Something asked for a sync while one was running — an edit saved
    /// meanwhile — so another runs as soon as it lands, rather than leaving
    /// the edit for the timer.
    again: bool,
    /// When the last sync finished, in seconds since the epoch, and what it
    /// came to: what moved, in one line, or what it could not do.
    synced: Option<(u64, Result<String, String>)>,
}

impl RootIssues {
    fn show(&mut self, number: u64) {
        self.selected = Some(number);
        self.form = None;
    }

    /// Take a read that has landed. Only the issues move: what is selected
    /// stays, so an issue asked for before the read arrived is still the one
    /// shown once it has.
    fn land(&mut self, read: Issues) {
        keep_newer(&mut self.issues, read);
    }
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

/// The form a new issue or an edit is written in.
struct Form {
    /// The issue being edited, or `None` for a new one.
    editing: Option<u64>,
    /// How the issue being edited is named on its forge; `None` for a draft
    /// or a new issue.
    editing_reference: Option<String>,
    title: Entity<InputState>,
    labels: Entity<InputState>,
    body: Entity<TextareaState>,
}

impl IssuesView {
    pub(crate) fn new(
        connectors: &'static [&'static dyn Connector],
        ask: Ask,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| Self {
            root: None,
            storage: None,
            roots: HashMap::new(),
            stale: false,
            split: cx.new(|_| gpui_component::ResizableState::default()),
            status: None,
            _load: None,
            connectors,
            query: None,
            showing: Showing::default(),
            label: None,
            ask,
            live: Vec::new(),
            runs: Vec::new(),
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
        self.stale = true;
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
        self.stale = true;
        cx.notify();
    }

    pub(crate) fn set_runs(
        &mut self,
        runs: &[IssueRun],
        offered: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        self.runs = runs.to_vec();
        self.offered = offered.to_vec();
        cx.notify();
    }

    pub(crate) fn set_live(&mut self, ids: &[String], cx: &mut Context<Self>) {
        self.live = ids.to_vec();
        cx.notify();
    }

    pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
        self.stale = true;
        cx.notify();
    }

    /// The active project's file, if this workspace keeps one.
    fn file(&self) -> Option<(PathBuf, PathBuf)> {
        let root = self.root.clone()?;
        let file = issues::file_for(self.storage.as_deref()?, &root);
        Some((root, file))
    }

    /// Read the active project's issues.
    ///
    /// The entry is made **here**, while the root is known to be in the
    /// workspace, and the read that lands later only fills it in — one made on
    /// the way back would bring back a project removed in the meantime.
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some((root, file)) = self.file() else {
            return;
        };
        self.roots.entry(root.clone()).or_default();
        let connectors = self.connectors;
        self._load = Some(cx.spawn(async move |view, cx| {
            let (read, forge) = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move {
                        let forge = connector::serving(connectors, &root)
                            .ok()
                            .map(|at| connectors[at]);
                        (issues::load_blocking(&file), forge)
                    }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                state.forge = forge;
                match read {
                    Ok(read) => {
                        state.land(read);
                        view.status = None;
                    }
                    Err(why) => view.status = Some(why),
                }
                cx.notify();
                view.sync(false, cx);
            });
        }));
    }

    /// Keep the active project in step with its forge, if it is kept in step
    /// with one. `now` for a sync something asked for — an edit to send, a
    /// press of *Sync now* — and not for the ones that only come around, which
    /// wait out [`SYNC_GAP`] since the last.
    fn sync(&mut self, now: bool, cx: &mut Context<Self>) {
        if let Some(root) = self.root.clone() {
            self.sync_root(root, now, cx);
        }
    }

    /// [`Self::sync`] for `root` in particular, which is what a sync that
    /// comes back for a project needs: the one on screen may have changed
    /// while it ran.
    fn sync_root(&mut self, root: PathBuf, now: bool, cx: &mut Context<Self>) {
        let Some(storage) = self.storage.as_deref() else {
            return;
        };
        let file = issues::file_for(storage, &root);
        let Some(state) = self.roots.get_mut(&root) else {
            return;
        };
        let (Some(forge), Some(kept)) = (state.forge, state.issues.as_ref()) else {
            return;
        };
        let wanted = kept.in_step_with(forge.name());
        let due = now
            || state
                .synced
                .as_ref()
                .is_none_or(|(at, _)| issues::now().saturating_sub(*at) >= SYNC_GAP.as_secs());
        if state.syncing && wanted && now {
            state.again = true;
            return;
        }
        if !wanted || !due || state.syncing {
            return;
        }
        state.syncing = true;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { sync::sync_blocking(&file, &root, forge, issues::now()) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                state.syncing = false;
                let again = std::mem::take(&mut state.again);
                let came_to = match done {
                    Ok((kept, report)) => {
                        keep_newer(&mut state.issues, kept);
                        failures(&report).map_or_else(|| Ok(said(&report, forge.name())), Err)
                    }
                    Err(why) => Err(why),
                };
                state.synced = Some((issues::now(), came_to));
                cx.notify();
                if again {
                    view.sync_root(root, true, cx);
                }
            });
        })
        .detach();
    }

    /// Start or stop keeping the active project in step with its forge.
    fn set_syncing(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(forge) = self.state_mut().and_then(|state| state.forge) else {
            return;
        };
        self.change(
            move |kept| {
                kept.sync_with(on.then(|| forge.name().to_string()));
                Ok(None)
            },
            cx,
        );
    }

    /// Send issue `number` to the forge, which is the only way anything written
    /// here reaches it.
    fn publish(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some((root, file)) = self.file() else {
            return;
        };
        let Some(forge) = self.state_mut().and_then(|state| state.forge) else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { sync::publish_blocking(&file, &root, forge, number) }
                })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                view.status = done.err();
                view.load(cx);
            });
        })
        .detach();
    }

    /// Settle issue `number`'s conflict one way or the other, then sync so the
    /// decision reaches the forge.
    fn resolve(&mut self, number: u64, keep_mine: bool, cx: &mut Context<Self>) {
        let now = issues::now();
        self.change(
            move |kept| kept.resolve(number, keep_mine, now).map(|()| Some(number)),
            cx,
        );
    }

    /// Make a change to the active project's issues and write it, then show
    /// what is now kept — selecting the issue the change names, if any.
    ///
    /// The whole read-change-write happens off the UI loop in one call, so it
    /// is made against what is on disk rather than against the copy on screen,
    /// and nothing else in this process can write in between.
    ///
    /// On a project kept in step with its forge the change is then synced
    /// straight away, so an edit reaches the forge without waiting for the
    /// timer.
    fn change(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<Option<u64>, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some((root, file)) = self.file() else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { issues::update_blocking(&file, change) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                match done {
                    Ok((kept, number)) => {
                        keep_newer(&mut state.issues, kept);
                        if number.is_some() {
                            state.selected = number;
                        }
                        state.form = None;
                        view.status = None;
                        cx.notify();
                        view.sync_root(root, true, cx);
                        return;
                    }
                    // The form stays open with what was typed in it: a refusal
                    // is something to correct, not a reason to start again.
                    Err(why) => view.status = Some(why),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn state_mut(&mut self) -> Option<&mut RootIssues> {
        let root = self.root.clone()?;
        self.roots.get_mut(&root)
    }

    fn select(&mut self, number: u64, cx: &mut Context<Self>) {
        if let Some(state) = self.state_mut() {
            state.show(number);
            cx.notify();
        }
    }

    /// Select issue `number` of the active project, asked from outside the
    /// mode. The project's entry is made here if its first read has not
    /// started yet, and the read only fills that entry in, so a selection
    /// made before the issues arrive is the one drawn once they do.
    pub(crate) fn show_issue(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.roots.entry(root).or_default().show(number);
        cx.notify();
    }

    /// Open the form, empty for a new issue or holding what issue `editing`
    /// says now.
    fn open_form(&mut self, editing: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.state_mut() else {
            return;
        };
        let current = editing.and_then(|n| state.issues.as_ref()?.get(n).cloned());
        let (title, labels, body) = match &current {
            Some(issue) => (
                issue.title.clone(),
                issue.labels.join(", "),
                issue.body.clone(),
            ),
            None => Default::default(),
        };
        let form = Form {
            editing,
            editing_reference: current
                .as_ref()
                .and_then(|issue| issue.reference().map(str::to_string)),
            title: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Title")
                    .default_value(title)
            }),
            labels: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Labels, separated by commas")
                    .default_value(labels)
            }),
            body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("What is it about? Markdown is fine.")
                    .default_value(body)
            }),
        };
        form.title.update(cx, |input, cx| input.focus(window, cx));
        if let Some(state) = self.state_mut() {
            state.form = Some(form);
        }
        self.status = None;
        cx.notify();
    }

    fn cancel_form(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.state_mut() {
            state.form = None;
            cx.notify();
        }
    }

    fn save_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.state_mut().and_then(|state| state.form.as_ref()) else {
            return;
        };
        let editing = form.editing;
        let draft = Draft {
            title: form.title.read(cx).value().to_string(),
            body: form.body.read(cx).value().to_string(),
            labels: issues::parse_labels(&form.labels.read(cx).value()),
        };
        let now = issues::now();
        self.change(
            move |kept| {
                match editing {
                    Some(number) => kept.edit(number, draft, now).map(|()| number),
                    None => kept.create(draft, now),
                }
                .map(Some)
            },
            cx,
        );
    }

    /// Ask before closing issue `number`: closing is the one way an issue
    /// leaves the work, and on a project kept in step it closes on the forge
    /// too. Reopening is not asked about — it takes nothing away.
    fn confirm_close(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.state_mut() else {
            return;
        };
        let Some(issue) = state.issues.as_ref().and_then(|kept| kept.get(number)) else {
            return;
        };
        let description = match &issue.link {
            Some(link) => format!(
                "\u{201c}{}\u{201d} moves to the closed list here and closes on {} at the next sync. It can be reopened.",
                issue.title, link.connector
            ),
            None => format!(
                "\u{201c}{}\u{201d} moves to the closed list. It can be reopened.",
                issue.title
            ),
        };
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
        let Some(state) = self.roots.get(&root) else {
            return;
        };
        let Some(link) = state
            .issues
            .as_ref()
            .and_then(|kept| kept.get(number))
            .and_then(|issue| issue.link.clone())
        else {
            return;
        };
        let Some(forge) = state.forge.filter(|forge| forge.name() == link.connector) else {
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
        if self.stale {
            self.stale = false;
            self.load(cx);
        }
        let body = self.body(window, cx);
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(self.status.clone().map(|status| status_line(status, cx)))
    }
}

impl IssuesView {
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(root) = self.root.clone() else {
            return hint("No project root", cx);
        };
        if self.storage.is_none() {
            return hint(
                "This workspace keeps nothing until it is bound to a storage folder in Settings",
                cx,
            );
        }
        let Some(issues) = self.roots.get(&root).and_then(|s| s.issues.clone()) else {
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
            return form_view(form, cx);
        }
        let Some(issue) = state.selected.and_then(|n| issues.get(n)).cloned() else {
            return hint("Pick an issue, or start a new one", cx);
        };
        // Publishing is offered where it can land: a project kept in step with
        // a forge, on an issue not already there.
        let publish_to = state
            .forge
            .filter(|forge| issues.in_step_with(forge.name()))
            .filter(|_| issue.link.is_none())
            .map(|forge| forge.name());
        let body = self.parsed_body(root, &issue, cx);
        let doing = Doing {
            session: working_in(&issue, &self.live).map(str::to_string),
            offered: self.offered.iter().any(|offered| offered == root),
            runs: self
                .runs
                .iter()
                .filter(|run| run.root == root && run.number == issue.number)
                .cloned()
                .collect(),
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
            .state_mut()
            .and_then(|state| state.issues.as_ref()?.get(number))
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

/// What a sync came to, in the one line the list's sync bar carries.
fn said(report: &sync::Report, forge: &str) -> String {
    let mut parts = Vec::new();
    for (n, what) in [
        (report.imported, "new"),
        (report.pulled, "updated here"),
        (report.pushed, "sent"),
        (report.conflicts, "to decide"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    let mut line = if parts.is_empty() {
        format!("In step with {forge}")
    } else {
        format!("Synced with {forge}: {}", parts.join(", "))
    };
    if report.cut {
        line.push_str(&format!(" (the newest {} open only)", sync::SYNC_CAP));
    }
    line
}

/// The standing line for what a sync could not do, or `None` when it did
/// everything: the first failure in full, and how many more there were.
fn failures(report: &sync::Report) -> Option<String> {
    let first = report.failures.first()?;
    // The cause first: the footer shows this in one line, cut to fit, and a
    // preamble there is what the cut would leave.
    Some(match report.failures.len() - 1 {
        0 => first.clone(),
        more => format!("{first} (and {more} more)"),
    })
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

/// Put `incoming` on screen unless what is there was written later. Two reads
/// or writes finish in whatever order the executor finishes them, which is not
/// always the order they reached the disk in, and the older landing second
/// would put an edit back the way it was.
fn keep_newer(shown: &mut Option<Issues>, incoming: Issues) {
    if shown
        .as_ref()
        .is_none_or(|shown| incoming.revision() >= shown.revision())
    {
        *shown = Some(incoming);
    }
}

#[cfg(test)]
mod tests;
