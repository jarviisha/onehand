//! The navigation rail.
//!
//! gpui-component's `Sidebar`. The rail is
//! **session-first**: every folder row lists its root's sessions underneath, and
//! clicking a session row selects root *and* session in one touch.
//!
//! What each row *shows* is decided in `onehand-core`, not here:
//!
//! - folder row — label, plus the root's branch and change count when it is a
//!   git repo (`onehand_core::gitstat`). A `SidebarMenuItem` is a fixed-height
//!   single row, so these ride in its suffix rather than on a second line,
//!   under a width cap that keeps the label first;
//! - session row — the conversation's own name once it has one, and a trailing
//!   mark **only** while the session carries a signal. Each of the four signals
//!   has a shape of its own, not a tint of one shared dot, and names itself in
//!   a tooltip. A calmly-ready session is a clean text row.

use crate::chat::pane::SessionSignal;
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, App, ClickEvent, Context, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, WeakEntity, Window,
    div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarMenu, SidebarMenuItem,
};
use gpui_component::spinner::Spinner;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Side, Sizable as _, StyledExt};
use onehand_core::agent::Session;

/// Project labels are structural anchors, not content: cap them so a deep path
/// cannot push the rail's width around. `SidebarMenuItem` clips its label with
/// `overflow_x_hidden` and no ellipsis, so this is what produces the `…`.
const MAX_LABEL: usize = 24;

/// The branch name is the *least* important thing on a folder row -- it must
/// never cost the project label its space. `SidebarMenuItem` gives the label
/// `flex_1` and the suffix its natural width, so an unbounded branch wins
/// outright: a row for `fix/architecture-hardening-and-open-telemetry` pushed
/// its own project name to zero width. Capping the branch is what keeps the
/// label first.
const MAX_BRANCH_W: gpui::Pixels = px(72.);

/// The agent's name beside a titled session row is a footnote about *how* the
/// conversation is being run, so it is capped hard on both counts -- the title
/// is what the user is reading the row for.
const MAX_AGENT_LABEL: usize = 12;
const MAX_AGENT_W: gpui::Pixels = px(64.);

fn ellipsize(s: &str, max: usize) -> SharedString {
    if s.chars().count() <= max {
        return SharedString::from(s.to_string());
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    SharedString::from(format!("{kept}…"))
}

/// One row on the rail's grid: a 16px icon column, then a label that truncates.
///
/// **Every** row the rail draws sits on this column -- the workspace identity,
/// the primary action, project and session rows (`SidebarMenuItem` uses the same
/// icon-then-label shape), and the footer's dialog triggers. A control that
/// centres its content instead breaks the column and reads as belonging to some
/// other surface, which is what a full-width [`gpui_component::button::Button`]
/// does: its inner content row hard-codes `justify_center` and is not
/// style-refinable from outside, so `.w_full()` on a Button can only ever
/// produce a centred banner.
///
/// Ghost, and that includes the primary action. A rail is chrome the
/// conversation sits in front of, and a filled row is the loudest thing that
/// can happen in one -- the fill *New session* used to carry made the panel's
/// quietest job, getting out of the way, impossible for the one control the eye
/// lands on first. What marks it as the primary action is its place at the top
/// of the rail and the tooltip naming the project it would start in.
///
/// Carries its own hover, so no caller may add a second one: `hover` panics in
/// debug when it is set twice.
pub(crate) fn rail_row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    cx: &App,
) -> Stateful<Div> {
    let radius = cx.theme().radius;
    // Resolved up front: the hover closure outlives this borrow of `cx`.
    let (accent, accent_fg) = (
        cx.theme().sidebar_accent,
        cx.theme().sidebar_accent_foreground,
    );
    div()
        .id(id)
        .hover(move |row| row.bg(accent.opacity(0.8)).text_color(accent_fg))
        .h_flex()
        .items_center()
        .w_full()
        .h_7()
        .gap_x_2()
        .px_2()
        .rounded(radius)
        .cursor_pointer()
        .text_sm()
        .child(Icon::new(icon).size_4())
        .child(div().flex_1().min_w_0().truncate().child(label))
}

/// A control the rail draws: ghost, extra small, icon-only.
///
/// The pointer is not set here any more. It used to be, because the library
/// draws its buttons with the arrow cursor and every other thing in the rail
/// that does something shows a pointer — but that was true of every button in
/// the app, and the fix belongs where all of them are built rather than in the
/// one place somebody noticed it.
fn rail_control(id: impl Into<ElementId>, icon: IconName) -> Button {
    crate::controls::action(id)
        .ghost()
        .xsmall()
        .icon(Icon::new(icon))
}

