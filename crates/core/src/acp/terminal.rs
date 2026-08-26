//! Client side of the ACP **terminal extension**.
//!
//! Advertised as `capabilities.terminal = true` in `initialize`. The agent then
//! drives terminals via reverse requests: `terminal/create` spawns a real PTY
//! (via `portable-pty`), `terminal/output` reads the buffered bytes,
//! `terminal/wait_for_exit` settles when the process exits, `terminal/kill`
//! signals it, and `terminal/release` drops it.
//!
//! Each terminal owns one OS thread that does the blocking PTY read: it appends
//! to a **byte-capped** buffer (the authoritative source for `terminal/output`)
//! *and* streams chunks to the UI over [`TermStream`] for a live card. The
//! thread also `wait()`s the child and notifies any parked `wait_for_exit`.

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;

/// Default cap on a terminal's retained output (most recent bytes kept).
const DEFAULT_BYTE_LIMIT: usize = 1 << 20; // 1 MiB

/// Ceiling on an agent-requested `outputByteLimit`.
///
/// The floor was already here; the ceiling was not, so an agent asking for
/// `2^60` got a buffer bounded only by RAM. The value is the
/// agent's *request*, not a promise we owe it — 64 MiB of captured command
/// output is far past anything a model reads, and past it the buffer is a
/// denial of service on the machine running the client.
const MAX_BYTE_LIMIT: usize = 64 << 20; // 64 MiB

/// A live event from a terminal's reader thread, forwarded to the UI.
#[derive(Debug, Clone)]
pub enum TermStream {
    Output { id: String, data: Vec<u8> },
    Exit { id: String, code: Option<i32> },
}

/// A byte-capped ring-ish buffer: keeps the most recent `limit` bytes.
struct Buffer {
    data: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl Buffer {
    fn push(&mut self, chunk: &[u8]) {
        self.data.extend_from_slice(chunk);
        if self.data.len() > self.limit {
            let overflow = self.data.len() - self.limit;
            self.data.drain(0..overflow);
            self.truncated = true;
        }
    }
}

/// Shared exit state, settled once by the reader thread.
#[derive(Default)]
pub struct ExitState {
    pub exited: bool,
    pub code: Option<i32>,
}

struct Term {
    buffer: Arc<Mutex<Buffer>>,
    exit: Arc<Mutex<ExitState>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// Kept alive so the PTY (and our reader) stays open.
    _master: Box<dyn MasterPty + Send>,
}

/// Owns all live terminals for one session (lives in the client task).
pub struct TerminalRegistry {
    terms: HashMap<String, Term>,
    next: u64,
    /// Bounded on purpose: a PTY can produce output at memory speed while the
    /// UI drains at frame rate. The reader thread's `blocking_send` parks when
    /// the channel is full, the PTY buffer fills, and the child blocks on
    /// write — the same flow control a real terminal applies.
    stream_tx: Sender<TermStream>,
}

impl TerminalRegistry {
    pub fn new(stream_tx: Sender<TermStream>) -> Self {
        Self {
            terms: HashMap::new(),
            next: 0,
            stream_tx,
        }
    }

