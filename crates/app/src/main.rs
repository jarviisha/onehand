//! Thin binary: everything real lives in the library.

use gpui::App;
use onehand::{assets::Assets, shell};

fn main() {
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            // Must run before anything else touches gpui-component.
            gpui_component::init(cx);
            cx.activate(true);
            shell::boot(cx);
        });
}
