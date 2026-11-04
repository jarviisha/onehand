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
use onehand_core::host::wire::{Build, Call, FromHost, Question, Reply};
use onehand_core::instance::{hold_lock, holding, socket_path};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::hub::{lock, Connector, Hub};

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// One line in `host.log`, or on stderr before the service has opened it.
pub(crate) fn log(message: &str) {
    match LOG.get() {
        Some(file) => {
            let _ = writeln!(
                file.lock().unwrap_or_else(|e| e.into_inner()),
                "{} {message}",
                unix_secs()
            );
        }
        None => eprintln!("onehand-host: {message}"),
    }
}

/// Seconds since the Unix epoch, or 0 on a clock set before it.
pub(crate) fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How long the old attach gets to take its `Kicked` before it is cut off.
const KICK_GRACE: Duration = Duration::from_secs(2);

pub(crate) async fn run(dir: PathBuf, build: Build) -> ExitCode {
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
    let mut hub = Hub::new(dir, path, connector);
    hub.load();
    let hub = Arc::new(Mutex::new(hub));
    accept(listener, hub, build).await
}

/// Take each attach as it comes, telling the one before it that it was taken over.
async fn accept(listener: UnixListener, hub: Arc<Mutex<Hub>>, build: Build) -> ! {
    let mut current: Option<(UnboundedSender<FromHost>, JoinHandle<()>)> = None;
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                log(&format!("accept failed: {e}"));
                continue;
            }
        };
        if let Some((out, mut task)) = current.take() {
            let _ = out.send(FromHost::Kicked);
            if tokio::time::timeout(KICK_GRACE, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
            lock(&hub).unwatch_all();
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
    let path = match tokio::time::timeout(Duration::from_secs(10), output).await {
        Ok(Ok(out)) if out.status.success() => after_marker(&out.stdout).map(<[u8]>::to_vec),
        Ok(Ok(_)) | Ok(Err(_)) | Err(_) => None,
    };
    match path {
        Some(path) => Some(OsString::from_vec(path)),
        None => {
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
    build: Build,
    out: UnboundedSender<FromHost>,
    mut lines: UnboundedReceiver<FromHost>,
) {
    let (read, mut write) = stream.into_split();
    let mut asks = BufReader::new(read).lines();
    loop {
        tokio::select! {
            ask = asks.next_line() => {
                let Ok(Some(ask)) = ask else { break };
                // Answered whole, a Watch after its replay, before the next ask
                // is read: the laptop tells whose replay is whose by that order.
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
                    FromHost::Kicked => true,
                    FromHost::Answer { .. } | FromHost::Replay { .. } | FromHost::Entry { .. } => false,
                };
                if send(&mut write, &line).await.is_err() || kicked {
                    return;
                }
            }
        }
    }
    // The laptop went away: say what was already queued, then stop watching for it.
    flush(&mut write, &mut lines).await;
    lock(&hub).unwatch_all();
}

/// Answer one line; whether the service should now exit.
fn answer(
    hub: &Arc<Mutex<Hub>>,
    build: &Build,
    out: &UnboundedSender<FromHost>,
    line: &str,
) -> bool {
    let Ok(Question { id, call }) = serde_json::from_str::<Question>(line) else {
        let _ = out.send(FromHost::Answer {
            id: 0,
            result: Err("unreadable request".into()),
        });
        return false;
    };
    let mut exit = false;
    let result = match call {
        Call::Hello { .. } => {
            let busy = lock(hub).list();
            Ok(Reply::Welcome {
                build: build.clone(),
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
        Call::List => Ok(Reply::Sessions(lock(hub).list())),
        Call::Open {
            cwd,
            agent,
            opening,
        } => Hub::open(hub, cwd, agent, opening).map(|session| Reply::Opened { session }),
        Call::Watch { session } => Hub::watch(hub, &session, out.clone()).map(|()| Reply::Done),
        Call::Request { session, request } => {
            lock(hub).request(&session, request).map(|()| Reply::Done)
        }
        Call::Probe { path } => Ok(Reply::Probed {
            is_dir: fs::metadata(path).is_ok_and(|m| m.is_dir()),
        }),
        call @ (Call::ReadDir { .. }
        | Call::ReadFile { .. }
        | Call::SaveFile { .. }
        | Call::GitStatus { .. }) => {
            // Off the loop: a big folder or a slow repository must not hold
            // up the asks behind it, and the answer goes out when it is done.
            let out = out.clone();
            tokio::task::spawn_blocking(move || {
                let result = crate::files::answer(call);
                let _ = out.send(FromHost::Answer { id, result });
            });
            return false;
        }
    };
    let _ = out.send(FromHost::Answer { id, result });
    exit
}

/// Write every line already queued.
async fn flush(write: &mut OwnedWriteHalf, lines: &mut UnboundedReceiver<FromHost>) {
    while let Ok(line) = lines.try_recv() {
        if send(write, &line).await.is_err() {
            return;
        }
    }
}

async fn send(write: &mut OwnedWriteHalf, line: &FromHost) -> std::io::Result<()> {
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
