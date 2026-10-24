//! The app's own defaults over the component library's controls.
//!
//! ## The pointer is the only thing that says "this does something"
//!
//! gpui-component draws every button variant except `link` and `text` with the
//! **arrow** cursor (`button.rs`: `cursor_default()`, then `cursor_pointer()`
//! only for those two). That is the platform convention this app is not
//! following: a session row, a completion candidate, a selector chip, an ask
//! choice and a fold strip are all hand-made `div`s that show a pointer,
//! because that is the one feedback a control gets *before* it is pressed.
//! Half the actions answering the pointer and half not is worse than either
//! rule applied whole — the cursor stops meaning anything, and the only way
//! left to find out whether something is clickable is to click it.
//!
//! So actions go through [`action`]. One place decides it, a guard keeps it
//! that way, and the library's own default is overridden exactly once rather
//! than at forty call sites that each have to remember.

use gpui::ElementId;

mod setting_row;
use gpui_component::button::Button;
pub(crate) use setting_row::{SettingPick, setting_row};

/// A button that answers the pointer, which is every button this app draws.
///
/// The rule and its reasoning live with the definition in
/// [`onehand_ui::action`], which is where a feature crate can reach
/// them too — a feature crate draws buttons and cannot reach into the binary hosting
/// it, and two copies of this is two places for the library's default to be let
/// through. This is the name the app's own call sites already use.
///
/// **Refuse through [`Refuses`] on anything that can be disabled.** A pointer
/// over a control that refuses is the same lie as a Send that stays lit over a
/// prompt it will discard: it promises a press will do something.
pub(crate) fn action(id: impl Into<ElementId>) -> Button {
    onehand_ui::action(id)
}

/// Menu rows that answer the pointer, and a menu that opens below its
/// trigger. The rules and their reasoning live with the definitions in
/// [`onehand_ui`], which is where a feature crate can reach them too
/// — a feature crate that draws a menu cannot reach into the binary hosting it, and a
/// second copy is a second place for the arrow cursor to come back.
pub(crate) use onehand_ui::{menu_below, menu_command, menu_item, menu_row, split_action};

/// A text field at a control's corner. The rule and its reasoning live with
/// the definition in [`onehand_ui`], where a feature crate's fields
/// reach it too.
pub(crate) use onehand_ui::text_input;

/// A search field and a field of several lines, on a field's corner and
/// surface. The rules and their reasoning live with the definitions in
/// [`onehand_ui`], where a feature crate's fields reach them too.
pub(crate) use onehand_ui::{search_input, text_area};

/// A ghost button hovering on the selected step. The rule and its reasoning
/// live with the definition in [`onehand_ui`], where a feature crate's
/// buttons reach it too.
pub(crate) use onehand_ui::Quiet;

/// A control at a dialog's or a form's scale. The rule and its reasoning live
/// with the definition in [`onehand_ui`], where a feature crate's
/// controls reach it too.
pub(crate) use onehand_ui::Short;

/// Refusing, and looking like it: the cursor half and the behaviour half in
/// one call. The rule and its reasoning live with the definitions in
/// [`onehand_ui`], where a feature crate's buttons reach them too.
pub(crate) use onehand_ui::Refuses;

/// Put every tooltip away whenever what it stood for may have gone: a key
/// pressed, as gpui's own tooltips do, a press of the pointer, and one of the
/// window's dialogs or sheets coming or going ([`tooltip_guard`]).
///
/// The library's tooltip is put away when its trigger reports the pointer
/// left, and a trigger that stops being drawn reports nothing. A row's
/// actions are drawn only while the row is hovered, and a key pressed ends
/// the hover (the window stops counting the pointer once the keyboard is
/// used); a dialog closes by itself under a pointer resting on its button.
/// Each left a tooltip on screen, across pages, until another replaced it.
///
/// And a press puts away its control's tooltip only for a moment: the
/// control counts as hovered again once the press is over, or once the menu
/// it opened has settled, and asks for the same tooltip anew, which then
/// stood over the menu. So a tooltip asked for while the pointer rests where
/// it was pressed is put away as it shows. The tooltips live in one overlay
/// per window, kept as each is made.
pub(crate) fn put_tooltips_away(cx: &mut gpui::App) {
    cx.observe_new(|_: &mut gpui_base::TooltipOverlay, _, cx| {
        let overlay = cx.weak_entity();
        cx.default_global::<Overlays>().overlays.push(overlay);
        cx.observe_self(|overlay, cx| {
            if cx
                .try_global::<Overlays>()
                .is_some_and(|o| o.pressed.is_some())
            {
                overlay.hide(cx);
            }
        })
        .detach();
    })
    .detach();
    cx.intercept_keystrokes(|_, _, cx| hide_tooltips(cx))
        .detach();
}

/// How far the pointer moves from where it was pressed before a tooltip may
/// show again: past a hand's tremble, short of reaching another control.
const PRESS_SLOP: gpui::Pixels = gpui::px(2.);

/// Every window's tooltip overlay, whether each window last drew a dialog or
/// sheet, and where the pointer was pressed while it has not moved away.
#[derive(Default)]
struct Overlays {
    overlays: Vec<gpui::WeakEntity<gpui_base::TooltipOverlay>>,
    layered: std::collections::HashMap<gpui::WindowId, bool>,
    pressed: Option<gpui::Point<gpui::Pixels>>,
}

impl gpui::Global for Overlays {}

fn hide_tooltips(cx: &mut gpui::App) {
    let overlays = std::mem::take(&mut cx.default_global::<Overlays>().overlays);
    let live = overlays
        .into_iter()
        .filter(|overlay| overlay.update(cx, |o, cx| o.hide(cx)).is_ok())
        .collect();
    cx.default_global::<Overlays>().overlays = live;
}

/// What the window mounts once, beside its layers, so a press of the pointer
/// and a dialog or sheet (`layered`) coming or going put tooltips away.
/// Listened for on the way down, past every layer: a dialog, a menu or a
/// loading button that stops a press stops it only after this.
pub(crate) fn tooltip_guard(
    layered: bool,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> impl gpui::IntoElement + use<> {
    use gpui::{DispatchPhase, MouseDownEvent, MouseMoveEvent, Styled as _};
    let id = window.window_handle().window_id();
    let was = cx.default_global::<Overlays>().layered.insert(id, layered);
    if was.is_some_and(|was| was != layered) {
        hide_tooltips(cx);
    }
    gpui::canvas(
        |_, _, _| {},
        |_, _, window, _| {
            window.on_mouse_event(|event: &MouseDownEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    cx.default_global::<Overlays>().pressed = Some(event.position);
                    hide_tooltips(cx);
                }
            });
            window.on_mouse_event(|event: &MouseMoveEvent, phase, _, cx| {
                let pressed = &mut cx.default_global::<Overlays>().pressed;
                if phase == DispatchPhase::Capture
                    && pressed
                        .is_some_and(|at| (event.position - at).magnitude() > PRESS_SLOP.into())
                {
                    *pressed = None;
                }
            });
        },
    )
    .absolute()
    .size_0()
}
