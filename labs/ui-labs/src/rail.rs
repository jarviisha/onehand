//! The rail: the workspace and its menu, a search over sessions and projects
//! beside *New*, the pages, the sessions under a filter, and a one-row foot
//! with the lab's own pages and Settings. Only the list scrolls. It hides
//! completely, never to an icon column.
use super::*;
use crate::controls::faded;
use gpui::{Anchor, Entity, FocusHandle, Focusable as _, Subscription};
use gpui_component::Disableable as _;
use gpui_component::button::DropdownButton;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::tooltip::Tooltip;
use model::{Filter, PROJECTS, Session, Status};

mod model;
mod rows;
mod tree;

#[cfg(test)]
mod tests;

/// Workspaces opened before: a name and the folder it lives in. The first is
/// the one on screen.
const WORKSPACES: [(&str, &str); 3] = [
    ("Audit workspace", "…/work/audit"),
    ("Atlas clients", "…/work/clients"),
    ("Scratch", "…/tmp/scratch"),
];

/// The agents a session can start with; the first is the default.
const AGENTS: [&str; 2] = ["claude", "codex"];

/// The row the keyboard is on in the list.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Cursor {
    Project(usize),
    Session(usize),
}

/// What the rail keeps between frames.
pub(super) struct Rail {
    sessions: Vec<Session>,
    filter: Filter,
    /// The filter the attention chip replaced, which its second click restores.
    filter_before: Filter,
    /// Projects whose sessions are hidden, by index into the projects.
    folded: Vec<usize>,
    /// Projects drawn first, in pinned order.
    pinned: Vec<usize>,
    /// The session the chat shows, by index into `sessions`.
    session: Option<usize>,
    cursor: Option<Cursor>,
    /// Groups the person showed in full past `SESSION_CAP`: a project, or
    /// `None` for the flat list.
    uncapped: Vec<Option<usize>>,
    search: Entity<InputState>,
    /// The list's own focus, so Enter and the arrows reach it and not a field.
    focus: FocusHandle,
    _search: Subscription,
}

impl Rail {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Labs>) -> Self {
        let sessions = model::seed();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search…"));
        let _search = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        // A project opens with the rail only when something in it waits on
        // the person; after that, folding is the person's.
        let folded = (0..PROJECTS.len())
            .filter(|p| !model::badge(&sessions, *p).is_some_and(Status::needs_attention))
            .collect();
        Self {
            sessions,
            filter: Filter::ByProject,
            filter_before: Filter::ByProject,
            folded,
            pinned: vec![0],
            session: Some(0),
            cursor: None,
            uncapped: Vec::new(),
            search,
            focus: cx.focus_handle(),
            _search,
        }
    }
}

