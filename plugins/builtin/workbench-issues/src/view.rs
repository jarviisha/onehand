//! The Issues mode's own state — which project, what it has kept, what is
//! being read or written — and how it is drawn.
//!
//! The issues themselves are core's (`onehand_core::issues`): the file, the
//! numbering, the rules a draft has to meet. What is here is the half that
//! needs a window: the list, the reading, and the form.

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
use onehand_core::issues::{self, Draft, Issues, LocalIssue};
use onehand_plugin_host::{action, hint, status_line};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            root: None,
            storage: None,
            roots: HashMap::new(),
            stale: false,
            split: cx.new(|_| gpui_component::ResizableState::default()),
            status: None,
            _load: None,
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
        self._load = Some(cx.spawn(async move |view, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { issues::load_blocking(&file) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                match read {
                    Ok(read) => {
                        state.issues = Some(read);
                        view.status = None;
                    }
                    Err(why) => view.status = Some(why),
                }
                cx.notify();
            });
        }));
    }

    /// Make a change to the active project's issues and write it, then show
    /// what is now kept — selecting the issue the change names, if any.
    ///
    /// The whole read-change-write happens off the UI loop in one call, so it
    /// is made against what is on disk rather than against the copy on screen,
    /// and nothing else in this process can write in between.
    fn change(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<u64, String> + Send + 'static,
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
                        state.issues = Some(kept);
                        state.selected = Some(number);
                        state.form = None;
                        view.status = None;
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
            state.selected = Some(number);
            state.form = None;
            cx.notify();
        }
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
            move |kept| match editing {
                Some(number) => kept.edit(number, draft, now).map(|()| number),
                None => kept.create(draft, now),
            },
            cx,
        );
    }

    fn set_open(&mut self, number: u64, open: bool, cx: &mut Context<Self>) {
        let now = issues::now();
        self.change(
            move |kept| kept.set_open(number, open, now).map(|()| number),
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

    fn row(&self, issue: &LocalIssue, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let number = issue.number;
        let muted = cx.theme().muted_foreground;
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
        let body = self.parsed_body(root, &issue, cx);
        issue_view(&issue, body, window, cx)
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
    window: &mut Window,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let number = issue.number;
    let open = issue.open;
    let muted = cx.theme().muted_foreground;
    let header =
        div()
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
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.set_open(number, !open, cx)
                    })),
            );

    let facts = div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(muted)
        .child(if issue.labels.is_empty() {
            (if open { "Open" } else { "Closed" }).to_string()
        } else {
            format!(
                "{} · {}",
                if open { "Open" } else { "Closed" },
                issue.labels.join(", ")
            )
        });

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
