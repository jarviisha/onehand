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
    // What onehand gives a session's agent to reach onehand's tools: a proxy
    // between the agent's stdio and the socket, speaking for the session the
    // token names. Before the lock, which belongs to the app, not to this.
    let args: Vec<String> = std::env::args().collect();
    if let [_, flag, socket, token] = args.as_slice()
        && flag == "--mcp-proxy"
    {
        let stdin = std::io::BufReader::new(std::io::stdin());
        let done = onehand_core::control::proxy(socket.as_ref(), token, stdin, std::io::stdout());
        if let Err(why) = done {
            eprintln!("onehand: the proxy could not reach {socket}: {why}");
            std::process::exit(1);
        }
        return;
    }
    shell::hold_or_forward();

    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            // Must run before anything else touches gpui-component.
            gpui_component::init(cx);
            cx.activate(true);
            shell::boot(cx);
        });
}
