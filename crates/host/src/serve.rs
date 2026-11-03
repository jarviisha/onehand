//! The service itself: one per data directory, behind its lock, listening on
//! a Unix socket only its user can open. One laptop is attached at a time; a
//! new attach takes over from the old one, whose stream is told so and closed.
//! Sessions run on either way.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt as _;
use onehand_core::acp;
use onehand_core::host::wire::{Ask, Call, Line, Reply};
use onehand_core::instance::{hold_lock, holding, socket_path};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::hub::{Connector, Hub};

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// One line in `host.log`, or on stderr before the service has opened it.
pub(crate) fn log(message: &str) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    match LOG.get() {
        Some(file) => {
            let _ = writeln!(
                file.lock().unwrap_or_else(|e| e.into_inner()),
                "{secs} {message}"
            );
        }
        None => eprintln!("onehand-host: {message}"),
    }
}

/// How long the old attach gets to take its `Kicked` before it is cut off.
const KICK_GRACE: Duration = Duration::from_secs(2);

pub(crate) async fn run(dir: PathBuf, build: String) -> ExitCode {
    if let Err(e) = private_dir(&dir) {
        eprintln!("onehand-host: cannot make {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    if hold_lock(&dir).is_err() {
        // Another service already runs on this directory, which is what was wanted.
        return ExitCode::SUCCESS;
    }
    if !holding() {
        // Without the lock this process cannot know a socket there is stale,
        // so it must not pull it from under a service that may own it.
        eprintln!(
            "onehand-host: no lock could be taken on {}; not serving",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    match open_log(&dir) {
        Ok(file) => {
            let _ = LOG.set(Mutex::new(file));
        }
        Err(e) => eprintln!("onehand-host: no log: {e}"),
    }
    let listener = match bind(&dir) {
        Ok(listener) => listener,
        Err(e) => {
            log(&format!("cannot listen: {e}"));
            return ExitCode::FAILURE;
        }
    };
    log(&format!("serving build {build}"));
    // Bound first, so an attach waiting on the socket connects while this runs.
    let path = login_path().await;
    let connector: Connector = Arc::new(|launch, opts| acp::connect(launch, opts).boxed());
    let hub = Arc::new(Mutex::new(Hub::new(dir, path, connector)));
    accept(listener, hub, build).await
}

/// Take each attach as it comes, telling the one before it that it was taken over.
async fn accept(listener: UnixListener, hub: Arc<Mutex<Hub>>, build: String) -> ! {
    let mut current: Option<(UnboundedSender<Line>, JoinHandle<()>)> = None;
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                log(&format!("accept failed: {e}"));
                continue;
            }
        };
        if let Some((out, mut task)) = current.take() {
            let _ = out.send(Line::Kicked);
            if tokio::time::timeout(KICK_GRACE, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
            hub.lock().unwrap_or_else(|e| e.into_inner()).unwatch_all();
        }
        let (out, lines) = mpsc::unbounded_channel();
        let task = tokio::spawn(connection(
            stream,
            hub.clone(),
            build.clone(),
            out.clone(),
            lines,
        ));
        current = Some((out, task));
    }
}

/// `host.log` in `dir`, for appending.
pub(crate) fn open_log(dir: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("host.log"))
}

