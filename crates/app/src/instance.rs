//! The lock a launch starts with, and the socket its holder listens on.
//!
//! A launch that finds the lock held hands its folder to the process holding
//! it and exits, before GPUI is started at all. The holder serves the socket on
//! `onehand_io::socket`'s runtime, which crosses to GPUI over a `futures`
//! channel; what a line means is decided in core's `instance`, and this only
//! carries it.

use futures::StreamExt as _;
use gpui::App;
use onehand_core::instance::{self, Arrivals, Forwarded, Unforwarded};
use onehand_io::socket::{Incoming, Socket};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// The socket's runtime, kept for the life of the process.
static SERVED: OnceLock<Socket> = OnceLock::new();

/// The socket, bound the moment the lock is taken and served once GPUI is up.
/// A second launch in between waits in the socket's backlog instead of
/// finding nobody there.
static BOUND: Mutex<Option<std::os::unix::net::UnixListener>> = Mutex::new(None);

/// Take the instance lock, or hand this launch's folder to the process that
/// holds it and exit. Runs before GPUI starts, so a second launch never opens
/// a window, and before the config directory's files are moved, which the
/// lock is there to protect.
pub fn hold_or_forward() {
    let dir = onehand_core::config::config_dir();
    if instance::hold_lock(&dir).is_ok() {
        bind(&dir);
        return;
    }
    // The first argument, as a first launch reads it: no flag becomes a folder
    // here either, because `--version` was answered before this.
    let path = std::env::args().nth(1).map(PathBuf::from);
    match instance::forward(&instance::socket_path(&dir), path.as_deref()) {
        Ok(()) => std::process::exit(0),
        Err(Unforwarded::Refused(why)) => {
            eprintln!("onehand: {why}");
            std::process::exit(1);
        }
        Err(Unforwarded::Unreachable) => {
            eprintln!("onehand is already running on {}", dir.display());
            std::process::exit(1);
        }
    }
}

/// Bind the socket beside the lock on `dir`.
///
/// Only the lock's holder does: it alone knows a socket file already there is
/// stale. A socket that cannot be bound is reported and the app carries on; a
/// second launch then says the app is already running, as it always did.
fn bind(dir: &std::path::Path) {
    if !instance::holding() {
        return;
    }
    let socket = instance::socket_path(dir);
    let _ = std::fs::remove_file(&socket);
    match std::os::unix::net::UnixListener::bind(&socket) {
        Ok(listener) => {
            // The user's own, as the config directory beside it is.
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
            *BOUND.lock().unwrap_or_else(|e| e.into_inner()) = Some(listener);
        }
        Err(e) => eprintln!("onehand: no socket at {}: {e}", socket.display()),
    }
}

/// Serve the bound socket for as long as the process lives.
pub(crate) fn listen(cx: &mut App) {
    let Some(listener) = BOUND.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let (socket, mut rx) = match Socket::serve(listener, crate::control::front()) {
        Ok(served) => served,
        Err(e) => {
            eprintln!("onehand: no socket for a second launch: {e}");
            return;
        }
    };
    let _ = SERVED.set(socket);
    cx.spawn(async move |cx| {
        while let Some(incoming) = rx.next().await {
            cx.update(|cx| arrived(incoming, cx));
        }
    })
    .detach();
}

/// What has reached the socket and waits for the first window.
#[derive(Default)]
struct Held(Arrivals<Incoming>);

impl gpui::Global for Held {}

/// `incoming` reached the socket: carried now when a window is up, else held
/// for the first.
fn arrived(incoming: Incoming, cx: &mut App) {
    let passed = cx.default_global::<Held>().0.arrived(incoming);
    if let Some(incoming) = passed {
        carry(incoming, cx);
    }
}

/// The first window is registered: carry what waited for it, in order.
/// Never inside an entity's update: a call reaches into its window's shell.
pub(crate) fn window_up(cx: &mut App) {
    let held = cx.default_global::<Held>().0.up();
    for incoming in held {
        carry(incoming, cx);
    }
}

fn carry(incoming: Incoming, cx: &mut App) {
    match incoming {
        Incoming::Open(forwarded) => open_forwarded(forwarded, cx),
        Incoming::Call(peer, call, answer) => crate::control::perform(peer, call, answer, cx),
    }
}