/// What a signal says, in words.
///
/// Every mark on the rail has one, and that is the point: a mark is a code, and
/// a code has to be learned before it can be read. The tooltip is the only form
/// of the signal that needs no learning at all — and the only one that works
/// for a reader who does not separate red from green.
///
/// Pure and separate because it is the rule rather than the rendering.
fn signal_hint(signal: SessionSignal) -> &'static str {
    match signal {
        SessionSignal::Lost => "The agent went away — Ctrl+Shift+R restarts it",
        SessionSignal::AwaitingUser => "Waiting for you",
        SessionSignal::Busy => "Working",
        SessionSignal::UnseenTurn => "Finished while you were away",
    }
}

/// The mark a signal draws on a row.
///
/// **Four states, four shapes** — not four tints of one dot. Colour alone was
/// the whole code before, which meant the rail could only be read by someone
/// who had already learned it, and could not be read at all by someone who does
/// not separate red from green: a busy session and a failed one were the same
/// small circle. Now the shape carries the meaning and the colour reinforces
/// it, and the tooltip says it outright.
///
/// **Nothing at all** for a session that is connected, idle and already read.
/// That is the case that makes the other four legible — a rail where every row
/// is marked is a rail where no mark means anything.
///
/// The tints follow the transcript's conventions, so the same colour means the
/// same thing wherever it appears.
///
/// Shared with the status bar, which says the same thing about the session on
/// screen: a spinner on a rail row and a dot in the bar for one condition would
/// be a code with two spellings, and only one of them ever learned.
pub(crate) fn signal_mark(signal: SessionSignal, cx: &App) -> impl IntoElement + use<> {
    let hint = signal_hint(signal);
    let theme = cx.theme();
    // Accent, not warning: a parked question means the agent is not in trouble,
    // it is waiting for the user. This is the one mark that means "go and do
    // something".
    let status = crate::theme::status_ink(cx);
    let (danger, warning, primary, success) =
        (status.danger, status.warning, theme.primary, status.success);

    // `flex_none`: the mark is the whole reason the row is worth looking at, so
    // it is the last thing any width may take.
    div()
        .id("signal")
        .flex_none()
        .h_flex()
        .items_center()
        .map(|mark| match signal {
            // The one state that *moves*, because it is the one state that
            // resolves on its own. Motion says "still going" with no colour and
            // no word.
            SessionSignal::Busy => mark.child(Spinner::new().xsmall().color(warning)),
            // The shape this app already uses for "something is wrong".
            SessionSignal::Lost => mark.child(
                Icon::new(IconName::TriangleAlert)
                    .size_3()
                    .text_color(danger),
            ),
            SessionSignal::AwaitingUser => mark.child(dot(primary)),
            // Calmest of the four, and the only one about the past rather than
            // about now, so it keeps the quietest shape.
            SessionSignal::UnseenTurn => mark.child(dot(success)),
        })
        .tooltip(move |window, cx| Tooltip::new(hint).build(window, cx))
}

/// The plain mark: a small filled circle.
fn dot(color: gpui::Hsla) -> impl IntoElement + use<> {
    div().size(px(6.)).rounded_full().bg(color)
}

/// What a session row is called: the conversation's own name once it has one,
/// otherwise the agent that runs it.
///
/// Separate and pure because it is the rule, not the rendering. Every session
/// on a root used to be labelled with its agent's name, so three sessions on
/// one project read "Claude Code" three times and the rail could not be used to
/// tell them apart -- which is the one thing a session-first rail is for.
fn session_label(title: Option<&str>, agent: &str) -> SharedString {
    ellipsize(title.unwrap_or(agent), MAX_LABEL)
}

/// Everything a session row offers.
///
/// Built once and used twice: as the ••• button's dropdown on the active row,
/// and as the right-click menu on **every** row. Right-click is what "context
/// menu" means and costs nothing to offer, but a menu reachable only by
/// right-click is a menu most people never find — so the row the user is
/// already on shows the button too.
///
/// *Close* lives in here rather than beside it as a ✕. Both row kinds now
/// carry one ••• and nothing else, and a ✕ next to a ••• invites the reading
/// that one closes the tab and the other holds the rest, which was never true:
/// a session ✕ ends an agent.
///
/// Restart and Export select the session first. Both act on the conversation on
/// screen, and a menu entry that restarts an agent the user cannot see is worse
/// than one that takes them there on the way.
/// Written against `&mut App` rather than `&mut Context<PopupMenu>`: the two
/// menu hosts disagree about that argument, and `Context` derefs to `App`, so
/// this is the signature both can be handed.
fn session_menu(
    root_idx: usize,
    session_idx: usize,
    uid: u64,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, cx: &mut App| {
        let danger = crate::theme::status_ink(cx).danger;
        let (rename, restart, export, close) =
            (shell.clone(), shell.clone(), shell.clone(), shell.clone());
        menu.item(
            PopupMenuItem::new("Rename…")
                .icon(Icon::new(IconName::Replace))
                .on_click(move |_, window, cx: &mut App| {
                    rename
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.begin_rename(uid, window, cx);
                        })
                        .ok();
                }),
        )
        .item(
            PopupMenuItem::new("Restart the agent")
                .icon(Icon::new(IconName::Redo))
                .on_click(move |_, window, cx: &mut App| {
                    restart
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.restart_session_at(root_idx, session_idx, window, cx);
                        })
                        .ok();
                }),
        )
        .item(
            PopupMenuItem::new("Export as Markdown…")
                .icon(Icon::new(IconName::ExternalLink))
                .on_click(move |_, window, cx: &mut App| {
                    export
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.export_session_at(root_idx, session_idx, window, cx);
                        })
                        .ok();
                }),
        )
        .separator()
        .item(
            PopupMenuItem::element(move |_, _| div().text_color(danger).child("Close session"))
                .icon(Icon::new(IconName::Close).text_color(danger))
                .on_click(move |_, window, cx: &mut App| {
                    close
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.close_session(root_idx, session_idx, window, cx);
                        })
                        .ok();
                }),
        )
    }
}

