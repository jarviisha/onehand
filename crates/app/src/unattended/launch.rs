//! What a person in a window starts: an issue picked by hand, a search
//! asked for now, and a pull request's review answered.

use gpui::App;

mod answer;
mod pick;

pub use answer::start_answer;
pub use pick::{look_now, start_picked};

/// Say `why` in `window`, as the transient notice it is. Deferred: a search
/// asked for now is answered inside the update of the window it was pressed
/// in, which cannot be updated again from there.
pub(super) fn warn(window: gpui::AnyWindowHandle, why: String, cx: &mut App) {
    use gpui_component::WindowExt as _;
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| {
            window.push_notification(crate::toast::warning(why), cx);
        });
    });
}
