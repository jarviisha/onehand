//! The Issues page: the issues of every project of the workspace in one
//! list, and the one picked beside it, drawn in full.
//!
//! What the list shows, in what order and what each row says is core's
//! (`onehand_core::task::work::list`); what is here holds the page's state —
//! the filters, the list as it is on screen, the pull requests read — and
//! draws what core decides. Everything lives on the view's entity, so
//! leaving the page for a session and picking it again finds it as it was.

use super::{IssuesView, full, review};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::issues::{self, IssueKey, LocalIssue};
use onehand_core::task::work::IssueWork;
use onehand_core::task::work::list::{Filters, Held, Item, PrReads, list};
use onehand_plugin_host::{action, hint};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

mod head;
mod prs;
mod row;

use row::page_row;

/// The page's width, in rems, below which the list and the issue no longer
/// fit side by side: the list is shown, then the issue alone.
const TWO_COLUMNS: f32 = 56.;

/// The list's width beside the issue, in rems.
const LIST_W: f32 = 24.;

/// What the page holds beside what every Issues view does.
#[derive(Default)]
pub(super) struct PageState {
    /// Every project of the workspace, in rail order, with what it is called.
    projects: Vec<(PathBuf, SharedString)>,
    /// The filters but the search, which is the view's own input.
    filters: Filters,
    /// The list as it was last drawn, kept still by core.
    held: Held,
    /// The rows on screen that stopped matching, which leave once another
    /// issue is picked.
    stopped: Vec<IssueKey>,
    /// Narrow, the issue is shown alone; *Back* is the list again.
    alone: bool,
    /// The list's scroll, kept across leaving the page and *Back*.
    scroll: ScrollHandle,
    /// Each project's pull requests, as last read.
    pub(super) prs: HashMap<PathBuf, PrReads>,
    /// The generation of each project's pull request read, so an answer to
    /// an older read is dropped.
    pub(super) pr_asked: HashMap<PathBuf, u64>,
    /// The page was just shown: its pull requests are read.
    pub(super) arrived: bool,
    /// What the issue on screen left, as read off git.
    pub(super) full: full::FullState,
    /// The review block, while one is open.
    pub(super) review: review::ReviewState,
}

impl PageState {
    /// The filters changed: the list is drawn afresh, the pin let go.
    pub(super) fn refilter(&mut self) {
        self.held = Held::default();
    }

    /// What project `root` is called on the page.
    pub(super) fn label_of(&self, root: &Path) -> Option<SharedString> {
        self.projects
            .iter()
            .find(|(r, _)| r == root)
            .map(|(_, label)| label.clone())
    }

    /// What the full form draws from this page's state.
    pub(super) fn full(&self) -> full::Full<'_> {
        self.full.view()
    }
}

impl IssuesView {
    /// Tell the page which projects the workspace has.
    pub(crate) fn set_projects(
        &mut self,
        projects: Vec<(PathBuf, SharedString)>,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        if page.projects != projects {
            page.projects = projects;
            cx.notify();
        }
    }

    /// The page was put on screen: every file it draws is read again, and so
    /// are the pull requests.
    pub(crate) fn page_shown(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.page.as_mut() {
            page.arrived = true;
        }
        self.mark_stale(cx);
    }

    /// Open issue `number` of `root` on the page, from outside it. The
    /// filters stay as they are; an issue they leave out is pinned at the top.
    pub(crate) fn open_on_page(&mut self, root: &Path, number: u64, cx: &mut Context<Self>) {
        let key = self.key(root, number);
        let Some(page) = self.page.as_mut() else {
            return;
        };
        if !page.review.open_on(key.as_ref()) {
            page.review = review::ReviewState::default();
        }
        page.held.pinned = key;
        page.alone = true;
        self.root = Some(root.to_path_buf());
        if let Some(state) = self.state_for(root, cx) {
            state.selected = Some(number);
        }
        cx.notify();
    }

