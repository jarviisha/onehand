//! The Issues mode's own state — which project, what it has kept, what is
//! being read or written — and how it is drawn.
//!
//! The issues themselves are core's (`onehand_core::issues`): the file, the
//! numbering, the rules a draft has to meet, and keeping them in step with a
//! forge. What is here is the half that needs a window: the list, the reading,
//! the form, and when a sync runs.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement, Render, StatefulInteractiveElement as _, Styled, Task, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::text::{TextView, TextViewState, TextViewStyle};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable as _, StyledExt, h_resizable, resizable_panel,
};
use onehand_core::connector::{self, Connector};
use onehand_core::issues::{self, Draft, Issues, LocalIssue, sync};
use onehand_plugin_host::{action, hint, status_ink, status_line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often a project kept in step with its forge is synced while it is the
/// one this mode is on, if nothing else has synced it sooner.
const SYNC_EVERY: Duration = Duration::from_secs(300);

/// The least time between two syncs that nothing asked for. Being shown, and
/// reading the file again, are frequent; the forge does not need to hear about
/// each of them.
const SYNC_GAP: Duration = Duration::from_secs(60);

/// The list's width before anybody drags it, and the range a drag may take it
/// through — pixels, because that is the only thing the split accepts. Its
/// rows are a number and a title, so the floor is what a short title needs.
const LIST_W: f32 = 240.;
const LIST_MIN: f32 = 160.;
const LIST_MAX: f32 = 420.;

/// How many rows the list draws. A project with more than this many issues is
/// said to have them, and the rest are not built: every row is an element, and
/// a list nobody scrolls to the end of is not worth building to the end.
const LIST_CAP: usize = 500;

/// How many of an issue's notes are drawn under it: the latest ones, since a
/// note is what a run said about how it ended and the last run is the one that
/// is read.
const NOTES_SHOWN: usize = 5;

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
}

/// One project's issues and what is open among them.
#[derive(Default)]
struct RootIssues {
    /// `None` until the first read lands.
    issues: Option<Issues>,
    selected: Option<u64>,
    form: Option<Form>,
    /// The selected issue's body, parsed, and the stamp it was parsed at — so
    /// it is parsed again when the issue changes and never on a frame when it
    /// has not.
    body: Option<(u64, u64, Entity<TextViewState>)>,
    /// The forge that serves this project, if one does — found when its
    /// issues are read, since asking reads the project's git remote.
    forge: Option<&'static dyn Connector>,
    /// A sync is on its way; a second is not started beside it.
    syncing: bool,
    /// Something asked for a sync while one was running — an edit saved
    /// meanwhile — so another runs as soon as it lands, rather than leaving
    /// the edit for the timer.
    again: bool,
    /// When the last sync finished, and what it came to in one line.
    synced: Option<(Instant, String)>,
}

impl RootIssues {
    fn show(&mut self, number: u64) {
        self.selected = Some(number);
        self.form = None;
    }
}

/// The form a new issue or an edit is written in.
struct Form {
    /// The issue being edited, or `None` for a new one.
    editing: Option<u64>,
    title: Entity<InputState>,
    labels: Entity<InputState>,
    body: Entity<TextareaState>,
}

