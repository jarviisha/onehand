use super::row::{
    DragGhost, MAX_AGENT_W, RailRow, Row, SessionDrag, ellipsize, hover_fill, menu_button,
    rail_control,
};
use crate::chat::pane::SessionSignal;
use crate::shell::Shell;
use crate::state::WorkspaceWindow;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use gpui_component::menu::PopupMenu;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, StyledExt};
use onehand_core::agent::Session;

/// What a signal says, in words.
///
/// Every mark on the rail has one, and that is the point: a mark is a code, and
/// a code has to be learned before it can be read. The tooltip is the only form
/// of the signal that needs no learning at all — and the only one that works
/// for a reader who does not separate red from green.
///
/// Pure and separate because it is the rule rather than the rendering.
pub(super) fn signal_hint(signal: SessionSignal) -> &'static str {
    match signal {
        SessionSignal::Lost => "The agent went away — restart it from the session menu",
        SessionSignal::AwaitingUser => "Waiting for you",
        SessionSignal::Busy => "Working",
        SessionSignal::UnseenTurn => "Finished while you were away",
    }
}

/// What a signal is called where there is room for a name but not a sentence.
///
/// Separate from [`signal_hint`], which is what to *do* about the state: a
/// tooltip is read on purpose and can afford a clause, while this is read in
/// passing and can afford two words. Both live here so no two readers of one
/// condition can end up calling it two things -- today the remote bridge's
/// session listing is the other one.
pub(crate) fn signal_word(signal: SessionSignal) -> &'static str {
    match signal {
        SessionSignal::Lost => "Disconnected",
        SessionSignal::AwaitingUser => "Waiting for you",
        SessionSignal::Busy => "Working",
        SessionSignal::UnseenTurn => "Finished",
    }
}

/// The mark a signal draws on a row.
///
/// **The one state that is wrong takes a shape of its own.** A lost adapter is
/// a triangle, because colour alone cannot be read at all by someone who does
/// not separate red from green, and that is the mark it must never happen to.
/// The other three are one dot in three tints, which is a code — so each one
/// names itself in the tooltip, and the conversation header says the same word
/// beside the name.
///
/// **Nothing at all** for a session that is connected, idle and already read.
/// That is the case that makes the other four legible — a rail where every row
/// is marked is a rail where no mark means anything.
///
/// The tints follow the transcript's conventions, so the same colour means the
/// same thing wherever it appears.
///
/// **This is the only place a signal is drawn.** It was shared with a badge in
/// the conversation header, which said the same thing about the session on
/// screen; that badge is gone, and one consequence is worth knowing here -- a
/// lost adapter is reported by this mark and nothing else, so a hidden rail
/// leaves it reported nowhere.
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
            // A plain dot, not a spinner: this is a state a row carries for
            // minutes at a time, and the only thing moving on an otherwise
            // still rail pulls the eye for as long as it runs. The tooltip
            // says "Working" in words and so does the running line at the foot
            // of the transcript; the dot only has to say the row is not idle.
            SessionSignal::Busy => mark.child(dot(warning)),
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

/// More characters than the widest rail can draw, fewer than a paste.
///
/// A cost bound and not a fit rule: what fits is decided in pixels at the row,
/// by the fade -- but the label is shaped on every frame the rail draws, and a
/// conversation's derived title is free text somebody can open with a whole
/// paragraph. The ellipsis this writes sits far behind the fade and never
/// reaches the screen.
pub(super) const LABEL_SHAPE_CAP: usize = 80;

