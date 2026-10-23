//! The lock a launch starts with, and the socket its holder listens on.
//!
//! A launch that finds the lock held hands its folder to the process holding
//! it and exits, before GPUI is started at all. The holder reads those lines on
//! a tokio runtime of its own and crosses to GPUI over a `futures` channel; what
//! a line means is decided in core's `instance`, and this only carries it.

use crate::state::Shared;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::App;
use onehand_core::instance::{self, Forwarded, Unforwarded};
use onehand_core::workspace::Workspace;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixListener;

/// The listener's runtime, kept for the life of the process.
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Take the instance lock, or hand this launch's folder to the process that
/// holds it and exit. Runs before GPUI starts, so a second launch never opens
/// a window, and before the config directory's files are moved, which the
/// lock is there to protect.
pub fn hold_or_forward() {
    let dir = onehand_core::config::config_dir();
    if instance::hold_lock(&dir).is_ok() {
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

/// Listen on the socket beside the lock, for as long as the process lives.
///
/// Only the lock's holder does: it alone knows a socket file already there is
/// stale. A socket that cannot be bound is reported and the app carries on; a
/// second launch then says the app is already running, as it always did.
pub(crate) fn listen(cx: &mut App) {
    if !instance::holding() {
        return;
    }
    let socket = instance::socket_path(&onehand_core::config::config_dir());
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
    let _ = std::fs::remove_file(&socket);
    let listener = {
        let _in_runtime = rt.enter();
        UnixListener::bind(&socket)
    };
    let listener = match listener {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("onehand: no socket at {}: {e}", socket.display());
            return;
        }
    };
    // The user's own, as the config directory beside it is.
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
    }
    let (tx, mut rx) = unbounded();
    rt.spawn(serve(listener, tx));
    let _ = RUNTIME.set(rt);
    cx.spawn(async move |cx| {
        while let Some(forwarded) = rx.next().await {
            cx.update(|cx| open_forwarded(forwarded, cx));
        }
    })
    .detach();
}

/// Answer every connection: one line in, one line out, then closed. What a
/// line asks to be opened goes to GPUI before the answer goes back, so a
/// second launch that hears yes has been heard.
async fn serve(listener: UnixListener, tx: UnboundedSender<Forwarded>) {
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
            let mut line = String::new();
            if BufReader::new(read).read_line(&mut line).await.is_err() {
                return;
            }
            let (reply, forwarded) = instance::answer(&line);
            if let Some(forwarded) = forwarded {
                let _ = tx.unbounded_send(forwarded);
            }
            let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
        });
    }
}

/// Open what a second launch forwarded in the window in front, or the first
/// one, and raise it. Never inside another entity's update: this runs from the
/// channel's own task.
fn open_forwarded(forwarded: Forwarded, cx: &mut App) {
    let active = cx.active_window();
    let front = {
        let windows = &Shared::global(cx).windows;
        windows
            .iter()
            .find(|w| Some(w.handle) == active)
            .or(windows.first())
            .and_then(|w| Some((w.handle, w.shell.upgrade()?)))
    };
    let Some((handle, shell)) = front else {
        if let Some(path) = forwarded.path {
            crate::shell::open_or_focus(Workspace::seeded(path), cx);
        }
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
    use std::io::{BufRead as _, Write as _};

    #[test]
    fn a_second_launch_reaches_the_listener() {
        let dir = std::env::temp_dir().join(format!("onehand-listen-{}", std::process::id()));
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
        let (tx, mut rx) = unbounded();
        rt.spawn(serve(listener, tx));

        // A line that is not an open is answered, and the listener goes on.
        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        writeln!(stream, "nonsense").unwrap();
        let mut reply = String::new();
        std::io::BufReader::new(&stream)
            .read_line(&mut reply)
            .unwrap();
        assert!(reply.contains("-32700"), "{reply}");

        assert!(instance::forward(&socket, Some(&dir)).is_ok());
        assert_eq!(
            rx.try_recv().ok(),
            Some(Forwarded {
                path: Some(dir.clone())
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
