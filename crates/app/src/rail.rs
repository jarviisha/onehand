//! The navigation rail.
//!
//! gpui-component's `Sidebar`. The rail is
//! **session-first**: every folder row lists its root's sessions underneath, and
//! clicking a session row selects root *and* session in one touch.
//!
//! It draws one of two lists at a time ([`RailTab`]): the project tree, which
//! answers "what is in this workspace", and every session flat, which answers
//! "what wants me" and sorts itself to say so.
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
use gpui_component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::sidebar::{Sidebar, SidebarCollapsible, SidebarMenuItem};
use gpui_component::spinner::Spinner;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Selectable as _, Side, Sizable as _, StyledExt};
use onehand_core::agent::Session;

/// Names are structural anchors, not content: cap them so a deep path cannot
/// push the rail's width around. `SidebarMenuItem` clips its label with
/// `overflow_x_hidden` and no ellipsis, so this is what produces the `…`.
///
/// This is the cap at the rail's *default* width — the width it was drawn at
/// while the figure was chosen by eye, which is what makes that one pairing
/// the fixed point [`label_cap`] works out from.
///
/// Everything else still capped by this constant rather than by [`label_cap`]
/// is a string the rail's width does not decide: each of them already sits
/// behind something that clips on its own — a pixel cap, one of our own
/// truncating divs, or a popup sized by its own contents — so none of them is
/// waiting on the drag, and the figure here is only their upper bound.
const MAX_LABEL: usize = 24;

/// How much of a name a rail row carries at the width the rail is drawn at.
///
/// The rail is draggable (232–320px) while this was one constant, so widening
/// it bought nothing: every name stayed cut at the same character and the
/// handle promised width the rows never spent. The rule is [`MAX_LABEL`] at
/// the default width and one character per `CHAR_W` either side of it.
///
/// Characters and not pixels, because the label is a string by the time the
/// library sees it. `CHAR_W` is an average across a proportional face, so a
/// name in capitals cuts a character early and a name of `i`s a character
/// late; measuring for real needs the text system, which means being inside a
/// paint, and what being wrong here costs is one character of a name.
fn label_cap(rail_w: f32) -> usize {
    /// The width of one character of a name, averaged over a name.
    const CHAR_W: f32 = 7.;
    let default = onehand_core::config::PanelLayout::default().rail_w;
    // Saturating on the way to `usize`, which is what a rail narrower than
    // anything it can be dragged to would need -- and the cast already does
    // it, so there is nothing here to guard.
    (MAX_LABEL as f32 + (rail_w - default) / CHAR_W) as usize
}

/// One row of the rail's list, and whatever is nested under it.
#[derive(Clone)]
struct Row {
    /// What the row is *called*, as opposed to where it sits.
    key: ElementId,
    item: SidebarMenuItem,
    /// A project's sessions, drawn inside its rule. Empty for every row in the
    /// flat list, which nests nothing.
    children: Vec<(ElementId, SidebarMenuItem)>,
}

impl Row {
    fn flat(key: ElementId, item: SidebarMenuItem) -> Self {
        Self {
            key,
            item,
            children: Vec::new(),
        }
    }
}

/// The rail's own menu: a column of rows, each named by what it *is*, and each
/// opened and closed by the window rather than by itself.
///
/// **This is the whole fix for three bugs that looked unrelated**, and all
/// three were one thing: `SidebarMenuItem` keeps whether it is expanded in
/// `window.use_keyed_state`, which is neither named nor scoped the way the
/// rail needs.
///
/// It was named by *position*. Every container in the library hands its id
/// down as one — a `Sidebar` names its children by their index, a group names
/// its children by theirs, and `SidebarMenu` named the rows by theirs — so a
/// project's expanded state belonged to the slot rather than to the project in
/// it. Pinning a project moved it up the list and it arrived carrying whatever
/// the project previously in that slot had been doing; removing one shifted
/// every project after it the same way. And the id began with the group's
/// index, so the moment a second group appeared above Projects — which
/// happened on its own, as soon as a workspace held enough sessions for a
/// second section to earn its place — every project's id changed at once and
/// the whole tree collapsed. Naming each row for itself answered that much.
///
/// It did not answer the third, because element state is **scoped to
/// consecutive frames the key is accessed in**: gpui carries forward only the
/// states a frame actually touched and drops the rest. The tab that shows the
/// flat list does not draw a single project row, so every project's fold was
/// destroyed on the way out and re-seeded on the way back — a folded project
/// sprang open and an unfolded one snapped shut, for no reason a user could
/// see. So the fold is the window's (`Shell::project_unfolded`), which outlives
/// any list, and this draws the nesting the library's submenu would have:
/// children inside one rule, which is also what keeps the rule continuous
/// instead of one dash per row.
///
/// This stands exactly where `SidebarMenu` stood and draws what it drew: a
/// flex column with a gap, and the pointer for every row inside it. **The gap
/// has to be here.** `SidebarGroup` wraps its children in `div().gap_2()
/// .flex_col()`, and a gpui `div` starts at `display: block` while `flex_col`
/// sets only the direction — so both of those declarations do nothing there,
/// and the space between rows was always this column's to draw.
///
/// Nothing here re-implements a *row*: `SidebarMenuItem` still does every bit
/// of that drawing, and `Sidebar` and `SidebarGroup` are both generic over
/// their item type precisely so a host can answer what a row is called.
#[derive(Clone)]
struct KeyedMenu {
    rows: Vec<Row>,
    collapsed: bool,
}

impl KeyedMenu {
    fn new(rows: Vec<Row>) -> Self {
        Self {
            rows,
            collapsed: false,
        }
    }
}

