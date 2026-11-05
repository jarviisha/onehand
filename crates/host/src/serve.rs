//! The service itself: one per data directory, behind its lock, listening on
//! a Unix socket only its user can open. One laptop is attached at a time; a
//! new attach takes over from the old one, whose stream is told so and closed,
//! unless its hello asks not to take a host another laptop holds, when it is
//! refused instead. Sessions run on either way.

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
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::hub::{lock, Connector, Hub};
use crate::terms::{self, Terms};

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
    let shell = std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let terms = Arc::new(Mutex::new(Terms::new(shell, path.clone())));
    let mut hub = Hub::new(dir, path, connector);
    hub.load();
    let hub = Arc::new(Mutex::new(hub));
    accept(listener, hub, terms, build).await
}

/// The attach being served: where its lines go, the task serving it, and
/// the laptop run its hello named.
struct Attached {
    out: UnboundedSender<FromHost>,
    task: JoinHandle<()>,
    laptop: Option<String>,
}

type Asks = Lines<BufReader<OwnedReadHalf>>;

/// Take each attach as it comes, once it has said its first line.
async fn accept(
    listener: UnixListener,
    hub: Arc<Mutex<Hub>>,
    terms: Arc<Mutex<Terms>>,
    build: Build,
) -> ! {
    let current = Arc::new(tokio::sync::Mutex::new(None));
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                log(&format!("accept failed: {e}"));
                continue;
            }
        };
        tokio::spawn(admit(
            stream,
            current.clone(),
            hub.clone(),
            terms.clone(),
            build.clone(),
        ));
    }
}

/// Serve `stream` from its first line on, telling the attach before it that
/// it was taken over; or, when that line is a hello that does not take and
/// another laptop's attach is live, answer `Held` and close.
async fn admit(
    stream: UnixStream,
    current: Arc<tokio::sync::Mutex<Option<Attached>>>,
    hub: Arc<Mutex<Hub>>,
    terms: Arc<Mutex<Terms>>,
    build: Build,
) {
    let (read, mut write) = stream.into_split();
    let mut asks = BufReader::new(read).lines();
    let Ok(Some(first)) = asks.next_line().await else {
        return;
    };
    let mut current = current.lock().await;
    if let Some(id) = held(current.as_ref(), &first) {
        let held = FromHost::Answer {
            id,
            result: Ok(Reply::Held),
        };
        let _ = send(&mut write, &held).await;
        return;
    }
    if let Some(Attached { out, mut task, .. }) = current.take() {
        let _ = out.send(FromHost::Kicked);
        if tokio::time::timeout(KICK_GRACE, &mut task).await.is_err() {
            task.abort();
            let _ = task.await;
        }
        lock(&hub).unwatch_all();
        terms::lock(&terms).unwatch_all();
    }
    let (out, lines) = mpsc::unbounded_channel();
    let task = tokio::spawn(connection(
        asks,
        write,
        first.clone(),
        hub,
        terms,
        build,
        out.clone(),
        lines,
    ));
    *current = Some(Attached {
        out,
        task,
        laptop: laptop(&first),
    });
}

/// The id to answer `Held` to, when `first` is a hello that does not take
/// and `current` is another laptop's attach, still live.
fn held(current: Option<&Attached>, first: &str) -> Option<u64> {
    let Ok(Question {
        id,
        call: Call::Hello {
            take: false,
            laptop,
            ..
        },
    }) = serde_json::from_str(first)
    else {
        return None;
    };
    let live = current.filter(|now| !now.task.is_finished())?;
    // Its own attach, gone on its side but not yet on this one, is not held.
    let own = laptop.is_some() && live.laptop == laptop;
    (!own).then_some(id)
}