/// One session row, nested under its root's folder row.
fn session_row(
    shell: &Shell,
    root_idx: usize,
    session_idx: usize,
    session: &Session,
    active: bool,
    // Which agent ran a session is worth saying only where there is more than
    // one to be: with a single configured agent it is the same word on every
    // row, and a column of identical words is what the title just replaced.
    show_agent: bool,
    cx: &mut Context<Shell>,
) -> SidebarMenuItem {
    let uid = session.uid;
    let state = shell.session_row(uid, cx);
    let signal = state.signal;
    let label = session_label(state.title.as_deref(), session.title());
    // Only alongside a conversation title: where the row has fallen back to the
    // agent's name, the suffix would repeat the label it sits next to.
    let agent =
        (show_agent && state.title.is_some()).then(|| ellipsize(session.title(), MAX_AGENT_LABEL));
    // A weak handle because both menu closures outlive this frame.
    let menu_target = cx.entity().downgrade();
    let suffix_target = menu_target.clone();

    SidebarMenuItem::new(label)
        .active(active)
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.select_root_session(root_idx, session_idx, window, cx);
            }),
        )
        .context_menu(session_menu(root_idx, session_idx, uid, menu_target))
        .suffix(move |_, cx: &mut App| {
            let suffix_target = suffix_target.clone();
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_shrink(1.)
                .min_w_0()
                .when_some(agent.clone(), |row, agent| {
                    row.child(
                        div()
                            .max_w(MAX_AGENT_W)
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(agent),
                    )
                })
                .when_some(signal, |row, signal| row.child(signal_mark(signal, cx)))
                // Offered on the **active** row only, as the project row's is:
                // a rail where every row carries a control is a rail of
                // controls, and the user selects a session to see what is in it
                // before acting on it anyway. Every other row still has the
                // same menu on right-click.
                //
                // `occlude`: the row's own click selects the session, and
                // opening a menu must not do that on its way past.
                .when(active, |row| {
                    let build = session_menu(root_idx, session_idx, uid, suffix_target);
                    row.child(
                        div().flex_none().occlude().child(
                            rail_control(("session-menu", uid), IconName::Ellipsis)
                                .tooltip("What can be done with this session")
                                .dropdown_menu_with_anchor(
                                    Anchor::TopRight,
                                    move |menu, window, cx| build(menu, window, cx),
                                ),
                        ),
                    )
                })
        })
}

/// The git facts a folder row carries: the branch, and how many files differ.
///
/// Both are truncated hard so the project's own name keeps the row, which
/// leaves the full text somewhere to live -- the tooltip, which also carries
/// the root's path because the label is a folder name and two projects can
/// share one.
///
/// The count is a **badge**, not a coloured number. As a bare figure in the
/// warning tint its colour was the whole message, and a colour is a message
/// only to someone who already knows the code: a project with a lot of ordinary
/// work in it read as a project in trouble. The pill says "this is a count";
/// the tooltip says a count of what.
fn git_facts(
    branch: Option<SharedString>,
    changed: usize,
    path: SharedString,
    cx: &App,
) -> impl IntoElement + use<> {
    let radius = cx.theme().radius;
    let (badge_bg, badge_fg) = (cx.theme().secondary, cx.theme().secondary_foreground);
    let full_branch = branch.clone();

    div()
        .id("git")
        .h_flex()
        .items_center()
        .gap_1()
        .min_w_0()
        .when_some(branch, |row, branch| {
            row.child(
                div()
                    .max_w(MAX_BRANCH_W)
                    .truncate()
                    .child(ellipsize(&branch, MAX_LABEL)),
            )
        })
        // `flex_none`: the change count is a signal, not detail. It is the one
        // thing on this row that must survive any width.
        .when(changed > 0, |row| {
            row.child(
                div()
                    .flex_none()
                    .px_1()
                    .rounded(radius)
                    .bg(badge_bg)
                    .text_color(badge_fg)
                    .child(format!("{changed}")),
            )
        })
        .tooltip(move |window, cx| {
            let (branch, path) = (full_branch.clone(), path.clone());
            Tooltip::element(move |_, _| {
                div()
                    .v_flex()
                    .gap_0p5()
                    .when_some(branch.clone(), |col, branch| {
                        col.child(format!("Branch: {branch}"))
                    })
                    .when(changed > 0, |col| {
                        col.child(format!(
                            "{changed} changed {}",
                            if changed == 1 { "file" } else { "files" }
                        ))
                    })
                    .child(path.clone())
            })
            .build(window, cx)
        })
}