impl Labs {
    pub(super) fn rail(
        &self,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focused = self.rail.focus.is_focused(window);
        v_flex()
            .w(rems(self.rail_w))
            .h_full()
            .flex_none()
            .bg(p.sunken)
            .child(self.workspace_row(p, cx))
            .child(
                h_flex()
                    .px_3()
                    .pt_1()
                    .pb_3()
                    .gap_1p5()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.rail.search)
                                .small()
                                .bordered(false)
                                .prefix(Icon::new(IconName::Search).small().text_color(p.muted))
                                .suffix(crate::composer::key("ctrl-k")),
                        ),
                    )
                    .child(self.new_button(cx)),
            )
            .child(
                v_flex()
                    .px_2()
                    .pb_3()
                    .gap_0p5()
                    .child(self.nav_row(
                        p,
                        Page::Overview,
                        "Overview",
                        IconName::LayoutDashboard,
                        None,
                        "What is waiting, working and open across every project",
                        cx,
                    ))
                    .child(self.nav_row(
                        p,
                        Page::Tasks,
                        "Tasks",
                        IconName::Inbox,
                        Some("2"),
                        "Every task, and what each needs",
                        cx,
                    ))
                    .child(self.nav_row(
                        p,
                        Page::Issues,
                        "Issues",
                        Icon::empty().path(crate::assets::CIRCLE_DOT),
                        None,
                        "Every issue of every project, and where its work stands",
                        cx,
                    ))
                    .child(self.nav_row(
                        p,
                        Page::Workflows,
                        "Workflows",
                        IconName::Play,
                        None,
                        "Every workflow, to run one or write one",
                        cx,
                    )),
            )
            .child(self.sessions_header(p, cx))
            .child(
                v_flex()
                    .id("rail-list")
                    .track_focus(&self.rail.focus)
                    .key_context("LabsRail")
                    .on_action(cx.listener(|this, _: &OpenCursor, _, cx| this.open_cursor(cx)))
                    .on_action(
                        cx.listener(|this, _: &FoldCursor, _, cx| this.fold_cursor(true, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &UnfoldCursor, _, cx| this.fold_cursor(false, cx)),
                    )
                    .flex_1()
                    .overflow_y_scroll()
                    .px_2()
                    .pt_2()
                    .pb_3()
                    .gap_0p5()
                    .children(self.list(p, focused, cx)),
            )
            .child(self.foot(p, cx))
    }

    /// A letter tile and the workspace's name, the switcher that opens the
    /// workspace menu, and *Hide the rail*.
    fn workspace_row(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (name, _) = WORKSPACES[0];
        let initial: String = name.chars().take(1).collect();
        let muted = p.muted;
        h_flex()
            .h(rems(BAR_H))
            .flex_none()
            .px_3()
            .gap_2()
            .child(
                div()
                    .size_5()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(cx.theme().radius)
                    .bg(p.chip_on)
                    .text_xs()
                    .font_medium()
                    .text_color(p.text)
                    .child(initial),
            )
            .child(
                faded(
                    "workspace-name",
                    name,
                    "workspace".into(),
                    p.sunken,
                    p.sunken,
                )
                .font_medium()
                .text_color(p.text)
                .tooltip(move |window, cx| Tooltip::new(name).build(window, cx)),
            )
            .child(
                action("workspace")
                    .ghost()
                    .small()
                    .icon(Icon::new(IconName::ChevronsUpDown).text_color(p.muted))
                    .tooltip("Workspaces, and the projects in this one")
                    .dropdown_menu(move |menu, _, _| {
                        let menu = WORKSPACES.iter().enumerate().fold(
                            menu.label("Workspaces"),
                            |menu, (i, (name, folder))| {
                                menu.item(
                                    PopupMenuItem::element(move |_, _| {
                                        v_flex()
                                            .child(*name)
                                            .child(div().text_xs().text_color(muted).child(*folder))
                                    })
                                    .checked(i == 0)
                                    .disabled(i == 0),
                                )
                            },
                        );
                        menu.separator()
                            .item(PopupMenuItem::new("Open workspace…").icon(IconName::FolderOpen))
                            .item(PopupMenuItem::new("New workspace…").icon(IconName::Plus))
                    }),
            )
            .child(
                self.icon_button("hide-rail", IconName::PanelLeftClose, "Hide the rail", cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rail_hidden = true;
                        this.hovered = None;
                        cx.notify();
                    })),
            )
    }

    /// A page row, marked while its page shows.
    #[allow(clippy::too_many_arguments)]
    fn nav_row(
        &self,
        p: &Palette,
        page: Page,
        label: &'static str,
        icon: impl Into<Icon>,
        count: Option<&'static str>,
        tip: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let on = self.page == page;
        h_flex()
            .id(label)
            .h(rems(ROW_H))
            .px_1p5()
            .gap_2()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .hover(|d| d.bg(p.selected))
            .when(on, |d| d.bg(p.selected).text_color(p.text))
            .when(!on, |d| d.text_color(p.text2))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                this.task_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .w(rems(DOT_COLUMN))
                    .flex()
                    .justify_center()
                    .child(icon.into().small()),
            )
            .child(div().flex_1().child(label))
            .children(count.map(|n| {
                div()
                    .px_1()
                    .rounded(cx.theme().radius)
                    .bg(p.selected)
                    .text_xs()
                    .text_color(p.text2)
                    .child(n)
            }))
    }

    /// *New* in the project the keyboard or the chat is on, with the default
    /// agent, and a caret choosing another project or agent. With no such
    /// project only the caret starts one.
    fn new_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let names = self.project_names();
        let target = self.target_project();
        let this = cx.entity().downgrade();
        let menu = move |menu, _: &mut Window, _: &mut Context<PopupMenu>| {
            start_menu(menu, &names, target, this.clone())
        };
        let new = action("new-session")
            .icon(IconName::Plus)
            .label("New")
            .tooltip(match target {
                Some(project) => format!(
                    "Start a session in {} with {}",
                    PROJECTS[project].name, AGENTS[0]
                ),
                None => "Choose a project from the caret".into(),
            })
            .disabled(target.is_none())
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(project) = target {
                    this.new_session(project, AGENTS[0], cx)
                }
            }));
        // Ghost, like the rail's other controls: no edge, a fill under the
        // pointer.
        DropdownButton::new("new-session-group")
            .ghost()
            .small()
            .button(new)
            .dropdown_menu_with_anchor(Anchor::TopRight, menu)
    }

    /// *Sessions*, how many wait on the person, and the filter.
    fn sessions_header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let waiting = model::attention_count(&self.rail.sessions);
        let filter = self.rail.filter;
        let attention = filter == Filter::NeedsAttention;
        let this = cx.entity().downgrade();
        h_flex()
            .flex_none()
            .px_3()
            .pt_1()
            .pb_1p5()
            .gap_2()
            // The line sits under the header, so it is also the edge the
            // list scrolls under.
            .border_b_1()
            .border_color(p.hairline)
            .text_xs()
            .child(div().font_medium().text_color(p.text2).child("Sessions"))
            // The count is also the switch for its filter; while the filter is
            // on it stays, even at none, so it can be switched off.
            .when(waiting > 0 || attention, |d| {
                let verb = if waiting == 1 { "needs" } else { "need" };
                d.child(
                    div()
                        .id("attention")
                        .px_1()
                        .rounded(cx.theme().radius)
                        .cursor_pointer()
                        .text_color(p.warning)
                        .hover(|d| d.bg(p.selected))
                        .when(attention, |d| d.bg(p.chip_on).font_medium())
                        .tooltip(move |window, cx| {
                            Tooltip::new(if attention {
                                "Show every session again"
                            } else {
                                "Show only sessions that need input or failed"
                            })
                            .build(window, cx)
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            let rail = &mut this.rail;
                            if rail.filter != Filter::NeedsAttention {
                                rail.filter_before = rail.filter;
                            }
                            rail.filter = model::toggle_attention(rail.filter, rail.filter_before);
                            cx.notify();
                        }))
                        .child(format!("{waiting} {verb} attention")),
                )
            })
            .child(div().flex_1())
            .child(rows::labelled(
                "rail-filter-name",
                "Filter sessions",
                action("rail-filter")
                    .ghost()
                    .xsmall()
                    .icon(Icon::empty().path(crate::assets::LIST_FILTER).text_color(
                        if filter == Filter::ByProject {
                            p.muted
                        } else {
                            p.text
                        },
                    ))
                    .tooltip(format!("Showing: {}", filter.label()))
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        Filter::ALL.iter().fold(menu, |menu, f| {
                            let (f, this) = (*f, this.clone());
                            menu.item(PopupMenuItem::new(f.label()).checked(f == filter).on_click(
                                move |_, _, cx| {
                                    this.update(cx, |labs, cx| {
                                        labs.rail.filter = f;
                                        if f != Filter::NeedsAttention {
                                            labs.rail.filter_before = f;
                                        }
                                        cx.notify();
                                    })
                                    .ok();
                                },
                            ))
                        })
                    }),
            ))
    }

    /// One row: the lab's own pages on the left, Settings on the right.
    fn foot(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let composer = self.page == Page::Composer;
        h_flex()
            .flex_none()
            .h(rems(BAR_H))
            .px_2()
            .border_t_1()
            .border_color(p.hairline)
            .child(
                action("labs-pages")
                    .ghost()
                    .small()
                    .label("Labs")
                    .text_color(p.muted)
                    .tooltip("The lab's own pages, which are not part of the proposal")
                    .dropdown_menu(move |menu, _, _| {
                        let this = this.clone();
                        menu.item(
                            PopupMenuItem::new("Composer cards")
                                .icon(IconName::Frame)
                                .checked(composer)
                                .on_click(move |_, _, cx| {
                                    this.update(cx, |labs, cx| {
                                        labs.page = Page::Composer;
                                        cx.notify();
                                    })
                                    .ok();
                                }),
                        )
                    }),
            )
            .child(div().flex_1())
            .child(
                self.icon_button(
                    "settings",
                    IconName::Settings,
                    "Appearance, the workspace, agents and connections",
                    cx,
                )
                .selected(self.page == Page::Settings)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.page = Page::Settings;
                    cx.notify();
                })),
            )
    }

    /// Where *New* starts: the keyboard's project, else the chat's.
    fn target_project(&self) -> Option<usize> {
        match self.rail.cursor {
            Some(Cursor::Project(p)) => Some(p),
            Some(Cursor::Session(s)) => Some(self.rail.sessions[s].project),
            None => self.rail.session.map(|s| self.rail.sessions[s].project),
        }
    }

    pub(super) fn new_session(
        &mut self,
        project: usize,
        agent: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.rail.sessions.push(Session {
            project,
            title: "New session",
            agent,
            status: Status::Idle,
            age: 0,
            diff: None,
        });
        self.set_folded(project, false);
        self.open_session(self.rail.sessions.len() - 1, cx);
    }

    fn open_session(&mut self, i: usize, cx: &mut Context<Self>) {
        self.rail.session = Some(i);
        self.rail.cursor = Some(Cursor::Session(i));
        self.page = Page::Chat;
        cx.notify();
    }

    /// What Stop and Retry do here: the session takes the status, from now.
    fn set_status(&mut self, i: usize, status: Status, cx: &mut Context<Self>) {
        let s = &mut self.rail.sessions[i];
        s.status = status;
        s.age = 0;
        cx.notify();
    }

    fn set_folded(&mut self, project: usize, fold: bool) {
        self.rail.folded.retain(|f| *f != project);
        if fold {
            self.rail.folded.push(project);
        }
    }

    /// Drops the sessions `gone` picks, keeping the selection and the cursor
    /// on the sessions they were on. A dropped selection leaves nothing
    /// chosen, and the chat it showed gives way to the overview.
    fn drop_sessions(&mut self, gone: impl Fn(usize, &Session) -> bool) {
        let keep: Vec<bool> = self
            .rail
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| !gone(i, s))
            .collect();
        let moved = |i: usize| keep[i].then(|| keep[..i].iter().filter(|k| **k).count());
        let mut at = 0;
        self.rail.sessions.retain(|_| {
            at += 1;
            keep[at - 1]
        });
        self.rail.session = self.rail.session.and_then(moved);
        if self.rail.session.is_none() && self.page == Page::Chat {
            self.page = Page::Overview;
        }
        self.rail.cursor = match self.rail.cursor {
            Some(Cursor::Session(s)) => moved(s).map(Cursor::Session),
            other => other,
        };
    }

    /// Archive and Close: the session leaves the list. The one on screen
    /// hands over to the row drawn below it, or above it at the end, so the
    /// chat never jumps to a row the person cannot see.
    pub(super) fn close_session(&mut self, i: usize, cx: &mut Context<Self>) {
        let order = self.order(cx);
        let next = order.iter().position(|s| *s == i).and_then(|at| {
            order
                .get(at + 1)
                .or_else(|| at.checked_sub(1).and_then(|b| order.get(b)))
                .copied()
        });
        let shown = self.rail.session == Some(i) && self.page == Page::Chat;
        let cursor_on = self.rail.cursor == Some(Cursor::Session(i));
        self.drop_sessions(|at, _| at == i);
        // Indices after the dropped one move up by one.
        let next = next.map(|n| if n > i { n - 1 } else { n });
        match next {
            Some(n) if shown => self.open_session(n, cx),
            Some(n) if cursor_on => self.rail.cursor = Some(Cursor::Session(n)),
            Some(_) | None => {}
        }
        cx.notify();
    }

    /// The project leaves the workspace, and its sessions with it.
    pub(super) fn remove_project(&mut self, name: &'static str) {
        self.removed.push(name);
        self.drop_sessions(|_, s| PROJECTS[s.project].name == name);
        if let Some(Cursor::Project(p)) = self.rail.cursor
            && PROJECTS[p].name == name
        {
            self.rail.cursor = None;
        }
    }

    pub(super) fn focus_rail_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rail.search.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(super) fn new_session_here(&mut self, cx: &mut Context<Self>) {
        if let Some(project) = self.target_project().or_else(|| self.projects().next()) {
            self.new_session(project, AGENTS[0], cx);
        }
    }

    /// Opens the session before or after the current one in the list's order.
    pub(super) fn step_session(&mut self, forward: bool, cx: &mut Context<Self>) {
        let order = self.order(cx);
        let from = match self.rail.cursor {
            Some(Cursor::Session(s)) => Some(s),
            _ => self.rail.session,
        };
        let next = match from.and_then(|s| order.iter().position(|o| *o == s)) {
            Some(at) if forward => order.get(at + 1).or(order.last()),
            Some(at) => order.get(at.saturating_sub(1)),
            None => order.first(),
        };
        if let Some(next) = next.copied() {
            self.open_session(next, cx);
        }
    }

    fn open_cursor(&mut self, cx: &mut Context<Self>) {
        match self.rail.cursor {
            Some(Cursor::Session(s)) => self.open_session(s, cx),
            Some(Cursor::Project(p)) => {
                let fold = !self.rail.folded.contains(&p);
                self.fold(p, fold, cx);
            }
            None => {}
        }
    }

    /// Folds or unfolds the cursor's project; folding from a session moves
    /// the cursor up to its project, so it stays on a row that is drawn.
    fn fold_cursor(&mut self, fold: bool, cx: &mut Context<Self>) {
        let project = match self.rail.cursor {
            Some(Cursor::Project(p)) => p,
            Some(Cursor::Session(s)) => self.rail.sessions[s].project,
            None => return,
        };
        self.fold(project, fold, cx);
        if fold {
            self.rail.cursor = Some(Cursor::Project(project));
        }
    }

    /// The person folding a project. While a search is on every project
    /// stays open, so a fold would change nothing on screen and then land
    /// unasked when the search clears; it waits instead.
    pub(super) fn fold(&mut self, project: usize, fold: bool, cx: &mut Context<Self>) {
        if !self.searching(cx) {
            self.set_folded(project, fold);
        }
        cx.notify();
    }
}

