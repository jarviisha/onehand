//! The on-and-off control.

use gpui::ElementId;
use gpui_component::switch::Switch;
use gpui_component::{FocusableExt as _, Sizable as _};

/// A setting turned on or off: the library's `Switch` at its small size,
/// `on` when the setting is.
///
/// Its focus ring is off. The library draws it whenever the switch holds
/// focus, and a press of the pointer gives it focus, so every switch clicked
/// kept a ring the keyboard never asked for.
pub fn toggle(id: impl Into<ElementId>, on: bool) -> Switch {
    Switch::new(id).checked(on).small().focus_ring(false)
}
