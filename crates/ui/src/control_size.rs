//! The sizes a control takes, each the role it serves.

use crate::tokens;
use gpui::Styled as _;
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::Input;

/// A control a step over [`Short`]: [`tokens::FIELD_H`] high, its words and
/// its icon at the small size's, the reading size, given that height and
/// padding on the spacing steps. The library ties a control's words and icon
/// to its size and its default size letters a button at the reading size, so
/// this is the small size made taller rather than the default one.
pub trait Tall {
    /// The small size at [`tokens::FIELD_H`].
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
        self.small().size(tokens::FIELD_H).px_0()
    }
}

impl Tall for Input {
    fn tall(self) -> Self {
        // `Styled::h` by name: the field's own `h` sets a multi-line field's
        // height only, and a one-line field kept the small size's.
        gpui::Styled::h(self.small(), tokens::FIELD_H)
    }
}

/// A control at a form's scale: a form's fields and buttons, a dialog's
/// footer, a page's filter bar. The small size's height, its words a step
/// down at the meta size, so they sit in the box with room above and below
/// rather than filling it edge to edge.
///
/// A button takes the extra small size for its words, the only way to reach
/// the box the library letters them in, and is given back the small height
/// and inset; its icon comes down with the words, and an icon alone stays
/// square. A field's text size is its own style, which lands after the
/// library's.
pub trait Short {
    /// The small size, lettered at the meta size.
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
        self.small().text_xs()
    }
}

/// The sizes a builder's controls take, each the role it serves:
/// `.short()` the default, `.tall()` in a list page's head.
///
/// For a builder that sizes the controls it makes, so a caller names one of
/// these: the library's own scale also holds its default size, which
/// letters a button at the reading size and is never used.
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
