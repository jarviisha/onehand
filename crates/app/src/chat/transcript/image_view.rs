//! An attached picture, whole.
//!
//! A plate shows a picture at the size that tells one screenshot from another
//! and no larger. Pressing it opens the picture in a dialog, fitted to the
//! window and kept to its own proportions, from the transcript and from the
//! tray of staged files alike.

use crate::controls::{Quiet as _, Short as _};
use gpui::{
    App, InteractiveElement as _, IntoElement as _, ObjectFit, ParentElement, Rems, SharedString,
    Styled, StyledImage as _, Window, div, rems,
};
use gpui_component::{ActiveTheme, StyledExt as _, WindowExt as _};
use std::path::PathBuf;

/// How wide the dialog opens, as the code view does.
const IMAGE_VIEW_W: Rems = rems(60.);
/// How tall the picture stands in it; a taller one is scaled to fit.
const IMAGE_VIEW_MAX_H: Rems = rems(36.);

/// Put the picture at `path` up whole, under its `name`.
pub(in crate::chat) fn open(path: PathBuf, name: SharedString, window: &mut Window, cx: &mut App) {
    let width = crate::dialogs::width_in(window, IMAGE_VIEW_W);
    window.open_dialog(cx, move |dialog, _, cx| {
        let ink = cx.theme().muted_foreground;
        dialog
            .w(width)
            .close_button(false)
            .child(crate::dialogs::title_row(name.clone(), cx))
            .child(
                div()
                    .id("image-view")
                    .h_flex()
                    .justify_center()
                    .w_full()
                    .child(
                        gpui::img(path.clone())
                            .max_w_full()
                            .max_h(IMAGE_VIEW_MAX_H)
                            .object_fit(ObjectFit::Contain)
                            // gpui draws nothing for a file that has moved or
                            // fails to decode, which would read as a blank
                            // dialog rather than as a picture that is gone.
                            .with_fallback(move || {
                                div()
                                    .text_color(ink)
                                    .child("This image could not be loaded")
                                    .into_any_element()
                            }),
                    ),
            )
            .footer(
                crate::dialogs::footer_row().justify_end().child(
                    crate::controls::action("image-view-close")
                        .short()
                        .quiet(cx)
                        .label("Close")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}