    /// Pick issue `number` of `root` from the page's list.
    ///
    /// A changed draft in that project is asked about first; cancelled,
    /// nothing in the list moves. The pin goes once another issue is picked.
    fn pick(&mut self, root: PathBuf, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key(&root, number);
        let arriving = self.root.as_deref() != Some(root.as_path());
        self.root = Some(root.clone());
        let drafting = self
            .state_for(&root, cx)
            .is_some_and(|state| state.form.is_some());
        // Coming to another project that holds a draft shows the draft, which
        // is kept across a switch of project; picking within it asks.
        if arriving && drafting {
            if let Some(page) = self.page.as_mut() {
                page.alone = true;
            }
            cx.notify();
            return;
        }
        self.unless_drafting(window, cx, move |view, _, cx| {
            if let Some(page) = view.page.as_mut() {
                if let Some(key) = &key {
                    let stopped = std::mem::take(&mut page.stopped);
                    page.held.pick(key, &stopped);
                }
                // Picking another issue closes the review; the same keeps it.
                if !page.review.open_on(key.as_ref()) {
                    page.review = review::ReviewState::default();
                }
                page.alone = true;
            }
            if let Some(state) = view.state_mut() {
                state.selected = Some(number);
                state.form = None;
            }
            cx.notify();
        });
    }

