//! The Issues page: the issues of every project of the workspace in one
//! list, and the one picked beside it, drawn in full.
//!
//! What the list shows, in what order and what each row says is core's
//! (`onehand_core::task::work::list`); what is here holds the page's state —
//! the filters, the list as it is on screen, the pull requests read — and
//! draws what core decides. Everything lives on the view's entity, so
//! leaving the page for a session and picking it again finds it as it was.

use super::{IssuesView, full};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, Size, StyledExt};
use onehand_core::issues::{self, IssueKey, LocalIssue};
use onehand_core::task::work::IssueWork;
use onehand_core::task::work::list::{
    Filters, Held, Incomplete, Item, Listed, PrReads, Progress, Row, list,
};
use onehand_plugin_host::{action, hint, menu_below, menu_item, status_ink, switch};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

mod prs;

/// The page's width, in rems, below which the list and the issue no longer
/// fit side by side: the list is shown, then the issue alone.
const TWO_COLUMNS: f32 = 56.;

/// The list's width beside the issue, in rems.
const LIST_W: f32 = 24.;

/// How many labels a row draws before it counts the rest.
const ROW_LABELS: usize = 3;

/// How many labels the label filter offers.
const LABELS_SHOWN: usize = 100;

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
}

impl PageState {
    /// The filters changed: the list is drawn afresh, the pin let go.
    pub(super) fn refilter(&mut self) {
        self.held = Held::default();
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
        self.root = Some(root.clone());
        self.state_for(&root, cx);
        self.unless_drafting(window, cx, move |view, _, cx| {
            if let Some(page) = view.page.as_mut() {
                if let Some(key) = &key {
                    let stopped = std::mem::take(&mut page.stopped);
                    page.held.pick(key, &stopped);
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
                div().flex_none().px_2().pt_2().child(
                    Input::new(&query)
                        .small()
                        .prefix(Icon::new(IconName::Search).xsmall().text_color(muted))
                        .cleanable(true),
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

    /// The filters: open or closed, progress, project and label, and how
    /// old the pull request reading is.
    fn page_head(
        &self,
        listed: &Listed<'_>,
        filters: &Filters,
        projects: &[(PathBuf, SharedString)],
        labels: Vec<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity();
        let showing = switch(
            "issues-page-showing",
            &[
                SharedString::from(format!("Open {}", listed.open)),
                SharedString::from(format!("Closed {}", listed.closed)),
            ],
            usize::from(filters.closed),
            Size::XSmall,
            move |picked, _, cx| {
                let closed = *picked == 1;
                view.update(cx, |view, cx| view.filter(|f| f.closed = closed, cx))
            },
            cx,
        );
        let progress = {
            let picked = filters.progress;
            let view = cx.entity();
            menu_below(
                "issues-page-progress",
                trigger("issues-page-progress-trigger", picked.label()),
                move |mut menu, _, _| {
                    for choice in Progress::ALL {
                        let view = view.clone();
                        menu = menu.item(
                            menu_item(choice.label())
                                .checked(choice == picked)
                                .on_click(move |_, _, cx: &mut App| {
                                    view.update(cx, |view, cx| {
                                        view.filter(|f| f.progress = choice, cx)
                                    })
                                }),
                        );
                    }
                    menu
                },
            )
        };
        let project = {
            let picked = filters.project.clone();
            let name = picked
                .as_ref()
                .and_then(|only| projects.iter().find(|(root, _)| root == only))
                .map_or_else(
                    || "All projects".to_string(),
                    |(_, label)| label.to_string(),
                );
            let projects = projects.to_vec();
            let view = cx.entity();
            menu_below(
                "issues-page-project",
                trigger("issues-page-project-trigger", &name),
                move |menu, _, _| {
                    let pick = |only: Option<PathBuf>| {
                        let view = view.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            let only = only.clone();
                            view.update(cx, |view, cx| view.filter(|f| f.project = only, cx))
                        }
                    };
                    let mut menu = menu.item(
                        menu_item("All projects")
                            .checked(picked.is_none())
                            .on_click(pick(None)),
                    );
                    for (root, label) in &projects {
                        menu = menu.item(
                            menu_item(label.clone())
                                .checked(picked.as_ref() == Some(root))
                                .on_click(pick(Some(root.clone()))),
                        );
                    }
                    menu
                },
            )
        };
        let label = (!labels.is_empty()).then(|| {
            let picked = filters.label.clone();
            let left_out = labels.len().saturating_sub(LABELS_SHOWN);
            let view = cx.entity();
            menu_below(
                "issues-page-label",
                trigger(
                    "issues-page-label-trigger",
                    picked.as_deref().unwrap_or("Label"),
                ),
                move |menu, window, _| {
                    let pick = |label: Option<String>| {
                        let view = view.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            let label = label.clone();
                            view.update(cx, |view, cx| view.filter(|f| f.label = label, cx))
                        }
                    };
                    let tall = window.rem_size() * 20.;
                    let mut menu = menu.scrollable(true).max_h(tall).item(
                        menu_item("All labels")
                            .checked(picked.is_none())
                            .on_click(pick(None)),
                    );
                    for label in labels.iter().take(LABELS_SHOWN) {
                        menu = menu.item(
                            menu_item(label.clone())
                                .checked(picked.as_ref() == Some(label))
                                .on_click(pick(Some(label.clone()))),
                        );
                    }
                    if left_out > 0 {
                        menu = menu.label(format!("… {left_out} more labels not shown"));
                    }
                    menu
                },
            )
        });
        let read = self.prs_read_at().map(|at| {
            div()
                .flex_none()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "read {}",
                    onehand_core::rel_time(issues::now(), at)
                ))
        });
        div()
            .flex_none()
            .v_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(showing)
            .child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(progress)
                    .child(project)
                    .children(label)
                    .child(div().flex_1())
                    .children(read)
                    .child(
                        action("issues-page-refresh")
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Redo))
                            .tooltip("Read the pull requests again")
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.read_prs(cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// What the list cannot vouch for: the projects whose pull requests
    /// could not be read, and under *Pull request open* the issues not read.
    fn page_notices(
        &self,
        listed: &Listed<'_>,
        projects: &[(PathBuf, SharedString)],
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let warning = status_ink(cx).warning;
        let failed: Vec<String> = listed
            .failed
            .iter()
            .filter_map(|root| projects.iter().find(|(r, _)| r == root))
            .map(|(_, label)| label.to_string())
            .collect();
        if failed.is_empty() && listed.incomplete.is_none() {
            return None;
        }
        // A stale reading or a project whose read failed is read again
        // whole; otherwise the branches a capped read missed are looked up.
        let again = !listed.failed.is_empty();
        let incomplete = listed.incomplete.map(|Incomplete { not_read, stale }| {
            let said = match not_read {
                0 => "The reading is old; the list may be incomplete".to_string(),
                1 => "1 issue not read; the list may be incomplete".to_string(),
                n => format!("{n} issues not read; the list may be incomplete"),
            };
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .child(div().flex_1().min_w_0().text_color(warning).child(said))
                .child(
                    action("issues-page-read-them")
                        .xsmall()
                        .ghost()
                        .label("Read them")
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            if stale || again {
                                view.read_prs(cx);
                            } else {
                                view.read_missing(cx);
                            }
                            cx.notify();
                        })),
                )
        });
        Some(
            div()
                .flex_none()
                .v_flex()
                .gap_0p5()
                .px_2()
                .py_1()
                .text_xs()
                .when(!failed.is_empty(), |notes| {
                    notes.child(div().text_color(warning).child(format!(
                        "Pull requests could not be read for {}",
                        failed.join(", ")
                    )))
                })
                .children(incomplete)
                .into_any_element(),
        )
    }