impl IssuesView {
    pub(crate) fn new(connectors: &'static [&'static dyn Connector], cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            root: None,
            storage: None,
            roots: HashMap::new(),
            stale: false,
            split: cx.new(|_| gpui_component::ResizableState::default()),
            status: None,
            _load: None,
            connectors,
            _sync_every: cx.spawn(async move |view, cx| {
                loop {
                    cx.background_executor().timer(SYNC_EVERY).await;
                    if view
                        .update(cx, |view: &mut Self, cx| view.sync(false, cx))
                        .is_err()
                    {
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
                        keep_newer(&mut state.issues, read);
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
                .is_none_or(|(at, _)| at.elapsed() >= SYNC_GAP);
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
                match done {
                    Ok((kept, report)) => {
                        keep_newer(&mut state.issues, kept);
                        state.synced = Some((Instant::now(), said(&report, forge.name())));
                        view.status = failures(&report);
                    }
                    Err(why) => {
                        state.synced = Some((
                            Instant::now(),
                            format!("Could not sync with {}", forge.name()),
                        ));
                        view.status = Some(why);
                    }
                }
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

        let list = self.list(&issues, cx);
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

    /// The list: a header with the way to start a new issue, then a row per
    /// issue, open ones first.
    fn list(&self, issues: &Issues, cx: &mut Context<Self>) -> AnyElement {
        let listed = issues.listed();
        let open = listed.iter().filter(|i| i.open).count();
        let selected = self
            .root
            .as_ref()
            .and_then(|root| self.roots.get(root))
            .and_then(|state| state.selected);
        let muted = cx.theme().muted_foreground;
        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{open} open, {} closed", listed.len() - open)),
            )
            .child(
                action("issues-new")
                    .xsmall()
                    .ghost()
                    .icon(Icon::new(IconName::Plus))
                    .label("New issue")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.open_form(None, window, cx)
                    })),
            );

        let bar = self.sync_bar(issues, cx);
        let rows: Vec<AnyElement> = listed
            .iter()
            .take(LIST_CAP)
            .map(|issue| self.row(issue, selected == Some(issue.number), cx))
            .collect();
        let cut = listed.len().saturating_sub(LIST_CAP);

        div()
            .size_full()
            .v_flex()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(header)
            .children(bar)
            .child(
                div()
                    .id("issues-list")
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .p_1()
                    .overflow_y_scroll()
                    .when(listed.is_empty(), |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(muted)
                                .child("No issues yet"),
                        )
                    })
                    .children(rows)
                    .when(cut > 0, |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("… {cut} more not shown")),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The line under the list's header saying whether this project is kept in
    /// step with its forge, and the controls for it. Drawn only where a forge
    /// serves the project — elsewhere there is nothing to be in step with.
    fn sync_bar(&self, issues: &Issues, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.roots.get(self.root.as_ref()?)?;
        let forge = state.forge?;
        let on = issues.in_step_with(forge.name());
        let line = match (&state.synced, on, state.syncing) {
            (_, true, true) => format!("Syncing with {}…", forge.name()),
            (Some((_, said)), true, false) => said.clone(),
            (None, true, false) => format!("Kept in step with {}", forge.name()),
            (_, false, _) => format!("Not kept in step with {}", forge.name()),
        };
        let bar = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(line),
            );
        Some(if on {
            // Not offered while one is running: pressed then, it would do
            // nothing, and a control that answers with nothing reads as broken.
            bar.when(!state.syncing, |bar| {
                bar.child(
                    action("issues-sync-now")
                        .xsmall()
                        .ghost()
                        .label("Sync now")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.sync(true, cx))),
                )
            })
            .child(
                action("issues-sync-off")
                    .xsmall()
                    .ghost()
                    .label("Stop")
                    .tooltip("Stop keeping these issues in step; the links are kept")
                    .on_click(
                        cx.listener(|view, _: &ClickEvent, _, cx| view.set_syncing(false, cx)),
                    ),
            )
            .into_any_element()
        } else {
            bar.child(
                action("issues-sync-on")
                    .xsmall()
                    .ghost()
                    .label(format!("Sync with {}", forge.name()))
                    .tooltip("Bring in its open issues and keep both sides in step")
                    .on_click(
                        cx.listener(|view, _: &ClickEvent, _, cx| view.set_syncing(true, cx)),
                    ),
            )
            .into_any_element()
        })
    }

    fn row(&self, issue: &LocalIssue, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let number = issue.number;
        let muted = cx.theme().muted_foreground;
        let conflicted = issue.link.as_ref().is_some_and(|l| l.conflict.is_some());
        div()
            .id(("issue-row", number))
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .h_6()
            .px_2()
            .rounded(cx.theme().radius)
            .text_sm()
            .cursor_pointer()
            .when(selected, |row| row.bg(cx.theme().accent))
            .hover(|row| row.bg(cx.theme().accent.opacity(0.5)))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("#{number}")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    // A closed issue stays in the list as a record, drawn
                    // quieter than the work that is still open.
                    .when(!issue.open, |title| title.text_color(muted).line_through())
                    .child(issue.title.clone()),
            )
            // An issue waiting on a person is marked in words, in the warning
            // ink: it is the one row nothing will move until somebody opens it.
            .when(conflicted, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(status_ink(cx).warning)
                        .child("decide"),
                )
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(number, cx)))
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
        issue_view(&issue, body, publish_to, window, cx)
    }

    /// The selected issue's body as parsed markdown, parsed again only when
    /// the issue has changed since the last time.
    fn parsed_body(
        &mut self,
        root: &Path,
        issue: &LocalIssue,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TextViewState>> {
        if issue.body.is_empty() {
            return None;
        }
        let state = self.roots.get_mut(root)?;
        match &state.body {
            Some((n, at, parsed)) if *n == issue.number && *at == issue.updated => {
                Some(parsed.clone())
            }
            _ => {
                let parsed = cx.new(|cx| TextViewState::markdown(&issue.body, cx));
                state.body = Some((issue.number, issue.updated, parsed.clone()));
                Some(parsed)
            }
        }
    }
}

