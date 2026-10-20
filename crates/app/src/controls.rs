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
use gpui_component::button::Button;

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
