//! The on-and-off control.

use gpui::ElementId;
use gpui_component::switch::Switch;
use gpui_component::{Disableable as _, FocusableExt as _, Sizable as _};

/// A setting turned on or off: the library's `Switch` at its small size,
/// `on` when the setting is.
///
/// Its focus ring is off. The library draws it whenever the switch holds
/// focus, and a press of the pointer gives it focus, so every switch clicked
/// kept a ring the keyboard never asked for.
pub fn toggle(id: impl Into<ElementId>, on: bool) -> Switch {
    Switch::new(id).checked(on).small().focus_ring(false)
}

/// A switch a person cannot flip now, drawn as the library draws a disabled
/// one: its state still shows, and a press does nothing.
///
/// A switch's own word for it is the library's `disabled`, which a button
/// shares; a button refuses with its reason instead, so a switch is locked
/// here and the library's word stays out of the call sites.
pub trait Locked: Sized {
    /// Locked while `locked` holds; otherwise the switch as it was.
    fn locked(self, locked: bool) -> Self;
}

impl Locked for Switch {
    #[expect(
        clippy::disallowed_methods,
        reason = "a switch is the one control locked through the library's disabled"
    )]
    fn locked(self, locked: bool) -> Self {
        self.disabled(locked)
    }
}
