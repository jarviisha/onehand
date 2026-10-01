//! Popup menus a plugin can draw as the app does: rows that answer the
//! pointer, and a menu that opens below its trigger.

use gpui::{
    App, Context, DismissEvent, ElementId, Entity, Focusable as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, Window, div, px,
};
use gpui_component::StyledExt as _;
use gpui_component::button::Button;
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::popover::Popover;
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
/// **Below is where the library's top-left anchor already puts it**, with
/// its own quarter-rem nudge between the two, so this adds no offset. The
/// anchor names the trigger's top-left corner, but the positioner holding the
/// menu is an absolutely placed child of the block that also holds the
/// trigger, and its place in that block is under the trigger. An inset of a
/// trigger's height on top of that, written when the menu still landed over
/// its trigger, left every menu a whole button's height away from it.
/// The open state, the second-press close and the focus hand-off stay the
/// library's, and the click-out dismiss is the menu's own mouse-down-out.
///
/// **This mirrors the popover wiring inside
/// [`gpui_component::menu::DropdownMenu`]** -- the slot, the build-on-open,
/// the focus, the dismiss subscription -- and has to, because that wiring is
/// private to the library. Bumping the `gpui-component` rev means diffing
/// this function against it.
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
    build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> Popover {
    let build = Rc::new(build);
    let id = id.into();
    let slot_key = id.clone();
    Popover::new(id)
        .appearance(false)
        .overlay_closable(false)
        .trigger(trigger)
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
