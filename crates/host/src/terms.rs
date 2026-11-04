//! The host's terminals: shells on PTYs that run on while no laptop is
//! attached, each keeping its latest output for the next watch to replay.
//! They sit behind a lock of their own, not the hub's, so a busy shell holds
//! up no session.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::sync::{mpsc as std_mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use onehand_core::host::wire::{Bytes, FromHost, HostTerm, TermInfo};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc::UnboundedSender;

/// How much of a terminal's output is kept for a replay.
pub(crate) const TERM_KEEP: usize = 1024 * 1024;

/// Said first in a replay whose start was trimmed away.
pub(crate) const NOT_KEPT: &str = "\r\n[Earlier output was not kept]\r\n";

/// How many bytes of a terminal's output may be queued for its watcher and
/// not yet written to the attach. Past it the terminal's reader waits, so a
/// program printing faster than the link carries it is slowed down, as in a
/// real terminal, instead of piling its output up in the service.
pub(crate) const TERM_IN_FLIGHT: usize = 256 * 1024;

/// How much the reader takes from the PTY at a time.
const READ_CHUNK: usize = 8192;

/// How often a reader held up by a watcher checks that the watcher's attach
/// is still there, in case it went away without the reader being woken.
const LOST_CHECK: Duration = Duration::from_secs(1);

pub(crate) struct Terms {
    next: u64,
    map: HashMap<HostTerm, Term>,
    /// The program every terminal runs.
    shell: OsString,
    /// The login shell's `PATH`, for the shell's environment.
    path: Option<OsString>,
    /// Woken whenever a reader held up by its watcher may go on.
    space: Arc<Condvar>,
}

struct Term {
    cwd: PathBuf,
    /// To a thread of the terminal's own that writes into the PTY: a program
    /// that reads no input fills the PTY until a write blocks, and that must
    /// hold up neither the attach nor this lock.
    writer: std_mpsc::Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    kept: VecDeque<u8>,
    /// Whether output was trimmed from the front of `kept`.
    dropped: bool,
    /// The one watch output goes to; a newer watch replaces it.
    watcher: Option<Watcher>,
}

struct Watcher {
    sink: UnboundedSender<FromHost>,
    /// Bytes of output put on `sink` and not yet written to the attach.
    in_flight: usize,
}

impl Terms {
    pub(crate) fn new(shell: OsString, path: Option<OsString>) -> Terms {
        Terms {
            next: 1,
            map: HashMap::new(),
            shell,
            path,
            space: Arc::new(Condvar::new()),
        }
    }