/// One issue, read: its number and title with what can be done to it, its
/// labels, and its body.
fn issue_view(
    issue: &LocalIssue,
    body: Option<Entity<TextViewState>>,
    publish_to: Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number;
    let open = issue.open;
    let muted = cx.theme().muted_foreground;
    let header = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .flex_none()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex_none()
                .text_color(muted)
                .child(format!("#{number}")),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_semibold()
                .child(issue.title.clone()),
        )
        .children(publish_to.map(|forge| {
            action("issue-publish")
                .xsmall()
                .ghost()
                .label(format!("Publish to {forge}"))
                .tooltip("Open it there too, and keep the two in step")
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.publish(number, cx)))
        }))
        .child(
            action("issue-edit")
                .xsmall()
                .ghost()
                .label("Edit")
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                    view.open_form(Some(number), window, cx)
                })),
        )
        .child(
            action("issue-close")
                .xsmall()
                .ghost()
                .label(if open { "Close" } else { "Reopen" })
                .on_click(
                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.set_open(number, !open, cx)
                    }),
                ),
        );

    let mut facts = vec![(if open { "Open" } else { "Closed" }).to_string()];
    if let Some(link) = &issue.link {
        facts.push(format!("{} {}", link.connector, link.reference));
    }
    if !issue.labels.is_empty() {
        facts.push(issue.labels.join(", "));
    }
    let facts = div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(muted)
        .child(facts.join(" · "));
    let conflict = conflict_view(issue, cx);

    // What runs have said about it, newest last, under the body — the record of
    // what was tried, which the body itself never changes to say.
    let notes = (!issue.notes.is_empty()).then(|| {
        div()
            .flex_none()
            .max_h(gpui::rems(12.))
            .v_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(issue.notes.len() > NOTES_SHOWN, |notes| {
                notes.child(div().text_xs().text_color(muted).child(format!(
                    "… {} earlier notes not shown",
                    issue.notes.len() - NOTES_SHOWN
                )))
            })
            .children(
                issue
                    .notes
                    .iter()
                    .rev()
                    .take(NOTES_SHOWN)
                    .rev()
                    .map(|note| div().text_xs().text_color(muted).child(note.text.clone())),
            )
    });

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        .child(header)
        .child(facts)
        .children(conflict)
        .child(match body {
            Some(body) => div()
                .flex_1()
                .min_h_0()
                .p_3()
                .child(
                    TextView::new(&body)
                        .selectable(true)
                        .scrollable(true)
                        .style(body_style(window.rem_size(), cx)),
                )
                .into_any_element(),
            None => div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(muted)
                .child("No description")
                .into_any_element(),
        })
        .children(notes)
        .into_any_element()
}

