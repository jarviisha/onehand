//! The two families onehand ships inside the binary, and their registration.
//!
//! A family the machine happens to have is a different typeface on every
//! desktop, with metrics of its own: on one, SF Pro Display sat every button
//! label a pixel under its icon, because the line box is centred from the
//! face's declared ascent and descent, not from the letters. Shipping the faces
//! makes the text the same everywhere, and a family that is registered here
//! always resolves.
//!
//! Inter is drawn for interface text at the sizes the chrome uses; JetBrains
//! Mono for code and the terminal grid. Both are under the SIL Open Font
//! License, whose text sits beside the files in `assets/fonts/`.
//!
//! Static faces rather than the variable ones: one file per weight the app
//! asks for (regular, medium, semibold, bold, and italic for prose), so no
//! weight depends on how a platform's text stack handles variation axes.

use gpui::App;
use std::borrow::Cow;

/// The interface family: the theme's `font_family`.
pub(crate) const UI_FAMILY: &str = "Inter";
/// The family for code, paths, diffs and the terminal: the theme's
/// `mono_font_family`, unless `[font].monospace` names another installed one.
pub(crate) const MONO_FAMILY: &str = "JetBrains Mono";

const FACES: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf"),
];

/// Hand the faces to the text system. Before the theme is installed and before
/// the monospace scan, so both find the families already there. A failure is
/// reported and survived: the theme still names the families, and the text
/// falls back to the system's face as it did before they were shipped.
pub(crate) fn register(cx: &mut App) {
    let faces = FACES.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(err) = cx.text_system().add_fonts(faces) {
        eprintln!("onehand: the bundled fonts did not load: {err}");
    }
}