/// The laptop run a hello in `first` named, if it is one that named it.
fn laptop(first: &str) -> Option<String> {
    match serde_json::from_str(first) {
        Ok(Question {
            call: Call::Hello { laptop, .. },
            ..
        }) => laptop,
        Ok(_) | Err(_) => None,
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

/// Serve one attach, `first` the line it opened with: its asks in, its
/// answers and the entries it watches out.
#[allow(clippy::too_many_arguments)]
async fn connection(
    mut asks: Asks,
    mut write: OwnedWriteHalf,
    first: String,
    hub: Arc<Mutex<Hub>>,
    terms: Arc<Mutex<Terms>>,
    build: Build,
    out: UnboundedSender<FromHost>,
    mut lines: UnboundedReceiver<FromHost>,
) {
    // The one attach served names the laptop its mirrors are kept for.
    lock(&hub).laptop = laptop(&first);
    if answer(&hub, &terms, &build, &out, &first) {
        shut_down(&mut write, &mut lines).await;
    }
    loop {
        tokio::select! {
            ask = asks.next_line() => {
                let Ok(Some(ask)) = ask else { break };
                // Answered whole, a Watch after its replay, before the next ask
                // is read: the laptop tells whose replay is whose by that order.
                if answer(&hub, &terms, &build, &out, &ask) {
                    shut_down(&mut write, &mut lines).await;
                }
            }
            Some(line) = lines.recv() => {
                let (kicked, output) = match &line {
                    FromHost::Kicked => (true, None),
                    FromHost::TermOutput { term, data } => (false, Some((*term, data.0.len()))),
                    FromHost::Answer { .. }
                    | FromHost::Replay { .. }
                    | FromHost::Entry { .. }
                    | FromHost::TermExit { .. } => (false, None),
                };
                if send(&mut write, &line).await.is_err() || kicked {
                    return;
                }
                // Written, so the terminal's reader may go on if it waits for it.
                if let Some((term, n)) = output {
                    terms::lock(&terms).sent(term, n, &out);
                }
            }
        }
    }
    // The laptop went away: say what was already queued, then stop watching for it.
    flush(&mut write, &mut lines).await;
    lock(&hub).unwatch_all();
    terms::lock(&terms).unwatch_all();
}

/// Say what was queued, then exit, as a `Shutdown` asked.
async fn shut_down(write: &mut OwnedWriteHalf, lines: &mut UnboundedReceiver<FromHost>) -> ! {
    flush(write, lines).await;
    log("shut down by request");
    // ponytail: exit skips kill_on_drop, so agent children are left to end on
    // their own when their stdin closes; kill them here if an adapter is ever
    // seen to outlive it.
    std::process::exit(0);
}

/// Answer one line; whether the service should now exit.
fn answer(
    hub: &Arc<Mutex<Hub>>,
    terms: &Arc<Mutex<Terms>>,
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
                terms: terms::lock(terms).list().len(),
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
        // On the loop: it marks the session and drops its agent's task.
        Call::End { session } => lock(hub).end_session(&session).map(|()| Reply::Done),
        Call::Watch { session, after } => {
            Hub::watch(hub, &session, after, out.clone()).map(|()| Reply::Done)
        }
        Call::Request { session, request } => {
            lock(hub).request(&session, request).map(|()| Reply::Done)
        }
        Call::Probe { path } => Ok(Reply::Probed {
            is_dir: fs::metadata(path).is_ok_and(|m| m.is_dir()),
        }),
        call @ (Call::ReadDir { .. }
        | Call::ReadFile { .. }
        | Call::SaveFile { .. }
        | Call::GitStatus { .. }
        | Call::Home
        | Call::Clone { .. }
        | Call::MirrorFiles { .. }) => {
            if let Err(e) = touched(&call).map_or(Ok(()), |path| lock(hub).may_use(path)) {
                let _ = out.send(FromHost::Answer { id, result: Err(e) });
                return false;
            }
            // Off the loop: a big folder, a slow repository or a clone must
            // not hold up the asks behind it, and the answer goes out when it
            // is done.
            let out = out.clone();
            tokio::task::spawn_blocking(move || {
                let result = crate::files::answer(call);
                let _ = out.send(FromHost::Answer { id, result });
            });
            return false;
        }
        Call::TermOpen { cwd, cols, rows } => lock(hub)
            .may_use(&cwd)
            .and_then(|()| Terms::open(terms, cwd, cols, rows))
            .map(|term| Reply::TermOpened { term }),
        Call::TermList => Ok(Reply::Terms(terms::lock(terms).list())),
        // Answered after its replay, which is already on `out`.
        Call::TermWatch { term } => terms::lock(terms)
            .watch(term, out.clone())
            .map(|()| Reply::Done),
        Call::TermWrite { term, data } => {
            terms::lock(terms).write(term, data.0).map(|()| Reply::Done)
        }
        Call::TermResize { term, cols, rows } => terms::lock(terms)
            .resize(term, cols, rows)
            .map(|()| Reply::Done),
        Call::TermClose { term } => terms::lock(terms).close(term).map(|()| Reply::Done),
        // On the loop: it touches one small file.
        Call::Bypass { cwd, on } => {
            let mut hub = lock(hub);
            match on {
                None => Ok(hub.bypass(&cwd)),
                Some(on) => hub.set_bypass(cwd, on),
            }
            .map(|on| Reply::Bypass { on })
        }
        // On the loop, as the switch is: each touches one small file, and the
        // key's check and its change must not let a prompt in between.
        Call::Mirror { name, branch } => lock(hub)
            .mirror(&name, branch.as_deref())
            .map(Reply::Mirror),
        // Checked and marked on the loop, so no prompt slips in before the
        // merge; the merge itself off it, as a file call is.
        Call::MirrorUpdate { path, commit } => {
            let branch = match lock(hub).begin_update(&path) {
                Ok(branch) => branch,
                Err(e) => {
                    let _ = out.send(FromHost::Answer { id, result: Err(e) });
                    return false;
                }
            };
            let (hub, out) = (hub.clone(), out.clone());
            tokio::task::spawn_blocking(move || {
                let merged = crate::hub::update_blocking(&path, &branch, &commit);
                let mut hub = lock(&hub);
                hub.end_update(&path);
                let result = merged
                    .and_then(|()| hub.mirror_state(&path))
                    .map(Reply::Mirror);
                let _ = out.send(FromHost::Answer { id, result });
            });
            return false;
        }
        Call::MirrorKey {
            path,
            to,
            generation,
        } => lock(hub)
            .mirror_key(&path, to, generation)
            .map(Reply::Mirror),
    };
    let _ = out.send(FromHost::Answer { id, result });
    exit
}

/// The folder or file a file call reaches, which may lie in a mirror.
fn touched(call: &Call) -> Option<&Path> {
    match call {
        Call::ReadDir { path }
        | Call::ReadFile { path }
        | Call::SaveFile { path, .. }
        | Call::MirrorFiles { path }
        | Call::MirrorUpdate { path, .. } => Some(path),
        Call::GitStatus { root } => Some(root),
        Call::Clone { into, .. } => Some(into),
        Call::Hello { .. }
        | Call::Shutdown
        | Call::List
        | Call::Open { .. }
        | Call::Watch { .. }
        | Call::End { .. }
        | Call::Request { .. }
        | Call::Probe { .. }
        | Call::Home
        | Call::TermOpen { .. }
        | Call::TermList
        | Call::TermWatch { .. }
        | Call::TermWrite { .. }
        | Call::TermResize { .. }
        | Call::TermClose { .. }
        | Call::Bypass { .. }
        | Call::Mirror { .. }
        | Call::MirrorKey { .. } => None,
    }
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
