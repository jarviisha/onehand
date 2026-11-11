//! What a person in a window starts: an issue picked by hand, a search
//! asked for now, and a pull request's review answered.

use gpui::App;

mod answer;
mod pick;

pub use answer::start_answer;
pub use pick::{look_now, start_picked};

/// Say `why` in `window`, as the transient notice it is.
pub(super) fn warn(window: gpui::AnyWindowHandle, why: String, cx: &mut App) {
    use gpui_component::WindowExt as _;
    let _ = window.update(cx, |_, window, cx| {
        window.push_notification(crate::toast::warning(why), cx);
    });
}
