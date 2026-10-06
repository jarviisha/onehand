use super::project_page::count_of;
use super::{ChatPane, ChatPaneEvent, Page, SessionSignal, rel_time};
use gpui::{
    App, Context, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::ConvMeta;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// How many rows each of the workspace page's two activity groups draws: the
/// sessions and runs waiting on the user, and those working. Each holds an
/// agent open, so these are few by construction and this is a backstop rather
/// than an editorial cut.
const PAGE_ACTIVE: usize = 8;

/// How many projects the workspace page lists, one row each.
const PAGE_PROJECTS: usize = 20;

/// How many past conversations the workspace page offers to pick back up:
/// the newest few across every project, for "where was I".
const PAGE_RECENT: usize = 5;

/// How many open issues the workspace page lists. Enough to read down, not so
/// many that a workspace of busy projects builds a thousand rows nobody reaches.
const PAGE_ISSUES: usize = 100;

/// The width of the column the pages drawn in place of a conversation keep to:
/// the project page, the resume picker and the workspace page. One figure, so
/// moving between them does not move the column. In rems, so it follows the
/// pane's zoom with the text inside it.
pub(super) const PAGE_COLUMN: f32 = 35.;

/// The widest the workspace page grows, in rems: room for two cards side by
/// side and a few project tiles to a row, without lines too long to read.
pub(super) const PAGE_WIDE: f32 = 64.;

/// The width a workspace page card starts from before it grows into its row,
/// in rems. Two fit side by side within the page, and a narrower panel wraps
/// them one under the other.
const PAGE_CARD: f32 = 22.;

/// The width a project tile starts from, in rems.
const PAGE_TILE: f32 = 14.;

/// The widest a project tile grows into its row, in rems, so a short last row
/// does not stretch one tile across the page.
const PAGE_TILE_MAX: f32 = 21.;

/// How tall the open-issues card's list grows before it scrolls, in rems.
const PAGE_ISSUES_H: f32 = 24.;

/// The height of a workspace page card's title row, in rems.
const PAGE_TITLE_H: f32 = 1.75;

/// What a project filter reads while it is not narrowing the list at all:
/// the workspace page's issues, or the Tasks page.
pub(super) const ALL_PROJECTS: &str = "All projects";

/// One project as the workspace page lists it.
pub struct PageProject {
    pub label: SharedString,
    pub root: PathBuf,
    /// Its sessions, each with what to call it before its conversation has a
    /// name of its own.
    pub sessions: Vec<(u64, SharedString)>,
    /// Its branch and how much is changed, as the last `git status` sweep
    /// found them — `None` for a folder that is not a repository.
    pub git: Option<String>,
    /// The file its issues are kept in — `None` for a workspace bound to no
    /// storage, which keeps none.
    pub issues: Option<PathBuf>,
}

/// The page shown in place of a conversation when the user asks what the whole
/// workspace needs: the runs waiting on them, the runs working, and every
/// project's open issues.
pub(super) struct WorkspacePage {
    /// Every project in rail order.
    projects: Vec<PageProject>,
    /// The project the issue list is narrowed to, or `None` for all of them.
    filter: Option<PathBuf>,
    /// Every project's issues as last read. `None` while the first read is
    /// out, which is a wait rather than an answer.
    read: Option<Vec<(PathBuf, onehand_core::issues::Issues)>>,
    /// The files that could not be read, and why.
    failed: Vec<String>,
    /// What the list draws: `read` narrowed to the filter.
    shown: onehand_core::issues::Across,
    /// Every project's past conversations, newest first, with the project each
    /// belongs to. `None` while the read is out.
    recent: Option<Vec<(PathBuf, ConvMeta)>>,
    /// The read in flight, held so that a newer one drops it.
    _load: Option<gpui::Task<()>>,
}

impl WorkspacePage {
    /// Narrow what was read to the filter. Run when either changes, not per
    /// frame, since it copies every open issue it keeps.
    fn refilter(&mut self) {
        let Some(read) = &self.read else {
            return;
        };
        let kept = read
            .iter()
            .filter(|(root, _)| self.filter.as_ref().is_none_or(|only| only == root))
            .cloned()
            .collect();
        self.shown = onehand_core::issues::open_across(kept, PAGE_ISSUES);
    }

    /// What `root` is called here, or its folder's name for a project this
    /// window does not hold — a run's project can be in another window.
    fn label_of(&self, root: &Path) -> String {
        self.projects
            .iter()
            .find(|project| project.root == root)
            .map(|project| project.label.to_string())
            .or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default()
    }
}

impl ChatPane {
    /// Show the workspace page: what is waiting, what is working, and every
    /// project's open issues. `projects` is every project in rail order, with
    /// the file its issues are kept in.
    ///
    /// Leaves the shown session the way standing on a project does — the draft
    /// put down, the session left running — since the page is somewhere else to
    /// be and not a session closing.
    pub fn show_workspace(
        &mut self,
        projects: Vec<PageProject>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A filter survives the page being shown again, while its project does.
        let filter = match self.page.take() {
            Some(Page::Workspace(page)) => page.filter,
            Some(Page::Tasks(_) | Page::Issues(_)) | None => None,
        }
        .filter(|only| projects.iter().any(|project| &project.root == only));
        self.page = Some(Page::Workspace(WorkspacePage {
            projects,
            filter,
            read: None,
            failed: Vec::new(),
            shown: Default::default(),
            recent: None,
            _load: None,
        }));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        self.reload_workspace(cx);
        cx.notify();
    }

    /// Read every project's issues again, if the workspace page is showing.
    /// What is on screen stays until the new read lands, so a reload does not
    /// blank a list that already had something in it.
    pub fn reload_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(Page::Workspace(page)) = self.page.as_mut() else {
            return;
        };
        let files: Vec<(PathBuf, PathBuf)> = page
            .projects
            .iter()
            .filter_map(|project| Some((project.root.clone(), project.issues.clone()?)))
            .collect();
        let roots: Vec<PathBuf> = page.projects.iter().map(|p| p.root.clone()).collect();
        page._load = Some(cx.spawn(async move |pane, cx| {
            let (read, failed, recent) = cx
                .background_executor()
                .spawn(async move {
                    let (mut read, mut failed) = (Vec::new(), Vec::new());
                    for (root, file) in files {
                        match onehand_core::issues::load_blocking(&file) {
                            Ok(issues) => read.push((root, issues)),
                            Err(why) => failed.push(why),
                        }
                    }
                    let recent = onehand_core::chat::list_across(
                        &onehand_core::chat::conversations_dir(),
                        &roots,
                    );
                    (read, failed, recent)
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                let Some(Page::Workspace(page)) = pane.page.as_mut() else {
                    return;
                };
                page.read = Some(read);
                page.failed = failed;
                page.recent = Some(recent);
                page.refilter();
                cx.notify();
            });
        }));
    }

    /// Replace the page's projects with a fresher listing, keeping
    /// everything it has read. A filter on a project no longer listed goes.
    pub fn set_page_projects(&mut self, projects: Vec<PageProject>, cx: &mut Context<Self>) {
        let gone = |only: &Option<PathBuf>| {
            only.as_ref()
                .is_some_and(|only| !projects.iter().any(|project| &project.root == only))
        };
        match self.page.as_mut() {
            Some(Page::Workspace(page)) => {
                if gone(&page.filter) {
                    page.filter = None;
                    page.refilter();
                }
                page.projects = projects;
            }
            Some(Page::Tasks(page)) => {
                if gone(&page.filter) {
                    page.filter = None;
                }
                page.projects = projects;
            }
            Some(Page::Issues(_)) | None => return,
        }
        cx.notify();
    }

    /// Whether the workspace page is what the pane shows.
    pub fn showing_workspace(&self) -> bool {
        match self.page {
            Some(Page::Workspace(_)) => true,
            Some(Page::Tasks(_) | Page::Issues(_)) | None => false,
        }
    }

    /// Whether the Tasks page is what the pane shows.
    pub fn showing_tasks(&self) -> bool {
        match self.page {
            Some(Page::Tasks(_)) => true,
            Some(Page::Workspace(_) | Page::Issues(_)) | None => false,
        }
    }

    /// Whether the Issues page is what the pane shows.
    pub fn showing_issues(&self) -> bool {
        match self.page {
            Some(Page::Issues(_)) => true,
            Some(Page::Workspace(_) | Page::Tasks(_)) | None => false,
        }
    }

    /// Show the Issues page, `view`, as it was left. Leaves the shown session
    /// the way the other pages do.
    pub fn show_issues(
        &mut self,
        view: gpui::AnyView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page = Some(Page::Issues(view));
        self.leave_shown_session(window, cx);
        self.active = None;
        self.empty = None;
        cx.notify();
    }

    /// Take the caret back before the Issues page leaves the frame: its
    /// search may hold it, and a caret in nothing answers no key.
    pub fn leave_issues_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.showing_issues() {
            self.focus_handle.clone().focus(window, cx);
        }
    }

    /// Whether a page about more than one project is what the pane shows.
    pub fn showing_page(&self) -> bool {
        self.page.is_some()
    }

    /// Narrow the workspace page's issue list to one project, or `None` for all.
    fn filter_workspace(&mut self, only: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(Page::Workspace(page)) = self.page.as_mut() {
            page.filter = only;
            page.refilter();
            cx.notify();
        }
    }

    /// The workspace page: what waits on the user and what is working, across
    /// sessions and unattended runs alike, then every project in a row, the
    /// newest past conversations, and every project's open issues.
    ///
    /// Sessions and runs are read here, per frame, rather than held: a run
    /// starting, parking or ending already refreshes every window, and a
    /// session's signal is the same query the rail draws its dots from.
    pub(super) fn workspace_page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(Page::Workspace(page)) = self.page.as_ref() else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        let warning = crate::theme::status_ink(cx).warning;
        let muted_line = |text: String| div().text_xs().text_color(muted).child(text);
        let here = self.window;
        let now = onehand_core::chat::now_secs();

        // The two activity cards gather runs and sessions alike. A run's row is
        // an issue's row: its number, what it is about, and the project at the
        // end. For a waiting run what it is about is the question, since
        // answering that is what the row is pressed for.
        let (mut waiting, mut working) = (Vec::new(), Vec::new());
        for run in crate::task::live_issues(cx) {
            let (uid, window) = (run.uid, run.window);
            let waits = run.waiting.is_some();
            let row = crate::dialogs::issue_row(
                ("workspace-run", uid as usize),
                run.name,
                run.waiting.unwrap_or(run.title),
                &[],
                page.label_of(&run.repo),
                cx,
            )
            .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                cx.emit(ChatPaneEvent::ShowSession { uid, window });
            }))
            .into_any_element();
            match waits {
                true => waiting.push(row),
                false => working.push(row),
            }
        }
        // A session's row leads with the rail's mark, so the same dot means the
        // same thing in both places, and names the state in words at the end.
        // An idle session that has been read carries no signal and no row.
        for project in &page.projects {
            for (uid, fallback) in &project.sessions {
                let uid = *uid;
                let Some(signal) = self.signal(uid, cx) else {
                    continue;
                };
                let title = self
                    .title_for(uid, cx)
                    .unwrap_or_else(|| fallback.to_string());
                let row = crate::dialogs::page_row(
                    ("workspace-session", uid as usize),
                    crate::rail::signal_mark(signal, cx),
                    title,
                    format!("{} · {}", project.label, crate::rail::signal_word(signal)),
                    cx,
                )
                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                    cx.emit(ChatPaneEvent::ShowSession { uid, window: here });
                }))
                .into_any_element();
                match signal {
                    SessionSignal::Busy => working.push(row),
                    SessionSignal::Lost
                    | SessionSignal::AwaitingUser
                    | SessionSignal::UnseenTurn => waiting.push(row),
                }
            }
        }
        // A list card says what it holds when it holds nothing, rather than
        // leaving a gap: the activity cards stay put whether or not anything is
        // running, so the page keeps one shape from one look to the next.
        let list = |rows: Vec<gpui::AnyElement>, cap: usize, cut: &str, empty: &str| {
            let hidden = rows.len().saturating_sub(cap);
            let empty = rows.is_empty().then(|| muted_line(empty.to_string()));
            div()
                .v_flex()
                .children(empty)
                .children(rows.into_iter().take(cap))
                .children((hidden > 0).then(|| muted_line(format!("{hidden} {cut} not shown"))))
        };
        let (waiting_n, working_n) = (waiting.len(), working.len());
        let waiting = page_card("Waiting on you", Some(waiting_n), None, cx).child(list(
            waiting,
            PAGE_ACTIVE,
            "more",
            "Nothing is waiting on you.",
        ));
        let working = page_card("Working", Some(working_n), None, cx).child(list(
            working,
            PAGE_ACTIVE,
            "more",
            "Nothing is running.",
        ));

        // A tile per project: its name and the most urgent mark its sessions
        // carry, as on its rail row, then its branch, then what it holds.
        let tiles: Vec<gpui::AnyElement> = page
            .projects
            .iter()
            .take(PAGE_PROJECTS)
            .enumerate()
            .map(|(i, project)| {
                // `None` while the issues are still being read, which is a
                // wait and not an answer of none.
                let open = page.read.as_ref().map(|read| {
                    read.iter()
                        .find(|(root, _)| *root == project.root)
                        .map_or(0, |(_, issues)| {
                            issues.listed().iter().filter(|issue| issue.open).count()
                        })
                });
                let sessions = project.sessions.len();
                let holds: Vec<String> = [
                    (sessions > 0).then(|| count_of(sessions, "session")),
                    open.filter(|n| *n > 0).map(|n| count_of(n, "open issue")),
                ]
                .into_iter()
                .flatten()
                .collect();
                let holds = match (holds.is_empty(), open) {
                    (false, _) => holds.join(" · "),
                    (true, None) => "Looking for open issues…".to_string(),
                    (true, Some(_)) => "Nothing open".to_string(),
                };
                let rollup = SessionSignal::most_urgent(
                    project
                        .sessions
                        .iter()
                        .filter_map(|(uid, _)| self.signal(*uid, cx)),
                );
                let root = project.root.clone();
                card_box(cx)
                    .id(("workspace-project", i))
                    .flex_grow_1()
                    .flex_basis(rems(PAGE_TILE))
                    .max_w(rems(PAGE_TILE_MAX))
                    .gap_1()
                    .cursor_pointer()
                    .hover(|tile| tile.bg(cx.theme().list_hover))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(IconName::Folder).size_4().text_color(muted))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_semibold()
                                    .child(project.label.clone()),
                            )
                            .children(rollup.map(|signal| crate::rail::signal_mark(signal, cx))),
                    )
                    .child(muted_line(
                        project
                            .git
                            .clone()
                            .unwrap_or_else(|| "Not a git repository".to_string()),
                    ))
                    .child(muted_line(holds))
                    .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                        cx.emit(ChatPaneEvent::ShowProject(root.clone()));
                    }))
                    .into_any_element()
            })
            .collect();
        let hidden_projects = page.projects.len().saturating_sub(PAGE_PROJECTS);
        let projects = (!tiles.is_empty()).then(|| {
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .child(card_title("Projects", Some(page.projects.len()), None, cx))
                .child(div().h_flex().flex_wrap().gap_3().w_full().children(tiles))
                .children(
                    (hidden_projects > 0)
                        .then(|| muted_line(format!("{hidden_projects} more not shown"))),
                )
        });

        // The past conversations to pick back up. One already open in a session
        // is left out: it is on the rail, and resuming it again would put the
        // same conversation in two sessions.
        let open: HashSet<String> = self
            .conversations
            .values()
            .filter_map(|conv| conv.session()?.read(cx).chat.session_id.clone())
            .collect();
        let recent: Vec<gpui::AnyElement> = page
            .recent
            .iter()
            .flatten()
            .filter(|(_, meta)| !open.contains(&meta.session_id))
            .enumerate()
            .map(|(i, (root, meta))| {
                let (root, agent, archive) = (
                    root.clone(),
                    SharedString::from(meta.agent.clone()),
                    meta.dir.clone(),
                );
                crate::dialogs::page_row(
                    ("workspace-recent", i),
                    rel_time(now, meta.updated),
                    meta.title.clone(),
                    format!("{} · {}", page.label_of(&root), meta.agent),
                    cx,
                )
                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                    cx.emit(ChatPaneEvent::ResumeIn {
                        root: root.clone(),
                        agent: agent.clone(),
                        archive: archive.clone(),
                    });
                }))
                .into_any_element()
            })
            .collect();
        let recent = page_card("Recent conversations", None, None, cx).child(match page.recent {
            None => div().child(muted_line("Looking for past conversations…".to_string())),
            Some(_) => list(
                recent,
                PAGE_RECENT,
                "older",
                "No past conversations to pick up.",
            ),
        });

        let unbound = !page.projects.is_empty()
            && page.projects.iter().all(|project| project.issues.is_none());
        let filter_name = page
            .filter
            .as_deref()
            .map_or_else(|| ALL_PROJECTS.to_string(), |root| page.label_of(root));
        let choices: Vec<(SharedString, Option<PathBuf>)> =
            std::iter::once((ALL_PROJECTS.into(), None))
                .chain(
                    page.projects
                        .iter()
                        .map(|project| (project.label.clone(), Some(project.root.clone()))),
                )
                .collect();
        let current = page.filter.clone();
        let this = cx.entity();
        let filter = crate::controls::menu_below(
            "workspace-filter",
            crate::controls::action("workspace-filter-trigger")
                .ghost()
                .small()
                .label(filter_name)
                .icon(Icon::new(IconName::ChevronDown))
                .text_color(muted),
            move |mut menu, _, _| {
                for (label, only) in &choices {
                    let (only, this) = (only.clone(), this.clone());
                    menu = menu.item(
                        crate::controls::menu_item(label.clone())
                            .checked(only == current)
                            .on_click(move |_, _, cx: &mut App| {
                                this.update(cx, |pane: &mut Self, cx| {
                                    pane.filter_workspace(only.clone(), cx)
                                });
                            }),
                    );
                }
                menu
            },
        );
        let note = match &page.read {
            _ if unbound => {
                Some("This workspace is bound to no folder, so it keeps no issues.".to_string())
            }
            None => Some("Looking for open issues…".to_string()),
            Some(_) if page.shown.rows.is_empty() => Some("No open issues.".to_string()),
            Some(_) => None,
        };
        let rows: Vec<gpui::AnyElement> = page
            .shown
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let (root, number) = (row.root.clone(), row.number);
                let trailing = page.label_of(&row.root);
                crate::dialogs::issue_row(
                    ("workspace-issue", i),
                    row.reference.clone().unwrap_or_else(|| "Draft".to_string()),
                    row.title.clone(),
                    &row.labels,
                    trailing,
                    cx,
                )
                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                    cx.emit(ChatPaneEvent::OpenIssue {
                        root: root.clone(),
                        number,
                    });
                }))
                .into_any_element()
            })
            .collect();
        let (closed, left_out) = (page.shown.closed, page.shown.left_out);

        let issues = page_card(
            "Open issues",
            page.read.as_ref().map(|_| page.shown.rows.len() + left_out),
            (!unbound).then(|| filter.into_any_element()),
            cx,
        )
        .children(
            page.failed
                .iter()
                .map(|why| div().text_xs().text_color(warning).child(why.clone())),
        )
        .children(note.map(muted_line))
        .children((!rows.is_empty()).then(|| {
            // The one card that grows long, so the one that scrolls inside
            // itself: the cards beside and above it keep their place.
            div()
                .id("workspace-issues")
                .v_flex()
                .max_h(rems(PAGE_ISSUES_H))
                .overflow_y_scroll()
                .children(rows)
        }))
        // Said, not hidden: a list cut silently reads as the whole of it.
        .children((left_out > 0).then(|| muted_line(format!("{left_out} more not shown"))))
        .children((closed > 0 && !unbound).then(|| {
            muted_line(match closed {
                1 => "1 closed issue is not listed.".to_string(),
                n => format!("{n} closed issues are not listed."),
            })
        }));
        // Two cards to a row where the panel is wide enough, one under the
        // other where it is not.
        let pair = |left: gpui::Div, right: gpui::Div| {
            div()
                .h_flex()
                .flex_wrap()
                .items_start()
                .gap_4()
                .w_full()
                .child(left.flex_grow_1().flex_basis(rems(PAGE_CARD)))
                .child(right.flex_grow_1().flex_basis(rems(PAGE_CARD)))
        };

        div()
            .size_full()
            .v_flex()
            // The header stays for the way back to a hidden rail, and names the
            // page; the dock controls on it are left off here.
            .child(self.header(cx))
            .child(
                // **The page scrolls as one, from the top.** Cards that fill in
                // as their reads land would move a centred page every time one
                // arrived, and a single scroll keeps every card reachable however
                // tall the page grows.
                div()
                    .id("workspace-page")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        div().h_flex().w_full().justify_center().child(
                            div()
                                .v_flex()
                                .gap_6()
                                .w_full()
                                .max_w(rems(PAGE_WIDE))
                                .text_sm()
                                .child(pair(waiting, working))
                                .children(projects)
                                .child(pair(recent, issues)),
                        ),
                    ),
            )
            .into_any_element()
    }
}

/// A card on the workspace page or the Tasks page: a bordered box, its title in bold with a
/// muted count beside it, an optional control at the far end, and whatever it
/// holds below.
pub(super) fn page_card(
    title: impl Into<SharedString>,
    count: Option<usize>,
    control: Option<gpui::AnyElement>,
    cx: &App,
) -> Div {
    card_box(cx).child(card_title(title, count, control, cx))
}

/// The hairline box every card on the workspace page and the Tasks page is
/// drawn in, a project tile and a task's row included.
pub(super) fn card_box(cx: &App) -> Div {
    div()
        .v_flex()
        .gap_2()
        .min_w_0()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
}

/// A workspace page heading: the title in bold, a muted count beside it and an
/// optional control at the far end.
fn card_title(
    title: impl Into<SharedString>,
    count: Option<usize>,
    control: Option<gpui::AnyElement>,
    cx: &App,
) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .h_flex()
        .items_center()
        .gap_2()
        // The control's own height must not change the title row's, or a card
        // with a filter sits taller than the one beside it.
        .h(rems(PAGE_TITLE_H))
        .child(div().font_semibold().child(title.into()))
        .children(count.map(|n| div().text_xs().text_color(muted).child(n.to_string())))
        .child(div().flex_1())
        .children(control)
}
