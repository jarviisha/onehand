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
    Anchor, AnyElement, App, ClickEvent, Context, Div, ElementId, Hsla, InteractiveElement,
    IntoElement, ParentElement, Rems, SharedString, Stateful, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, px, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::sidebar::{Sidebar, SidebarCollapsible};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Side, Sizable as _, StyledExt};
use onehand_core::agent::Session;
use std::rc::Rc;

/// Names are structural anchors, not content: cap them so a deep path cannot
/// push a popup's width around. This is the bound for the one place a name is
/// still cut at a character — a menu row, which sizes its popup by its own
/// contents and sits on a surface the rail's fade machinery knows nothing
/// about. Every name in the rail's own list is cut in pixels instead, by
/// [`faded`].
const MAX_LABEL: usize = 24;

/// How wide the fade at the end of an overlong name is.
///
/// Rems, because the fade ends *text* and zoom moves the rem base: fixed in
/// pixels it would swallow three characters at one zoom and half of one at
/// another.
const FADE_W: Rems = rems(1.25);

/// The fill a hovered rail row takes: the selected fill at eight tenths,
/// which is the component library's own convention for a hovered sidebar row,
/// so a hovered row and the selected one stay apart without a second token.
///
/// One function because two kinds of reader depend on the same ingredient:
/// the rows *paint* this over the well, and [`row_surfaces`] *composites* it
/// over the well to know what colour is then on screen. The fade at the end
/// of a name is invisible only while those two agree, and while the `0.8` was
/// written out at each site there was nothing to keep them agreeing.
fn hover_fill(cx: &App) -> Hsla {
    cx.theme().sidebar_accent.opacity(0.8)
}

/// The two fills a rail row can be showing: at rest, and under the pointer.
///
/// Worked out once and handed around because the fade at the end of a name is
/// painted *in* them, and each has to be the composited colour actually on
/// screen: the hover fill is [`hover_fill`] over the well, so the fade's
/// endpoint is that blend and neither ingredient. The active row's fill does
/// not move under the pointer, so its pair is one colour twice.
fn row_surfaces(active: bool, cx: &App) -> (Hsla, Hsla) {
    let well = cx.theme().muted;
    match active {
        true => {
            let lit = well.blend(cx.theme().sidebar_accent);
            (lit, lit)
        }
        false => (well, well.blend(hover_fill(cx))),
    }
}

/// A name cut in pixels, fading into the row where its room ends, instead of
/// being cut at a character with an ellipsis.
///
/// The cut used to be counted in characters: a cap derived from the rail's
/// width through an assumed average glyph, charged again for the nest rule's
/// inset and again for the footnote — three guesses about pixels made from a
/// string, each wrong by a character or two in either direction, and a miss
/// on the long side was a word clipped mid-letter with nothing on screen to
/// say so. The fade is drawn where the room actually ends, so the guesses go
/// with it. It is also the honest mark: an ellipsis written into the string
/// asserts there is more even when the name happened to fit exactly, while a
/// fade only takes what is actually leaving.
///
/// The overlay is painted in the row's own fill, which is why it takes the two
/// surfaces instead of reading a token: what is behind the name changes under
/// the pointer, and a fade into the resting colour over a hovered row is a
/// smudge on exactly the row being looked at. Painted in the right colour it
/// is invisible wherever the name already ended — a gradient into the colour
/// it lies on — so nothing here needs to know whether the name overflowed,
/// which is a question about pixels a string cannot answer anyway.
fn faded(text: SharedString, group: SharedString, rest: Hsla, hovered: Hsla) -> Div {
    fn toward(surface: Hsla) -> gpui::Background {
        gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(surface.alpha(0.), 0.),
            gpui::linear_color_stop(surface, 1.),
        )
    }
    div()
        .relative()
        .overflow_hidden()
        .whitespace_nowrap()
        .child(text)
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .w(FADE_W)
                .bg(toward(rest))
                .group_hover(group, move |fade| fade.bg(toward(hovered))),
        )
}