/// What a person has to decide about an issue both sides changed: each field in
/// conflict, as it reads here and as it reads on the forge, and the two ways
/// out. `None` for an issue with nothing to decide.
fn conflict_view(issue: &LocalIssue, cx: &mut Context<IssuesView>) -> Option<AnyElement> {
    let link = issue.link.as_ref()?;
    let theirs = link.conflict.as_ref()?;
    let ours = issue.snapshot();
    let number = issue.number;
    let forge = link.connector.clone();
    let open = |open: bool| if open { "open" } else { "closed" }.to_string();
    let lines: Vec<String> = sync::merge(&link.base, &ours, theirs)
        .conflicts
        .into_iter()
        .map(|field| match field {
            sync::Field::Title => {
                format!("Title — here: {}; on {forge}: {}", ours.title, theirs.title)
            }
            sync::Field::State => format!(
                "State — here: {}; on {forge}: {}",
                open(ours.open),
                open(theirs.open)
            ),
            sync::Field::Description => {
                let (here, there) = (excerpt(&ours.body), excerpt(&theirs.body));
                if here == there {
                    // Both begin alike and part further on, past what the line
                    // shows: said, or the two would read as one.
                    format!("Description — the two differ further on; here it begins: {here}")
                } else {
                    format!("Description — here: {here}\non {forge}: {there}")
                }
            }
        })
        .collect();
    let ink = status_ink(cx).warning;
    Some(
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .mx_2()
            .my_1()
            .p_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(ink)
            .child(
                div()
                    .text_xs()
                    .text_color(ink)
                    .child(format!("Changed here and on {forge} since the last sync:")),
            )
            .children(lines.into_iter().map(|line| div().text_xs().child(line)))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        action("issue-keep-mine")
                            .xsmall()
                            .ghost()
                            .label("Keep mine")
                            .tooltip(format!("Send what it says here to {forge}"))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.resolve(number, true, cx)
                            })),
                    )
                    .child(
                        action("issue-take-theirs")
                            .xsmall()
                            .ghost()
                            .label(format!("Take {forge}'s"))
                            .tooltip("Replace what it says here with the other side")
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.resolve(number, false, cx)
                            })),
                    ),
            )
            .into_any_element(),
    )
}

/// The form: title, labels, body, then Save and Cancel under what they act on.
fn form_view(form: &Form, cx: &mut Context<IssuesView>) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        .gap_2()
        .p_2()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(match form.editing {
                    Some(number) => format!("Editing #{number}"),
                    None => "New issue".to_string(),
                }),
        )
        .child(Input::new(&form.title))
        .child(Input::new(&form.labels))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .child(Textarea::new(&form.body).h_full()),
        )
        .child(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .child(
                    action("issue-form-cancel")
                        .small()
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.cancel_form(cx))),
                )
                .child(
                    action("issue-form-save")
                        .small()
                        .primary()
                        .label(match form.editing {
                            Some(_) => "Save",
                            None => "Create issue",
                        })
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save_form(cx))),
                ),
        )
        .into_any_element()
}

/// Body styling. The renderer sizes its headings from an absolute pixel base,
/// which the panel's rem-base zoom cannot reach by itself, so the base in force
/// is written in by hand.
fn body_style(rem: gpui::Pixels, cx: &App) -> TextViewStyle {
    let mut style = TextViewStyle::default().code_block(
        gpui::StyleRefinement::default()
            .p(gpui::rems(0.75))
            .text_size(gpui::rems(0.8125))
            .bg(cx.theme().muted),
    );
    style.heading_base_font_size = rem;
    style
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
    Some(match report.failures.len() - 1 {
        0 => format!("Not kept in step: {first}"),
        more => format!("Not kept in step: {first} (and {more} more)"),
    })
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

/// The start of a description, for the line that sets two of them side by
/// side: enough to tell them apart, bounded so a long one does not push the
/// choice off the panel.
fn excerpt(text: &str) -> String {
    const SHOWN: usize = 160;
    match text.char_indices().nth(SHOWN) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None if text.is_empty() => "(empty)".to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issue_shown_before_the_read_lands_stays_selected() {
        let root = PathBuf::from("/p");
        let mut roots: HashMap<PathBuf, RootIssues> = HashMap::new();
        roots.entry(root.clone()).or_default().show(7);
        // What a read landing does to the entry.
        let state = roots.entry(root).or_default();
        keep_newer(&mut state.issues, Issues::default());
        assert_eq!(state.selected, Some(7));
    }
}