    /// Start a new issue in the project the list is filtered to, else the
    /// one the picked issue is in, else the first.
    fn new_issue_on_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let Some(root) = page
            .filters
            .project
            .clone()
            .or_else(|| self.root.clone())
            .or_else(|| page.projects.first().map(|(root, _)| root.clone()))
        else {
            return;
        };
        page.alone = true;
        self.root = Some(root.clone());
        self.state_for(&root, cx);
        self.open_form(None, window, cx);
    }

    /// Change the filters: the list is drawn afresh, the pin let go.
    fn filter(&mut self, change: impl FnOnce(&mut Filters), cx: &mut Context<Self>) {
        if let Some(page) = self.page.as_mut() {
            change(&mut page.filters);
            page.refilter();
            cx.notify();
        }
    }

    /// The page: the list and the issue side by side, or one at a time when
    /// narrow.
    pub(super) fn page_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.storage.is_none() {
            return hint(
                "This workspace keeps nothing until it is bound to a storage folder in Settings",
                cx,
            );
        }
        let projects = self
            .page
            .as_ref()
            .map(|p| p.projects.clone())
            .unwrap_or_default();
        if projects.is_empty() {
            return hint("No projects in this workspace", cx);
        }
        for (root, _) in &projects {
            if let Some(state) = self.state_for(root, cx) {
                state.file.update(cx, |file, cx| file.load_if_stale(cx));
            }
        }
        self.read_prs_if_due(cx);

        let wide = window.viewport_size().width >= window.rem_size() * TWO_COLUMNS;
        let alone = self.page.as_ref().is_some_and(|page| page.alone);
        let selected = self.selected_key();
        if !wide && alone && selected.is_some() {
            return self.page_issue(true, window, cx);
        }
        let list = self.page_list(&projects, selected.as_ref(), window, cx);
        if !wide {
            return list;
        }
        div()
            .size_full()
            .h_flex()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .w(rems(LIST_W))
                    .h_full()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .child(list),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.page_issue(false, window, cx)),
            )
            .into_any_element()
    }

    /// The issue picked, under a *Back* when it is shown alone.
    fn page_issue(
        &mut self,
        alone: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = match self.root.clone() {
            Some(root) => match self.issues_of(&root, cx).cloned() {
                Some(issues) => self.detail(&root, &issues, window, cx),
                None => hint("Reading issues…", cx),
            },
            None => hint("Pick an issue", cx),
        };
        div()
            .size_full()
            .v_flex()
            .when(alone, |column| {
                column.child(
                    div()
                        .flex_none()
                        .h_flex()
                        .px_2()
                        .py_1()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            action("issues-page-back")
                                .xsmall()
                                .ghost()
                                .icon(Icon::new(IconName::ArrowLeft))
                                .label("Back")
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    if let Some(page) = view.page.as_mut() {
                                        page.alone = false;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(div().flex_1().min_h_0().v_flex().child(detail))
            .into_any_element()
    }

    /// The issue the page has picked, by its file and number.
    fn selected_key(&self) -> Option<IssueKey> {
        let root = self.root.as_deref()?;
        let number = self.roots.get(root)?.selected?;
        self.key(root, number)
    }

    /// The list: the search and filters, what the reading says, the rows.
    fn page_list(
        &mut self,
        projects: &[(PathBuf, SharedString)],
        selected: Option<&IssueKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // What the rows borrow, held for the length of this draw.
        let owned: Vec<(PathBuf, LocalIssue)> = projects
            .iter()
            .flat_map(|(root, _)| {
                self.issues_of(root, cx)
                    .map(|kept| kept.listed().into_iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|issue| (root.clone(), issue))
            })
            .collect();
        let works = self.works.clone();
        let Some(storage) = self.storage.clone() else {
            return hint("No storage", cx);
        };
        let items: Vec<Item<'_>> = owned
            .iter()
            .map(|(root, issue)| {
                let key = IssueKey {
                    file: issues::file_for(&storage, root),
                    number: issue.number,
                };
                let work = works.iter().find(|work| work.key == key);
                Item {
                    root,
                    key,
                    issue,
                    work,
                }
            })
            .collect();
        let query = self.query(window, cx);
        let text = query.read(cx).value().to_string();
        let Some(page) = self.page.as_mut() else {
            return hint("No page", cx);
        };
        let mut filters = page.filters.clone();
        filters.query = text;
        // A project or label filter nothing carries any more is dropped: a
        // filter nothing on screen can clear hides every issue.
        if let Some(only) = &page.filters.project
            && !projects.iter().any(|(root, _)| root == only)
        {
            page.filters.project = None;
            filters.project = None;
        }
        let listed = list(&items, &filters, &page.prs, &page.held, issues::now());
        page.held.order = listed.order();
        page.stopped = listed.stopped_matching();
        let scroll = page.scroll.clone();
        let labels: Vec<String> = items
            .iter()
            .flat_map(|item| item.issue.labels.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let head = self.page_head(&listed, &filters, projects, labels, cx);
        let notices = self.page_notices(&listed, projects, cx);
        let muted = cx.theme().muted_foreground;
        let empty = if items.is_empty() {
            Some("No issues yet")
        } else if listed.rows.is_empty() {
            Some("No issues match")
        } else {
            None
        };
        let label_of = |root: &Path| {
            projects
                .iter()
                .find(|(r, _)| r == root)
                .map(|(_, label)| label.clone())
                .unwrap_or_default()
        };
        let rows: Vec<AnyElement> = listed
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let picked = selected == Some(&row.item.key);
                page_row(i, row, label_of(row.item.root), picked, cx)
            })
            .collect();
        let left_out = listed.left_out;

        div()
            .size_full()
            .v_flex()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .flex_none()
                    .px_2()
                    .pt_2()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&query)
                                .small()
                                .prefix(Icon::new(IconName::Search).xsmall().text_color(muted))
                                .cleanable(true),
                        ),
                    )
                    .child(
                        action("issues-page-new")
                            .small()
                            .ghost()
                            .icon(Icon::new(IconName::Plus))
                            .tooltip("New issue, in the project filtered to or the one picked")
                            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                                view.new_issue_on_page(window, cx)
                            })),
                    ),
            )
            .child(head)
            .children(notices)
            .child(
                div()
                    .id("issues-page-list")
                    .flex_1()
                    .min_h_0()
                    .v_flex()
                    .gap_0p5()
                    .p_1()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .children(
                        empty.map(|empty| {
                            div().px_2().py_1().text_xs().text_color(muted).child(empty)
                        }),
                    )
                    .children(rows)
                    .when(left_out > 0, |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("… {left_out} more not shown")),
                        )
                    }),
            )
            .into_any_element()
    }

    /// Every filter back to where it starts, which a pinned row offers.
    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(query) = self.query.clone() {
            query.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.filter(|f| *f = Filters::default(), cx);
    }
}

/// The work of every issue of `root`, by the file the page keys them with.
pub(super) fn works_of<'a>(
    works: &'a [IssueWork],
    file: &'a Path,
) -> impl Iterator<Item = &'a IssueWork> {
    works.iter().filter(move |work| work.key.file == file)
}