/// What a session row is called: the conversation's own name once it has one,
/// otherwise the agent that runs it.
///
/// Separate and pure because it is the rule, not the rendering. Every session
/// on a root used to be labelled with its agent's name, so three sessions on
/// one project read "Claude Code" three times and the rail could not be used to
/// tell them apart -- which is the one thing a session-first rail is for.
pub(super) fn session_label(title: Option<&str>, agent: &str) -> SharedString {
    ellipsize(title.unwrap_or(agent), LABEL_SHAPE_CAP)
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
            crate::controls::menu_item("Rename…")
                // Not the bundled `replace`, which is a find-and-replace mark
                // — two arrows around a letter, which reads as swapping this
                // conversation for another one rather than as writing a new
                // name on it. The bundled set has no pencil at all, which is
                // why the shape is one of the app's own.
                .icon(Icon::new(crate::icons::Icon::SquarePen))
                .on_click(move |_, window, cx: &mut App| {
                    rename
                        .update(cx, |shell: &mut Shell, cx| {
                            shell.begin_rename(uid, window, cx);
                        })
                        .ok();
                }),
        )
        .item(
            crate::controls::menu_item("Restart the agent")
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
            crate::controls::menu_item("Export as Markdown…")
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
            crate::controls::menu_row(move |_, _| div().text_color(danger).child("Close session"))
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

/// The footnote a session row carries beside its mark.
///
/// One row type serves both lists, so what it says here is the one thing the
/// two disagree about — and it is a footnote either way: small, muted, capped,
/// and never the thing the row is read for.
pub(super) enum Note {
    /// A row in the tree, nested under its project's folder row.
    ///
    /// It carries which agent runs it, and only where `among_many` says that
    /// is a question this project has more than one answer to. **The count is
    /// the project's sessions and not the configured agent menu**, which is
    /// what it used to be: three agents in the settings file put the same
    /// truncated word on every session of every project, including the nine
    /// projects running one agent apiece — and a column of identical words is
    /// what the conversation's title replaced. What decides it is whether the
    /// rows beside this one disagree.
    ///
    /// Gated again inside the row on the label *not* already being that word,
    /// which is why the string is resolved there rather than by the caller.
    Agent { among_many: bool },
    /// A row in the flat list, standing on its own. It carries which project
    /// it belongs to: what a row outside the tree has lost, since the tree
    /// said it by where the row sat and the flat list has nowhere to put that
    /// but here.
    Project(SharedString),
}

/// Which of the two a footnote is, once its text has been resolved.
///
/// [`Note`] carries the text for one arm and the rule for the other, so it
/// cannot be read a second time without re-deciding; this is what the row's
/// hover needs in order to say *what* the word beside it is.
#[derive(Clone, Copy)]
enum NoteKind {
    Agent,
    Project,
}

/// One session row: nested under its root's folder row, or standing on its own
/// in the flat list.
///
/// **One builder for both lists, deliberately.** A session is the same session
/// whichever list it is being read in — same click, same menu, same mark — and
/// the two rows were briefly written out separately, which left the flat one a
/// near-verbatim copy that would have drifted at the first edit to either.
/// Everything they actually disagree about is [`Note`].
pub(super) fn session_row(
    shell: &Shell,
    root_idx: usize,
    session_idx: usize,
    session: &Session,
    active: bool,
    note: Note,
    cx: &mut Context<Shell>,
) -> RailRow {
    let uid = session.uid;
    let state = shell.session_row(uid, cx);
    let signal = state.signal;
    let (footnote, note_kind) = match &note {
        // Only alongside a conversation title, and only where the project runs
        // more than one agent: where the row has fallen back to the agent's
        // name, the suffix would repeat the label it sits next to.
        Note::Agent { among_many } => (
            (*among_many && state.title.is_some())
                .then(|| SharedString::from(session.title().to_string())),
            NoteKind::Agent,
        ),
        // Always: the project is the one thing the flat row cannot say any
        // other way, and it is true whether or not the conversation has a name.
        Note::Project(project) => (Some(project.clone()), NoteKind::Project),
    };
    // Only the tree's rows are draggable: the flat list is in creation order
    // across every project, which is not an order this app keeps anywhere, so
    // there would be nothing for a drop to write into.
    let in_tree = matches!(note, Note::Agent { .. });
    let label = session_label(state.title.as_deref(), session.title());
    // The whole name, however long, for the hover: it is what the fade at the
    // row's edge may have cut.
    //
    // **And the footnote under it**, whole, because that is cut too -- at
    // `MAX_AGENT_W`, which is narrow enough that two projects sharing a prefix
    // clip to the same visible word. The flat list is where that bites: the
    // footnote is the only thing on the row saying which project a session
    // belongs to, so with it cut and unreadable anywhere the row loses the one
    // fact it exists to carry. Named rather than repeated bare, since out of
    // its column a word on its own does not say what it is.
    let hint = std::iter::once(
        state
            .title
            .clone()
            .unwrap_or_else(|| SharedString::from(session.title().to_string())),
    )
    .chain(footnote.clone().map(|note| match note_kind {
        NoteKind::Agent => SharedString::from(format!("Agent: {note}")),
        NoteKind::Project => SharedString::from(format!("Project: {note}")),
    }))
    .collect::<Vec<_>>();
    // The key carries nothing about which list drew the row, because only one
    // list is on screen at a time -- and it is the same key in both, so the
    // row's element state survives the tab switch.
    let key = SharedString::from(format!("rail-session-{uid}"));
    // A weak handle because both menu closures outlive this frame.
    let menu_target = cx.entity().downgrade();
    let suffix_target = menu_target.clone();
    let drag_target = menu_target.clone();
    let drag_label = label.clone();

    RailRow::new(
        key,
        label,
        cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
            shell.select_root_session(root_idx, session_idx, window, cx);
        }),
    )
    .active(active)
    .hint(hint)
    .menu(session_menu(root_idx, session_idx, uid, menu_target))
    .when(in_tree, |row| {
        let target = drag_target.clone();
        let ghost = drag_label.clone();
        row.reorder(move |row| {
            let (target, ghost) = (target.clone(), ghost.clone());
            row.on_drag(
                SessionDrag {
                    root: root_idx,
                    from: session_idx,
                },
                move |_, _, _, cx| {
                    let ghost = ghost.clone();
                    cx.new(|_| DragGhost(ghost))
                },
            )
            // The row the pointer is over takes the fill a hovered row has:
            // a drop lands *at* this row's place, so the row itself is what
            // is being aimed at and highlighting it says so. No line above
            // or below, because there is no above or below to promise --
            // the moved row takes this index and everything between closes
            // up behind it.
            // The same refusal as the drop below, and both are needed: this
            // one is what stops the row promising a drop it will not take,
            // that one is what stops it taking one.
            .drag_over::<SessionDrag>(move |style, drag, _, cx| match drag.root == root_idx {
                true => style.bg(hover_fill(cx)),
                false => style,
            })
            .on_drop(move |drag: &SessionDrag, _, cx| {
                // Another project's session, which this row has no place
                // for: a session is an agent bound to these files.
                if drag.root != root_idx {
                    return;
                }
                let from = drag.from;
                let _ = target.update(cx, |shell: &mut Shell, cx| {
                    shell.move_session(root_idx, from, session_idx, cx);
                });
            })
        })
    })
    .suffix(move |_, cx: &mut App| {
        let suffix_target = suffix_target.clone();
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .flex_shrink(1.)
            .min_w_0()
            // Capped and ellipsized rather than faded, because this box is
            // sized by its own string: a fade pinned to the right edge of a
            // box that shrink-wraps its text lands on the text, so a footnote
            // short enough to fit came out dissolving anyway. Where the cut is
            // real the `…` is honest, and what it cuts is on the row's hover.
            .when_some(footnote.clone(), |row, note| {
                row.child(
                    div()
                        .max_w(MAX_AGENT_W)
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(note),
                )
            })
            .when_some(signal, |row, signal| row.child(signal_mark(signal, cx)))
            // Offered on the **active** row only, as the project row's is:
            // a rail where every row carries a control is a rail of
            // controls, and the user selects a session to see what is in it
            // before acting on it anyway. Every other row still has the
            // same menu on right-click.
            //
            // The id is the session's uid and carries nothing about which
            // list drew it, because only one list is on screen at a time.
            .when(active, |row| {
                row.child(menu_button(
                    rail_control(("session-menu", uid), IconName::Ellipsis),
                    "What can be done with this session",
                    session_menu(root_idx, session_idx, uid, suffix_target),
                ))
            })
            .into_any_element()
    })
}

