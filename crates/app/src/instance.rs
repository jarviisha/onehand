//! The lock a launch starts with, and the socket its holder listens on.
//!
//! A launch that finds the lock held hands its folder to the process holding
//! it and exits, before GPUI is started at all. The holder serves the socket on
//! `onehand_io::socket`'s runtime, which crosses to GPUI over a `futures`
//! channel; what a line means is decided in core's `instance`, and this only
//! carries it.

use crate::state::Shared;
use futures::StreamExt as _;
use gpui::App;
use onehand_core::instance::{self, Forwarded, Unforwarded};
use onehand_io::socket::{Incoming, Socket};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The socket's runtime, kept for the life of the process.
static SERVED: OnceLock<Socket> = OnceLock::new();

/// The socket, bound the moment the lock is taken and served once GPUI is up.
/// A second launch in between waits in the socket's backlog instead of
/// finding nobody there.
static BOUND: Mutex<Option<std::os::unix::net::UnixListener>> = Mutex::new(None);

/// How often a line that arrived before the first window looks for it again.
const WINDOW_POLL: Duration = Duration::from_millis(50);

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
            // The first window registers a moment after boot. A line that lands
            // before it waits for it, rather than opening a second window onto
            // the workspace that one is about to show, or a call finding none.
            while cx.update(|cx| Shared::global(cx).windows.is_empty()) {
                cx.background_executor().timer(WINDOW_POLL).await;
            }
            cx.update(|cx| match incoming {
                Incoming::Open(forwarded) => open_forwarded(forwarded, cx),
                Incoming::Call(peer, call, answer) => {
                    crate::control::perform(peer, call, answer, cx)
                }
            });
        }
    })
    .detach();
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
