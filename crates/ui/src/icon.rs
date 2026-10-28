//! Marks the app serves for a feature crate.

use gpui::SharedString;

/// A mark the app picks from the kit's catalog for a feature crate, where the
/// default bundle has no drawing: an open issue's circle with a dot, and a
/// refresh. The app serves these, and its registry test holds each path to the
/// one it serves.
#[derive(Clone, Copy)]
pub enum AppIcon {
    CircleDot,
    Refresh,
}

impl gpui_component::IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::CircleDot => "icons/circle-dot.svg".into(),
            Self::Refresh => "icons/refresh-cw.svg".into(),
        }
    }
}