/// Everything a project row offers, behind one button.
///
/// **Not a ✕ any more.** A remove control sitting on the row that also *selects*
/// the project put the one irreversible action in the rail under its smallest
/// target, a few pixels from the thing users click most -- and next to a
/// session row's ✕, which closes one session, it read as "close this tab"
/// rather than "drop this project from the workspace". Behind a menu the
/// removal gets a full-width label that says what it removes, sits last, is
/// separated from the harmless entries above it, and is drawn in the danger
/// tint. The other four entries are things that were either buried or reachable
/// only by first selecting the project.
///
/// Offered on the **active** row only, for the same reason the ✕ was: a rail
/// where every row carries a control is a rail of controls, and the user
/// selects a project to see what is in it before acting on it anyway.
fn project_menu(
    root_idx: usize,
    pinned: bool,
    is_repo: bool,
    shell: WeakEntity<Shell>,
) -> impl IntoElement + use<> {
    rail_control(("project-menu", root_idx), IconName::Ellipsis)
        .tooltip("What can be done with this project")
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
            let danger = crate::theme::status_ink(cx).danger;
            let (pin, start, split, terminal, copy, refresh, remove) = (
                shell.clone(),
                shell.clone(),
                shell.clone(),
                shell.clone(),
                shell.clone(),
                shell.clone(),
                shell.clone(),
            );
            menu.item(
                // The label is the state readout as well as the action: with no
                // pin marker of its own a row would otherwise only say it is
                // pinned by *where* it is, which reads as an accident.
                PopupMenuItem::new(if pinned { "Unpin" } else { "Pin to top" })
                    .icon(Icon::new(IconName::Star))
                    .on_click(move |_, window, cx: &mut App| {
                        pin.update(cx, |shell: &mut Shell, cx| {
                            shell.toggle_pin(root_idx, window, cx);
                        })
                        .ok();
                    }),
            )
            .item(
                PopupMenuItem::new("New session")
                    .icon(Icon::new(IconName::Plus))
                    .on_click(move |_, window, cx: &mut App| {
                        start
                            .update(cx, |shell: &mut Shell, cx| {
                                shell.new_session_in(root_idx, window, cx);
                            })
                            .ok();
                    }),
            )
            // Only where there is a repository to split. On a plain folder the
            // entry could not do anything but report that git said no, and an
            // entry whose whole job is to fail is one the eye has to learn to
            // skip.
            .when(is_repo, |menu| {
                menu.item(
                    PopupMenuItem::new("New worktree…")
                        .icon(Icon::new(crate::icons::Icon::GitBranch))
                        .on_click(move |_, window, cx: &mut App| {
                            split
                                .update(cx, |shell: &mut Shell, cx| {
                                    shell.begin_worktree(root_idx, window, cx);
                                })
                                .ok();
                        }),
                )
            })
            .item(
                PopupMenuItem::new("Open terminal")
                    .icon(Icon::new(IconName::SquareTerminal))
                    .on_click(move |_, window, cx: &mut App| {
                        terminal
                            .update(cx, |shell: &mut Shell, cx| {
                                shell.open_terminal_in(root_idx, window, cx);
                            })
                            .ok();
                    }),
            )
            .item(
                PopupMenuItem::new("Copy project path")
                    .icon(Icon::new(IconName::Copy))
                    .on_click(move |_, window, cx: &mut App| {
                        copy.update(cx, |shell: &mut Shell, cx| {
                            shell.copy_root_path(root_idx, window, cx);
                        })
                        .ok();
                    }),
            )
            .item(
                PopupMenuItem::new("Refresh Git status")
                    .icon(Icon::new(IconName::Redo))
                    .on_click(move |_, _, cx: &mut App| {
                        refresh
                            .update(cx, |shell: &mut Shell, cx| shell.refresh_git(cx))
                            .ok();
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::element(move |_, _| {
                    div().text_color(danger).child("Remove from workspace")
                })
                .icon(Icon::new(IconName::Delete).text_color(danger))
                .on_click(move |_, window, cx: &mut App| {
                    remove
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.remove_root(root_idx, window, cx);
                        })
                        .ok();
                }),
            )
        })
}