/// Open what a second launch forwarded in the window in front, or the first
/// one, and raise it. Never inside another entity's update: this runs from the
/// channel's own task.
fn open_forwarded(forwarded: Forwarded, cx: &mut App) {
    let Some((handle, shell)) = crate::state::front(cx) else {
        return;
    };
    handle
        .update(cx, |_, window, cx| {
            window.activate_window();
            if let Some(path) = forwarded.path {
                shell.update(cx, |shell, cx| shell.open_root(path, window, cx));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc::UnboundedReceiver;
    use onehand_io::acp::Pass as _;
    use serde_json::{Value, json};
    use std::io::{BufRead as _, Write as _};
    use std::os::unix::net::UnixStream;

    /// A listener on a socket of its own, and what reaches GPUI from it.
    fn listening(name: &str) -> (PathBuf, Socket, UnboundedReceiver<Incoming>) {
        let dir = std::env::temp_dir().join(format!("onehand-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("onehand.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (served, rx) = Socket::serve(listener, crate::control::front()).unwrap();
        (dir, served, rx)
    }

    /// Send `msg` and read the answer.
    fn ask(stream: &mut UnixStream, msg: Value) -> Value {
        writeln!(stream, "{msg}").unwrap();
        let mut reply = String::new();
        std::io::BufReader::new(&*stream)
            .read_line(&mut reply)
            .unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    /// A call and a folder that reach the socket before the first window are
    /// carried once it is up, in order; one after it, at once.
    #[gpui::test]
    fn what_arrives_before_the_first_window_waits_for_it(cx: &mut gpui::TestAppContext) {
        use onehand_core::control::{Call, Peer};
        let dir = crate::shell::rig::folder("arrivals");
        let (_, shell, cx) = crate::shell::rig::shell(cx, dir.join("tasks"), Vec::new());
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let (tx, mut early) = futures::channel::oneshot::channel();
        let path = Some(project.clone());
        // Deferred out of the test window's update, as the socket's task
        // carries from no window's.
        cx.update(|_, cx| {
            cx.defer(move |cx| {
                arrived(Incoming::Call(Peer::Outside, Call::ListAgents, tx), cx);
                arrived(Incoming::Open(Forwarded { path }), cx);
            })
        });
        cx.run_until_parked();
        assert!(early.try_recv().unwrap().is_none(), "held for the window");
        let held = |cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| shell.read(cx).holds_root(&project))
        };
        assert!(!held(cx));
        cx.update(|_, cx| cx.defer(window_up));
        cx.run_until_parked();
        assert!(
            early.try_recv().unwrap().is_some(),
            "answered once it is up"
        );
        assert!(held(cx), "the folder opened once it is up");
        let (tx, mut late) = futures::channel::oneshot::channel();
        cx.update(|_, cx| {
            cx.defer(move |cx| arrived(Incoming::Call(Peer::Outside, Call::ListAgents, tx), cx))
        });
        cx.run_until_parked();
        assert!(late.try_recv().unwrap().is_some(), "carried at once");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_second_launch_reaches_the_listener() {
        let (dir, _rt, mut rx) = listening("listen");
        assert!(instance::forward(&dir.join("onehand.sock"), Some(&dir)).is_ok());
        match rx.try_recv() {
            Ok(Incoming::Open(forwarded)) => assert_eq!(forwarded.path, Some(dir.clone())),
            _ => panic!("nothing forwarded"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sessions_connection_is_let_go_when_its_client_hangs_up() {
        let (dir, _rt, _rx) = listening("hangup");
        let token = crate::control::Token::mint();
        let servers = token.servers();
        let name = servers[0]["args"][2].as_str().unwrap().to_string();
        let mut stream = UnixStream::connect(dir.join("onehand.sock")).unwrap();
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"_meta": {"onehand": {"session": name}}}});
        assert!(ask(&mut stream, init)["result"].is_object());
        // The proxy's stdin ended: the server's end has to close too, or the
        // proxy waits on it for ever.
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut rest = String::new();
        let read = std::io::Read::read_to_string(&mut stream, &mut rest);
        assert_eq!(read.unwrap(), 0, "the server kept the socket open");
        drop(token);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_outside_client_is_answered_over_the_socket() {
        let (dir, _rt, mut rx) = listening("outside");
        // GPUI's side, answering every call alike.
        std::thread::spawn(move || {
            while let Some(incoming) = futures::executor::block_on(rx.next()) {
                if let Incoming::Call(_, _, answer) = incoming {
                    let _ = answer.send(Ok("done".to_string()));
                }
            }
        });
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let socket = dir.join("onehand.sock");

        crate::control::boot(&onehand_core::config::ControlConfig { outside: false });
        let mut stream = UnixStream::connect(&socket).unwrap();
        assert_eq!(ask(&mut stream, init.clone())["error"]["code"], -32001);

        crate::control::boot(&onehand_core::config::ControlConfig { outside: true });
        let mut stream = UnixStream::connect(&socket).unwrap();
        assert!(ask(&mut stream, init)["result"].is_object());
        let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        // Every tool but those about a session's workers, which an outside
        // program owns none of.
        let listed: Vec<String> = ask(&mut stream, list)["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        for name in ["open_worker", "session_state", "send_prompt", "cancel_turn"] {
            assert!(!listed.contains(&name.to_string()), "{listed:?}");
        }
        assert_eq!(listed.len(), onehand_core::control::tool_names().len() - 4);
        let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "list_agents", "arguments": {}}});
        let reply = ask(&mut stream, call);
        assert_eq!(reply["result"]["content"][0]["text"], "done");

        // Bytes that are not UTF-8 are answered as malformed, and the
        // conversation goes on.
        stream.write_all(b"\xff\xfe\n").unwrap();
        let mut reply = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut reply)
            .unwrap();
        assert!(reply.contains("-32700"), "{reply}");
        let ping = json!({"jsonrpc": "2.0", "id": 4, "method": "ping"});
        assert_eq!(ask(&mut stream, ping)["result"], json!({}));

        // A second launch still finds the listener, on a connection of its own.
        assert!(instance::forward(&socket, Some(&dir)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
