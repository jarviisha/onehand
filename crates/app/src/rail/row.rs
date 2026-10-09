//! What every row of the rail shares: the fills, the fade at the end of a
//! name, the controls a row carries, and the drags that reorder the tree.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, App, Context, Div, ElementId, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Rems, Render, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Window, div, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{DropdownMenu as _, PopupMenu};
use gpui_component::{ActiveTheme, Icon, Sizable as _};

/// Names are structural anchors, not content: cap them so a deep path cannot
/// push a popup's width around. This is the bound for the one place a name is
/// still cut at a character — a menu row, which sizes its popup by its own
/// contents and sits on a surface the rail's fade knows nothing about. Every
/// name in the rail's own list is cut in pixels instead, by [`faded`].
pub(super) const MAX_LABEL: usize = 24;

/// How wide the fade at the end of an overlong name is.
const FADE_W: Rems = rems(1.25);

/// The branch is the least important thing on a project row and must never
/// cost the project's name its space, so it is capped.
pub(super) const MAX_BRANCH_W: Rems = rems(4.5);

/// The fill a hovered rail row takes: the ramp's step the rail owns, whole.
/// Thinned, as the library thins its hovered sidebar rows, it sank into the
/// well it lies on.
pub(super) fn hover_fill(cx: &App) -> Hsla {
    cx.theme().sidebar_accent
}

/// The fill of the row that is chosen, the one on screen or the keyboard's,
/// one step past the hover.
pub(super) fn chosen_fill(cx: &App) -> Hsla {
    crate::theme::rail_chosen(cx)
}

/// The two opaque fills a rail row can be showing: at rest, and under the
/// pointer. A chosen row's fill does not move under the pointer, so its pair
/// is one colour twice. The fade and every overlay on a row paint in these.
pub(super) fn row_surfaces(chosen: bool, cx: &App) -> (Hsla, Hsla) {
    match chosen {
        true => (chosen_fill(cx), chosen_fill(cx)),
        false => (cx.theme().muted, hover_fill(cx)),
    }
}

/// A name cut in pixels, fading into the row where its room ends, instead of
/// being cut at a character with an ellipsis.
///
/// The band is painted in the row's own fill, `rest`, and `hovered` while the
/// pointer is on the `group`, so it is invisible wherever the name already
/// ended. That holds only while the box is the room and not the text, which
/// is why it stretches: on a box that shrink-wraps its string the band would
/// land on the last glyphs of a name that fitted. Whatever cannot stretch
/// keeps `truncate` and its ellipsis instead. The caller says the whole name
/// on hover.
pub(super) fn faded(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    group: SharedString,
    rest: Hsla,
    hovered: Hsla,
) -> Stateful<Div> {
    fn toward(surface: Hsla) -> gpui::Background {
        gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(surface.alpha(0.), 0.),
            gpui::linear_color_stop(surface, 1.),
        )
    }
    div()
        .id(id)
        .relative()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .child(text.into())
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

/// A control inside a row whose own click means something — opening a
/// session, folding a project — named for assistive technology, which the
/// library's button takes only from a text label.
///
/// A press on it stays on it: the row would otherwise take the same click on
/// its way past. Stopped on the way up rather than by occluding the row, which
/// would also take the pointer off the row and hide a control shown only
/// while the row is hovered, under the pointer reaching for it.
pub(super) fn labelled(
    id: impl Into<ElementId>,
    label: &'static str,
    control: impl IntoElement,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .aria_label(label)
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(control)
}

/// Hidden until the pointer is on the row, unless `shown`; the space stays,
/// so nothing beside it moves when it appears.
pub(super) fn on_hover(group: &SharedString, shown: bool, child: impl IntoElement) -> Div {
    div()
        .flex_none()
        .when(!shown, |d| {
            d.invisible().group_hover(group.clone(), |s| s.visible())
        })
        .child(child)
}

/// A control the rail draws: ghost, extra small, icon-only, in muted ink.
pub(super) fn rail_control(id: impl Into<ElementId>, icon: impl Into<Icon>, cx: &App) -> Button {
    crate::controls::action(id)
        .ghost()
        .xsmall()
        .icon(icon.into().text_color(cx.theme().muted_foreground))
}

/// A row's `⋯`: the same builder its right-click menu is handed. The wrap is
/// what lets one builder serve both hosts: this one hands over
/// `&mut Context<PopupMenu>`, the context-menu host `&mut App`, and the first
/// derefs to the second.
pub(super) fn menu_button(
    control: Button,
    tooltip: &'static str,
    build: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
) -> impl IntoElement {
    control
        .tooltip(tooltip)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
            build(menu, window, cx)
        })
}

/// What a project's row is called, as far as the window is concerned: by its
/// path and not its label, since two checkouts of one repository share a
/// folder name and are two projects.
pub(super) fn project_key(path: &std::path::Path) -> SharedString {
    SharedString::from(format!("rail-project-{}", path.display()))
}

/// A project row under the pointer, named by where it is *drawn*: that is
/// what `Workspace::move_root` takes, and pinned projects are drawn first.
#[derive(Clone, Copy)]
pub(super) struct ProjectDrag {
    pub(super) from: usize,
}

/// A session row under the pointer, named by its project and its place in it.
/// A type of its own because gpui dispatches a drop by the payload's type, so
/// a project row never offers to take a session; the project rides along so a
/// row of another project refuses it too.
#[derive(Clone, Copy)]
pub(super) struct SessionDrag {
    pub(super) root: usize,
    pub(super) from: usize,
}

/// What follows the pointer through a drag: the row's own name, on the
/// selected fill. `gpui` wants an entity here, so this is the smallest one
/// that can carry a string.
pub(super) struct DragGhost(pub(super) SharedString);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_0p5()
            .rounded(cx.theme().radius)
            .bg(chosen_fill(cx))
            .text_sm()
            .text_color(cx.theme().sidebar_accent_foreground)
            .child(self.0.clone())
    }
}

pub(super) fn ellipsize(s: &str, max: usize) -> SharedString {
    if s.chars().count() <= max {
        return SharedString::from(s.to_string());
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    SharedString::from(format!("{kept}…"))
}