/// One folder row, with its sessions nested beneath it.
fn folder_row(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    root_idx: usize,
    show_agent: bool,
    cx: &mut Context<Shell>,
) -> SidebarMenuItem {
    let root = &window_state.workspace.roots[root_idx];
    let is_active = window_state.workspace.active_root == root_idx;
    let active_session = root.active_session;
    let pinned = root.pinned;

    // Branch and count are read as two fields rather than through
    // `GitStatus::label()`: the label is one string, and one string can only
    // shrink as a unit -- which is how the count, the more valuable half, ended
    // up being the part that got clipped off the right edge.
    let git = window_state.git.get(&root.path);
    // A status at all is the answer to "is this a git repository": the sweep
    // only records a root `git status` succeeded in.
    let is_repo = git.is_some();
    let branch = git.map(|status| SharedString::from(status.branch.clone()));
    let changed = git.map(|status| status.changed).unwrap_or(0);
    let path = SharedString::from(root.path.display().to_string());
    // What the sessions inside add up to. A collapsed project used to be silent
    // about everything in it: an agent could be waiting on an answer, or dead,
    // and nothing said so until someone thought to expand that row.
    let rollup = SessionSignal::most_urgent(
        root.sessions
            .iter()
            .filter_map(|session| shell.session_row(session.uid, cx).signal),
    );
    let mut children = root
        .sessions
        .iter()
        .enumerate()
        .map(|(i, session)| {
            session_row(
                shell,
                root_idx,
                i,
                session,
                is_active && active_session == i,
                show_agent,
                cx,
            )
        })
        .collect::<Vec<_>>();

    // A project with nothing running expands into the one thing to do about
    // it. Before this it expanded into nothing at all while the centre of the
    // window asked the user to pick a session -- from a list that was empty,
    // which is the state every freshly added project starts in.
    //
    // The offer alone, with no "No sessions yet" above it. That line said what
    // the empty list already said, and being the one unclickable row in a
    // column of clickable ones it took the pointer cursor from its
    // neighbours -- `SidebarMenuItem` is not `Styled`, so the cursor is set
    // once on the menu they all sit in and cannot be taken back per row.
    if children.is_empty() {
        children.push(
            SidebarMenuItem::new("Start a session")
                .icon(Icon::new(IconName::Plus))
                .on_click(
                    cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                        shell.new_session_in(root_idx, window, cx);
                    }),
                ),
        );
    }

    // A weak handle because the suffix closure outlives this frame.
    let menu_target = cx.entity().downgrade();

    SidebarMenuItem::new(ellipsize(&root.label, MAX_LABEL))
        .icon(Icon::new(IconName::Folder))
        // The selected project is marked whether or not it has sessions. While
        // this was `is_active && sessions.is_empty()`, a project holding the
        // conversation on screen was the one project in the rail with no mark
        // at all -- the highlight moved to its session row and the row naming
        // the *project* went plain, so nothing on screen said which project the
        // user was in.
        .active(is_active)
        // Only the selected project starts expanded. With every project open
        // and every one of them now carrying at least two rows, a workspace of
        // ten roots was a rail nobody could see the bottom of; clicking a
        // project both selects it and opens it, so the rest are one click away.
        .default_open(is_active)
        .click_to_toggle(true)
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.select_root(root_idx, window, cx);
            }),
        )
        .children(children)
        .suffix(move |_, cx: &mut App| {
            let menu_target = menu_target.clone();
            div()
                .h_flex()
                .items_center()
                .flex_shrink(1.)
                .min_w_0()
                .gap_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                // A pinned project must say so on the row. Position alone does
                // not: "first in the list" is where a project can also be by
                // accident, so a pinned row and an ordinary top row would look
                // identical and the order would read as the app rearranging
                // things on its own.
                .when(pinned, |row| {
                    row.child(Icon::new(IconName::Star).size_3().flex_none())
                })
                .when(branch.is_some() || changed > 0, |row| {
                    row.child(git_facts(branch.clone(), changed, path.clone(), cx))
                })
                .when_some(rollup, |row, signal| row.child(signal_mark(signal, cx)))
                // `occlude`: the menu button sits inside a row whose own click
                // selects the project and toggles it open, and opening a menu
                // must not do either on its way past.
                .when(is_active, |row| {
                    row.child(div().flex_none().occlude().child(project_menu(
                        root_idx,
                        pinned,
                        is_repo,
                        menu_target,
                    )))
                })
        })
}

