use super::row::{
    DragGhost, MAX_BRANCH_W, ProjectDrag, RailRow, Row, hover_fill, menu_button, project_key,
    rail_control,
};
use super::session::{Note, session_row, signal_mark};
use crate::chat::pane::{ProjectFacts, SessionSignal};
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::agent::Session;

/// What a project row says on hover: the whole of everything the row cuts.
///
/// The full name first, then the branch, the count in words, and the root's
/// path -- the last because the label is a folder name and two projects can
/// share one. This used to hang off the git suffix alone, which meant a
/// project that was neither a repository nor had changes drew no suffix and so
/// had nothing to hover; the row is its own hover target now, so every project
/// answers.
pub(super) fn project_hint(
    label: &str,
    branch: Option<&SharedString>,
    changed: usize,
    auto: Option<&SharedString>,
    path: &SharedString,
) -> Vec<SharedString> {
    let mut hint = vec![SharedString::from(label.to_string())];
    if let Some(branch) = branch {
        hint.push(SharedString::from(format!("Branch: {branch}")));
    }
    if changed > 0 {
        hint.push(SharedString::from(format!(
            "{changed} changed {}",
            if changed == 1 { "file" } else { "files" }
        )));
    }
    if let Some(auto) = auto {
        hint.push(auto.clone());
    }
    hint.push(path.clone());
    hint
}

