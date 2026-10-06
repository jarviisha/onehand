use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, App, ClickEvent, Context, Div, ElementId, Hsla, InteractiveElement,
    IntoElement, ParentElement, Rems, Render, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Window, div, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use std::rc::Rc;

/// Names are structural anchors, not content: cap them so a deep path cannot
/// push a popup's width around. This is the bound for the one place a name is
/// still cut at a character — a menu row, which sizes its popup by its own
/// contents and sits on a surface the rail's fade machinery knows nothing
/// about. Every name in the rail's own list is cut in pixels instead, by
/// [`faded`].
pub(super) const MAX_LABEL: usize = 24;

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
pub(super) fn hover_fill(cx: &App) -> Hsla {
    cx.theme().sidebar_accent.opacity(0.8)
}

/// The two fills a rail row can be showing: at rest, and under the pointer.
///
/// Worked out once and handed around because the fade at the end of a name is
/// painted *in* them, and each has to be the composited colour actually on
/// screen: the hover fill is [`hover_fill`] over the well, so the fade's
/// endpoint is that blend and neither ingredient. The active row's fill does
/// not move under the pointer, so its pair is one colour twice.
///
/// **That one-colour-twice arm is load-bearing, not a coincidence.** The row
/// it belongs to sets no hover style at all, and gpui allocates the element
/// state a group-hover repaint needs only for an element that has one — so a
/// selected row's fade would never be told the pointer had arrived. It stays
/// right today because there is nothing to repaint. Giving the selected row
/// any hover treatment means giving the fade a way to hear about it first,
/// or the fill moves under the pointer while the fade stays behind: the exact
/// smudge the two surfaces exist to prevent, on the one row being looked at.
pub(super) fn row_surfaces(active: bool, cx: &App) -> (Hsla, Hsla) {
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
///
/// **That last part is only true while the box is the room and not the text**,
/// which is why the stretch is written in here rather than left to the caller.
/// The band is pinned to this box's right edge, so on a box that shrink-wraps
/// its string the band lands on the last glyphs of a name that *fitted* —
/// `main` on a branch row came out as `m` dissolving into the fill, which is
/// the ellipsis's dishonesty back again in a worse form, since nothing says a
/// cut happened. Stretched, the right edge is where the room runs out, and the
/// band falls on empty track for every name short enough not to reach it.
/// Anything that cannot be stretched — a footnote capped at its own width, a
/// menu row sized by its popup — keeps `truncate` and its ellipsis instead.
pub(super) fn faded(text: SharedString, group: SharedString, rest: Hsla, hovered: Hsla) -> Div {
    fn toward(surface: Hsla) -> gpui::Background {
        gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(surface.alpha(0.), 0.),
            gpui::linear_color_stop(surface, 1.),
        )
    }
    div()
        .relative()
        .flex_1()
        .min_w_0()
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
pub(super) struct RailRow {
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
    /// The drag-and-drop this row takes part in, if it sits in an order that
    /// can be changed: it hangs `on_drag`, `drag_over` and `on_drop` on the
    /// row's own box. `None` on the rows that are not in any order — the
    /// *Start a session* offers, the empty states.
    ///
    /// Written at the call site rather than described to this struct, because
    /// the payload types differ per kind of row and the indices they carry are
    /// only in scope where the row is built.
    reorder: Option<RowReorder>,
}

/// The row's handlers, named so the struct above reads as a row and not as a
/// wall of `dyn Fn` signatures.
type RowClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type RowMenu = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;
type RowSuffix = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
type RowReorder = Rc<dyn Fn(Stateful<Div>) -> Stateful<Div>>;

/// Only for `when`: a row is built conditionally in two places now, and the
/// trait is a blanket `Sized` one with no required methods, so this is the whole
/// of what it costs to stop writing the same `if` out as a rebind.
impl gpui::prelude::FluentBuilder for RailRow {}

impl RailRow {
    pub(super) fn new(
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
            reorder: None,
        }
    }

    pub(super) fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub(super) fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub(super) fn hint(mut self, hint: Vec<SharedString>) -> Self {
        self.hint = hint;
        self
    }

    pub(super) fn menu(
        mut self,
        menu: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(menu));
        self
    }

    pub(super) fn suffix(
        mut self,
        suffix: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.suffix = Some(Rc::new(suffix));
        self
    }

    pub(super) fn reorder(
        mut self,
        reorder: impl Fn(Stateful<Div>) -> Stateful<Div> + 'static,
    ) -> Self {
        self.reorder = Some(Rc::new(reorder));
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
            .child(faded(self.label, self.key.clone(), rest, hovered))
            .when_some(self.suffix, |row, suffix| row.child(suffix(window, cx)))
            .on_click(move |event, window, cx| on_click(event, window, cx))
            .when(has_hint, |row| {
                row.tooltip(move |window, cx| {
                    let hint = hint.clone();
                    Tooltip::element(move |_, _| div().v_flex().gap_0p5().children(hint.clone()))
                        .build(window, cx)
                })
            });
        let row = match self.reorder {
            Some(reorder) => reorder(row),
            None => row,
        };
        match self.menu {
            Some(menu) => row
                .context_menu(move |popup, window, cx| menu(popup, window, cx))
                .into_any_element(),
            None => row.into_any_element(),
        }
    }
}