impl gpui_component::Collapsible for KeyedMenu {
    fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl gpui_component::sidebar::SidebarItem for KeyedMenu {
    /// The id the group offers is ignored, and that is the point: it is a
    /// position, and a position is what these rows must not be named by.
    fn render(
        self,
        _position: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let collapsed = self.collapsed;
        let nest = cx.theme().sidebar_border;
        div()
            .v_flex()
            .gap_2()
            // On the rows and not on the gaps between them. A project row and
            // a session row are the most-clicked things in the window,
            // `SidebarMenuItem` sets no cursor and is not `Styled` so it cannot
            // be told to, and gpui resolves the cursor from the topmost hitbox
            // that names one. gpui-component's own `Button` sets
            // `cursor_default` on itself, so the ••• inside a row still keeps
            // the arrow, which is upstream's intent.
            .cursor_pointer()
            .children(self.rows.into_iter().map(|row| {
                let children = row.children;
                div()
                    .v_flex()
                    .child(
                        row.item
                            .collapsed(collapsed)
                            .render(row.key, window, cx)
                            .into_any_element(),
                    )
                    // One rule down the whole nest rather than a segment per
                    // row, which is what drawing it per child would give.
                    .when(!children.is_empty(), |block| {
                        block.child(
                            div()
                                .v_flex()
                                .gap_1()
                                .ml_3p5()
                                .pl_2p5()
                                .py_0p5()
                                .border_l_1()
                                .border_color(nest)
                                .children(children.into_iter().map(|(key, child)| {
                                    child
                                        .collapsed(collapsed)
                                        .render(key, window, cx)
                                        .into_any_element()
                                })),
                        )
                    })
                    .into_any_element()
            }))
    }
}

/// What a project's row is called, as far as the window is concerned.
///
/// **The path and not the label**: two checkouts of one repository have the
/// same folder name and are two different projects, so a key made from what
/// the row says would hand one project's expanded state to the other. It is
/// the same identity pinning already uses, for the same reason.
fn project_key(path: &std::path::Path) -> ElementId {
    ElementId::Name(SharedString::from(format!(
        "rail-project-{}",
        path.display()
    )))
}

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
/// Ghost, which is every row here but one: a rail is chrome the conversation
/// sits in front of, and a column of filled rows is a panel shouting over the
/// thing it exists to get you to. The exception is [`rail_row_outlined`], and
/// it is one row.
///
/// Carries its own hover, so no caller may add a second one: `hover` panics in
/// debug when it is set twice.
pub(crate) fn rail_row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    cx: &App,
) -> Stateful<Div> {
    // Resolved up front: the hover closure outlives this borrow of `cx`.
    let (accent, accent_fg) = (
        cx.theme().sidebar_accent,
        cx.theme().sidebar_accent_foreground,
    );
    row_shape(id, icon, label, cx)
        .hover(move |row| row.bg(accent.opacity(0.8)).text_color(accent_fg))
}

/// The one row the rail draws an outline around: *New session*.
///
/// **An outline and not a fill**, which was tried first and is the version this
/// replaced. A filled row is the loudest thing that can happen in a panel whose
/// job is to get out of the way, and the fill this row used to carry is exactly
/// what an earlier pass took off it. The hairline says the same thing for the
/// price of one pixel: everything else on the rail is a name in a column, and
/// this is the one thing with an edge around it.
///
/// Ghost underneath, so it hovers like every other row — the outline marks what
/// the control *is*, not what the pointer is doing.
///
/// **The seam is the caller's.** A split control's two halves share one line
/// between them, so this draws three sides when it is about to be joined and
/// four when it stands alone; the caret's own left border is the divider.
pub(crate) fn rail_row_outlined(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    joined: bool,
    cx: &App,
) -> Stateful<Div> {
    rail_row(id, icon, label, cx)
        .border_color(cx.theme().border)
        .map(|row| match joined {
            true => row.border_l_1().border_t_1().border_b_1(),
            false => row.border_1(),
        })
}