    /// `terminal/create`: spawn the command in a PTY, returning `{ terminalId }`.
    pub fn create(&mut self, params: &Value, default_cwd: &Path) -> Result<Value, String> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .ok_or("missing command")?;
        let args: Vec<String> = params
            .get("args")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| default_cwd.to_path_buf());
        let limit = params
            .get("outputByteLimit")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_BYTE_LIMIT)
            .clamp(1024, MAX_BYTE_LIMIT);

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(command);
        for a in &args {
            cmd.arg(a);
        }
        cmd.cwd(&cwd);
        if let Some(env) = params.get("env").and_then(Value::as_array) {
            for e in env {
                if let (Some(k), Some(v)) = (
                    e.get("name").and_then(Value::as_str),
                    e.get("value").and_then(Value::as_str),
                ) {
                    cmd.env(k, v);
                }
            }
        }

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave); // close the slave so the reader EOFs when the child exits
        let killer = child.clone_killer();
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;

        self.next += 1;
        let id = format!("term-{}", self.next);
        let buffer = Arc::new(Mutex::new(Buffer {
            data: Vec::new(),
            limit,
            truncated: false,
        }));
        let exit = Arc::new(Mutex::new(ExitState::default()));

        // Reader thread: blocking PTY read → buffer + UI stream, until EOF.
        {
            let id = id.clone();
            let buffer = buffer.clone();
            let stream_tx = self.stream_tx.clone();
            std::thread::spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 4096];
                // Streamed chunks are `from_utf8_lossy`'d independently on the
                // client side, so a multibyte char split across two reads
                // (Vietnamese, box-drawing, `→`) would render as `�`. Hold the
                // incomplete trailing bytes back until the next read completes
                // them. The byte buffer above needs no such care — it stays
                // contiguous and is decoded whole.
                let mut carry: Vec<u8> = Vec::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buffer.lock().unwrap().push(&buf[..n]);
                            carry.extend_from_slice(&buf[..n]);
                            let send = utf8_complete_prefix_len(&carry);
                            if send > 0 {
                                let data: Vec<u8> = carry.drain(..send).collect();
                                let _ = stream_tx.blocking_send(TermStream::Output {
                                    id: id.clone(),
                                    data,
                                });
                            }
                        }
                    }
                }
                // EOF with a held-back partial char: flush it as-is (the
                // lossy decode shows `�`, which is what it truly was).
                if !carry.is_empty() {
                    let _ = stream_tx.blocking_send(TermStream::Output {
                        id: id.clone(),
                        data: carry,
                    });
                }
            });
        }
        // Waiter thread, separate from the reader on purpose: the exit state
        // must settle when the *child* exits, not when the PTY EOFs. A child
        // that forks a background process (`bash -c "server &"`) exits while
        // the grandchild keeps the slave fd open — the master never EOFs, and
        // gating on it left the parked `wait_for_exit` (and thus the agent's
        // whole turn) hanging forever.
        {
            let id = id.clone();
            let exit = exit.clone();
            let stream_tx = self.stream_tx.clone();
            std::thread::spawn(move || {
                let mut child = child;
                let code = child.wait().ok().map(|s| s.exit_code() as i32);
                {
                    let mut e = exit.lock().unwrap();
                    e.exited = true;
                    e.code = code;
                }
                let _ = stream_tx.blocking_send(TermStream::Exit { id, code });
            });
        }

        self.terms.insert(
            id.clone(),
            Term {
                buffer,
                exit,
                killer,
                _master: pair.master,
            },
        );
        Ok(json!({ "terminalId": id }))
    }

    /// `terminal/output`: the current buffered output + truncation + exit.
    pub fn output(&self, id: &str) -> Result<Value, String> {
        let term = self.terms.get(id).ok_or("unknown terminal")?;
        let buf = term.buffer.lock().unwrap();
        let output = String::from_utf8_lossy(&buf.data).into_owned();
        let exit = term.exit.lock().unwrap();
        Ok(json!({
            "output": output,
            "truncated": buf.truncated,
            "exitStatus": exit_status_value(&exit),
        }))
    }

    /// Whether a terminal has already exited (answer `wait_for_exit` inline).
    pub fn exit_now(&self, id: &str) -> Option<Value> {
        let term = self.terms.get(id)?;
        let exit = term.exit.lock().unwrap();
        exit.exited
            .then(|| json!({ "exitStatus": exit_status_value(&exit) }))
    }

    /// The shared exit state for parking a `wait_for_exit` (when not yet
    /// exited). The client polls it. Returns `None` for an unknown terminal.
    pub fn waiter(&self, id: &str) -> Option<Arc<Mutex<ExitState>>> {
        self.terms.get(id).map(|t| t.exit.clone())
    }

    /// `terminal/kill`: signal the child to stop (the terminal stays available
    /// so the agent can still read the final output).
    pub fn kill(&mut self, id: &str) -> Result<Value, String> {
        let term = self.terms.get_mut(id).ok_or("unknown terminal")?;
        let _ = term.killer.kill();
        Ok(Value::Null)
    }

    /// `terminal/release`: kill and drop the terminal entirely.
    pub fn release(&mut self, id: &str) -> Result<Value, String> {
        if let Some(mut term) = self.terms.remove(id) {
            let _ = term.killer.kill();
        }
        Ok(Value::Null)
    }
}

/// The registry dying (Ctrl+Shift+R restart, session close, app exit) must not orphan
/// agent-spawned processes: only `terminal/release` kills otherwise, so a dev
/// server started by the agent would outlive its session invisibly. Killing here
/// also EOFs each reader thread and settles any parked `wait_for_exit` poller.
impl Drop for TerminalRegistry {
    fn drop(&mut self) {
        for term in self.terms.values_mut() {
            let _ = term.killer.kill();
        }
    }
}

/// The longest prefix of `bytes` that does not end mid-way through a UTF-8
/// character. Genuinely invalid bytes (binary output) count as complete —
/// only an *incomplete trailing* sequence, which the next PTY read may
/// finish, is held back (at most 3 bytes).
fn utf8_complete_prefix_len(bytes: &[u8]) -> usize {
    let mut from = 0;
    loop {
        match std::str::from_utf8(&bytes[from..]) {
            Ok(_) => return bytes.len(),
            Err(e) => match e.error_len() {
                // Invalid sequence mid-stream: skip it and keep scanning.
                Some(len) => from += e.valid_up_to() + len,
                // Incomplete sequence at the very end: hold it back.
                None => return from + e.valid_up_to(),
            },
        }
    }
}

/// Build the `exitStatus` value (or `null` if still running).
fn exit_status_value(exit: &ExitState) -> Value {
    if exit.exited {
        json!({ "exitCode": exit.code })
    } else {
        Value::Null
    }
}

/// Build the `wait_for_exit` result from a settled [`ExitState`].
pub fn wait_result(exit: &ExitState) -> Value {
    json!({ "exitStatus": exit_status_value(exit) })
}

#[cfg(test)]
mod tests {
    use super::utf8_complete_prefix_len;

    #[test]
    fn complete_ascii_and_utf8_pass_whole() {
        assert_eq!(utf8_complete_prefix_len(b"hello"), 5);
        assert_eq!(utf8_complete_prefix_len("xin chào →".as_bytes()), 13);
        assert_eq!(utf8_complete_prefix_len(b""), 0);
    }

    #[test]
    fn incomplete_trailing_char_is_held_back() {
        // "à" is 0xC3 0xA0; cut after the lead byte.
        let mut bytes = b"xin ch".to_vec();
        bytes.push(0xC3);
        assert_eq!(utf8_complete_prefix_len(&bytes), 6);
        // "→" is 3 bytes; keep only the first two.
        let mut bytes = b"ok ".to_vec();
        bytes.extend_from_slice(&"→".as_bytes()[..2]);
        assert_eq!(utf8_complete_prefix_len(&bytes), 3);
    }

    #[test]
    fn invalid_bytes_are_not_held_back() {
        // A stray continuation byte mid-stream is real garbage, not a split
        // char — it must flow through (lossy decode renders `�`).
        assert_eq!(utf8_complete_prefix_len(&[b'a', 0x80, b'b']), 3);
        // …unless the tail after it is itself an incomplete char.
        assert_eq!(utf8_complete_prefix_len(&[b'a', 0x80, 0xC3]), 2);
    }
}
