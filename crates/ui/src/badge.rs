//! The badge, and an issue's label drawn as one.

use crate::{Tint, tint};
use gpui::{App, Hsla, ParentElement as _, SharedString, Styled as _};
use gpui_component::StyledExt as _;
use gpui_component::{ActiveTheme as _, Colorize as _};

/// An issue's label as a badge, in its [`label_tint`]. Every badge of a label
/// is drawn through this, so a label reads one way wherever it is.
pub fn label_badge(label: impl Into<SharedString>, cx: &App) -> gpui_component::tag::Tag {
    let label = label.into();
    let kind = label_tint(&label, cx);
    badge(label, kind).flex_none()
}

/// The tint of an issue's label by its kind: a label that asks for something
/// is blue, an enhancement green, one about the code's structure violet, one
/// about the documents yellow, an agent's report cyan; one that only files the
/// issue stays neutral. A badge takes the whole tint; a label written inside a
/// line of meta text takes only its ink, so both read the kind in one hue.
pub fn label_tint(label: &str, cx: &App) -> Tint {
    use onehand_core::issues::LabelKind;
    let theme = cx.theme();
    match LabelKind::of(label) {
        LabelKind::Category => Tint {
            fill: theme.secondary,
            ink: theme.secondary_foreground,
        },
        LabelKind::Action => tint(theme.blue, cx),
        LabelKind::Enhancement => tint(theme.green, cx),
        LabelKind::Structure => tint(theme.magenta, cx),
        LabelKind::Docs => tint(theme.yellow, cx),
        LabelKind::Report => tint(theme.cyan, cx),
    }
}

/// A short state or label: the library's `Tag` at its corner, in the
/// `tint`'s fill and ink with no edge of its own, lettered at the micro size
/// a step heavier than the text around it and held to a slim inset, so it
/// marks a row without outweighing it.
pub fn badge(text: impl Into<SharedString>, tint: Tint) -> gpui_component::tag::Tag {
    gpui_component::tag::Tag::custom(tint.fill, tint.ink, tint.fill)
        .text_size(crate::tokens::MICRO_TEXT)
        .font_medium()
        .px_1p5()
        .py_0()
        .child(text.into())
}

/// An edge in a tint's own hue, halfway from its fill to its ink, for a tinted
/// part that still draws one so it reads the same on any surface under it. A
/// badge draws none.
pub fn badge_edge(tint: Tint) -> Hsla {
    tint.ink.mix_oklab(tint.fill, 0.5)
}