/// The caret's menu: which project, then which agent. Each starts a session.
fn start_menu(
    menu: PopupMenu,
    projects: &[(usize, &'static str)],
    current: Option<usize>,
    this: gpui::WeakEntity<Labs>,
) -> PopupMenu {
    let start = move |this: gpui::WeakEntity<Labs>, project: usize, agent: &'static str| {
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            this.update(cx, |labs, cx| labs.new_session(project, agent, cx))
                .ok();
        }
    };
    let menu = projects
        .iter()
        .fold(menu.label("Start in"), |menu, (i, name)| {
            menu.item(
                PopupMenuItem::new(*name)
                    .icon(IconName::Folder)
                    .checked(current == Some(*i))
                    .on_click(start(this.clone(), *i, AGENTS[0])),
            )
        });
    let Some(project) = current.or(projects.first().map(|(i, _)| *i)) else {
        return menu;
    };
    AGENTS
        .iter()
        .fold(menu.separator().label("With agent"), |menu, agent| {
            menu.item(
                PopupMenuItem::new(*agent)
                    .icon(IconName::Bot)
                    .on_click(start(this.clone(), project, agent)),
            )
        })
}

/// A tooltip that names the thing, then says more under it.
fn two_lines(
    title: impl Into<SharedString>,
    more: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyView {
    let (title, more) = (title.into(), more.into());
    Tooltip::element(move |_, _| {
        v_flex()
            .child(title.clone())
            .child(div().text_xs().child(more.clone()))
    })
    .build(window, cx)
}