/// One row of the rail's list, drawn by the rail itself.
///
/// The library's `SidebarMenuItem` drew these until the row's label had to be
/// an element rather than a string. That component holds its label as a bare
/// `SharedString` inside its own clipping box — no hook for a tooltip on it,
/// no ellipsis, nowhere to hang the fade — so every answer to an overlong
/// name was a guess made outside the row about what would fit inside it.
/// Owning the row is what buys the label the three things it needs: the fade
/// at its end, the full name on hover, and a cut made in pixels rather than
/// characters.
///
/// Handlers ride in `Rc`s because `Sidebar` clones its content on every frame
/// it draws — its child bound is `Clone` — and this is what makes the clone
/// cheap.
///
/// No collapse handling anywhere in it: the rail never collapses to an icon
/// column (an icon-width rail is ten identical folder icons), so a row has
/// exactly one shape.
#[derive(Clone)]
struct RailRow {
    /// One string naming the row twice over: the element id gpui keys the
    /// row's state by, and the hover group the fade overlay watches. A row is
    /// named by what it *is* — a project by its path, a session by its uid —
    /// never by its place in the list, because a place changes hands when the
    /// list re-sorts and whatever state rode on it changes hands too.
    key: SharedString,
    icon: Option<IconName>,
    label: SharedString,
    /// The full text behind the row, one line per entry, offered on hover.
    /// What the fade cuts has to be readable somewhere, and the row itself is
    /// the only hover target that exists on every row.
    hint: Vec<SharedString>,
    active: bool,
    on_click: RowClick,
    /// The right-click menu, on every row that has one — the same builder the
    /// active row's ••• is handed.
    menu: Option<RowMenu>,
    /// Everything after the label: footnotes, marks, controls. A builder and
    /// not an element, because elements are single-use and this row is cloned.
    suffix: Option<RowSuffix>,
}

/// The row's handlers, named so the struct above reads as a row and not as a
/// wall of `dyn Fn` signatures.
type RowClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type RowMenu = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;
type RowSuffix = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

impl RailRow {
    fn new(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            key: key.into(),
            icon: None,
            label: label.into(),
            hint: Vec::new(),
            active: false,
            on_click: Rc::new(on_click),
            menu: None,
            suffix: None,
        }
    }

    fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    fn hint(mut self, hint: Vec<SharedString>) -> Self {
        self.hint = hint;
        self
    }

    fn menu(
        mut self,
        menu: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(menu));
        self
    }

    fn suffix(mut self, suffix: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        self.suffix = Some(Rc::new(suffix));
        self
    }

    fn render(self, window: &mut Window, cx: &mut App) -> AnyElement {
        let (rest, hovered) = row_surfaces(self.active, cx);
        let (accent, hover, accent_fg) = (
            cx.theme().sidebar_accent,
            hover_fill(cx),
            cx.theme().sidebar_accent_foreground,
        );
        let on_click = self.on_click;
        let hint = self.hint;
        let has_hint = !hint.is_empty();
        let row = div()
            .id(ElementId::Name(self.key.clone()))
            .group(self.key.clone())
            .h_flex()
            .items_center()
            .w_full()
            .h_7()
            .px_2()
            .gap_x_2()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .text_sm()
            .map(|row| match self.active {
                true => row.font_medium().bg(accent).text_color(accent_fg),
                false => row.hover(move |row| row.bg(hover).text_color(accent_fg)),
            })
            .when_some(self.icon, |row, icon| row.child(Icon::new(icon).size_4()))
            .child(
                faded(self.label, self.key.clone(), rest, hovered)
                    .flex_1()
                    .min_w_0(),
            )
            .when_some(self.suffix, |row, suffix| row.child(suffix(window, cx)))
            .on_click(move |event, window, cx| on_click(event, window, cx))
            .when(has_hint, |row| {
                row.tooltip(move |window, cx| {
                    let hint = hint.clone();
                    Tooltip::element(move |_, _| div().v_flex().gap_0p5().children(hint.clone()))
                        .build(window, cx)
                })
            });
        match self.menu {
            Some(menu) => row
                .context_menu(move |popup, window, cx| menu(popup, window, cx))
                .into_any_element(),
            None => row.into_any_element(),
        }
    }
}

/// One row of the rail's list, and whatever is nested under it.
#[derive(Clone)]
struct Row {
    item: RailRow,
    /// A project's sessions, drawn inside its rule. Empty for every row in the
    /// flat list, which nests nothing.
    children: Vec<RailRow>,
}