/// The column every row tone shares: everything but the fill and the hover.
fn row_shape(id: &'static str, icon: IconName, label: &'static str, cx: &App) -> Stateful<Div> {
    let radius = cx.theme().radius;
    div()
        .id(id)
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

/// The rail's header rows, one step above the list they stand over.
///
/// Two rows get this — the workspace's name and the primary action — and they
/// get it together, because they are a block: what everything below is *inside*,
/// and the one thing to do about it. At the list's own size and tone they read
/// as its first two entries, which is exactly what they are not; a step up in
/// height, text size and weight is what makes the eye take them once and then
/// scan the list underneath.
///
/// **The icon column does not move.** The label's x is what every row in the
/// rail shares, so the icons stay 16px and only the row around them grows —
/// bigger icons here would leave the header's labels a few pixels off the ones
/// below, which reads as a mistake rather than as a hierarchy.
fn lead_row(row: Stateful<Div>) -> Stateful<Div> {
    row.h_8().text_base().font_medium()
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

/// A control in the rail that opens a menu.
///
/// Three draw exactly this, so it is built once: the ••• a project row and a
/// session row carry while active, and the caret beside *New session*. They
/// differ in the sentence and the builder, and in nothing about the wiring —
/// which is what the two that already existed proved, having been written out
/// twice before this was extracted.
///
/// **The control is handed in rather than named**, because the caret is the one
/// of the three that is not a free-standing ••• : it is the right half of the
/// *New session* control, so it is sized and squared off to join the row beside
/// it, and the only thing that can express that is the button itself.
///
/// `occlude`, because the button sits inside something whose own click already
/// means something — selecting a session, selecting a project, starting one —
/// and opening a menu must not do that on its way past.
///
/// The wrapping closure is what lets one builder serve both this and a row's
/// right-click menu: `SidebarMenuItem::context_menu` hands its builder
/// `&mut App` while this host hands over a `&mut Context<PopupMenu>`, which
/// derefs to it. Written once here rather than at each call site, which is
/// where the copies of it were.
fn menu_button(
    control: Button,
    tooltip: &'static str,
    build: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
) -> impl IntoElement {
    div().flex_none().occlude().child(
        control
            .tooltip(tooltip)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                build(menu, window, cx)
            }),
    )
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

/// What a signal is called where there is room for a name but not a sentence.
///
/// Separate from [`signal_hint`], which is what to *do* about the state: a
/// tooltip is read on purpose and can afford a clause, a badge is read in
/// passing and can afford two words. Both live here so the rail's mark and the
/// conversation header's badge cannot end up calling one condition two things.
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
fn session_label(title: Option<&str>, agent: &str, cap: usize) -> SharedString {
    ellipsize(title.unwrap_or(agent), cap)
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
enum Note {
    /// Which agent runs it. Worth saying only where there is more than one to
    /// be — with a single configured agent it is the same word on every row,
    /// and a column of identical words is what the conversation's title
    /// replaced — and only where the row's own label is *not* already that
    /// word, which is why this is resolved inside the row rather than by the
    /// caller.
    Agent,
    /// Which project it belongs to. What a row outside the tree has lost: the
    /// tree said it by where the row sat, and the flat list has nowhere to put
    /// that but here.
    Project(SharedString),
}

/// One session row: nested under its root's folder row, or standing on its own
/// in the flat list.
///
/// **One builder for both lists, deliberately.** A session is the same session
/// whichever list it is being read in — same click, same menu, same mark — and
/// the two rows were briefly written out separately, which left the flat one a
/// near-verbatim copy that would have drifted at the first edit to either.
/// Everything they actually disagree about is [`Note`].
fn session_row(
    shell: &Shell,
    root_idx: usize,
    session_idx: usize,
    session: &Session,
    active: bool,
    note: Option<Note>,
    cx: &mut Context<Shell>,
) -> SidebarMenuItem {
    let uid = session.uid;
    let state = shell.session_row(uid, cx);
    let signal = state.signal;
    let label = session_label(
        state.title.as_deref(),
        session.title(),
        label_cap(shell.rail_width(cx)),
    );
    let note = match note {
        // Only alongside a conversation title: where the row has fallen back to
        // the agent's name, the suffix would repeat the label it sits next to.
        Some(Note::Agent) => state
            .title
            .is_some()
            .then(|| ellipsize(session.title(), MAX_AGENT_LABEL)),
        // Always: the project is the one thing the flat row cannot say any
        // other way, and it is true whether or not the conversation has a name.
        Some(Note::Project(project)) => Some(project),
        None => None,
    };
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
                .when_some(note.clone(), |row, note| {
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
        })
}

/// The git facts a folder row carries: the branch, and how many files differ.
///
/// Both are truncated hard so the project's own name keeps the row, which
/// leaves the full text somewhere to live -- the tooltip, which also carries
/// the root's path because the label is a folder name and two projects can
/// share one.
///
/// **The project's own untruncated name leads it**, because the row's label is
/// cut to fit and `SidebarMenuItem` offers nowhere to hang a tooltip of its
/// own -- its label is a bare string, not an element. This is the one hover
/// target on the row that can carry the whole name, so it does. What it does
/// not cover is a project that is neither a repository nor has changes: there
/// is no suffix drawn there at all, so there is nothing to hover, and the
/// answer to that one is the row type this rail does not own.
///
/// The count is a **badge**, not a coloured number. As a bare figure in the
/// warning tint its colour was the whole message, and a colour is a message
/// only to someone who already knows the code: a project with a lot of ordinary
/// work in it read as a project in trouble. The pill says "this is a count";
/// the tooltip says a count of what.
fn git_facts(
    label: SharedString,
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
            let (label, branch, path) = (label.clone(), full_branch.clone(), path.clone());
            Tooltip::element(move |_, _| {
                div()
                    .v_flex()
                    .gap_0p5()
                    .child(label.clone())
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
/// reason [`session_menu`] is: the dropdown host and the context-menu host
/// disagree about that argument, and `Context` derefs to `App`.
fn project_menu(
    root_idx: usize,
    pinned: bool,
    is_repo: bool,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, cx: &mut App| {
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
            crate::controls::menu_item(if pinned { "Unpin" } else { "Pin to top" })
                .icon(Icon::new(IconName::Star))
                .on_click(move |_, window, cx: &mut App| {
                    pin.update(cx, |shell: &mut Shell, cx| {
                        shell.toggle_pin(root_idx, window, cx);
                    })
                    .ok();
                }),
        )
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

/// One folder row, with its sessions nested beneath it.
fn folder_row(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    root_idx: usize,
    show_agent: bool,
    cx: &mut Context<Shell>,
) -> Row {
    let root = &window_state.workspace.roots[root_idx];
    let is_active = window_state.workspace.active_root == root_idx;
    let active_session = root.active_session;
    let pinned = root.pinned;
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
    let mut children = match unfolded {
        // Folded is not "drawn and hidden": a closed project builds no rows at
        // all, which is what keeps a workspace of ten roots cheap to draw.
        false => Vec::new(),
        true => root
            .sessions
            .iter()
            .enumerate()
            .map(|(i, session)| {
                (
                    // Named by the session for the reason the project row is
                    // named by its path: a child keyed by its place in the list
                    // is a child that changes identity when a session above it
                    // closes.
                    ElementId::Name(SharedString::from(format!("rail-nested-{}", session.uid))),
                    session_row(
                        shell,
                        root_idx,
                        i,
                        session,
                        is_active && active_session == i,
                        show_agent.then_some(Note::Agent),
                        cx,
                    ),
                )
            })
            .collect::<Vec<_>>(),
    };

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
    if unfolded && children.is_empty() {
        children.push((
            ElementId::Name(SharedString::from(format!("rail-start-{root_idx}"))),
            SidebarMenuItem::new("Start a session")
                .icon(Icon::new(IconName::Plus))
                .on_click(
                    cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                        shell.new_session_in(root_idx, window, cx);
                    }),
                ),
        ));
    }

    // A weak handle because both menu closures outlive this frame.
    let menu_target = cx.entity().downgrade();
    let suffix_target = menu_target.clone();
    let label = SharedString::from(root.label.clone());
    let key = project_key(&root.path);

    // A weak handle for the caret, which outlives this frame as the menus do.
    let fold_target = cx.entity().downgrade();

    Row {
        key,
        item: SidebarMenuItem::new(ellipsize(&root.label, label_cap(shell.rail_width(cx))))
            .icon(Icon::new(IconName::Folder))
            // The selected project is marked whether or not it has sessions. While
            // this was `is_active && sessions.is_empty()`, a project holding the
            // conversation on screen was the one project in the rail with no mark
            // at all -- the highlight moved to its session row and the row naming
            // the *project* went plain, so nothing on screen said which project the
            // user was in.
            .active(is_active)
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
            .on_click(
                cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
                    shell.select_root(root_idx, window, cx);
                }),
            )
            .context_menu(project_menu(root_idx, pinned, is_repo, menu_target))
            .suffix(move |_, cx: &mut App| {
                let (suffix_target, fold_target) = (suffix_target.clone(), fold_target.clone());
                let fold_path = fold_path.clone();
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
                        row.child(git_facts(
                            label.clone(),
                            branch.clone(),
                            changed,
                            path.clone(),
                            cx,
                        ))
                    })
                    .when_some(rollup, |row, signal| row.child(signal_mark(signal, cx)))
                    .when(is_active, |row| {
                        row.child(menu_button(
                            rail_control(("project-menu", root_idx), IconName::Ellipsis),
                            "What can be done with this project",
                            project_menu(root_idx, pinned, is_repo, suffix_target),
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
            }),
        children,
    }
}

/// Which of the rail's two lists is showing.
///
/// The tree answers "what is in this workspace"; the flat list answers "what
/// wants me". They are two questions about one set of sessions, and the reason
/// they are tabs rather than two stacked groups is that the second one
/// **reorders itself**: a section that rearranges under the eye cannot sit above
/// a tree the user navigates by position, because every glance at the busy list
/// moves the quiet one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailTab {
    Projects,
    Sessions,
}

impl RailTab {
    /// Both of them, in the order they are drawn.
    const ALL: [Self; 2] = [Self::Projects, Self::Sessions];

    fn label(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Sessions => "All sessions",
        }
    }
}

/// Where a session sits in the flat list: what it wants first, then when it was
/// last looked at.
///
/// **The signal leads, and that is the whole feature.** A session with a parked
/// question or a dead adapter is one the user has to do something about, and in
/// a workspace of a dozen conversations it is exactly the one that goes unseen
/// at the bottom of a tree. [`SessionSignal::rank`] is the order — the same one
/// a row's mark and a project's roll-up use, so "more urgent" means one thing in
/// the rail rather than three.
///
/// A session carrying no signal at all sorts last as a block, and inside that
/// block recency decides: with nothing asking for attention, "where was I" is
/// the only question left. A session the recency list has never seen goes to the
/// very end rather than to the front, because a list that has not been visited
/// is not a list that was visited long ago.
///
/// Pure, and separate from the rendering, because it is a rule about attention —
/// and rules about attention are what regress silently.
fn session_order(signal: Option<SessionSignal>, recency: Option<usize>) -> (u8, usize) {
    (
        signal.map_or(u8::MAX, SessionSignal::rank),
        recency.unwrap_or(usize::MAX),
    )
}

/// Every session in the workspace, flat, most in need of an answer first.
///
/// The row is [`session_row`], the same one the tree draws — same click, same
/// menu, same mark. All this adds is the order and the project in the suffix.
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
fn session_rows(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
) -> Vec<Row> {
    // Read once into a lookup rather than scanned per session: the list is
    // walked inside a render, and a scan per row makes that quadratic in a
    // workspace's sessions for a fact each row wants exactly once.
    let recency: std::collections::HashMap<u64, usize> = shell
        .recent_order()
        .iter()
        .enumerate()
        .map(|(place, &uid)| (uid, place))
        .collect();
    let active = window_state
        .workspace
        .active_root()
        .and_then(|root| root.active_session().map(|s| s.uid));

    let mut rows: Vec<((u8, usize), Row)> = Vec::new();
    for (root_idx, root) in window_state.workspace.roots.iter().enumerate() {
        let project = ellipsize(&root.label, MAX_AGENT_LABEL);
        for (session_idx, session) in root.sessions.iter().enumerate() {
            let uid = session.uid;
            let signal = shell.session_row(uid, cx).signal;
            rows.push((
                session_order(signal, recency.get(&uid).copied()),
                Row::flat(
                    // Named by the session and not by its place, because its
                    // place moves the moment an agent starts working: a row keyed
                    // by position would hand one conversation's state to another
                    // every time the list re-sorted.
                    ElementId::Name(SharedString::from(format!("rail-session-{uid}"))),
                    session_row(
                        shell,
                        root_idx,
                        session_idx,
                        session,
                        Some(uid) == active,
                        Some(Note::Project(project.clone())),
                        cx,
                    ),
                ),
            ));
        }
    }

    if rows.is_empty() {
        return vec![Row::flat(
            "rail-no-sessions".into(),
            SidebarMenuItem::new("Start a session")
                .icon(Icon::new(IconName::Plus))
                .on_click(
                    cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                        shell.new_session(window, cx);
                    }),
                ),
        )];
    }

    // Stable, so two sessions whose whole key matches -- the same signal and
    // neither one visited -- keep the order the tree puts them in rather than
    // swapping places on every frame. Anything the recency list has seen is
    // already separated by it.
    rows.sort_by_key(|(order, _)| *order);
    rows.into_iter().map(|(_, row)| row).collect()
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
    let show_agent = window_state_shell.agents(cx).len() > 1;
    let workspace_dir = window_state.workspace.storage_dir.clone();
    let workspace_recents = window_state_shell.recents(cx);
    let workspace_target = cx.entity().downgrade();
    let tab = window_state_shell.rail_tab();
    // Only the list being drawn is built. The other tab's rows cost a lookup per
    // session and hang a handler on each, and none of that reaches the screen.
    let rows = match tab {
        // `display_order`, not `0..len`: pinned projects are drawn first while
        // the roots themselves stay put, so every index a row hands back still
        // means the project the user clicked.
        RailTab::Projects => window_state
            .workspace
            .display_order()
            .into_iter()
            .map(|idx| folder_row(window_state_shell, window_state, idx, show_agent, cx))
            .collect::<Vec<_>>(),
        RailTab::Sessions => session_rows(window_state_shell, window_state, cx),
    };

    // Every `Sidebar` child must be the same type, so the primary action rides
    // in the header next to the workspace identity and the tab bar, and the
    // content is one keyed menu -- which is where it belongs anyway: starting a
    // session is about the workspace, not about the project list.
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
                // A step wider than the list's own row gap. Three rows of
                // roughly one height, stacked at the spacing a list uses, read
                // as the first three entries of that list -- which is the one
                // thing the header is not.
                .gap_2p5()
                .w_full()
                .min_w_0()
                .child(workspace_identity(
                    name.into(),
                    workspace_dir,
                    workspace_recents,
                    workspace_target,
                    cx,
                ))
                // Above *New session*, because it is what a workspace with no
                // project needs first and because both of them are about the
                // workspace rather than about the list underneath. Quieter than
                // the primary action right below it: adding a project is done
                // once per project, starting a session is done all day.
                .child(
                    rail_row("rail-add-project", IconName::FolderOpen, "Add project…", cx)
                        .text_color(cx.theme().muted_foreground)
                        .tooltip(|window, cx| {
                            Tooltip::new("Add a project root to this workspace").build(window, cx)
                        })
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.add_root(cx);
                        })),
                )
                .child(new_session_block(window_state_shell, window_state, cx))
                // The hairline is where the header stops being about the
                // workspace and starts being about the list: everything above
                // it acts on the whole window, everything from the tabs down is
                // what is in it. Space alone said it too quietly, since the
                // rows above are already spaced.
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .pt_3()
                        .border_t_1()
                        .border_color(cx.theme().sidebar_border)
                        .child(tab_bar(tab, cx)),
                ),
        )
        .child(KeyedMenu::new(rows))
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

