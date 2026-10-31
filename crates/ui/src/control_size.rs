//! The sizes a control takes, each the role it serves.

use crate::tokens;
use gpui::Styled as _;
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::Input;

/// A page's control, [`tokens::FIELD_H`] high: a page's head and form, a
/// dialog's footer. Built on the library's small size, its words at the
/// chrome size so a button and the row beside it letter alike.
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
            .text_size(tokens::CHROME_TEXT)
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
        gpui::Styled::h(self.small(), tokens::FIELD_H).text_size(tokens::CHROME_TEXT)
    }
}

/// A control in dense chrome: a strip, a row, a toolbar, the composer, a
/// page's filter bar. [`tokens::CONTROL_H`] high, on the library's
/// extra-small words, icon and padding.
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
        // `Styled::h` by name, for the reason `Tall` gives.
        gpui::Styled::h(self.small(), tokens::CONTROL_H).text_size(tokens::CHROME_TEXT)
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