/// How many sessions a workspace needs before *Recent* earns its space.
///
/// Below this the whole tree is on screen already and Recent would be the same
/// rows written twice, one scroll apart.
const RECENT_THRESHOLD: usize = 4;
/// How many it lists. Short on purpose: this is "where was I", not a second
/// copy of the tree, and a long list is one the eye has to search rather than
/// recognize.
const RECENT_ROWS: usize = 5;

/// The *Recent* rows, most recently viewed first — or nothing at all.
///
/// Flat, and **the tree keeps its own order**: jumping back to a conversation
/// must not rearrange the project list underneath it, because a list that
/// reorders itself has to be re-read every time it is looked at. So recency
/// gets a section of its own and the tree stays exactly where the user left it.
///
/// A uid that no longer resolves is dropped rather than drawn: sessions close,
/// and a recency list is the last place that should be keeping one alive.
fn recent_rows(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
) -> Vec<SidebarMenuItem> {
    let total: usize = window_state
        .workspace
        .roots
        .iter()
        .map(|root| root.sessions.len())
        .sum();
    if total < RECENT_THRESHOLD {
        return Vec::new();
    }

    let active = window_state
        .workspace
        .active_root()
        .and_then(|root| root.active_session().map(|s| s.uid));

    shell
        .recent_order()
        .iter()
        .copied()
        // The conversation already on screen is not somewhere to go back to,
        // and it would take the top row every time.
        .filter(|uid| Some(*uid) != active)
        .filter_map(|uid| {
            let (root_idx, root) = window_state
                .workspace
                .roots
                .iter()
                .enumerate()
                .find(|(_, root)| root.sessions.iter().any(|s| s.uid == uid))?;
            let session_idx = root.sessions.iter().position(|s| s.uid == uid)?;
            let state = shell.session_row(uid, cx);
            let label = session_label(state.title.as_deref(), root.sessions[session_idx].title());
            let project = ellipsize(&root.label, MAX_AGENT_LABEL);
            let signal = state.signal;

            Some(
                SidebarMenuItem::new(label)
                    .icon(Icon::new(IconName::Undo))
                    .on_click(
                        cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                            shell.select_root_session(root_idx, session_idx, window, cx);
                        }),
                    )
                    // Which project it is in, because out of the tree the row
                    // has lost the one thing that said so.
                    .suffix(move |_, cx: &mut App| {
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .flex_shrink(1.)
                            .min_w_0()
                            .child(
                                div()
                                    .max_w(MAX_AGENT_W)
                                    .truncate()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(project.clone()),
                            )
                            .when_some(signal, |row, signal| row.child(signal_mark(signal, cx)))
                    }),
            )
        })
        .take(RECENT_ROWS)
        .collect()
}

