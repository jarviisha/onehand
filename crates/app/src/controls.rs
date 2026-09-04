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

use gpui::{
    Div, ElementId, Hsla, InteractiveElement, Interactivity, IntoElement, Stateful,
    StyleRefinement, Styled,
};
use gpui_component::Selectable;
use gpui_component::button::Button;
use gpui_component::menu::DropdownMenu;

/// A button that answers the pointer, which is every button this app draws.
///
/// The rule and its reasoning live with the definition in
/// [`onehand_plugin_host::action`], which is where a built-in plugin can reach
/// them too — a plugin draws buttons and cannot reach into the binary hosting
/// it, and two copies of this is two places for the library's default to be let
/// through. This is the name the app's own call sites already use.
///
/// **Pair it with [`resting`] on anything that can be disabled.** A pointer
/// over a control that refuses is the same lie as a Send that stays lit over a
/// prompt it will discard: it promises a press will do something.
pub(crate) fn action(id: impl Into<ElementId>) -> Button {
    onehand_plugin_host::action(id)
}

/// A hand-made row used whole as the trigger for a dropdown menu.
///
/// The library opens a menu from anything that is [`Selectable`] — a `Button`,
/// a `Tab`, a `ListItem` — and a `div` is not one. Without this, a row can only
/// carry a menu by putting a small button at its end, which makes the target a
/// few pixels wide while the thing the eye is pointing at is the whole row.
/// Both the trait and `Stateful<Div>` come from other crates, so the impl
/// cannot be written where either of them lives; a newtype here can carry it.
///
/// It forwards style and interactivity to the row, so the row is still what
/// decides its own padding, hover and id, and answers `Selectable` on its own
/// behalf — the menu sets that while it is open, which is what keeps the row lit
/// under an open menu instead of going quiet as soon as the pointer moves off
/// it and onto the menu.
pub(crate) struct MenuTrigger {
    row: Stateful<Div>,
    open: bool,
    /// What the row is filled with while its menu is open.
    ///
    /// Handed in rather than read from the theme here: the rail's rows and the
    /// conversation's header are two surfaces with two resting colours, and a
    /// trigger that picked one for both would light in the wrong one somewhere.
    /// It is resolved at build time because [`Selectable::selected`] is handed
    /// no context to read a theme from.
    lit: Hsla,
}

impl MenuTrigger {
    pub(crate) fn new(row: Stateful<Div>, lit: Hsla) -> Self {
        Self {
            row,
            open: false,
            lit,
        }
    }
}

impl Selectable for MenuTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.open = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.open
    }
}

impl Styled for MenuTrigger {
    fn style(&mut self) -> &mut StyleRefinement {
        self.row.style()
    }
}

impl InteractiveElement for MenuTrigger {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.row.interactivity()
    }
}

impl IntoElement for MenuTrigger {
    type Element = Stateful<Div>;

    fn into_element(self) -> Self::Element {
        let lit = self.lit;
        let open = self.open;
        // `when` is gpui-component's builder helper; a plain branch keeps this
        // free of the import and says the same thing.
        if open { self.row.bg(lit) } else { self.row }
    }
}

impl DropdownMenu for MenuTrigger {}

/// The cursor for an action that is currently refusing.
///
/// Applied on the disabled branch, so the pointer is a promise the control can
/// keep: it appears over things that will act on a click and nowhere else.
pub(crate) fn resting(button: Button) -> Button {
    use gpui::Styled as _;

    button.cursor_default()
}

/// Refusing, and looking like it, in one call.
///
/// [`resting`] is the cursor half of a refusal and `disabled` is the behaviour
/// half, and while they were two separate calls six controls here had made one
/// without the other: a Save with nothing to save, a Submit with nothing
/// chosen, an Unbind with nothing bound, a Cancel already cancelling. Every one
/// of them sat under a pointer promising a press would do something.
///
/// It has to be explicit because the library re-applies the caller's own style
/// refinement *after* the cursor it picks, so [`action`]'s pointer outlives
/// being disabled — the control goes quiet, refuses the click, and still
/// beckons. Taking the pointer back in the same call that refuses is what stops
/// the two halves drifting apart again.
pub(crate) trait Refuses: Sized {
    /// Refuse presses while `refusing` holds, and say so in the cursor.
    fn refuses(self, refusing: bool) -> Self;
}

impl Refuses for Button {
    fn refuses(self, refusing: bool) -> Self {
        use gpui_component::Disableable as _;

        match refusing {
            true => resting(self).disabled(true),
            false => self,
        }
    }
}
