//! A host's terminals over the attach. A terminal's bytes are read and written
//! through `std::io`, on a thread of the caller's: a reader blocks on what the
//! pump hands it, and a writer, a resize or a close only queues a line, so none
//! of them needs the runtime the pump runs on.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use super::{Call, Cmd, HostConn, HostError};
use crate::host::wire::{Bytes, HostTerm, TermInfo};

impl HostConn {
    /// Start a shell in `cwd` on the host, sized `cols` by `rows`.
    pub async fn term_open(
        &self,
        cwd: PathBuf,
        cols: u16,
        rows: u16,
    ) -> Result<HostTerm, HostError> {
        let call = Call::TermOpen { cwd, cols, rows };
        self.ask_for(call, |a| a.term_opened).await
    }

    /// The terminals the host keeps running.
    pub async fn term_list(&self) -> Result<Vec<TermInfo>, HostError> {
        self.ask_for(Call::TermList, |a| a.terms).await
    }

    /// What `term` printed and prints from now on. The reader ends when the
    /// terminal exits, the connection goes, or a newer watch takes the term.
    pub fn term_watch(&self, term: HostTerm) -> TermReader {
        let (tx, rx) = std::sync::mpsc::channel();
        // On a connection already gone the command comes back with `tx`,
        // which drops here: the reader ends at once.
        let _ = self.cmds.send(Cmd::TermWatch(term, tx));
        TermReader {
            rx,
            held: Vec::new(),
            at: 0,
            _keep: self.cmds.clone(),
        }
    }

    /// Where what is typed into `term` goes.
    pub fn term_writer(&self, term: HostTerm) -> TermWriter {
        TermWriter {
            cmds: self.cmds.clone(),
            term,
        }
    }

    pub fn term_resize(&self, term: HostTerm, cols: u16, rows: u16) {
        self.fire(Call::TermResize { term, cols, rows });
    }

    /// End `term`'s process on the host.
    pub fn term_close(&self, term: HostTerm) {
        self.fire(Call::TermClose { term });
    }

    /// Send `call` and drop its answer; it goes in turn with everything else asked.
    fn fire(&self, call: Call) {
        let _ = self.cmds.send(Cmd::Fire(call));
    }

    /// A connection to `host` that answers nothing and keeps the calls fired
    /// at it for `Recorded::fired`. Building one needs no runtime.
    pub fn recording(host: &str) -> (HostConn, Recorded) {
        let (cmds, rx) = mpsc::unbounded_channel();
        let conn = Self {
            cmds,
            host: host.into(),
            taken: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        (conn, Recorded(Mutex::new(rx)))
    }
}

/// What a recording connection was told.
pub struct Recorded(Mutex<mpsc::UnboundedReceiver<Cmd>>);

impl Recorded {
    /// The calls fired since the last look, in order. Anything asked rather
    /// than fired is dropped unanswered.
    pub fn fired(&self) -> Vec<Call> {
        let mut rx = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut fired = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Fire(call) => fired.push(call),
                Cmd::Ask(..)
                | Cmd::Watch(..)
                | Cmd::Request(..)
                | Cmd::TermWatch(..)
                | Cmd::Forget => {}
            }
        }
        fired
    }
}

/// A terminal's output, read as it comes; it reads 0 once the output has ended.
pub struct TermReader {
    rx: Receiver<Vec<u8>>,
    /// The chunk being read out, and how far.
    held: Vec<u8>,
    at: usize,
    /// The reader keeps the connection up while it is read.
    _keep: mpsc::UnboundedSender<Cmd>,
}

impl io::Read for TermReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.at == self.held.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.held = chunk;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.held.len() - self.at);
        buf[..n].copy_from_slice(&self.held[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// Writes to a terminal: each `write` is one line to the host, queued at
/// once, so what is typed arrives in the order it was typed.
pub struct TermWriter {
    cmds: mpsc::UnboundedSender<Cmd>,
    term: HostTerm,
}

impl io::Write for TermWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let call = Call::TermWrite {
            term: self.term,
            data: Bytes(buf.to_vec()),
        };
        self.cmds
            .send(Cmd::Fire(call))
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The pump's end of each watched terminal: the sender of its latest watch,
/// with that watch's id. Dropping a sender ends its reader, so the stream
/// ending ends them all.
#[derive(Default)]
pub(super) struct Terms(HashMap<HostTerm, (u64, Sender<Vec<u8>>)>);

impl Terms {
    /// `tx` reads `term` from now on, for the watch asked as `id`; an earlier
    /// reader of it ends.
    pub(super) fn watch(&mut self, id: u64, term: HostTerm, tx: Sender<Vec<u8>>) {
        self.0.insert(term, (id, tx));
    }

    /// The watch asked as `id` was refused: its reader ends, unless a newer
    /// watch has taken the term since.
    pub(super) fn refused(&mut self, id: u64, term: HostTerm) {
        if self.0.get(&term).is_some_and(|(watch, _)| *watch == id) {
            self.0.remove(&term);
        }
    }

    pub(super) fn output(&mut self, term: HostTerm, data: Vec<u8>) {
        let Some((_, tx)) = self.0.get(&term) else {
            return;
        };
        if tx.send(data).is_err() {
            self.0.remove(&term);
        }
    }

    pub(super) fn exited(&mut self, term: HostTerm) {
        self.0.remove(&term);
    }
}