    /// Start a shell in `cwd`, sized `cols` by `rows`.
    pub(crate) fn open(
        terms: &Arc<Mutex<Terms>>,
        cwd: PathBuf,
        cols: u16,
        rows: u16,
    ) -> Result<HostTerm, String> {
        let (shell, path) = {
            let this = lock(terms);
            (this.shell.clone(), this.path.clone())
        };
        let pty = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut command = CommandBuilder::new(shell);
        // A login shell, so the user's profile is read as in their own terminals.
        command.arg("-l");
        command.cwd(&cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        if let Some(path) = path {
            command.env("PATH", path);
        }
        let child = pty
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        drop(pty.slave);
        let reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut pty_writer = pty.master.take_writer().map_err(|e| e.to_string())?;
        let (writer, typed) = std_mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for data in typed {
                if pty_writer.write_all(&data).is_err() {
                    return;
                }
            }
        });
        let mut this = lock(terms);
        let term = HostTerm(this.next);
        this.next += 1;
        this.map.insert(
            term,
            Term {
                cwd,
                writer,
                master: pty.master,
                child,
                kept: VecDeque::new(),
                dropped: false,
                watcher: None,
            },
        );
        drop(this);
        let terms = terms.clone();
        std::thread::spawn(move || read(&terms, term, reader));
        Ok(term)
    }

    pub(crate) fn list(&self) -> Vec<TermInfo> {
        self.map
            .iter()
            .map(|(term, t)| TermInfo {
                term: *term,
                cwd: t.cwd.clone(),
            })
            .collect()
    }

    /// Send what `term` kept to `sink`, then make it the terminal's one
    /// watcher. Under the lock the reader appends under, so the replay and
    /// what follows neither overlap nor leave a gap.
    pub(crate) fn watch(
        &mut self,
        term: HostTerm,
        sink: UnboundedSender<FromHost>,
    ) -> Result<(), String> {
        let t = self.map.get_mut(&term).ok_or_else(|| unknown(term))?;
        // The same attach watching again shares its queue, and so its count.
        let mut in_flight = t
            .watcher
            .take()
            .filter(|w| w.sink.same_channel(&sink))
            .map_or(0, |w| w.in_flight);
        // A reader held up by the watcher this one replaces must look again.
        self.space.notify_all();
        let mut replay = Vec::with_capacity(t.kept.len() + NOT_KEPT.len());
        if t.dropped {
            replay.extend_from_slice(NOT_KEPT.as_bytes());
        }
        replay.extend(t.kept.iter());
        if !replay.is_empty() {
            let replay_len = replay.len();
            let line = FromHost::TermOutput {
                term,
                data: Bytes(replay),
            };
            if sink.send(line).is_err() {
                return Ok(());
            }
            in_flight += replay_len;
        }
        t.watcher = Some(Watcher { sink, in_flight });
        Ok(())
    }

    /// `n` bytes of `term`'s output went out to the attach behind `sink`,
    /// so its reader may go on if it waits for them.
    pub(crate) fn sent(&mut self, term: HostTerm, n: usize, sink: &UnboundedSender<FromHost>) {
        let Some(w) = self.map.get_mut(&term).and_then(|t| t.watcher.as_mut()) else {
            return;
        };
        // Output written for an attach that no longer watches counts for nothing.
        if w.sink.same_channel(sink) {
            w.in_flight = w.in_flight.saturating_sub(n);
            self.space.notify_all();
        }
    }

    pub(crate) fn write(&self, term: HostTerm, data: Vec<u8>) -> Result<(), String> {
        let t = self.map.get(&term).ok_or_else(|| unknown(term))?;
        t.writer.send(data).map_err(|_| ENDED.to_string())
    }

    pub(crate) fn resize(&self, term: HostTerm, cols: u16, rows: u16) -> Result<(), String> {
        let t = self.map.get(&term).ok_or_else(|| unknown(term))?;
        t.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())
    }

    /// Let `term` go at once and end its shell, which may ignore the hangup
    /// or have a job holding the PTY open: either way it is listed no more.
    pub(crate) fn close(&mut self, term: HostTerm) -> Result<(), String> {
        let mut t = self.map.remove(&term).ok_or_else(|| unknown(term))?;
        if let Some(w) = t.watcher.take() {
            let _ = w.sink.send(FromHost::TermExit { term });
        }
        self.space.notify_all();
        // On a thread: the kill sends SIGHUP, gives the shell a moment to
        // leave on its own, then sends SIGKILL; the wait reaps it.
        // ponytail: a background job that keeps the PTY open outlives the
        // shell, and so does its reader thread; kill the session to end both.
        std::thread::spawn(move || {
            let _ = t.child.kill();
            let _ = t.child.wait();
        });
        Ok(())
    }

    /// Drop every watcher (the attach went away or was replaced). Terminals run on.
    pub(crate) fn unwatch_all(&mut self) {
        for t in self.map.values_mut() {
            t.watcher = None;
        }
        self.space.notify_all();
    }
}

/// Keep and pass on what `term` prints until it ends, then say so, let it go
/// and reap its shell.
fn read(terms: &Mutex<Terms>, term: HostTerm, mut reader: Box<dyn std::io::Read + Send>) {
    let mut buf = [0u8; READ_CHUNK];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let mut this = lock(terms);
        let Some(t) = this.map.get_mut(&term) else {
            return;
        };
        keep(&mut t.kept, &mut t.dropped, &buf[..n]);
        if let Some(w) = &mut t.watcher {
            let line = FromHost::TermOutput {
                term,
                data: Bytes(buf[..n].to_vec()),
            };
            if w.sink.send(line).is_err() {
                t.watcher = None;
            } else {
                w.in_flight += n;
            }
        }
        // The wait lets the lock go, so the attach can count its writes down,
        // and a new watch, an unwatch or a close can wake it.
        let space = this.space.clone();
        while held_up(&this, term) {
            this = space
                .wait_timeout(this, LOST_CHECK)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
    let gone = lock(terms).map.remove(&term);
    if let Some(mut t) = gone {
        if let Some(w) = t.watcher.take() {
            let _ = w.sink.send(FromHost::TermExit { term });
        }
        let _ = t.child.wait();
    }
}

/// Whether `term`'s reader must wait for its watcher's attach to catch up.
/// With no watcher, or one whose attach is gone, it never waits: `kept` is
/// capped on its own.
fn held_up(terms: &Terms, term: HostTerm) -> bool {
    terms
        .map
        .get(&term)
        .and_then(|t| t.watcher.as_ref())
        .is_some_and(|w| w.in_flight > TERM_IN_FLIGHT && !w.sink.is_closed())
}

/// Append `data` to `kept`, trimming its front to `TERM_KEEP` and noting so.
fn keep(kept: &mut VecDeque<u8>, dropped: &mut bool, data: &[u8]) {
    kept.extend(data);
    if kept.len() > TERM_KEEP {
        kept.drain(..kept.len() - TERM_KEEP);
        *dropped = true;
    }
}

pub(crate) fn lock(terms: &Mutex<Terms>) -> MutexGuard<'_, Terms> {
    terms.lock().unwrap_or_else(|e| e.into_inner())
}

const ENDED: &str = "this terminal has ended";

fn unknown(term: HostTerm) -> String {
    format!("no terminal {}", term.0)
}

#[cfg(test)]
mod tests;