/// The entry that opens a project's open issues, to pick one and start on it
/// now — in both menus that carry it, built here once for the reason
/// [`unattended_item`] is.
///
/// Offered where the switch is — on a repository that is not a run's own
/// worktree — because it starts the same kind of run by a different road.
pub fn pick_item(click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> PopupMenuItem {
    crate::controls::menu_item("Work an issue…")
        .icon(Icon::new(IconName::Inbox))
        .on_click(click)
}

/// The entry that turns unattended runs on and off for a project, in both menus
/// that carry it — the rail's and the project page's — built here once so the
/// two cannot come to say it differently.
///
/// A check rather than an on/off pair of labels: the same switch is a switch in
/// Settings, and the check is what makes it read as one here.
pub fn unattended_item(
    on: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> PopupMenuItem {
    crate::controls::menu_item("Work labelled issues")
        .icon(Icon::new(IconName::Bot))
        .checked(on)
        .on_click(click)
}

/// What a project row says about unattended runs.
#[derive(Debug, PartialEq)]
pub(super) struct AutoStatus {
    /// The word in its pill.
    pub(super) badge: SharedString,
    /// The line its hover carries.
    pub(super) line: SharedString,
    /// The switch is on and nothing can come of it — the pill takes the
    /// warning ink, and the line says why.
    pub(super) stuck: bool,
}

/// The run a project's row names: one on that project's issues, as its issue
/// number and whether it is waiting on a card, the working one ahead of a
/// waiting one. `runs` is each run's project, issue and whether it waits.
pub(super) fn run_on<'a>(
    runs: impl IntoIterator<Item = (&'a std::path::Path, u64, bool)>,
    root: &std::path::Path,
) -> Option<(u64, bool)> {
    runs.into_iter()
        .filter(|&(repo, _, _)| repo == root)
        .map(|(_, number, waiting)| (number, waiting))
        .min_by_key(|&(_, waiting)| waiting)
}

/// What a project row says about unattended runs, or `None` while the project
/// is not switched on.
///
/// A run can only be working on a project that is switched on, but it is read
/// independently so a project switched off mid-run still says the run is
/// there — switching off stops the next run, not the one already going.
///
/// **A switch that is on while nothing can happen is the one state that looks
/// exactly like working**, so every way it can be stuck is said on the row, as
/// `stuck`: a config that stops every run (no label, an interval that does not
/// parse, a mode the agent does not offer), or the last look at this project
/// failing — a remote that is not on GitHub, a `gh` that is missing or signed
/// out.
pub(super) fn auto_status(
    unattended: bool,
    run: Option<(u64, bool)>,
    label: &str,
    stuck: Option<String>,
) -> Option<AutoStatus> {
    let status = |badge: String, line: String, stuck: bool| AutoStatus {
        badge: badge.into(),
        line: line.into(),
        stuck,
    };
    match (unattended, run, stuck) {
        (_, Some((n, false)), _) => Some(status(
            format!("auto · #{n}"),
            format!("Unattended run working on issue #{n}"),
            false,
        )),
        // Said apart from working: a run standing still on a card is waiting
        // for the person reading this, and "working" would tell them there is
        // nothing to do.
        (_, Some((n, true)), _) => Some(status(
            format!("auto · #{n} waiting"),
            format!("Unattended run on issue #{n} is waiting for an answer"),
            false,
        )),
        (false, None, _) => None,
        (true, None, Some(why)) => Some(status(
            "auto".into(),
            format!("Unattended runs on, but nothing can be picked up here: {why}"),
            true,
        )),
        (true, None, None) => Some(status(
            "auto".into(),
            format!("Unattended runs on: issues you opened labelled `{label}` are picked up"),
            false,
        )),
    }
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
/// The button is offered on the **active** row only, for the same reason the ✕
/// was: a rail where every row carries a control is a rail of controls, and the
/// user selects a project to see what is in it before acting on it anyway.
/// Every other row still reaches the same entries by right-click, exactly as a
/// session row does — that parity is the point. While this menu existed only as
/// a dropdown, *Remove from workspace* and *New worktree…* could be reached on
/// one row in the rail and nowhere else, so acting on a project always meant
/// selecting it first and tearing down whatever was on screen on the way.
///
/// Written against `&mut App` rather than `&mut Context<PopupMenu>` for the
/// reason `session::session_menu` is: the dropdown host and the context-menu host
/// disagree about that argument, and `Context` derefs to `App`.
fn project_menu(
    root_idx: usize,
    ProjectFacts {
        pinned,
        is_repo,
        unattended,
    }: ProjectFacts,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, cx: &mut App| {
        let danger = crate::theme::status_ink(cx).danger;
        let (pin, auto, pick, start, split, terminal, copy, refresh, remove) = (
            shell.clone(),
            shell.clone(),
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
            crate::controls::menu_item(if pinned { "Unpin" } else { "Pin to top" })
                .icon(Icon::new(IconName::Star))
                .on_click(move |_, window, cx: &mut App| {
                    pin.update(cx, |shell: &mut Shell, cx| {
                        shell.toggle_pin(root_idx, window, cx);
                    })
                    .ok();
                }),
        )
        .when_some(unattended, |menu, on| {
            menu.item(unattended_item(on, move |_, window, cx: &mut App| {
                auto.update(cx, |shell: &mut Shell, cx| {
                    shell.toggle_unattended(root_idx, window, cx);
                })
                .ok();
            }))
        })
        .when(is_repo && unattended.is_some(), |menu| {
            menu.item(pick_item(move |_, _, cx: &mut App| {
                pick.update(cx, |shell: &mut Shell, cx| shell.begin_pick(root_idx, cx))
                    .ok();
            }))
        })
        .item(
            crate::controls::menu_item("New session")
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
                crate::controls::menu_item("New worktree…")
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
            crate::controls::menu_item("Open terminal")
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
            crate::controls::menu_item("Copy project path")
                .icon(Icon::new(IconName::Copy))
                .on_click(move |_, window, cx: &mut App| {
                    copy.update(cx, |shell: &mut Shell, cx| {
                        shell.copy_root_path(root_idx, window, cx);
                    })
                    .ok();
                }),
        )
        .item(
            crate::controls::menu_item("Refresh Git status")
                .icon(Icon::new(IconName::Redo))
                .on_click(move |_, _, cx: &mut App| {
                    refresh
                        .update(cx, |shell: &mut Shell, cx| shell.refresh_git(cx))
                        .ok();
                }),
        )
        .separator()
        .item(
            crate::controls::menu_row(move |_, _| {
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
    }
}

/// Whether naming the agent on a project's session rows tells the reader
/// anything.
///
/// It does exactly when the sessions disagree about it. Configured agents are
/// the wrong count and were the one this used: a second entry in the settings
/// file put the same word — truncated, since it shares the row with the
/// conversation's title — on every session of every project, including all the
/// projects running one agent apiece. What the footnote is for is telling two
/// rows apart, so the question it answers has to be asked of the rows.
pub(super) fn runs_more_than_one_agent<'a>(agents: impl Iterator<Item = &'a str>) -> bool {
    let mut seen: Option<&str> = None;
    for agent in agents {
        match seen {
            Some(first) if first != agent => return true,
            Some(_) => {}
            None => seen = Some(agent),
        }
    }
    false
}

/// One folder row, with its sessions nested beneath it.
pub(super) fn folder_row(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    root_idx: usize,
    // Where this row is *drawn*, which is not `root_idx`: pinned projects come
    // first. It is what a drag hands back, since a drop means "put it where
    // this row is" and that is a place in the list rather than a place in
    // `roots`.
    at: usize,
    cx: &mut Context<Shell>,
) -> Row {
    let root = &window_state.workspace.roots[root_idx];
    let is_active = window_state.workspace.active_root == root_idx;
    // The workspace page is about no one project, so while it shows, no project
    // or session row is drawn as the one on screen.
    let marked = is_active && !shell.workspace_shown(cx);
    let active_session = root.active_session;
    let pinned = root.pinned;
    let unattended = root.unattended;
    // The issue a run is working on in this project right now, if one is. The
    // run's own session sits under a worktree's row of its own, so without this
    // the project the issue belongs to would say nothing about it.
    // A working run is named ahead of a waiting one, since a project can hold
    // both and the older, usually the waiting one, would otherwise hide it.
    let runs = crate::unattended::live_runs(cx);
    let run = run_on(
        runs.iter()
            .map(|run| (run.repo.as_path(), run.number, run.waiting.is_some())),
        &root.path,
    );
    let auto = auto_status(
        unattended,
        run,
        &crate::unattended::label(cx),
        // What stops every run outranks what stops this project's.
        crate::unattended::blocked(cx).or_else(|| crate::unattended::problem(&root.path, cx)),
    );
    // Only the selected project shows what is in it until somebody says
    // otherwise, or a workspace of ten roots is a rail nobody can see the
    // bottom of. The answer is the window's rather than the row's: the row is
    // not drawn at all while the flat list shows, and a fold kept inside it
    // died every time the user looked at the other tab.
    let unfolded = shell.project_unfolded(&root.path);
    let fold_path = root.path.clone();

    // Branch and count are read as two fields rather than through
    // `GitStatus::label()`: the label is one string, and one string can only
    // shrink as a unit -- which is how the count, the more valuable half, ended
    // up being the part that got clipped off the right edge.
    let git = window_state.git.get(&root.path);
    // A status at all is the answer to "is this a git repository": the sweep
    // only records a root `git status` succeeded in.
    let is_repo = git.is_some();
    let facts = ProjectFacts::of(root, is_repo);
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
    // Whether the agent is worth naming on the rows below, answered by this
    // project's own sessions rather than by the length of the agent menu.
    let among_many = runs_more_than_one_agent(root.sessions.iter().map(Session::title));
    let mut children = match unfolded {
        // Folded is not "drawn and hidden": a closed project builds no rows at
        // all, which is what keeps a workspace of ten roots cheap to draw.
        false => Vec::new(),
        true => root
            .sessions
            .iter()
            .enumerate()
            .map(|(i, session)| {
                session_row(
                    shell,
                    root_idx,
                    i,
                    session,
                    marked && active_session == i,
                    Note::Agent { among_many },
                    cx,
                )
            })
            .collect::<Vec<_>>(),
    };

    // A project with nothing running expands into the one thing to do about
    // it. Before this it expanded into nothing at all while the centre of the
    // window asked the user to pick a session -- from a list that was empty,
    // which is the state every freshly added project starts in.
    //
    // The offer alone, with no "No sessions yet" above it: that line said what
    // the empty list already said.
    if unfolded && children.is_empty() {
        children.push(
            RailRow::new(
                format!("rail-start-{root_idx}"),
                "Start a session",
                cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                    shell.new_session_in(root_idx, window, cx);
                }),
            )
            .icon(IconName::Plus),
        );
    }

    // A weak handle because both menu closures outlive this frame.
    let menu_target = cx.entity().downgrade();
    let suffix_target = menu_target.clone();
    let key = project_key(&root.path);

    // A weak handle for the caret, which outlives this frame as the menus do.
    let fold_target = cx.entity().downgrade();
    let drag_target = cx.entity().downgrade();
    let drag_label = root.label.clone();

    Row {
        item: RailRow::new(
            key,
            root.label.clone(),
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                shell.select_root(root_idx, window, cx);
            }),
        )
        .icon(IconName::Folder)
        // The full name, the branch, the count in words and the root's path,
        // on the row itself: every part the row draws is cut to keep the
        // name first, so the hover is where the whole of each lives.
        .hint(project_hint(
            &root.label,
            branch.as_ref(),
            changed,
            auto.as_ref().map(|auto| &auto.line),
            &path,
        ))
        // The selected project is marked whether or not it has sessions. While
        // this was `is_active && sessions.is_empty()`, a project holding the
        // conversation on screen was the one project in the rail with no mark
        // at all -- the highlight moved to its session row and the row naming
        // the *project* went plain, so nothing on screen said which project the
        // user was in.
        .active(marked)
        // **Selecting a project and folding it away are two different
        // intentions, so they are two different targets.** While the whole
        // row toggled, every click on a project both switched to it and
        // snapped its sessions shut -- so reaching a session in the project
        // you had just arrived at meant clicking the row a second time to
        // undo what the first click did.
        //
        // Open and never toggle, which is not the same as leaving the fold
        // alone: a row that only selected would hide the sessions of every
        // project the user had ever folded, and arriving at one would mean
        // hunting the caret to see what is in it -- the same extra click,
        // in mirror image. Going to a project is asking what is in it, so
        // `Shell::select_root` reveals; only the caret puts it away again.
        // (The click handler rides in `RailRow::new` above.)
        .menu(project_menu(root_idx, facts, menu_target))
        // Dragged and dropped by display position: `Workspace::move_root`
        // writes the permutation back into `roots`, so the order the row was
        // dropped into is the order the workspace file keeps. Crossing the pin
        // line is clamped there rather than refused here, since the row has no
        // way to know which side of it a drop landed on.
        .reorder(move |row| {
            let (target, ghost) = (drag_target.clone(), drag_label.clone());
            row.on_drag(ProjectDrag { from: at }, move |_, _, _, cx| {
                let ghost = ghost.clone();
                cx.new(|_| DragGhost(SharedString::from(ghost)))
            })
            .drag_over::<ProjectDrag>(|style, _, _, cx| style.bg(hover_fill(cx)))
            .on_drop(move |drag: &ProjectDrag, window, cx| {
                let from = drag.from;
                let _ = target.update(cx, |shell: &mut Shell, cx| {
                    shell.move_root(from, at, window, cx);
                });
            })
        })
        .suffix(move |_, cx: &mut App| {
            let auto_badge = auto.as_ref().map(|auto| (auto.badge.clone(), auto.stuck));
            let warning = crate::theme::status_ink(cx).warning;
            let (suffix_target, fold_target) = (suffix_target.clone(), fold_target.clone());
            let fold_path = fold_path.clone();
            let radius = cx.theme().radius;
            let (badge_bg, badge_fg) = (cx.theme().secondary, cx.theme().secondary_foreground);
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
                // The branch, written out on the selected row alone. It is
                // what you read while you are working *in* a project, and on
                // the ten rows you are not in it is ten strings cut short --
                // where `feat/consol` and `feat/codoh` say nothing to tell
                // their projects apart and every one is taking width from the
                // name that would. The row's hover carries it whole for every
                // repository either way.
                // Ellipsized and not faded, for the reason a session row's
                // footnote is: this box is sized by the branch name itself, so
                // a fade at its right edge would eat the tail of `main` as
                // readily as the tail of a name that overran. The row's hover
                // carries the whole branch either way.
                .when_some(branch.clone().filter(|_| is_active), |row, branch| {
                    row.child(div().max_w(MAX_BRANCH_W).truncate().child(branch))
                })
                // The count on every row, and as a **badge**, not a coloured
                // number: as a bare figure in the warning tint its colour was
                // the whole message, and a colour is a message only to someone
                // who already knows the code -- a project with a lot of
                // ordinary work in it read as a project in trouble. The pill
                // says "this is a count"; the hover says a count of what.
                //
                // `flex_none`: the count is a signal, not detail. It is the
                // one thing here that must survive any width.
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
                // Whether this project's labelled issues are worked unattended,
                // and the issue a run is on right now. A **word** in the same
                // pill as the count rather than an icon: the pill already reads
                // as "a fact about this project", and a glyph would be one more
                // shape to learn. `flex_none` for the count's reason — a
                // permission to push that is quietly cut off the row is the
                // worst thing this row could hide.
                // Stuck takes the warning ink and keeps the word: the colour
                // says "look here", and the hover says what is wrong.
                .when_some(auto_badge, |row, (badge, stuck)| {
                    row.child(
                        div()
                            .flex_none()
                            .px_1()
                            .rounded(radius)
                            .bg(badge_bg)
                            .text_color(if stuck { warning } else { badge_fg })
                            .child(badge),
                    )
                })
                .when_some(rollup, |row, signal| row.child(signal_mark(signal, cx)))
                .when(is_active, |row| {
                    row.child(menu_button(
                        rail_control(("project-menu", root_idx), IconName::Ellipsis),
                        "What can be done with this project",
                        project_menu(root_idx, facts, suffix_target),
                    ))
                })
                // Last, so it is in the same place on every row whatever
                // else the row happens to be carrying -- a control the eye
                // has to find is not a target, and this is the one the
                // whole click-reveals rule sends people to.
                //
                // `occlude`, as the ••• beside it is: the row's own click
                // selects the project, and putting its sessions away must
                // not do that on the way past.
                .child(
                    div().flex_none().occlude().child(
                        rail_control(
                            ("project-fold", root_idx),
                            match unfolded {
                                true => IconName::ChevronDown,
                                false => IconName::ChevronRight,
                            },
                        )
                        .tooltip(match unfolded {
                            true => "Hide this project's sessions",
                            false => "Show this project's sessions",
                        })
                        .on_click(move |_, _, cx: &mut App| {
                            let fold_path = fold_path.clone();
                            fold_target
                                .update(cx, |shell: &mut Shell, cx| {
                                    shell.toggle_fold(fold_path, cx);
                                })
                                .ok();
                        }),
                    ),
                )
                .into_any_element()
        }),
        children,
    }
}
