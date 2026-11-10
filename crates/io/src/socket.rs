//! The socket beside the instance lock, served: a second launch's line, or an
//! MCP conversation with onehand's tools.
//!
//! It runs on a tokio runtime of its own, and crosses to the front end over a
//! `futures` channel only to hand it what a second launch forwarded or a tool
//! call to carry out ([`Incoming`]). What a line means is core's: a second
//! launch's is `onehand_core::instance`'s, every MCP message
//! `onehand_core::control`'s. Who may call what is asked of the front end on
//! every message ([`Front::standing`]), since it is the front end's drive
//! that knows.

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::channel::oneshot;
use onehand_core::control::{self, Call, Conn, Handled, Peer, Standing};
use onehand_core::instance::{self, Forwarded};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::UnixListener;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

/// How long a connection may take to send its first line, and how long any
/// line may be: past either it is dropped, not waited on.
const LINE_WAIT: Duration = Duration::from_secs(5);
const LINE_CAP: u64 = 64 * 1024;

/// Said when a call's answer cannot come, because the app is going.
const CLOSING: &str = "onehand is closing";

/// What the socket hands the front end.
pub enum Incoming {
    /// A second launch's folder, or only a window to raise.
    Open(Forwarded),
    /// A tool's call, who made it, and where its answer goes.
    Call(Peer, Call, oneshot::Sender<Result<String, String>>),
}

/// Where what a session's connection is told unasked goes, by its token.
pub type Joined = Arc<dyn Fn(&str, UnboundedSender<String>) + Send + Sync>;

/// What the socket asks the front end, from its own thread.
#[derive(Clone)]
pub struct Front {
    /// What a connection may do now: asked on every message it sends.
    pub standing: Arc<dyn Fn(&Peer) -> Standing + Send + Sync>,
    /// A session's connection said whose it is, by `token`: what it is told
    /// unasked (that its tools changed) goes on the sender.
    pub joined: Joined,
}

/// The socket, served on a runtime of its own for the life of the process.
pub struct Socket {
    _rt: tokio::runtime::Runtime,
}

impl Socket {
    /// Serve `listener`, bound before the front end was up, and what reaches
    /// the front end from it.
    pub fn serve(
        listener: std::os::unix::net::UnixListener,
        front: Front,
    ) -> std::io::Result<(Self, UnboundedReceiver<Incoming>)> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("onehand-socket")
            .build()?;
        listener.set_nonblocking(true)?;
        let listener = {
            let _in_runtime = rt.enter();
            UnixListener::from_std(listener)?
        };
        let (tx, rx) = unbounded();
        rt.spawn(accept(listener, tx, front));
        Ok((Self { _rt: rt }, rx))
    }
}

/// Answer every connection. One whose first line is a second launch's open
/// is answered and closed; any other is an MCP conversation, carried on
/// until the client hangs up. What a line asks for goes to the front end
/// before the answer goes back, so a second launch that hears yes has been
/// heard.
async fn accept(listener: UnixListener, tx: UnboundedSender<Incoming>, front: Front) {
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
        let (tx, front) = (tx.clone(), front.clone());
        tokio::spawn(async move {
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let first = tokio::time::timeout(LINE_WAIT, next_line(&mut reader)).await;
            let Ok(Some(first)) = first else {
                return;
            };
            let Some((reply, forwarded)) = instance::open(&first) else {
                return converse(first, reader, write, tx, front).await;
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
/// than being held in memory. Bytes that are not UTF-8 are kept as a line
/// all the same, so it is answered as malformed and the connection goes on.
async fn next_line(reader: &mut BufReader<OwnedReadHalf>) -> Option<String> {
    let mut line = Vec::new();
    match (&mut *reader)
        .take(LINE_CAP)
        .read_until(b'\n', &mut line)
        .await
    {
        Ok(0) | Err(_) => None,
        Ok(_) if line.last() != Some(&b'\n') && line.len() as u64 >= LINE_CAP => None,
        Ok(_) => Some(String::from_utf8_lossy(&line).into_owned()),
    }
}

/// Carry one MCP conversation, `first` its first line, until the client hangs
/// up: each line answered by core, each call carried out by the front end and
/// its answer written back before the next line is read.
async fn converse(
    first: String,
    mut reader: BufReader<OwnedReadHalf>,
    mut writer: OwnedWriteHalf,
    tx: UnboundedSender<Incoming>,
    front: Front,
) {
    // One writer for the replies and for what the session is told unasked,
    // so the two never interleave inside a line.
    let (out, mut lines) = unbounded::<String>();
    tokio::spawn(async move {
        use futures::StreamExt as _;
        while let Some(line) = lines.next().await {
            if writer
                .write_all(format!("{line}\n").as_bytes())
                .await
                .is_err()
            {
                return;
            }
        }
    });
    let mut conn = Conn::default();
    let mut told = false;
    let mut line = Some(first);
    while let Some(text) = line.take() {
        let reply = match control::handle(&mut conn, &text, &*front.standing) {
            Handled::Reply(reply) => Some(reply),
            Handled::Quiet => None,
            Handled::Perform { id, call } => {
                let peer = conn.peer().cloned().unwrap_or(Peer::Outside);
                let (answer, answered) = oneshot::channel();
                let outcome = match tx.unbounded_send(Incoming::Call(peer, *call, answer)) {
                    Ok(()) => answered.await.unwrap_or_else(|_| Err(CLOSING.to_string())),
                    Err(_) => Err(CLOSING.to_string()),
                };
                Some(control::answered(&id, outcome))
            }
        };
        // A session's connection, once it has said whose it is, is told when
        // that session's switch changes.
        if let (false, Some(Peer::Session(token))) = (told, conn.peer()) {
            told = true;
            (front.joined)(token, out.clone());
        }
        if let Some(reply) = reply
            && out.unbounded_send(reply).is_err()
        {
            break;
        }
        line = next_line(&mut reader).await;
    }
    // For every clone, the one kept to tell the session too: the writer then
    // lets the socket go, so the proxy on the other end hears it close.
    out.close_channel();
}