/// A project row under the pointer, named by where it is *drawn*.
///
/// The display position and not the roots index, because that is what
/// `Workspace::move_root` takes: the rail drags what it draws, and pinned
/// projects are drawn first.
#[derive(Clone, Copy)]
pub(super) struct ProjectDrag {
    pub(super) from: usize,
}

/// A session row under the pointer, named by its root and its place in it.
///
/// **A type of its own rather than a second variant of one drag enum**, because
/// gpui dispatches a drop by the payload's type: two kinds in one type means a
/// project row lights up under a session being dragged and the handler has to
/// refuse the drop afterwards, where two types means the row never offers in
/// the first place. The root rides along for the same reason one level down —
/// sessions reorder inside their own project, and a row of another project must
/// not take the drop.
#[derive(Clone, Copy)]
pub(super) struct SessionDrag {
    pub(super) root: usize,
    pub(super) from: usize,
}

/// What follows the pointer through a drag: the row's own name, on the fill a
/// selected row has.
///
/// The name and not a copy of the row, because the row's suffix is a live thing
/// — a mark that changes, a ••• that opens a menu — and none of it means
/// anything an inch from where it belongs. `gpui` wants an entity here, so this
/// is the smallest one that can carry a string.
pub(super) struct DragGhost(pub(super) SharedString);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_0p5()
            .rounded(cx.theme().radius)
            .bg(cx.theme().sidebar_accent)
            .text_sm()
            .text_color(cx.theme().sidebar_accent_foreground)
            .child(self.0.clone())
    }
}

/// One row of the rail's list, and whatever is nested under it.
#[derive(Clone)]
pub(super) struct Row {
    pub(super) item: RailRow,
    /// A project's sessions, drawn inside its rule. Empty for every row in the
    /// flat list, which nests nothing.
    pub(super) children: Vec<RailRow>,
}

impl Row {
    pub(super) fn flat(item: RailRow) -> Self {
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
pub(super) struct KeyedMenu {
    rows: Vec<Row>,
    collapsed: bool,
}

impl KeyedMenu {
    pub(super) fn new(rows: Vec<Row>) -> Self {
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
pub(super) fn project_key(path: &std::path::Path) -> SharedString {
    SharedString::from(format!("rail-project-{}", path.display()))
}

/// The branch name is the *least* important thing on a folder row -- it must
/// never cost the project label its space. The label is `flex_1` and the
/// suffix takes its natural width, so an unbounded branch wins outright: a row
/// for `fix/architecture-hardening-and-open-telemetry` pushed its own project
/// name to zero width. Capping the branch is what keeps the label first.
pub(super) const MAX_BRANCH_W: gpui::Rems = rems(4.5);

/// The footnote beside a session row's label -- the agent on a tree row, the
/// project on a flat one -- is about *how* the conversation is being run, so
/// it is capped hard: the title is what the user is reading the row for.
pub(super) const MAX_AGENT_W: gpui::Rems = rems(4.);

pub(super) fn ellipsize(s: &str, max: usize) -> SharedString {
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
    icon: impl Into<Icon>,
    label: &'static str,
    cx: &App,
) -> Stateful<Div> {
    // Resolved up front: the hover closure outlives this borrow of `cx`.
    let (hover, accent_fg) = (hover_fill(cx), cx.theme().sidebar_accent_foreground);
    row_shape(id, icon, label, cx).hover(move |row| row.bg(hover).text_color(accent_fg))
}

/// A [`rail_row`] drawn as the one on screen, the way a selected list row is:
/// the selected fill, its ink and a weight up, and no hover of its own, since
/// the fill does not move under the pointer on a row that is already chosen.
pub(crate) fn rail_row_marked(
    id: &'static str,
    icon: impl Into<Icon>,
    label: &'static str,
    cx: &App,
) -> Stateful<Div> {
    row_shape(id, icon, label, cx)
        .font_medium()
        .bg(cx.theme().sidebar_accent)
        .text_color(cx.theme().sidebar_accent_foreground)
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
    icon: impl Into<Icon>,
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
fn row_shape(
    id: &'static str,
    icon: impl Into<Icon>,
    label: &'static str,
    cx: &App,
) -> Stateful<Div> {
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
        .child(icon.into().size_4())
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
pub(super) fn lead_row(row: Stateful<Div>) -> Stateful<Div> {
    row.h_8().text_base().font_medium()
}

/// A control the rail draws: ghost, extra small, icon-only.
///
/// The pointer is not set here any more. It used to be, because the library
/// draws its buttons with the arrow cursor and every other thing in the rail
/// that does something shows a pointer — but that was true of every button in
/// the app, and the fix belongs where all of them are built rather than in the
/// one place somebody noticed it.
pub(super) fn rail_control(id: impl Into<ElementId>, icon: IconName) -> Button {
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
pub(super) fn menu_button(
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
