//! The Issues page: the issues of every project of the workspace in one
//! list, and the one picked floating over it, drawn in full.
//!
//! The list keeps to the column the other workspace pages keep to, under one
//! toolbar, a row a line. The issue picked floats over the list's right side
//! in a panel of its own (`panel`), which never moves the list: closing it
//! finds every row where it was. Nothing is open when the page is first
//! shown.
//!
//! What the list shows, in what order and what each row says is core's
//! (`onehand_core::task::work::list`); what is here holds the page's state —
//! the filters, the list as it is on screen, the pull requests read — and
//! draws what core decides. Everything lives on the view's entity, so
//! leaving the page for a session and picking it again finds it as it was.

use super::{IssuesView, full, review};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled, Window,
    div,
};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::issues::{self, IssueKey, LocalIssue};
use onehand_core::task::work::IssueWork;
use onehand_core::task::work::list::{Filters, Held, Item, PrReads, list};
use onehand_plugin_host::{PAGE_WIDE, hint};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

mod head;
mod panel;
mod prs;
mod row;

use row::page_row;

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
    /// The issue (or the form) floats over the list. Closed, the row stays
    /// picked, so the arrow keys and opening it again start from it.
    open: bool,
    /// The panel's width in rems, once dragged; kept while the app runs.
    panel_w: Option<f32>,
    /// The list's scroll, kept across leaving the page.
    scroll: ScrollHandle,
    /// The picked row is scrolled into view on the next draw: the arrow keys
    /// moved past the list's edge, or an issue was opened from outside.
    reveal: bool,
    /// The rows on screen, in order, which the arrow keys step through.
    shown: Vec<(PathBuf, u64)>,
    /// Where the arrow keys and Esc land: the page itself, once a row is
    /// picked.
    focus: Option<FocusHandle>,
    /// An issue was opened from outside the page: the page takes the
    /// keyboard on its next draw, so Esc and the arrow keys answer at once.
    claim_focus: bool,
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
        page.open = true;
        page.reveal = true;
        page.claim_focus = true;
        self.root = Some(root.to_path_buf());
        if let Some(state) = self.state_for(root, cx) {
            state.selected = Some(number);
        }
        cx.notify();
    }

    /// Pick issue `number` of `root` from the page's list, and open it.
    ///
    /// A changed draft in that project is asked about first; cancelled,
    /// nothing in the list moves. The pin goes once another issue is picked.
    fn pick(&mut self, root: PathBuf, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key(&root, number);
        // The page takes the keyboard, so the arrow keys and Esc work from here.
        if let Some(focus) = self.page.as_ref().and_then(|page| page.focus.clone()) {
            window.focus(&focus, cx);
        }
        let arriving = self.root.as_deref() != Some(root.as_path());
        self.root = Some(root.clone());
        let drafting = self
            .state_for(&root, cx)
            .is_some_and(|state| state.form.is_some());
        // Coming to another project that holds a draft shows the draft, which
        // is kept across a switch of project; picking within it asks.
        if arriving && drafting {
            if let Some(page) = self.page.as_mut() {
                page.open = true;
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
                page.open = true;
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
        page.open = true;
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

    /// The keys, while the page holds the keyboard itself rather than a
    /// field inside it: Esc closes the issue, up and down pick the row above
    /// or below the one picked and open it.
    fn page_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.root.clone().zip(self.selected_number());
        let Some(page) = self.page.as_mut() else {
            return;
        };
        if !page
            .focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
        {
            return;
        }
        let step: isize = match event.keystroke.key.as_str() {
            "escape" if page.open => {
                page.open = false;
                cx.stop_propagation();
                cx.notify();
                return;
            }
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        cx.stop_propagation();
        let at = current.and_then(|(root, number)| {
            page.shown
                .iter()
                .position(|(r, n)| *r == root && *n == number)
        });
        // Closed, the first press opens the row picked rather than moving.
        let next = match at {
            Some(at) if !page.open => Some(at),
            Some(at) => at.checked_add_signed(step),
            None => Some(0),
        };
        if let Some((root, number)) = next.and_then(|next| page.shown.get(next)).cloned() {
            page.reveal = true;
            self.pick(root, number, window, cx);
        }
    }

    /// The issue number picked in the project on screen.
    fn selected_number(&self) -> Option<u64> {
        self.roots.get(self.root.as_deref()?)?.selected
    }

    /// The page: the toolbar over the list, and the issue picked floating
    /// over the list while it is open.
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

        let selected = self.selected_key();
        let (focus, claim) = match self.page.as_mut() {
            Some(page) => (
                page.focus.get_or_insert_with(|| cx.focus_handle()).clone(),
                std::mem::take(&mut page.claim_focus),
            ),
            None => return hint("No page", cx),
        };
        if claim {
            window.focus(&focus, cx);
        }
        let (top, rows) = self.page_list(&projects, selected.as_ref(), window, cx);
        let open = self.page.as_ref().is_some_and(|page| page.open);
        let panel = (open && self.has_page_issue()).then(|| self.page_panel(window, cx));
        div()
            .id("issues-page")
            .track_focus(&focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                view.page_key(event, window, cx)
            }))
            .size_full()
            .v_flex()
            .child(top)
            .child(
                // The panel is placed on the list's area, under the toolbar,
                // so the filters stay in reach while an issue is open.
                div()
                    .id("issues-page-area")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .on_drag_move(cx.listener(Self::drag_panel))
                    .child(rows)
                    .children(panel),
            )
            .into_any_element()
    }

    /// Whether a form is open in the project on screen.
    fn drafting(&self) -> bool {
        self.root
            .as_deref()
            .and_then(|root| self.roots.get(root))
            .is_some_and(|state| state.form.is_some())
    }

    /// Whether there is an issue or a form to float over the list.
    fn has_page_issue(&self) -> bool {
        self.drafting() || self.selected_key().is_some()
    }

    /// The issue the page has picked, by its file and number.
    fn selected_key(&self) -> Option<IssueKey> {
        let root = self.root.as_deref()?;
        let number = self.roots.get(root)?.selected?;
        self.key(root, number)
    }

    /// The list: the toolbar and what the reading says, then, apart, the
    /// rows, which the panel floats over.
    fn page_list(
        &mut self,
        projects: &[(PathBuf, SharedString)],
        selected: Option<&IssueKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement) {
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
        let storage = self.storage.clone().unwrap_or_default();
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
        let columns = self.width.get().0 >= row::COLUMNS_MIN;
        let page_focused = self
            .page
            .as_ref()
            .and_then(|page| page.focus.as_ref())
            .is_some_and(|focus| focus.is_focused(window));
        let Some(page) = self.page.as_mut() else {
            return (hint("No page", cx), div().into_any_element());
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
        page.shown = listed
            .rows
            .iter()
            .map(|row| (row.item.root.to_path_buf(), row.item.issue.number))
            .collect();
        let scroll = page.scroll.clone();
        // The picked row into view; the handle scrolls by child, and each row
        // is one child.
        if std::mem::take(&mut page.reveal)
            && let Some(at) = listed
                .rows
                .iter()
                .position(|row| selected == Some(&row.item.key))
        {
            scroll.scroll_to_item(at);
        }
        let labels: Vec<String> = items
            .iter()
            .flat_map(|item| item.issue.labels.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let toolbar = self.page_head(&listed, &filters, projects, labels, &query, cx);
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
        // The project is named on a row only where it tells rows apart: every
        // project listed, and more than one of them on screen.
        let mixed = filters.project.is_none()
            && listed
                .rows
                .iter()
                .any(|row| row.item.root != listed.rows[0].item.root);
        let last = listed.rows.len().saturating_sub(1);
        // The columns are named over rows, never over an empty list.
        let head = !listed.rows.is_empty();
        let focused = page_focused;
        let rows: Vec<AnyElement> = listed
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let picked = selected == Some(&row.item.key);
                let project = mixed.then(|| label_of(row.item.root));
                let place = row::Place {
                    first: i == 0 && !head,
                    last: i == last,
                    picked,
                    focused,
                    columns,
                };
                page_row(i, place, row, project, cx)
            })
            .collect();
        let left_out = listed.left_out;
        let line =
            |text: String| column(div().px_3().py_2().text_xs().text_color(muted).child(text));

        let top = div()
            .flex_none()
            .px_4()
            .pt_6()
            .child(column(
                div().v_flex().gap_2().child(toolbar).children(notices),
            ))
            // Outside the scroll, so the names stay over the rows they name.
            .map(|top| match head {
                true => top.child(div().mt_6().child(row::columns_head(columns, cx))),
                false => top.pb_6(),
            })
            .into_any_element();
        let rows = div()
            .id("issues-page-list")
            .size_full()
            .v_flex()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px_4()
            .pb_6()
            .children(rows)
            .children(empty.map(|empty| line(empty.to_string())))
            .when(left_out > 0, |list| {
                list.child(line(format!("… {left_out} more not shown")))
            })
            .into_any_element();
        (top, rows)
    }

    /// Every filter back to where it starts, which a pinned row offers.
    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(query) = self.query.clone() {
            query.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.filter(|f| *f = Filters::default(), cx);
    }
}

/// `inner` in the column the workspace pages keep to, centred.
fn column(inner: gpui::Div) -> gpui::Div {
    inner.w_full().max_w(PAGE_WIDE).mx_auto()
}

/// The work of every issue of `root`, by the file the page keys them with.
pub(super) fn works_of<'a>(
    works: &'a [IssueWork],
    file: &'a Path,
) -> impl Iterator<Item = &'a IssueWork> {
    works.iter().filter(move |work| work.key.file == file)
}
