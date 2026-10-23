//! The lock a launch starts with, and the socket its holder listens on.
//!
//! A launch that finds the lock held hands its folder to the process holding
//! it and exits, before GPUI is started at all. The holder reads those lines on
//! a tokio runtime of its own and crosses to GPUI over a `futures` channel; what
//! a line means is decided in core's `instance`, and this only carries it.

use crate::state::Shared;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui::App;
use onehand_core::control::Call;
use onehand_core::instance::{self, Forwarded, Unforwarded};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixListener;
use tokio::net::unix::OwnedReadHalf;

/// The listener's runtime, kept for the life of the process.
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// The socket, bound the moment the lock is taken and served once GPUI is up.
/// A second launch in between waits in the socket's backlog instead of
/// finding nobody there.
static BOUND: Mutex<Option<std::os::unix::net::UnixListener>> = Mutex::new(None);

/// How long a connection may take to send its first line, and how long any
/// line may be: past either it is dropped, not waited on.
const LINE_WAIT: Duration = Duration::from_secs(5);
const LINE_CAP: u64 = 64 * 1024;

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
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .thread_name("onehand-socket")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("onehand: no socket for a second launch: {e}");
            return;
        }
    };
    let listener = listener.set_nonblocking(true).and_then(|()| {
        let _in_runtime = rt.enter();
        UnixListener::from_std(listener)
    });
    let listener = match listener {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("onehand: no socket for a second launch: {e}");
            return;
        }
    };
    let (tx, mut rx) = unbounded();
    rt.spawn(serve(listener, tx));
    let _ = RUNTIME.set(rt);
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
                Incoming::Call(call, answer) => {
                    let _ = answer.send(crate::control::perform(call, cx));
                }
            });
        }
    })
    .detach();
}

/// What the socket hands to GPUI.
pub(crate) enum Incoming {
    /// A second launch's folder, or only a window to raise.
    Open(Forwarded),
    /// A tool's call, and where its answer goes.
    Call(Call, oneshot::Sender<Result<String, String>>),
}

/// Answer every connection. One whose first line is a second launch's open
/// is answered and closed; any other is an MCP conversation, carried on
/// until the client hangs up. What a line asks for goes to GPUI before the
/// answer goes back, so a second launch that hears yes has been heard.
async fn serve(listener: UnixListener, tx: UnboundedSender<Incoming>) {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                // ponytail: one failed accept ends the listener; retry with a
                // pause if it ever happens for a reason that passes.
                eprintln!("onehand: the socket stopped listening: {e}");
                return;
            }
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let first = tokio::time::timeout(LINE_WAIT, next_line(&mut reader)).await;
            let Ok(Some(first)) = first else {
                return;
            };
            let Some((reply, forwarded)) = instance::open(&first) else {
                return crate::control::converse(first, reader, write, tx).await;
            };
            if let Some(forwarded) = forwarded {
                let _ = tx.unbounded_send(Incoming::Open(forwarded));
            }
            let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
        });
    }
}

/// The next line `reader` holds, or `None` at its end, on a read error, or
/// past [`LINE_CAP`] without a newline, which ends the connection rather
/// than being held in memory.
pub(crate) async fn next_line(reader: &mut BufReader<OwnedReadHalf>) -> Option<String> {
    let mut line = String::new();
    match (&mut *reader).take(LINE_CAP).read_line(&mut line).await {
        Ok(0) | Err(_) => None,
        Ok(_) if !line.ends_with('\n') && line.len() as u64 >= LINE_CAP => None,
        Ok(_) => Some(line),
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
    use serde_json::{Value, json};
    use std::io::{BufRead as _, Write as _};
    use std::os::unix::net::UnixStream;

    /// A listener on a socket of its own, and what reaches GPUI from it.
    fn listening(
        name: &str,
    ) -> (
        PathBuf,
        tokio::runtime::Runtime,
        UnboundedReceiver<Incoming>,
    ) {
        let dir = std::env::temp_dir().join(format!("onehand-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("onehand.sock");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let listener = {
            let _in_runtime = rt.enter();
            UnixListener::bind(&socket).unwrap()
        };
        let (tx, rx) = unbounded();
        rt.spawn(serve(listener, tx));
        (dir, rt, rx)
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
    fn an_outside_client_is_answered_over_the_socket() {
        let (dir, _rt, mut rx) = listening("outside");
        // GPUI's side, answering every call alike.
        std::thread::spawn(move || {
            while let Some(incoming) = futures::executor::block_on(rx.next()) {
                if let Incoming::Call(_, answer) = incoming {
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
        assert_eq!(
            ask(&mut stream, list)["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            7
        );
        let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "list_agents", "arguments": {}}});
        let reply = ask(&mut stream, call);
        assert_eq!(reply["result"]["content"][0]["text"], "done");

        // A second launch still finds the listener, on a connection of its own.
        assert!(instance::forward(&socket, Some(&dir)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