/// Build the rail for a window.
pub fn rail(
    window_state_shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
    // `use<>`: the returned sidebar is fully owned (every string is cloned and
    // every handler is an Rc), so it must not capture the borrows of `self` and
    // `cx` that edition 2024 would otherwise infer -- the caller needs `cx` back
    // to attach its action handlers.
) -> impl IntoElement + use<> {
    let name = window_state.workspace.name.clone();
    let active_root = window_state
        .workspace
        .active_root()
        .map(|root| root.label.clone());
    let show_agent = window_state_shell.agents(cx).len() > 1;
    let recent = recent_rows(window_state_shell, window_state, cx);
    // `display_order`, not `0..len`: pinned projects are drawn first while the
    // roots themselves stay put, so every index a row hands back still means
    // the project the user clicked.
    let mut roots = window_state
        .workspace
        .display_order()
        .into_iter()
        .map(|idx| folder_row(window_state_shell, window_state, idx, show_agent, cx))
        .collect::<Vec<_>>();
    // Last in the group, not in the header: adding a project is a *list*
    // action, and it reads as the end of the list it extends. The header holds
    // the one action that is about the session, not the tree.
    roots.push(
        SidebarMenuItem::new("Add project…")
            .icon(Icon::new(IconName::FolderOpen))
            .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                shell.add_root(cx);
            })),
    );

    // Every `Sidebar` child must be the same type, so the primary action rides
    // in the header next to the workspace identity and "Projects" is the sole
    // group -- which is where it belongs anyway: starting a session is about
    // the workspace, not about the project list.
    Sidebar::new("rail")
        .side(Side::Left)
        // The rail is a panel in a draggable split now, so its width is the
        // split's to decide: `w_full` is what hands it over. Left at its own
        // fixed default the drag would move the boundary and the rail would
        // stay the width it always was, with the gap behind it.
        //
        // `SidebarCollapsible::None` for the same reason. The library's
        // collapse animates the sidebar between a stored width and 48px, which
        // is a second thing driving the same number -- and this app does not
        // collapse the rail at all, it hides it.
        .collapsible(SidebarCollapsible::None)
        .w_full()
        // One hairline down the rail's edge, not two. `Sidebar` draws a 1px
        // right border of its own, and the split it sits in draws a 1px drag
        // handle hard against it in the same border colour -- so the edge read
        // as a 2px rule that no single declaration accounted for. The handle is
        // the one to keep: it is the affordance, it brightens while the rail is
        // being dragged, and it is drawn whenever the rail is, since a hidden
        // rail takes the whole split with it.
        .border_r_0()
        // Not `SidebarHeader`: it carries a hover highlight of its own, so the
        // workspace identity lit up on hover as though it were a control. Its
        // children carry their own `px_2` instead, which is the inset
        // `SidebarMenuItem` gives the project rows -- that is what puts every
        // icon in the rail on one column.
        .header(
            div()
                .v_flex()
                .gap_2()
                .w_full()
                .min_w_0()
                .child(workspace_identity(name.into(), cx))
                .child(new_session_block(
                    window_state_shell,
                    active_root.as_deref(),
                    cx,
                )),
        )
        // Above Projects, and only once there are enough sessions for "where
        // was I" to be a real question. Empty means no group at all rather than
        // a heading over nothing.
        .children((!recent.is_empty()).then(|| {
            SidebarGroup::new("Recent").child(SidebarMenu::new().cursor_pointer().children(recent))
        }))
        // The pointer belongs on the *menu*, not on the rows. A project row and
        // a session row are the most-clicked things in the window and had the
        // plain arrow over them, because `SidebarMenuItem` sets no cursor and
        // exposes no way to -- it is not `Styled`. GPUI resolves the cursor from
        // the topmost hitbox that names one, and the rows name none, so one
        // declaration on the container they all sit in covers every row at once.
        // gpui-component's own `Button` sets `cursor_default` on itself, so the
        // ✕ and ••• inside a row keep the arrow, which is upstream's intent.
        .child(
            SidebarGroup::new("Projects")
                .child(SidebarMenu::new().cursor_pointer().children(roots)),
        )
        // Not `SidebarFooter`: that is an `h_flex justify_between` with its own
        // hover highlight, meant for one row of controls. Three stacked triggers
        // inside it made hovering any one of them light up the whole block.
        .footer(
            div()
                .v_flex()
                .gap_0p5()
                .w_full()
                .min_w_0()
                // Each modal is its own trigger, so "at most one open" is
                // structural rather than an invariant to maintain.
                .child(crate::dialogs::agent_manager(window_state_shell, cx))
                .child(crate::dialogs::workspace_settings(window_state_shell, cx))
                .child(crate::dialogs::help(cx)),
        )
}

/// The workspace identity line.
///
/// A workspace name is free text and users write sentences into it -- the one in
/// the screenshot wrapped onto two lines and pushed the primary action down the
/// rail. It is an *identity*, so it gets exactly one line: truncated, on the
/// rail's icon column like everything else.
fn workspace_identity(name: SharedString, cx: &App) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    div()
        .h_flex()
        .items_center()
        .w_full()
        .min_w_0()
        .h_7()
        .px_2()
        .gap_x_2()
        .child(
            Icon::new(IconName::LayoutDashboard)
                .size_4()
                .text_color(muted),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .font_semibold()
                .child(name),
        )
}

/// The rail's primary action, and the agent list behind it.
///
/// Filled rather than outlined so it still reads as *the* action, but on the
/// same icon column as every other row -- the `+` used to sit mid-rail while the
/// workspace icon above it and the folder icons below it were at the left edge.
///
/// **One agent stays one click.** The chevron and the list appear only when
/// there is more than one configured, because with one there is nothing to
/// choose; and the row itself always starts the default, so adding a second
/// agent never makes the common case slower. Before this the second agent was
/// configurable in the agent manager and unreachable everywhere else.
///
/// The list expands *in the rail* rather than in a popup: the rail already
/// nests rows under rows (sessions under folders), so this is the shape it
/// already has, and it needs no overlay layer to own.
///
/// **The row names the project it would start in**, in its tooltip. A session
/// belongs to exactly one project root and this button silently picks the
/// selected one -- which is only obvious to someone who already knows that, and
/// invisible to someone reading a rail with ten projects in it.
/// What the primary action promises, in words.
///
/// Pure and separate because it is the rule rather than the rendering: the
/// button starts a session on the *selected* project, and with ten projects in
/// the rail that is not something a `+` can say on its own.
fn new_session_hint(root: Option<&str>, agent: Option<&str>) -> SharedString {
    match (root, agent) {
        (Some(root), Some(agent)) => format!("Start a new session in {root} with {agent}").into(),
        (Some(root), None) => format!("Start a new session in {root}").into(),
        // Nothing to start one *in*. Saying so beats naming a project that is
        // not there, and beats a tooltip that promises what the click cannot do.
        (None, _) => "Add a project first — every session belongs to one".into(),
    }
}

