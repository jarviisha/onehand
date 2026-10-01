//! Popup menus a plugin can draw as the app does: rows that answer the
//! pointer, and a menu that opens below its trigger.

use gpui::{
    App, Context, DismissEvent, ElementId, Entity, Focusable as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, Window, div, px, rems,
};
use gpui_component::button::Button;
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::popover::Popover;
use gpui_component::{Sizable as _, Size, StyledExt as _};
use std::rc::Rc;

/// The inset a `PopupMenu` puts between its edge and a row's content.
///
/// The library's own number, hard-coded there in pixels rather than read from a
/// theme, and matched here in pixels for that reason — a rem would follow this
/// panel's zoom while the row it is cancelling out would not, and the two would
/// come apart at every size but one.
const MENU_INSET: gpui::Pixels = px(8.);

/// One row of a popup menu that does something, drawn with the pointer.
///
/// The same problem [`crate::action`] solves for buttons, in the one other place the
/// library leaves it: a `PopupMenu` row sets no cursor at all, so every menu in
/// this app drew the arrow over rows that act while the buttons an inch away
/// drew the pointer. Half the actions answering the pointer and half not is what
/// makes the cursor stop meaning anything.
///
/// **The content carries the cursor, stretched back over the row's own inset.**
/// The library gives no hook on the row itself — only on what goes inside it —
/// and content sitting within that inset leaves a strip at each end of every row
/// still drawing the arrow, which is the same half-rule one step smaller. So the
/// negative margin is not a layout trick: it is what makes the pointer's region
/// and the clickable region the same shape.
///
/// **Only for a row that acts.** A disabled entry keeps the library's default
/// and should, for the reason [`resting`] exists: a pointer over something that
/// refuses is a promise the control cannot keep.
pub fn menu_row<E: IntoElement>(
    render: impl Fn(&mut Window, &mut App) -> E + 'static,
) -> PopupMenuItem {
    PopupMenuItem::element(move |window, cx| {
        div()
            .h_flex()
            .items_center()
            .w_full()
            .mx(-MENU_INSET)
            .px(MENU_INSET)
            .cursor_pointer()
            .child(render(window, cx))
    })
}

/// A plain worded menu row that does something.
///
/// The shape almost every entry in the app's menus has, so it is spelled once:
/// [`menu_row`] is for the rows that draw something other than a line of text.
pub fn menu_item(label: impl Into<SharedString>) -> PopupMenuItem {
    let label = label.into();
    menu_row(move |_, _| div().child(label.clone()))
}

/// A menu that opens *below* its trigger, left-aligned to it.
///
/// The library's own `dropdown_menu` cannot say "below": every corner anchor
/// it offers pins a corner of the *menu* to the *top* of the trigger --
/// `TopLeft` lays the menu's first row over the control that opened it, and
/// `BottomLeft` hangs the whole menu above -- and the side-positioned path its
/// selects and tooltips use is not reachable from a menu trigger. What this
/// rides instead is the popover's own offset mechanism: [`Popover`] applies
/// the caller's style refinement to the popup's content, and a `top` inset
/// there is exactly how the popover nudges its own popup off its trigger.
/// The open state, the second-press close and the focus hand-off stay the
/// library's, and the click-out dismiss is the menu's own mouse-down-out.
/// What the inset moves is the *drawn* menu, which carries its own hitbox;
/// the popover's occluding wrapper stays where the positioner put it, over
/// the trigger.
///
/// **This mirrors the popover wiring inside
/// [`gpui_component::menu::DropdownMenu`]** -- the slot, the build-on-open,
/// the focus, the dismiss subscription -- and has to, because that wiring is
/// private to the library. Bumping the `gpui-component` rev means diffing
/// this function against it.
///
/// **The trigger's size is handed over and set here**, and the inset is that
/// size's height plus a quarter rem. The library keeps a button's size to
/// itself, so the inset cannot read it; restating one height for every trigger
/// left the menu half a rem below an extra-small one. Setting the size from the
/// same argument the inset is worked out from is what keeps the two from
/// drifting apart.
///
/// One ceiling, accepted for a trigger sitting at the top of its panel: the
/// inset lands *after* the positioner has clamped the popup into the viewport,
/// so in a window shorter than the menu the last rows overhang the bottom edge
/// rather than flipping above the trigger.
///
/// The menu entity is kept in window state and built once per open, not once
/// per frame: the content closure runs on every render while the popup is up,
/// and a menu rebuilt each frame forgets which row the keyboard was on.
/// Emptying the slot on dismiss is what lets the menu entity go -- the
/// subscription that empties it is keyed on the menu and holds the slot, so
/// a slot never emptied is a menu never dropped -- and the slot itself is
/// collected by the window a frame after the popup closes, since nothing
/// accesses it while closed.
///
/// # The id has to name what the menu acts on
///
/// Both the open state and that held menu are keyed by `id`, and **the rows
/// are frozen at the press**: they close over whatever the builder was handed
/// on the frame the menu opened. So an `id` that stays the same while the
/// thing underneath changes is a menu that survives the change with its old
/// rows and its old captures, now aimed at something else -- which for a row
/// that deletes is the wrong thing deleted. Nothing here can detect that, and
/// closing on any change is not the library's behaviour to give: what an `id`
/// carrying the session's or the project's identity buys instead is that the
/// key stops being accessed, so the window collects the state and the menu is
/// simply gone.
///
/// It must also differ from the trigger's own id, since the popover wraps the
/// trigger and two nested elements under one id collide.
pub fn menu_below(
    id: impl Into<ElementId>,
    trigger: Button,
    size: Size,
    build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> Popover {
    let build = Rc::new(build);
    let id = id.into();
    let slot_key = id.clone();
    Popover::new(id)
        .appearance(false)
        .overlay_closable(false)
        .trigger(trigger.with_size(size))
        // The quarter rem keeps the menu's edge off the trigger. In rems, so
        // a zoomed panel moves the menu with the button it belongs to.
        .top(rems(trigger_height(size) + 0.25))
        .content(move |_, window, cx| {
            let slot = window.use_keyed_state((slot_key.clone(), "menu-below"), cx, |_, _| {
                None::<Entity<PopupMenu>>
            });
            match slot.read(cx).clone() {
                Some(menu) => menu,
                None => {
                    let build = build.clone();
                    let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
                        build(menu, window, cx)
                    });
                    slot.update(cx, |state, _| *state = Some(menu.clone()));
                    menu.focus_handle(cx).focus(window, cx);
                    let popover = cx.entity();
                    window
                        .subscribe(&menu, cx, {
                            let slot = slot.clone();
                            move |_, _: &DismissEvent, window, cx| {
                                popover.update(cx, |state, cx| state.dismiss(window, cx));
                                slot.update(cx, |state, _| *state = None);
                            }
                        })
                        .detach();
                    menu
                }
            }
        })
}

/// How tall the library draws a button of `size`, in rems: its own heights,
/// restated because it keeps them private. A size given in pixels is read as
/// the small one, which is what nothing here hands over.
fn trigger_height(size: Size) -> f32 {
    match size {
        Size::XSmall => 1.25,
        Size::Small | Size::Size(_) => 1.5,
        Size::Medium => 2.,
        Size::Large => 2.75,
    }
}
