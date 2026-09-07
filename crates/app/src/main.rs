//! Thin binary: everything real lives in the library.

use gpui::App;
use onehand::{assets::Assets, shell};

fn main() {
    // Answered before a window is opened, because the first thing a bug report
    // needs is the build it came from and asking for that must not launch the
    // app. Read from the first argument, which is where the project root is
    // read from too -- so there is no path by which a flag becomes a folder.
    if let Some(arg) = std::env::args().nth(1)
        && (arg == "--version" || arg == "-V")
    {
        println!("onehand {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            // Must run before anything else touches gpui-component.
            gpui_component::init(cx);
            cx.activate(true);
            shell::boot(cx);
        });
}