    /// Every filter back to where it starts, which a pinned row offers.
    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(query) = self.query.clone() {
            query.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.filter(|f| *f = Filters::default(), cx);
    }
}

/// A filter's trigger.
fn trigger(id: &'static str, label: &str) -> gpui_component::button::Button {
    action(id)
        .xsmall()
        .ghost()
        .label(label.to_string())
        .icon(Icon::new(IconName::ChevronDown))
}

/// One row: the title, then the project and how the issue is named, its
/// line of work, and its labels.
fn page_row(
    i: usize,
    row: &Row<'_>,
    project: SharedString,
    picked: bool,
    cx: &mut Context<IssuesView>,
) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let warning = status_ink(cx).warning;
    let issue = row.item.issue;
    let (root, number) = (row.item.root.to_path_buf(), issue.number);
    let named = issue
        .reference()
        .map_or_else(|| "Draft".to_string(), str::to_string);
    let shown = issue
        .labels
        .iter()
        .take(ROW_LABELS)
        .cloned()
        .collect::<Vec<_>>();
    let more = issue.labels.len().saturating_sub(ROW_LABELS);
    let line = row.line.clone().map(|line| {
        div()
            .truncate()
            .text_color(if row.attention { warning } else { muted })
            .child(line)
    });
    div()
        .id(("issues-page-row", i))
        .v_flex()
        .gap_0p5()
        .w_full()
        .px_2()
        .py_1()
        .rounded(theme.radius)
        .cursor_pointer()
        .map(|div| {
            if picked {
                div.bg(theme.accent).text_color(theme.accent_foreground)
            } else {
                div.hover(|div| div.bg(theme.list_hover))
            }
        })
        .when(row.outside, |column| {
            column.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().child("Outside current filters"))
                    .child(
                        action(("issues-page-clear", i))
                            .xsmall()
                            .ghost()
                            .label("Clear filters")
                            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                                view.clear_filters(window, cx)
                            })),
                    ),
            )
        })
        .child(
            div()
                .text_sm()
                .font_semibold()
                .line_clamp(2)
                .when(!issue.open, |title| title.text_color(muted).line_through())
                .child(issue.title.clone()),
        )
        .child(
            div()
                .h_flex()
                .gap_1()
                .min_w_0()
                .text_xs()
                .text_color(muted)
                .child(div().flex_none().child(project))
                .child("·")
                .child(div().flex_none().child(named)),
        )
        .child(
            div()
                .h_flex()
                .gap_1()
                .min_w_0()
                .text_xs()
                .children(line)
                .when(row.earlier_attention, |line| {
                    line.child(
                        div()
                            .flex_none()
                            .text_color(warning)
                            .child("earlier task needs attention"),
                    )
                })
                .children(
                    row.left
                        .clone()
                        .map(|left| div().flex_none().text_color(muted).child(left)),
                ),
        )
        .when(!shown.is_empty(), |column| {
            column.child(
                div()
                    .h_flex()
                    .gap_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .children(shown.into_iter().map(|label| super::list::chip(label, cx)))
                    .when(more > 0, |labels| {
                        labels.child(div().text_color(muted).child(format!("+{more}")))
                    }),
            )
        })
        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
            view.pick(root.clone(), number, window, cx)
        }))
        .into_any_element()
}

/// The work of every issue of `root`, by the file the page keys them with.
pub(super) fn works_of<'a>(
    works: &'a [IssueWork],
    file: &'a Path,
) -> impl Iterator<Item = &'a IssueWork> {
    works.iter().filter(move |work| work.key.file == file)
}
