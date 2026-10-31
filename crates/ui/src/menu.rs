//! Popup menus a feature crate can draw as the app does: rows that answer the
//! pointer, and a menu that opens below its trigger.

use gpui::{
    Action, Anchor, App, Context, DismissEvent, ElementId, Entity, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::StyledExt as _;
use gpui_component::button::Button;
use gpui_component::kbd::Kbd;
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
            // At the chrome size, as the rows and controls around the menu
            // are: the library letters every row a step larger.
            .text_size(crate::tokens::CHROME_TEXT)
            .child(render(window, cx))
    })
}

/// A menu row that does not act now: a refusal, or a line that is only
/// read. At the size [`menu_row`] letters its words, and with the library's
/// own cursor, since a pointer over it would promise a press.
pub fn menu_still<E: IntoElement>(
    render: impl Fn(&mut Window, &mut App) -> E + 'static,
) -> PopupMenuItem {
    PopupMenuItem::element(move |window, cx| {
        div()
            .text_size(crate::tokens::CHROME_TEXT)
            .child(render(window, cx))
    })
    .disabled(true)
}

/// A plain worded menu row that does something.
///
/// The shape almost every entry in the app's menus has, so it is spelled once:
/// [`menu_row`] is for the rows that draw something other than a line of text.
pub fn menu_item(label: impl Into<SharedString>) -> PopupMenuItem {
    let label = label.into();
    menu_row(move |_, _| said(&label).child(label.clone()))
}

/// The box a worded row's words stand in, named for assistive technology.
///
/// The library names its own rows by their label, but a row drawn as an
/// element has none to give, so a menu of the app's rows read as nameless
/// items; the words' box carries the name instead.
fn said(label: &SharedString) -> gpui::Stateful<gpui::Div> {
    div()
        .id(ElementId::Name(label.clone()))
        .aria_label(label.clone())
}

/// A line in a menu that heads a group or says what the menu leaves out:
/// read, never pressed, at the size the menu's rows take. The library's own
/// label letters a step larger, over the rows it heads. It stands as tall as
/// a row, which the library sets for every row drawn as an element.
pub fn menu_label(label: impl Into<SharedString>) -> PopupMenuItem {
    let label = label.into();
    menu_still(move |_, _| said(&label).child(label.clone()))
}

/// A worded menu row that runs what a keymap command runs, showing that
/// command's key at its end, as a control bound to a command shows its key.
///
/// The library's own rows show a key only for its plain items, which take no
/// pointer; this is [`menu_item`] with the key looked up the way the library
/// looks it up, so a rebound key is the one shown. The row's press still goes
/// through `on_click`, which acts on what the menu is about.
pub fn menu_command(label: impl Into<SharedString>, action: Box<dyn Action>) -> PopupMenuItem {
    let label = label.into();
    menu_row(move |window, _| {
        div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_3()
            .child(said(&label).flex_1().child(label.clone()))
            // Bare, as the library's own menu rows letter their keys: no
            // fill and no edge, so a key reads the same in every menu.
            .children(
                Kbd::binding_for_action(action.as_ref(), None, window).map(|key| {
                    key.p_0()
                        .flex_nowrap()
                        .border_0()
                        .bg(gpui::transparent_white())
                }),
            )
    })
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
/// Emptying the slot on dismiss is what lets the menu entity go, and the slot
/// itself is collected by the window a frame after the popup closes, since
/// nothing accesses it while closed. The dismiss listener holds the slot and
/// the popover only weakly, so a trigger that leaves the frame with its menu
/// open takes the menu with it.
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
                    // Weak, both: the listener lives as long as the menu,
                    // which the slot holds, so a strong capture of the slot
                    // is a cycle that keeps a menu open on a trigger that
                    // has gone from the frame alive for good.
                    let popover = cx.entity().downgrade();
                    window
                        .subscribe(&menu, cx, {
                            let slot = slot.downgrade();
                            move |_, _: &DismissEvent, window, cx| {
                                _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                                _ = slot.update(cx, |state, _| *state = None);
                            }
                        })
                        .detach();
                    menu
                }
            }
        })
}

/// An action and the menu of its alternatives as one control: `main` and a
/// caret half that opens `build`, joined edge to edge like the library's
/// `ButtonGroup`, the inner corners square.
///
/// **Not the library's `ButtonGroup` or `DropdownButton`.** A group takes
/// only buttons, so neither half can carry a menu; the split button builds
/// its caret half itself, past [`crate::action`], with the arrow cursor and
/// the library's size rather than the role's. So both halves come from the
/// caller, through `action`, at their role's size and one variant, and only
/// the joining and the menu are done here.
///
/// A caret standing apart beside its action reads as a second, unrelated
/// button; this is the one way to offer one.
///
/// **A seam in `surface`, the fill the control sits on, parts the halves.**
/// Each half answers the pointer on its own, and without it the two read as
/// one button that does two things depending on where it is pressed. Drawn in
/// the surface's own colour it reads as a cut through the control rather
/// than an edge on it, so no button gains an edge.
pub fn split_action(
    id: impl Into<ElementId>,
    main: Button,
    caret: Button,
    surface: gpui::Hsla,
    anchor: Anchor,
    build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .flex_none()
        .child(main.rounded_tr_none().rounded_br_none())
        .child(
            menu_below(
                id,
                caret
                    .dropdown_caret(true)
                    .rounded_tl_none()
                    .rounded_bl_none()
                    .border_l_1()
                    .border_color(surface),
                build,
            )
            .anchor(anchor),
        )
}