/// The workspace identity line, and the switcher behind it.
///
/// A workspace name is free text and users write sentences into it -- the one in
/// the screenshot wrapped onto two lines and pushed the primary action down the
/// rail. It is an *identity*, so it gets exactly one line: truncated, on the
/// rail's icon column like everything else.
///
/// **The whole row is the switcher.** The workspace the rail is drawing was the
/// one thing on screen with no way to be changed from the rail at all -- the
/// switch was reachable only by opening Settings, two surfaces away from the
/// name it changes. Behind a chevron at the row's end the target would have been
/// a few pixels wide while the thing being pointed at is the name beside it, so
/// the row carries the menu itself.
///
/// **Nothing marks it but the pointer.** No chevron, no second icon: this row
/// sits above the rail's quietest chrome and a caret on it competed with the
/// primary action right below. What says it is a control is the hover and the
/// cursor, which this row deliberately did not have while it was a label, plus
/// the tooltip that names what a press does.
fn workspace_identity(
    name: SharedString,
    current: Option<std::path::PathBuf>,
    recents: Vec<std::path::PathBuf>,
    shell: WeakEntity<Shell>,
    cx: &App,
) -> impl IntoElement + use<> {
    let radius = cx.theme().radius;
    // Resolved up front: the hover closure outlives this borrow of `cx`.
    let (accent, accent_fg) = (
        cx.theme().sidebar_accent,
        cx.theme().sidebar_accent_foreground,
    );
    let row = lead_row(
        div()
            .id("workspace-identity")
            .h_flex()
            .items_center()
            .w_full()
            .min_w_0()
            .px_2()
            .gap_x_2()
            .rounded(radius)
            .cursor_pointer()
            .hover(move |row| row.bg(accent.opacity(0.8)).text_color(accent_fg))
            .tooltip(|window, cx| Tooltip::new("Switch to another workspace").build(window, cx)),
    )
    // Full ink, not muted. It was the dimmest thing in the header while
    // standing for the thing the header is about, so the eye read the row
    // beginning at the name and the mark before it as decoration.
    .child(Icon::new(IconName::LayoutDashboard).size_4())
    // Semibold rather than the header block's medium: this is the one name in
    // the window that says which workspace all of it belongs to.
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .font_semibold()
            .child(name),
    );

    crate::controls::MenuTrigger::new(row, accent)
        .dropdown_menu_with_anchor(Anchor::TopLeft, workspace_menu(current, recents, shell))
}