/// Make `dir`, and every folder it needs, readable by this user alone: the
/// journals in it hold whole transcripts.
pub(crate) fn private_dir(dir: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Listen on the socket in `dir`, clearing one a dead service left, open to
/// this user alone.
fn bind(dir: &Path) -> std::io::Result<UnixListener> {
    let socket = socket_path(dir);
    if !socket.starts_with(dir) {
        // A path too long for a socket address puts it in a shared folder,
        // where it would be open to others between the bind and the chmod.
        return Err(std::io::Error::other(format!(
            "{} is too long a path for the socket; set ONEHAND_HOST_DIR to a shorter one",
            dir.display()
        )));
    }
    let socket = socket.as_path();
    match fs::remove_file(socket) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// `PATH` as the user's login shell sets it: a service started by a bare SSH
/// command has a PATH that often lacks what the user's own shells add.
async fn login_path() -> Option<OsString> {
    let own = std::env::var_os("PATH");
    let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) else {
        return own;
    };
    let output = tokio::process::Command::new(shell)
        .args(["-ilc", &format!(r#"printf '\n{MARKER}%s' "$PATH""#)])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(Duration::from_secs(10), output).await {
        Ok(Ok(out)) if out.status.success() => match after_marker(&out.stdout) {
            Some(path) => Some(OsString::from_vec(path.to_vec())),
            None => {
                log("the login shell gave no PATH; using the service's own");
                own
            }
        },
        Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
            log("the login shell gave no PATH; using the service's own");
            own
        }
    }
}

/// What a login shell's output says after its last marker, so whatever its
/// startup files print before cannot end up in PATH.
const MARKER: &str = "__ONEHAND_PATH__";

fn after_marker(output: &[u8]) -> Option<&[u8]> {
    let marker = MARKER.as_bytes();
    let at = output.windows(marker.len()).rposition(|w| w == marker)?;
    Some(&output[at + marker.len()..]).filter(|path| !path.is_empty())
}

/// Serve one attach: its asks in, its answers and the entries it watches out.
async fn connection(
    stream: UnixStream,
    hub: Arc<Mutex<Hub>>,
    build: String,
    out: UnboundedSender<Line>,
    mut lines: UnboundedReceiver<Line>,
) {
    let (read, mut write) = stream.into_split();
    let mut asks = BufReader::new(read).lines();
    loop {
        tokio::select! {
            ask = asks.next_line() => {
                let Ok(Some(ask)) = ask else { break };
                let exit = answer(&hub, &build, &out, &ask);
                if exit {
                    flush(&mut write, &mut lines).await;
                    log("shut down by request");
                    // ponytail: exit skips kill_on_drop, so agent children are left
                    // to end on their own when their stdin closes; kill them here
                    // if an adapter is ever seen to outlive it.
                    std::process::exit(0);
                }
            }
            Some(line) = lines.recv() => {
                let kicked = match &line {
                    Line::Kicked => true,
                    Line::Answer { .. } | Line::Entry { .. } => false,
                };
                if send(&mut write, &line).await.is_err() || kicked {
                    return;
                }
            }
        }
    }
    // The laptop went away: say what was already queued, then stop watching for it.
    flush(&mut write, &mut lines).await;
    hub.lock().unwrap_or_else(|e| e.into_inner()).unwatch_all();
}

/// Answer one line; whether the service should now exit.
fn answer(hub: &Arc<Mutex<Hub>>, build: &str, out: &UnboundedSender<Line>, line: &str) -> bool {
    let Ok(Ask { id, call }) = serde_json::from_str::<Ask>(line) else {
        let _ = out.send(Line::Answer {
            id: 0,
            result: Err("unreadable request".into()),
        });
        return false;
    };
    let mut exit = false;
    let result = match call {
        Call::Hello { .. } => {
            let busy = hub.lock().unwrap_or_else(|e| e.into_inner()).list();
            Ok(Reply::Welcome {
                build: build.to_string(),
                busy: busy
                    .into_iter()
                    .filter(|s| s.busy)
                    .map(|s| s.session)
                    .collect(),
            })
        }
        Call::Shutdown => {
            exit = true;
            Ok(Reply::Done)
        }
        Call::List => Ok(Reply::Sessions(
            hub.lock().unwrap_or_else(|e| e.into_inner()).list(),
        )),
        Call::Open {
            cwd,
            agent,
            opening,
        } => Hub::open(hub, cwd, agent, opening).map(|session| Reply::Opened { session }),
        Call::Watch { session } => hub
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .watch(&session, out.clone())
            .map(|()| Reply::Done),
        Call::Request { session, request } => hub
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .request(&session, request)
            .map(|()| Reply::Done),
    };
    let _ = out.send(Line::Answer { id, result });
    exit
}

/// Write every line already queued.
async fn flush(write: &mut OwnedWriteHalf, lines: &mut UnboundedReceiver<Line>) {
    while let Ok(line) = lines.try_recv() {
        if send(write, &line).await.is_err() {
            return;
        }
    }
}

async fn send(write: &mut OwnedWriteHalf, line: &Line) -> std::io::Result<()> {
    let mut text = match serde_json::to_string(line) {
        Ok(text) => text,
        Err(e) => {
            log(&format!("a line could not be written: {e}"));
            return Ok(());
        }
    };
    text.push('\n');
    write.write_all(text.as_bytes()).await
}

#[cfg(test)]
mod tests;