impl Row {
    fn flat(item: RailRow) -> Self {
        Self {
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
/// flex column with a gap. **The gap has to be here.** `SidebarGroup` wraps
/// its children in `div().gap_2().flex_col()`, and a gpui `div` starts at
/// `display: block` while `flex_col` sets only the direction — so both of
/// those declarations do nothing there, and the space between rows was always
/// this column's to draw.
///
/// The rows inside are the rail's own ([`RailRow`]) rather than the library's,
/// and this is the second half of the same story: the state fix above could be
/// had by supplying our own item type *around* `SidebarMenuItem`, but the
/// label inside it is a bare string in the library's own clipping box, and the
/// fade, the tooltip and a cut made in pixels all need the label to be an
/// element the rail owns. `Sidebar` is still the panel — the frame, the
/// scroll, the header and footer slots — which is the half of the component
/// worth keeping.
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
        let nest = cx.theme().sidebar_border;
        div()
            .v_flex()
            .gap_2()
            .children(self.rows.into_iter().map(|row| {
                let children = row.children;
                div()
                    .v_flex()
                    .child(row.item.render(window, cx))
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
                                .children(
                                    children.into_iter().map(|child| child.render(window, cx)),
                                ),
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
fn project_key(path: &std::path::Path) -> SharedString {
    SharedString::from(format!("rail-project-{}", path.display()))
}

/// The branch name is the *least* important thing on a folder row -- it must
/// never cost the project label its space. The label is `flex_1` and the
/// suffix takes its natural width, so an unbounded branch wins outright: a row
/// for `fix/architecture-hardening-and-open-telemetry` pushed its own project
/// name to zero width. Capping the branch is what keeps the label first.
const MAX_BRANCH_W: gpui::Pixels = px(72.);

/// The footnote beside a session row's label -- the agent on a tree row, the
/// project on a flat one -- is about *how* the conversation is being run, so
/// it is capped hard: the title is what the user is reading the row for.
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
/// thing it exists to get you to. The exception is [`rail_row_filled`], and
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
    let (hover, accent_fg) = (hover_fill(cx), cx.theme().sidebar_accent_foreground);
    row_shape(id, icon, label, cx).hover(move |row| row.bg(hover).text_color(accent_fg))
}

/// The one filled row in the rail: *New session*.
///
/// This row has now worn all three coats, and each move had a reason. A loud
/// fill was taken off it early, because a rail is chrome and a filled row in
/// the panel's strongest colour shouts over the thing the panel exists to get
/// you to. The hairline outline that replaced it said "this one is different"
/// for the price of one pixel — and said the *wrong* thing: a bordered box
/// with a label on its left and a caret at its far end is the anatomy of an
/// input, and the rail's one action read as a field waiting to be typed in.
/// What tells a button from an input is the fill, so it fills — in the
/// theme's secondary triple, which is what the component library paints an
/// ordinary filled button with: enough fill to say *button*, nowhere near the
/// strongest on the surface, which stays with the selected row.
///
/// Hover is the same triple's second step (`secondary_hover`). The third,
/// `secondary_active`, marks the caret half while the menu it opened is up —
/// the one half that *has* an open state to mark; this half acts on the press
/// and is holding nothing open afterwards.
///
/// **The seam of the split control is a sliver of the well between the
/// halves**, drawn by the caller's gap — a border in the hairline token sat
/// on this fill with next to no contrast and disappeared. This half only
/// squares its right edge when it is about to be joined, which is also the
/// caller's call.
pub(crate) fn rail_row_filled(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    cx: &App,
) -> Stateful<Div> {
    let (fill, fill_fg, hover) = (
        cx.theme().secondary,
        cx.theme().secondary_foreground,
        cx.theme().secondary_hover,
    );
    row_shape(id, icon, label, cx)
        .bg(fill)
        .text_color(fill_fg)
        .hover(move |row| row.bg(hover))
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
/// Two draw exactly this, so it is built once: the ••• a project row and a
/// session row carry while active. They differ in the sentence and the
/// builder, and in nothing about the wiring — which is what they proved by
/// having been written out twice before this was extracted. (The caret beside
/// *New session* used to be the third; it is the right half of a filled split
/// control now, and a library button dressed to match a hand-filled half is
/// two components pretending to be one, so it draws itself.)
///
/// `occlude`, because the button sits inside something whose own click already
/// means something — selecting a session, selecting a project — and opening a
/// menu must not do that on its way past.
///
/// The wrapping closure is what lets one builder serve both this and a row's
/// right-click menu: the row's context-menu host hands its builder
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
const LABEL_SHAPE_CAP: usize = 80;

/// What a session row is called: the conversation's own name once it has one,
/// otherwise the agent that runs it.
///
/// Separate and pure because it is the rule, not the rendering. Every session
/// on a root used to be labelled with its agent's name, so three sessions on
/// one project read "Claude Code" three times and the rail could not be used to
/// tell them apart -- which is the one thing a session-first rail is for.
fn session_label(title: Option<&str>, agent: &str) -> SharedString {
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
enum Note {
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
    note: Note,
    cx: &mut Context<Shell>,
) -> RailRow {
    let uid = session.uid;
    let state = shell.session_row(uid, cx);
    let signal = state.signal;
    let footnote = match &note {
        // Only alongside a conversation title, and only where the project runs
        // more than one agent: where the row has fallen back to the agent's
        // name, the suffix would repeat the label it sits next to.
        Note::Agent { among_many } => (*among_many && state.title.is_some())
            .then(|| SharedString::from(session.title().to_string())),
        // Always: the project is the one thing the flat row cannot say any
        // other way, and it is true whether or not the conversation has a name.
        Note::Project(project) => Some(project.clone()),
    };
    let label = session_label(state.title.as_deref(), session.title());
    // The whole name, however long, for the hover: it is what the fade at the
    // row's edge may have cut.
    let hint = state
        .title
        .clone()
        .unwrap_or_else(|| SharedString::from(session.title().to_string()));
    // The key carries nothing about which list drew the row, because only one
    // list is on screen at a time -- and it is the same key in both, so the
    // row's element state survives the tab switch.
    let key = SharedString::from(format!("rail-session-{uid}"));
    let suffix_key = key.clone();
    // A weak handle because both menu closures outlive this frame.
    let menu_target = cx.entity().downgrade();
    let suffix_target = menu_target.clone();

    RailRow::new(
        key,
        label,
        cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| {
            shell.select_root_session(root_idx, session_idx, window, cx);
        }),
    )
    .active(active)
    .hint(vec![hint])
    .menu(session_menu(root_idx, session_idx, uid, menu_target))
    .suffix(move |_, cx: &mut App| {
        let suffix_target = suffix_target.clone();
        let (rest, hovered) = row_surfaces(active, cx);
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .flex_shrink(1.)
            .min_w_0()
            .when_some(footnote.clone(), |row, note| {
                row.child(
                    faded(note, suffix_key.clone(), rest, hovered)
                        .max_w(MAX_AGENT_W)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground),
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

/// What a project row says on hover: the whole of everything the row cuts.
///
/// The full name first, then the branch, the count in words, and the root's
/// path -- the last because the label is a folder name and two projects can
/// share one. This used to hang off the git suffix alone, which meant a
/// project that was neither a repository nor had changes drew no suffix and so
/// had nothing to hover; the row is its own hover target now, so every project
/// answers.
fn project_hint(
    label: &str,
    branch: Option<&SharedString>,
    changed: usize,
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
    hint.push(path.clone());
    hint
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

/// Whether naming the agent on a project's session rows tells the reader
/// anything.
///
/// It does exactly when the sessions disagree about it. Configured agents are
/// the wrong count and were the one this used: a second entry in the settings
/// file put the same word — truncated, since it shares the row with the
/// conversation's title — on every session of every project, including all the
/// projects running one agent apiece. What the footnote is for is telling two
/// rows apart, so the question it answers has to be asked of the rows.
fn runs_more_than_one_agent<'a>(agents: impl Iterator<Item = &'a str>) -> bool {
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
fn folder_row(
    shell: &Shell,
    window_state: &WorkspaceWindow,
    root_idx: usize,
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
                    is_active && active_session == i,
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
    let suffix_key = key.clone();

    // A weak handle for the caret, which outlives this frame as the menus do.
    let fold_target = cx.entity().downgrade();

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
        .hint(project_hint(&root.label, branch.as_ref(), changed, &path))
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
        // (The click handler rides in `RailRow::new` above.)
        .menu(project_menu(root_idx, pinned, is_repo, menu_target))
        .suffix(move |_, cx: &mut App| {
            let (suffix_target, fold_target) = (suffix_target.clone(), fold_target.clone());
            let fold_path = fold_path.clone();
            let (rest, hovered) = row_surfaces(is_active, cx);
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
                .when_some(branch.clone().filter(|_| is_active), |row, branch| {
                    row.child(faded(branch, suffix_key.clone(), rest, hovered).max_w(MAX_BRANCH_W))
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
                .into_any_element()
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
        let project = SharedString::from(root.label.clone());
        for (session_idx, session) in root.sessions.iter().enumerate() {
            let uid = session.uid;
            let signal = shell.session_row(uid, cx).signal;
            rows.push((
                session_order(signal, recency.get(&uid).copied()),
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
    // Only so the Settings dialog can size itself against the frame it opens
    // in; nothing here holds the borrow past this call.
    window: &Window,
    cx: &mut Context<Shell>,
    // `use<>`: the returned sidebar is fully owned (every string is cloned and
    // every handler is an Rc), so it must not capture the borrows of `self` and
    // `cx` that edition 2024 would otherwise infer -- the caller needs `cx` back
    // to attach its action handlers.
) -> impl IntoElement + use<> {
    let name = window_state.workspace.name.clone();
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
            .map(|idx| folder_row(window_state_shell, window_state, idx, cx))
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
        // **The well, asked for by name rather than through the sidebar
        // token.** The rail sat on the reading surface for a while, separated
        // from the conversation by the hairline down its edge alone -- the one
        // piece of chrome in the window still dressed as a place text is read.
        //
        // The well, asked for here rather than left to the library's own
        // `sidebar` token: that token ships with a value of its own and the ramp
        // writes the reading surface into it, so the panel would come up level
        // with the conversation beside it. The library applies the caller's
        // refinement after its own `bg`, which is what lets this win.
        //
        // **The rail is the only panel in the window still lifted off the
        // reading surface**, and it is the only one that is not about the work:
        // a workspace, its projects, its sessions. The two dock cards used to
        // take the same step, which made lifted mean nothing more precise than
        // "not the conversation", and with both docks open left the conversation
        // as the one region on screen that nothing had raised. They are flat on
        // the reading surface now, marked by their borders, so this step says
        // what it always meant to.
        //
        // Nothing else about the rail moves with it: `sidebar_accent` and the
        // selected fill are both well clear of this step in either palette, so a
        // hovered row and a marked one still read.
        .bg(cx.theme().muted)
        // **No line down the rail's edge, because the fill is the edge.**
        // `Sidebar` draws a 1px right border of its own and this turns it off:
        // the rail is on the well and the conversation beside it is on the
        // reading surface, and a surface that changes at a seam already says
        // where the seam is. Ruled as well, it was a line drawn along a
        // boundary that was not in doubt.
        //
        // **What makes that safe to say is a number rather than a taste**: those
        // two surfaces are the ramp's own reading surface and its well, a pair
        // the ramp's tests hold at 1.14 or better in either palette. This edge
        // and the transparent resize handle beside it were changed together and
        // each could otherwise be read as leaning on the other; neither does --
        // both lean on that step.
        //
        // This has been both ways. While the rail was on the reading surface
        // there was nothing else marking that seam, so the border had to stay --
        // and before *that* the library's drag handle drew a rule hard against
        // it in the same colour, which read as a 2px edge no single declaration
        // accounted for. The handle is drawn in nothing at rest now; it is still
        // the affordance and still brightens under a drag.
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
        // hover highlight, meant for one row of controls. Stacked triggers
        // inside it made hovering any one of them light up the whole block.
        //
        // One row, where there were three. Agents and the keyboard table are
        // pages of Settings now, and a rail footer listing every page of one
        // dialog is a table of contents for a dialog nobody has opened yet.
        .footer(
            div()
                .v_flex()
                .gap_0p5()
                .w_full()
                .min_w_0()
                .child(crate::dialogs::settings(window, cx)),
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
    let accent = cx.theme().sidebar_accent;
    let (hover, accent_fg) = (hover_fill(cx), cx.theme().sidebar_accent_foreground);
    let (rest, hovered) = row_surfaces(false, cx);
    let hover_name = name.clone();
    let row = lead_row(
        div()
            .id("workspace-identity")
            .group("workspace-identity")
            .h_flex()
            .items_center()
            .w_full()
            .min_w_0()
            .px_2()
            .gap_x_2()
            .rounded(radius)
            .cursor_pointer()
            .hover(move |row| row.bg(hover).text_color(accent_fg))
            // The name leads it, because this row is the one place a workspace
            // name is written and it is written on one line: users put
            // sentences in that field, and truncated there it could be read
            // nowhere at all. The row is its own hover target, so the tooltip
            // that says what a press does is also the only one that can carry
            // the whole name.
            .tooltip(move |window, cx| {
                let name = hover_name.clone();
                Tooltip::element(move |_, _| {
                    div()
                        .v_flex()
                        .gap_0p5()
                        .child(name.clone())
                        .child("Workspaces, and the projects in this one")
                })
                .build(window, cx)
            }),
    )
    // Full ink, not muted. It was the dimmest thing in the header while
    // standing for the thing the header is about, so the eye read the row
    // beginning at the name and the mark before it as decoration.
    .child(Icon::new(IconName::LayoutDashboard).size_4())
    // Semibold rather than the header block's medium: this is the one name in
    // the window that says which workspace all of it belongs to.
    //
    // Faded like every list row's name, and one corner is taken knowingly:
    // while this row's *menu* is open the trigger fills it with the accent,
    // and a fade painted for the resting fill is then a step off -- visible
    // only on a name long enough to fade, while its menu is open, with the
    // pointer somewhere else. The fade cannot follow that fill because the
    // open flag is applied by the menu host after this row is already built.
    .child(
        faded(name, "workspace-identity".into(), rest, hovered)
            .flex_1()
            .min_w_0()
            .font_semibold(),
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
pub(crate) fn ellipsize_front(s: &str, max: usize) -> SharedString {
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

    let radius = cx.theme().radius;
    let (fill, fill_fg, fill_hover, open_fill) = (
        cx.theme().secondary,
        cx.theme().secondary_foreground,
        cx.theme().secondary_hover,
        cx.theme().secondary_active,
    );
    let primary = lead_row(rail_row_filled(
        "new-session",
        IconName::Plus,
        "New session",
        cx,
    ))
    // The two halves of one control, so the seam between them is square
    // and the outer edges keep the radius. Only while there is a caret to
    // join: a lone row squared off on one side reads as clipped.
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
        // The seam between the halves is a sliver of the well showing
        // through, not a border: a hairline in the border token sits on the
        // secondary fill with next to no contrast and disappeared there. The
        // well against that fill is the exact contrast that makes the button
        // itself visible, so the seam it draws can never be fainter than the
        // control it splits. With no caret there is one child and the gap
        // draws nothing.
        .gap(px(1.))
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
            // The other half of one filled control, drawn as a div rather
            // than a library button so the two halves share one fill and one
            // hover rule exactly. The seam between the halves is the bar's
            // 1px gap of well; hovering either half lights that half alone,
            // which is what says the control is split. `MenuTrigger` because
            // a div is not `Selectable` on its own, and the open state takes
            // the triple's third step so a held-open caret reads as pressed.
            //
            // Sized to the header row beside it rather than to the ••• the
            // menus share a look with: this is the right half of that
            // control, and a control half the height of its own other half
            // is two controls that happen to touch.
            let caret = div()
                .id("new-session-target")
                .h_flex()
                .items_center()
                .justify_center()
                .flex_none()
                .h_8()
                .w_7()
                .bg(fill)
                .text_color(fill_fg)
                .cursor_pointer()
                .hover(move |half| half.bg(fill_hover))
                .rounded_r(radius)
                .tooltip(move |window, cx| Tooltip::new(says).build(window, cx))
                .child(Icon::new(IconName::ChevronDown).size_4());
            // No `occlude` here, unlike the ••• menus: those sit *inside* a
            // row whose own click means something, while this caret is the
            // primary half's sibling with nothing behind it to protect.
            // The wrap re-borrows `Context<PopupMenu>` down to the `&mut App`
            // the builder is written against, as the ••• host does.
            let build = new_session_menu(projects, active_idx, agents, target);
            bar.child(
                crate::controls::MenuTrigger::new(caret, open_fill)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                        build(menu, window, cx)
                    }),
            )
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
/// **A switch drawn here, after both of the library's answers were tried.** The
/// two halves are one choice between two states, and what says so is a track
/// with a raised plate in one end of it — the shape needs a fill, an inset and
/// two halves of equal width, and neither component gives all three. A
/// `ButtonGroup` splits evenly (`flex_1` on each over a `w_full` group) but has
/// no track, so it reads as two outlined controls that happen to disagree. A
/// segmented `TabBar` is the track and the plate exactly, and sizes every tab to
/// its own label inside a `flex_shrink_0` nothing outside the library can
/// stretch — so *Projects* came out two thirds the width of *All sessions*, both
/// against the left edge of a bar as wide as the rail.
///
/// So the track is the library's own segmented fill and the two halves are
/// `flex_1`. Nothing else here is invented: the fills come from the theme, the
/// radius is the theme's, and the pointer is the same promise every other
/// clickable in this file makes.
fn tab_bar(active: RailTab, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let theme = cx.theme();
    // **The selected half is `accent`, not the reading surface.**
    //
    // It was `background`, on the reasoning that a raised plate is drawn in the
    // surface the control sits on -- which was true while the rail was drawn in
    // that surface too. The rail is drawn in the well now, so the plate became
    // the one thing in the window painted a step *below* what it sits on: a hole
    // rather than a plate, and at this size the shadow under it is not enough to
    // say which.
    //
    // `accent` is the app's own "this one, among several", and it is what the
    // terminal's tabs and the Workbench's mode chips already use. Three places
    // that mean the same thing now spell it the same way, which is the point --
    // a code learned once.
    let (track, plate, radius) = (theme.tokens.tab_bar_segmented, theme.accent, theme.radius);
    let (ink, ink_on) = (theme.muted_foreground, theme.accent_foreground);

    div()
        .h_flex()
        .w_full()
        .gap_0p5()
        .p_0p5()
        .rounded(radius)
        .bg(track)
        .children(RailTab::ALL.map(|tab| {
            let on = tab == active;
            div()
                .id(tab.label())
                .h_flex()
                .justify_center()
                .flex_1()
                .min_w_0()
                .rounded(radius)
                .cursor_pointer()
                .text_xs()
                .text_color(if on { ink_on } else { ink })
                // No shadow under it. It was there to lift a plate drawn in the
                // same value as its surroundings; a fill that differs does the
                // lifting by itself, and a drop shadow over a near-black surface
                // is invisible anyway.
                .when(on, |half| half.bg(plate))
                .on_click(cx.listener(move |shell: &mut Shell, _, _, cx| {
                    shell.set_rail_tab(tab, cx);
                }))
                .child(tab.label())
        }))
}

#[cfg(test)]
mod tests {
    use super::{
        LABEL_SHAPE_CAP, RailTab, new_session_hint, project_key, runs_more_than_one_agent,
        session_label, session_order, signal_hint,
    };
    use crate::chat::pane::SessionSignal;
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

    /// A first prompt is free text and users paste paragraphs into it. What
    /// fits the row is the fade's business, in pixels — this bound is the cost
    /// one, keeping a pasted essay from being shaped whole on every frame the
    /// rail draws. It has to sit past what the widest rail can show, or the
    /// ellipsis it writes would reach the screen and the fade would be a lie.
    #[test]
    fn a_long_title_is_bounded_for_cost_not_for_fit() {
        let label = session_label(Some(&"a".repeat(LABEL_SHAPE_CAP * 3)), "Claude Code");
        assert_eq!(label.chars().count(), LABEL_SHAPE_CAP);
        assert!(label.ends_with('…'));
    }

    /// The footnote naming the agent is there to tell two rows apart, so the
    /// question is asked of the rows. A project whose sessions all run one
    /// agent gets a column of identical words out of it and nothing else —
    /// which is what it was doing, because the count it used was the agent
    /// menu's and a second entry there is enough to mark up every project in
    /// the workspace.
    #[test]
    fn one_agent_across_a_projects_sessions_is_not_worth_saying() {
        assert!(!runs_more_than_one_agent(std::iter::empty()));
        assert!(!runs_more_than_one_agent(["Claude Code"].into_iter()));
        assert!(!runs_more_than_one_agent(
            ["Claude Code", "Claude Code", "Claude Code"].into_iter()
        ));
    }

    /// And the other half: where they disagree it is the only thing on the row
    /// that says which is which.
    #[test]
    fn two_agents_in_one_project_are_worth_saying() {
        assert!(runs_more_than_one_agent(
            ["Claude Code", "Mock UI"].into_iter()
        ));
        // The disagreement can be anywhere in the list, not only at its head.
        assert!(runs_more_than_one_agent(
            ["Claude Code", "Claude Code", "Mock UI"].into_iter()
        ));
    }
}