/// The parent path of a workspace folder, shortened from the *front*.
///
/// A path is read from its tail: the last few components are what tell two
/// checkouts of the same project apart, and they are exactly what trimming the
/// end throws away. Keeping the last characters and marking the cut with a
/// leading `…` is what makes the line identify a folder rather than name the
/// drive it happens to sit on.
fn ellipsize_front(s: &str, max: usize) -> SharedString {
    let count = s.chars().count();
    if count <= max {
        return SharedString::from(s.to_string());
    }
    let kept: String = s.chars().skip(count - max.saturating_sub(1)).collect();
    SharedString::from(format!("…{kept}"))
}

/// How much of a workspace's parent path a switcher row carries.
const MAX_PARENT: usize = 30;

/// Every workspace this launch knows about.
///
/// **Switching still means another window** -- one window hosts exactly one
/// workspace, so a row here opens the workspace it names, or focuses the window
/// already showing it. That is what makes the list safe to offer from the rail:
/// nothing on screen is torn down by a click in it, and the workspace the window
/// is drawing is marked and cannot be picked, because "switch to where you
/// already are" is a click that does nothing and reads as a failure.
///
/// A row is named by its folder, with the parent path beside it, because the
/// list is directories and a workspace's own name lives inside a file that
/// reading here would put disk I/O in the middle of a frame.
///
/// The builder, not the trigger: the menu is rebuilt every time it opens, so the
/// row it hangs off is free to be any element the rail wants.
fn workspace_menu(
    current: Option<std::path::PathBuf>,
    recents: Vec<std::path::PathBuf>,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, cx| {
        let muted = cx.theme().muted_foreground;
        let mut menu = menu;
        // No heading over nothing: a workspace that has never been bound
        // has no recents, and the two actions below stand on their own.
        if !recents.is_empty() {
            menu = menu.label("Workspaces");
        }
        for dir in &recents {
            let is_current = current.as_deref() == Some(dir.as_path());
            let label = ellipsize(
                &dir.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string()),
                MAX_LABEL,
            );
            let parent = dir
                .parent()
                .map(|parent| ellipsize_front(&parent.display().to_string(), MAX_PARENT));
            let target = shell.clone();
            let dir = dir.clone();
            // The workspace already on screen keeps the library's own row, and so
            // its cursor: it is checked and unpickable, and a pointer over it
            // would promise a press that is refused.
            let row =
                move |_: &mut Window, _: &mut App| {
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(div().flex_none().child(label.clone()))
                        .children(parent.clone().map(|parent| {
                            div().flex_none().text_xs().text_color(muted).child(parent)
                        }))
                };
            menu = menu.item(
                match is_current {
                    true => PopupMenuItem::element(row),
                    false => crate::controls::menu_row(row),
                }
                .checked(is_current)
                .disabled(is_current)
                .on_click(move |_, _, cx: &mut App| {
                    let dir = dir.clone();
                    target
                        .update(cx, |shell: &mut Shell, cx| shell.open_recent(dir, cx))
                        .ok();
                }),
            );
        }
        let (open, new) = (shell.clone(), shell.clone());
        menu.separator()
            .item(
                crate::controls::menu_item("Open workspace…")
                    .icon(Icon::new(IconName::FolderOpen))
                    .on_click(move |_, _, cx: &mut App| {
                        open.update(cx, |shell: &mut Shell, cx| shell.open_workspace(cx))
                            .ok();
                    }),
            )
            .item(
                crate::controls::menu_item("New workspace…")
                    .icon(Icon::new(IconName::Plus))
                    .on_click(move |_, _, cx: &mut App| {
                        new.update(cx, |shell: &mut Shell, cx| shell.new_workspace(cx))
                            .ok();
                    }),
            )
    }
}

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

