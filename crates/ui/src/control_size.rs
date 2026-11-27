//! The sizes a control takes, each the role it serves.
//!
//! What each size measures, as the pinned `gpui-component` draws it (its
//! `sizing.rs`, `button/button.rs`, `icon.rs`, `tag.rs`, `checkbox.rs`,
//! `switch.rs`, `kbd.rs`), in pixels at the 16px base. **px** marks a size the
//! library holds in pixels, which per-panel zoom does not scale; everything
//! else is in rems. Moving the library's version means checking these figures.
//!
//! | Size | Button: height / words / icon / icon–label gap / inset | Input: height / words / inset | Icon alone |
//! |---|---|---|---|
//! | `.xsmall()`: an icon-only button inside a small object only | 20 / 12 / 12 / 4 / 4 | 20 / 12 / 4 **px** | 12 |
//! | `.short()`: a button the library's extra small, a field its small | 24 / 12 / 12 / 4 / 8 | 24 / 13 / 8 **px** | 14 |
//! | `.short_icon()`: an icon-only button beside `.short()`, the library's small held square | 24 / – / 14 / – / 0 | – | 14 |
//! | `.tall()`: the library's small, held taller | 28 / 13 / 14 / 4 / 12 | 28 / 13 / 8 **px** | 16 |
//!
//! Their words, icons and insets are the library's. A field of several lines
//! is the library's `Textarea`, which takes no size: its text sits at the
//! default's inset, 10 **px**, which no wrapper reaches, so it starts 2px
//! inside a `.short()` field's above it.
//!
//! Components that keep geometry of their own, centred in their row rather
//! than sized to it:
//!
//! | Component | Geometry |
//! |---|---|
//! | `Tag` through `badge` | words 11 at 1.25 and medium weight, inset 6 × 0; its height follows its line |
//! | `Checkbox` | box 12 / 14 / 16 at `.xsmall()` / `.small()` / default; its label 12 / 14 / 16 |
//! | `Switch` | track 28 × 16 **px** at `.xsmall()` and `.small()`, 36 × 20 **px** at the default; thumb 12 / 16 **px** |
//! | `Kbd` | words 12, inset 4 × 2 |
//! | The library's table rows | 26 / 30 / 32 **px** at `.xsmall()` / `.small()` / default; the app draws its own rows at [`tokens::ROW_H`] |

use crate::tokens;
use gpui::Styled as _;
use gpui_component::Sizable as _;
use gpui_component::button::Button;
use gpui_component::input::Input;

/// A page form's control, [`tokens::FIELD_H`] high: a page's form, a
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

/// An icon alone beside [`Short`]: square at [`tokens::CONTROL_H`], its
/// icon at the library's small size.
///
/// The library's small icon button is a box of its own a step taller than
/// `CONTROL_H`, so beside a worded `short` peer it stood two pixels over it;
/// and `Short` on an icon gave it the extra-small icon, which a chevron reads
/// as a tick at, and padding meant for words that cut the icon's sides off.
pub trait ShortIcon {
    /// Square at [`tokens::CONTROL_H`].
    fn short_icon(self) -> Self;
}

impl ShortIcon for Button {
    fn short_icon(self) -> Self {
        self.small().size(tokens::CONTROL_H).px_0()
    }
}

impl Tall for Input {
    fn tall(self) -> Self {
        // `Styled::h` by name: the field's own `h` sets a multi-line field's
        // height only, and a one-line field kept the small size's.
        gpui::Styled::h(self.small(), tokens::FIELD_H).text_size(tokens::CHROME_TEXT)
    }
}

/// A control in dense chrome: a page's head and filter bar, a strip, a row,
/// a toolbar, the composer. [`tokens::CONTROL_H`] high, on the library's
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
/// `.short()` the default, `.tall()` in a page's form.
///
/// For a builder that sizes the controls it makes, so a caller names one of
/// these rather than a library size.
#[derive(Clone, Copy)]
pub enum ControlSize {
    Short,
    Tall,
}

impl ControlSize {
    /// The height a control of this role stands at.
    pub(crate) fn height(self) -> gpui::Rems {
        match self {
            Self::Short => tokens::CONTROL_H,
            Self::Tall => tokens::FIELD_H,
        }
    }

    pub(crate) fn on(self, button: Button) -> Button {
        match self {
            Self::Short => button.short(),
            Self::Tall => button.tall(),
        }
    }
}