/// Every session in the workspace, flat, oldest first.
///
/// The row is [`session_row`], the same one the tree draws — same click, same
/// menu, same mark. All this adds is the order and the project in the suffix.
///
/// **The order is when each session was made, and nothing else.** It was a
/// signal rank first — a parked question or a dead adapter rose to the top —
/// and what that cost is a list whose rows move under the pointer: a session
/// that starts working, finishes, or parks an ask reorders the panel the user
/// is aiming at. The mark on the row still says what each one wants, in a
/// place that does not move. `Session.uid` is the workspace-wide creation
/// counter, so sorting on it is the order the sessions were minted in across
/// every root.
///
/// A workspace with nothing running gets the offer instead of a blank panel,
/// the way a project with no sessions does in the tree: an empty list is
/// indistinguishable from a list that failed to load, and the one thing to do
/// about it is the thing the header already offers, said again where the eye
/// actually is.
///
/// **Uncapped, and that is the same ceiling the tree has.** Every row here is a
/// session somebody minted by hand, one press at a time, and the project tree
/// draws the same set the moment its projects are unfolded — so a cap here
/// would bound one view of a list and not the other, and would report a
/// workspace as truncated that the tab beside it draws in full. If this ever
/// needs a bound it needs the tree's at the same time, and it needs to say on
/// screen when one bit.
pub(super) fn session_rows(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
) -> Vec<Row> {
    let active = window_state
        .workspace
        .active_root()
        .and_then(|root| root.active_session().map(|s| s.uid))
        .filter(|_| !shell.page_shown(cx));

    let mut rows: Vec<(u64, Row)> = Vec::new();
    for (root_idx, root) in window_state.workspace.roots.iter().enumerate() {
        let project = SharedString::from(root.label.clone());
        for (session_idx, session) in root.sessions.iter().enumerate() {
            let uid = session.uid;
            rows.push((
                uid,
                Row::flat(session_row(
                    shell,
                    root_idx,
                    session_idx,
                    session,
                    Some(uid) == active,
                    Note::Project(project.clone()),
                    cx,
                )),
            ));
        }
    }

    if rows.is_empty() {
        return vec![Row::flat(
            RailRow::new(
                "rail-no-sessions",
                "Start a session",
                cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                    shell.new_session(window, cx);
                }),
            )
            .icon(IconName::Plus),
        )];
    }

    // No two sessions share a uid, so the order is total and nothing can swap
    // places between frames.
    rows.sort_unstable_by_key(|(uid, _)| *uid);
    rows.into_iter().map(|(_, row)| row).collect()
}