/// The rail's primary action, and the chooser beside it.
///
/// On the same icon column as every other row -- the `+` used to sit mid-rail
/// while the workspace icon above it and the folder icons below it were at the
/// left edge.
///
/// **The row itself is unchanged and stays one click**: the selected project,
/// the default agent. What the caret adds is the two things that click has to
/// pick silently, and the project is the one worth reaching first — a session is
/// bound to one project root for its whole life, so starting one somewhere else
/// used to mean selecting that project, tearing down whatever was on screen, and
/// only then pressing `+`. The agents follow underneath, and only where there is
/// more than one configured: with one there is nothing to choose.
///
/// A popup and not the list that used to expand in the rail. That list pushed
/// the whole tree down while it was open, which is affordable for two agents and
/// not for a workspace's worth of projects — and the state saying whether it was
/// open had to be carried on the shell and cleared on every path that started a
/// session.
///
/// **The row names the project it would start in**, in its tooltip. A session
/// belongs to exactly one project root and this button silently picks the
/// selected one -- which is only obvious to someone who already knows that, and
/// invisible to someone reading a rail with ten projects in it.
fn new_session_block(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let agents: Vec<SharedString> = shell
        .agents(cx)
        .iter()
        .map(|spec| ellipsize(&spec.name, MAX_LABEL))
        .collect();
    // Read off the workspace here rather than handed in: the caller was
    // deriving it from the very tree it was already passing, so the name and
    // the tree it came from travelled together and could disagree.
    let active_root = window_state.workspace.active_root().map(|root| &root.label);
    // The default agent is the one this row starts, which is what the tooltip
    // names.
    let hint = new_session_hint(
        active_root.map(String::as_str),
        agents.first().map(SharedString::as_ref),
    );
    // `display_order` so the menu lists projects the way the rail draws them:
    // two orders for one list is two lists as far as the reader is concerned.
    let projects: Vec<(usize, SharedString)> = window_state
        .workspace
        .display_order()
        .into_iter()
        .filter_map(|idx| {
            let root = window_state.workspace.roots.get(idx)?;
            Some((idx, ellipsize(&root.label, MAX_LABEL)))
        })
        .collect();
    let active_idx = window_state.workspace.active_root;
    // **More than one of either, or nothing to choose.** One project and one
    // agent leaves a caret whose whole menu is a single row doing exactly what
    // the button beside it does -- the same "control that exists to disappoint"
    // the agent list is gated on, and it has to be gated the same way or the
    // rule is one the rail applies in one place and not the other.
    let choosable = projects.len() > 1 || agents.len() > 1;
    let target = cx.entity().downgrade();

    let (radius, hairline) = (cx.theme().radius, cx.theme().border);
    let primary = lead_row(rail_row_outlined(
        "new-session",
        IconName::Plus,
        "New session",
        choosable,
        cx,
    ))
    // The two halves of one control, so the seam between them is square and
    // the outer edges keep the radius. Only while there is a caret to join:
    // a lone row squared off on one side reads as clipped.
    .when(choosable, |row| row.rounded_r(px(0.)))
    .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
    .on_click(
        cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
            shell.new_session(window, cx);
        }),
    );

    div()
        .h_flex()
        .items_center()
        .w_full()
        .min_w_0()
        .child(div().flex_1().min_w_0().child(primary))
        .when(choosable, |bar| {
            // The sentence names whichever section the menu will actually
            // carry. With one project and several agents there is no *Start
            // in* to open, and a caret promising another project over a menu
            // that has none is a control lying about itself before it is even
            // pressed.
            let says = match projects.len() > 1 {
                true => "Start a session in another project",
                false => "Start a session with a different agent",
            };
            bar.child(menu_button(
                // Sized to the header row rather than to the ••• it shares a
                // builder with: this one is the right half of the control
                // beside it, and a control half the height of its own other
                // half is two controls that happen to touch.
                rail_control("new-session-target", IconName::ChevronDown)
                    // The other half of one outlined control: its own left
                    // border is the line between the two halves, and the other
                    // three continue the row's. Still ghost underneath, so
                    // hovering either half lights that half alone.
                    .border_1()
                    .border_color(hairline)
                    .h_8()
                    .w_7()
                    .rounded_l(px(0.))
                    .rounded_r(radius),
                says,
                new_session_menu(projects, active_idx, agents, target),
            ))
        })
}

