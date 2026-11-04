//! The host's terminals: shells on PTYs that run on while no laptop is
//! attached, each keeping its latest output for the next watch to replay.
//! They sit behind a lock of their own, not the hub's, so a busy shell holds
//! up no session.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::sync::{mpsc as std_mpsc, Arc, Mutex, MutexGuard};

use onehand_core::host::wire::{Bytes, FromHost, HostTerm, TermInfo};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc::UnboundedSender;

/// How much of a terminal's output is kept for a replay.
pub(crate) const TERM_KEEP: usize = 1024 * 1024;

/// Said first in a replay whose start was trimmed away.
pub(crate) const NOT_KEPT: &str = "\r\n[Earlier output was not kept]\r\n";

pub(crate) struct Terms {
    next: u64,
    map: HashMap<HostTerm, Term>,
    /// The program every terminal runs.
    shell: OsString,
    /// The login shell's `PATH`, for the shell's environment.
    path: Option<OsString>,
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
    watcher: Option<UnboundedSender<FromHost>>,
}

impl Terms {
    pub(crate) fn new(shell: OsString, path: Option<OsString>) -> Terms {
        Terms {
            next: 1,
            map: HashMap::new(),
            shell,
            path,
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
        let mut replay = Vec::with_capacity(t.kept.len() + NOT_KEPT.len());
        if t.dropped {
            replay.extend_from_slice(NOT_KEPT.as_bytes());
        }
        replay.extend(t.kept.iter());
        if !replay.is_empty() {
            let line = FromHost::TermOutput {
                term,
                data: Bytes(replay),
            };
            if sink.send(line).is_err() {
                t.watcher = None;
                return Ok(());
            }
        }
        t.watcher = Some(sink);
        Ok(())
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
        if let Some(sink) = t.watcher.take() {
            let _ = sink.send(FromHost::TermExit { term });
        }
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
    }
}

/// Keep and pass on what `term` prints until it ends, then say so, let it go
/// and reap its shell.
fn read(terms: &Mutex<Terms>, term: HostTerm, mut reader: Box<dyn std::io::Read + Send>) {
    let mut buf = [0u8; 8192];
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
        if let Some(sink) = &t.watcher {
            let line = FromHost::TermOutput {
                term,
                data: Bytes(buf[..n].to_vec()),
            };
            if sink.send(line).is_err() {
                t.watcher = None;
            }
        }
    }
    let gone = lock(terms).map.remove(&term);
    if let Some(mut t) = gone {
        if let Some(sink) = t.watcher.take() {
            let _ = sink.send(FromHost::TermExit { term });
        }
        let _ = t.child.wait();
    }
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
