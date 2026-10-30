//! The sizes a control takes, each the role it serves.

use crate::tokens;
use gpui::Styled as _;
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::Input;

/// A control at the library's medium size, [`tokens::FIELD_H`] high: a page's
/// head and form, a dialog's footer. Its words, icon and padding are the
/// library's for that size.
pub trait Tall {
    /// The medium size at [`tokens::FIELD_H`].
    fn tall(self) -> Self;
}

impl Tall for Button {
    fn tall(self) -> Self {
        // `min_w` rather than `w`, so an icon alone stays square and a label
        // keeps its own width.
        self.small()
            .h(tokens::FIELD_H)
            .min_w(tokens::FIELD_H)
            .px_3()
    }
}

/// An icon alone at [`Tall`]'s height: square at [`tokens::FIELD_H`], with
/// no padding of its own.
///
/// Not [`Tall`] on an icon: the library sizes an icon alone by a box of its
/// own, and `Tall`'s padding, meant for words, taken inside that box left
/// the icon a few pixels wide.
pub trait TallIcon {
    /// Square at [`tokens::FIELD_H`].
    fn tall_icon(self) -> Self;
}

impl TallIcon for Button {
    fn tall_icon(self) -> Self {
        self.size(tokens::FIELD_H).px_0()
    }
}

impl Tall for Input {
    fn tall(self) -> Self {
        // `Styled::h` by name: the field's own `h` sets a multi-line field's
        // height only, and a one-line field kept the small size's.
        gpui::Styled::h(self.small(), tokens::FIELD_H)
    }
}

/// A control in dense chrome: a strip, a row, a toolbar, the composer, a
/// page's filter bar. The library's small size, [`tokens::CONTROL_H`] high,
/// with its own words, icon and padding for that size.
pub trait Short {
    /// The small size.
    fn short(self) -> Self;
}

impl Short for Button {
    fn short(self) -> Self {
        // `min_w` rather than `w`, so an icon alone stays square and a label
        // keeps its own width.
        self.xsmall()
            .h(tokens::CONTROL_H)
            .min_w(tokens::CONTROL_H)
            .px_2()
    }
}

impl Short for Input {
    fn short(self) -> Self {
        self.small()
    }
}

/// The sizes a builder's controls take, each the role it serves:
/// `.short()` the default, `.tall()` in a list page's head.
///
/// For a builder that sizes the controls it makes, so a caller names one of
/// these rather than a library size.
#[derive(Clone, Copy)]
pub enum ControlSize {
    Short,
    Tall,
}

impl ControlSize {
    pub(crate) fn on(self, button: Button) -> Button {
        match self {
            Self::Short => button.short(),
            Self::Tall => button.tall(),
        }
    }
}