fn new_session_block(
    shell: &Shell,
    active_root: Option<&str>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let agents: Vec<SharedString> = shell
        .agents(cx)
        .iter()
        .map(|spec| ellipsize(&spec.name, MAX_LABEL))
        .collect();
    let choosable = agents.len() > 1;
    let open = choosable && shell.agent_menu_open();
    // The default agent is the one this row starts, which is the whole reason
    // the chevron beside it is optional.
    let hint = new_session_hint(active_root, agents.first().map(SharedString::as_ref));

    let primary = rail_row("new-session", IconName::Plus, "New session", cx)
        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
        .on_click(
            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.new_session(window, cx);
            }),
        );

    div()
        .v_flex()
        .gap_0p5()
        .w_full()
        .min_w_0()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .min_w_0()
                .child(div().flex_1().min_w_0().child(primary))
                .when(choosable, |bar| {
                    bar.child(
                        rail_control(
                            "new-session-agent",
                            if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            },
                        )
                        .tooltip("Start a session with a different agent")
                        .on_click(cx.listener(
                            |shell: &mut Shell, _, _, cx| {
                                shell.toggle_agent_menu(cx);
                            },
                        )),
                    )
                }),
        )
        .when(open, |block| {
            block.children(agents.into_iter().enumerate().map(|(i, name)| {
                div()
                    .id(("agent-choice", i))
                    .h_flex()
                    .items_center()
                    .gap_x_2()
                    .w_full()
                    .min_w_0()
                    .h_7()
                    // Indented past the icon column so the list reads as
                    // belonging to the row above it, the same way session rows
                    // sit under their folder.
                    .pl_6()
                    .pr_2()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .cursor_pointer()
                    .hover(|row| row.bg(cx.theme().accent.opacity(0.5)))
                    .child(Icon::new(IconName::Bot).size_4())
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .on_click(cx.listener(move |shell: &mut Shell, _, window, cx| {
                        shell.new_session_with(i, window, cx);
                    }))
            }))
        })
}

#[cfg(test)]
mod tests {
    use super::{MAX_LABEL, new_session_hint, session_label, signal_hint};
    use crate::chat::pane::SessionSignal;

    /// Every signal says something, and no two say the same thing.
    ///
    /// The words are the half of the signal that needs no learning and that
    /// survives a reader who cannot separate the tints, so a mark that shares
    /// its neighbour's sentence has given that half back. A blank one has given
    /// it up entirely.
    #[test]
    fn each_signal_names_itself_and_no_two_alike() {
        let all = [
            SessionSignal::Lost,
            SessionSignal::AwaitingUser,
            SessionSignal::Busy,
            SessionSignal::UnseenTurn,
        ];
        let hints: Vec<&str> = all.iter().copied().map(signal_hint).collect();
        for (signal, hint) in all.iter().zip(&hints) {
            assert!(!hint.trim().is_empty(), "{signal:?} says nothing");
        }
        let mut unique = hints.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), hints.len(), "two signals say the same thing");
    }

    /// The button is one click and carries no visible target, so the project it
    /// would start in has to be said somewhere. This is that somewhere.
    #[test]
    fn the_primary_action_names_the_project_it_would_start_in() {
        let hint = new_session_hint(Some("foxai-pos-web"), Some("Claude Code"));
        assert!(hint.contains("foxai-pos-web"), "{hint}");
        assert!(hint.contains("Claude Code"), "{hint}");
    }

    /// With no project there is nothing to start a session in, and a tooltip
    /// promising one would be describing a click that cannot happen.
    #[test]
    fn with_no_project_the_hint_asks_for_one() {
        let hint = new_session_hint(None, Some("Claude Code"));
        assert!(hint.contains("Add a project"), "{hint}");
    }

    #[test]
    fn a_session_row_prefers_the_conversations_own_name() {
        assert_eq!(
            session_label(Some("Fix the login flow"), "Claude Code"),
            "Fix the login flow"
        );
    }

    /// Until a conversation has been prompted it has no name of its own, and a
    /// blank row would be worse than a repeated one.
    #[test]
    fn an_unprompted_session_falls_back_to_its_agent() {
        assert_eq!(session_label(None, "Claude Code"), "Claude Code");
    }

    /// A first prompt is free text and users paste paragraphs into it. The row
    /// is a fixed-height anchor, so the cap is what keeps a pasted essay from
    /// deciding the rail's width.
    #[test]
    fn a_long_title_is_capped() {
        let label = session_label(Some(&"a".repeat(MAX_LABEL * 3)), "Claude Code");
        assert_eq!(label.chars().count(), MAX_LABEL);
        assert!(label.ends_with('…'));
    }
}
