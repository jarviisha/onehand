//! The button every crate draws: the pointer it answers, the refusal that
//! says why, the name an icon alone carries, and the quiet ghost.

use crate::Short as _;
use gpui::{App, ElementId, SharedString, Styled as _};
use gpui_component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_component::{Icon, IconName, Selectable as _};

/// A button that answers the pointer, which is every button Onehand draws.
///
/// The component library draws every button variant except `link` and `text`
/// with the **arrow** cursor. That is the platform convention this app is not
/// following: a session row, a completion candidate, a selector chip and an ask
/// choice are all hand-made `div`s that show a pointer, because that is the one
/// feedback a control gets *before* it is pressed. Half the actions on screen
/// answering the pointer and half not is worse than either rule applied whole —
/// the cursor stops meaning anything, and the only way left to find out whether
/// something is clickable is to click it.
///
/// The library re-applies the caller's own style refinement last, after its
/// `cursor_default`, so setting the cursor here wins — which is the whole reason
/// this can be a wrapper rather than a fork of the control.
///
/// **It also sets the theme's corner**, the library's `radius`: the library's
/// buttons default to it, but a size can change it, and every button passes
/// through here and nowhere else.
///
/// **It lives here rather than in the app** because a feature crate draws
/// buttons too and cannot reach into the binary that hosts it. A second copy in
/// each half is two places for the library's default to be let through, and the
/// bypass is one line and looks exactly like ordinary code — which is why a
/// guard counts the call sites and why there must be only one of these to count
/// against.
pub fn action(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .cursor_pointer()
        .rounded(ButtonRounded::Medium)
}

/// The cursor for an action that is currently refusing.
///
/// Applied on the disabled branch, so the pointer is a promise the control can
/// keep: it appears over things that will act on a click and nowhere else.
fn resting(button: Button) -> Button {
    button.cursor_default()
}

/// Refusing, and looking like it, in one call.
///
/// [`resting`] is the cursor half of a refusal and `disabled` is the behaviour
/// half, and while they were two separate calls six controls in the app had
/// made one without the other: a Save with nothing to save, a Submit with
/// nothing chosen, an Unbind with nothing bound, a Cancel already cancelling.
/// Every one of them sat under a pointer promising a press would do something.
///
/// It has to be explicit because the library re-applies the caller's own style
/// refinement *after* the cursor it picks, so [`action`]'s pointer outlives
/// being disabled — the control goes quiet, refuses the click, and still
/// beckons. Taking the pointer back in the same call that refuses is what stops
/// the two halves drifting apart again.
///
/// **It takes the reason, and shows it as the tooltip**, so a refusal cannot
/// go without saying why: a control that only goes quiet leaves the person
/// to guess what it waits for. So the refusal is the reason being there, and
/// it is called after the control's own tooltip, which it replaces for as
/// long as it refuses.
///
/// Here rather than in the app for the reason [`action`] is: a feature crate
/// draws buttons that refuse while they work.
pub trait Refuses: Sized {
    /// Refuse presses while there is a reason to, say so in the cursor, and
    /// say why in the tooltip.
    fn refuses(self, why: Option<impl Into<SharedString>>) -> Self;
}

impl Refuses for Button {
    fn refuses(self, why: Option<impl Into<SharedString>>) -> Self {
        use gpui_component::Disableable as _;

        match why {
            Some(why) => resting(self).disabled(true).tooltip(why.into()),
            None => self,
        }
    }
}

/// An icon with no words named twice: in its tooltip, for the pointer, and as
/// its accessible name, for a screen reader. The library names a button by its
/// label, and an icon alone has none, so a tooltip by itself left the button
/// announced as nothing. Every icon-only button says what it does through this,
/// never a bare `tooltip`.
///
/// A refusal still shows its reason in the tooltip ([`Refuses`]) and keeps the
/// name, which says what the button does rather than why it waits.
pub trait Named: Sized {
    /// The tooltip and the accessible name, one string.
    fn named(self, name: impl Into<SharedString>) -> Self;

    /// The same, with the key bound to `action` shown beside the tooltip.
    fn named_with_action(
        self,
        name: impl Into<SharedString>,
        action: &dyn gpui::Action,
        context: Option<&str>,
    ) -> Self;
}

impl Named for Button {
    fn named(self, name: impl Into<SharedString>) -> Self {
        let name = name.into();
        self.accessibility_label(name.clone()).tooltip(name)
    }

    fn named_with_action(
        self,
        name: impl Into<SharedString>,
        action: &dyn gpui::Action,
        context: Option<&str>,
    ) -> Self {
        let name = name.into();
        self.accessibility_label(name.clone())
            .tooltip_with_action(name, action, context)
    }
}

/// The quiet button: the library's own ghost, with no fill or edge at rest
/// and its hover fill under the pointer, for familiar, low-emphasis commands
/// in a toolbar, a strip or a row.
///
/// **One call for every ghost**, so a change to how a quiet command looks is
/// made here once rather than at each call site. The `cx` is kept for the
/// call sites, which pass it already.
pub trait Quiet {
    /// The ghost variant.
    fn quiet(self, cx: &App) -> Self;

    /// The ghost of an icon with no words, `on` while what it switches is
    /// on: the library draws its mark in the ghost's ink, at rest as under the
    /// pointer, and marks it selected on its own fill.
    fn quiet_icon(self, on: bool, cx: &App) -> Self;
}

impl Quiet for Button {
    fn quiet(self, _: &App) -> Self {
        self.ghost()
    }

    fn quiet_icon(self, on: bool, cx: &App) -> Self {
        self.quiet(cx).selected(on)
    }
}

/// What opens a block held folded under it: its chevron first, then its
/// words, a button at a control's size, so the keyboard reaches it too.
/// The caller says what a press does, and refuses it where there is nothing
/// to open.
///
/// On the secondary variant rather than a ghost: it starts its own line at the
/// block's edge, where a ghost's padding would set its chevron in from the
/// text above it with nothing drawn to say where it begins.
pub fn fold_toggle(id: impl Into<ElementId>, label: impl Into<SharedString>, open: bool) -> Button {
    action(id)
        .secondary()
        .short()
        .icon(Icon::new(match open {
            true => IconName::ChevronDown,
            false => IconName::ChevronRight,
        }))
        .label(label.into())
}