/// Where a new session can go: a project, or — where there is a choice — an
/// agent.
///
/// Two lists in one menu because they answer the same question from two sides.
/// A project row starts the default agent there and **selects that project on
/// the way**, which is the same thing the project row's own *New session* entry
/// does: a session bound to a root the rail is not showing is an agent nobody is
/// watching. An agent row starts in the project already selected, since the
/// caret's whole promise is that the row above it is unchanged.
///
/// The project already selected is checked and still pickable, unlike the
/// workspace switcher's current row: picking it is not a no-op, it starts a
/// session exactly as the button above would.
fn new_session_menu(
    projects: Vec<(usize, SharedString)>,
    active: usize,
    agents: Vec<SharedString>,
    shell: WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + use<> {
    move |menu, _, _cx: &mut App| {
        let mut menu = menu;
        // One project is not a choice, and a heading over a single row that
        // repeats the button above it says there was one.
        if projects.len() > 1 {
            menu = menu.label("Start in");
            for (idx, label) in &projects {
                let (idx, label, target) = (*idx, label.clone(), shell.clone());
                menu = menu.item(
                    crate::controls::menu_item(label)
                        .icon(Icon::new(IconName::Folder))
                        .checked(idx == active)
                        .on_click(move |_, window, cx: &mut App| {
                            target
                                .update(cx, |shell: &mut Shell, cx| {
                                    shell.new_session_in(idx, window, cx);
                                })
                                .ok();
                        }),
                );
            }
        }
        // Only where there is a choice. A list of one agent is a control that
        // exists to disappoint, and it would sit under a heading naming a
        // decision nobody has.
        if agents.len() > 1 {
            if projects.len() > 1 {
                menu = menu.separator();
            }
            menu = menu.label("With agent");
            for (i, name) in agents.iter().enumerate() {
                let (name, target) = (name.clone(), shell.clone());
                menu = menu.item(
                    crate::controls::menu_item(name)
                        .icon(Icon::new(IconName::Bot))
                        .on_click(move |_, window, cx: &mut App| {
                            target
                                .update(cx, |shell: &mut Shell, cx| {
                                    shell.new_session_with(i, window, cx);
                                })
                                .ok();
                        }),
                );
            }
        }
        menu
    }
}

/// The two lists, as a segmented control.
///
/// In the header rather than in the scrolling content: it is the thing that says
/// what is underneath it, and a control that scrolls away from what it labels
/// leaves the reader with a list and no name for it.
///
/// The space and the hairline above it are the header's, not this control's:
/// they separate two halves of the header rather than decorating one element,
/// and the half below the line is this and the list under it.
///
/// **A `ButtonGroup` and not a segmented `TabBar`, because the two halves are
/// each half the rail.** A tab is sized by its label, and `TabBar` lays its tabs
/// out inside a content-sized row of its own that nothing outside the library
/// can stretch — so *Projects* came out a third of the width of *All sessions*
/// and the pair sat against the left edge with the rest of the rail empty beside
/// them. Every button in a group takes the instance style it is built with, so
/// `flex_1` on each over a `w_full` group is an even split at any rail width.
fn tab_bar(active: RailTab, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let target = cx.entity().downgrade();
    ButtonGroup::new("rail-tabs")
        .ghost()
        .outline()
        .small()
        .w_full()
        .children(RailTab::ALL.map(|tab| {
            crate::controls::action(tab.label())
                .label(tab.label())
                .selected(tab == active)
                .flex_1()
        }))
        .on_click(move |clicked, _, cx: &mut App| {
            let Some(tab) = clicked
                .first()
                .and_then(|ix| RailTab::ALL.get(*ix))
                .copied()
            else {
                return;
            };
            target
                .update(cx, |shell: &mut Shell, cx| shell.set_rail_tab(tab, cx))
                .ok();
        })
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_LABEL, RailTab, label_cap, new_session_hint, project_key, session_label, session_order,
        signal_hint,
    };
    use crate::chat::pane::SessionSignal;
    use onehand_core::config::PanelLayout;
    use std::path::Path;

    /// A project's row has to be named by the project, because the library
    /// keys a row's expanded state by whatever name the row is rendered under
    /// — and the name it hands out by default is the row's *position*, which
    /// moves when a project is pinned or another one is removed. What moved
    /// with it was the wrong project's expanded state.
    ///
    /// The folder name is not enough on its own: two checkouts of one
    /// repository are two projects sharing one folder name, and a worktree
    /// made from the rail is exactly that.
    #[test]
    fn two_projects_with_one_folder_name_are_two_different_rows() {
        assert_ne!(
            project_key(Path::new("/work/alpha/onehand")),
            project_key(Path::new("/work/beta/onehand")),
        );
    }

    /// The other half: the same project is the same row wherever it is drawn,
    /// which is what makes pinning safe to reorder the list.
    #[test]
    fn one_project_keeps_one_row_wherever_it_is_drawn() {
        assert_eq!(
            project_key(Path::new("/work/alpha/onehand")),
            project_key(Path::new("/work/alpha/onehand")),
        );
    }

    /// The rail is draggable, so the cap has to move with it.
    ///
    /// This is the whole point of deriving it: while it was one constant,
    /// dragging the rail wider bought nothing at all -- every name stayed cut
    /// at the same character, and the handle promised width the rows never
    /// spent.
    #[test]
    fn a_wider_rail_shows_more_of_a_name() {
        assert!(label_cap(PanelLayout::RAIL_MAX) > label_cap(PanelLayout::RAIL_MIN));
    }

    /// Growing with the width is not enough on its own: a rule that grows can
    /// still be wrong at both ends of the drag, leaving a name unreadably
    /// short where the rail is narrowest or running past what a row can draw
    /// where it is widest. The test above cannot see either, because both
    /// grow.
    ///
    /// The bounds are what the row has to be worth: sixteen characters is
    /// about where a conversation's title stops being a title, and past forty
    /// there is nothing left for the branch, the count and the mark that share
    /// the row.
    #[test]
    fn every_width_the_rail_can_have_leaves_a_name_worth_reading() {
        for width in [
            PanelLayout::RAIL_MIN,
            PanelLayout::default().rail_w,
            PanelLayout::RAIL_MAX,
        ] {
            let cap = label_cap(width);
            assert!(
                (16..=40).contains(&cap),
                "a {width}px rail caps a name at {cap} characters"
            );
        }
    }

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

    /// The whole reason the flat list exists: a session that wants an answer
    /// goes above one that is merely the last one looked at.
    #[test]
    fn a_session_wanting_an_answer_outranks_the_one_just_read() {
        let parked = session_order(Some(SessionSignal::AwaitingUser), Some(9));
        let just_read = session_order(None, Some(0));
        assert!(parked < just_read, "{parked:?} vs {just_read:?}");
    }

    /// Underneath the signals, recency is what is left — and a session the
    /// recency list has never seen has not been visited, which is the opposite
    /// of having been visited longest ago.
    #[test]
    fn with_nothing_asking_the_last_one_looked_at_leads_and_the_unseen_trail() {
        let seen = session_order(None, Some(0));
        let older = session_order(None, Some(3));
        let never = session_order(None, None);
        assert!(seen < older);
        assert!(older < never);
    }

    /// Two tabs, two names, and every tab in the list has one — a segmented
    /// control with a blank half is one nobody can press on purpose.
    #[test]
    fn each_tab_names_itself_and_no_two_alike() {
        let labels: Vec<&str> = RailTab::ALL.iter().map(|tab| tab.label()).collect();
        assert!(labels.iter().all(|label| !label.trim().is_empty()));
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn a_session_row_prefers_the_conversations_own_name() {
        assert_eq!(
            session_label(Some("Fix the login flow"), "Claude Code", MAX_LABEL),
            "Fix the login flow"
        );
    }

    /// Until a conversation has been prompted it has no name of its own, and a
    /// blank row would be worse than a repeated one.
    #[test]
    fn an_unprompted_session_falls_back_to_its_agent() {
        assert_eq!(session_label(None, "Claude Code", MAX_LABEL), "Claude Code");
    }

    /// A first prompt is free text and users paste paragraphs into it. The row
    /// is a fixed-height anchor, so the cap is what keeps a pasted essay from
    /// deciding the rail's width.
    #[test]
    fn a_long_title_is_capped() {
        let label = session_label(Some(&"a".repeat(MAX_LABEL * 3)), "Claude Code", MAX_LABEL);
        assert_eq!(label.chars().count(), MAX_LABEL);
        assert!(label.ends_with('…'));
    }
}
