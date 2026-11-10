//! The runtime a remote channel runs on.
//!
//! A channel (`onehand_core::remote::types::RemoteChannel`) is a serve loop
//! folded into the stream it returns, and a real one is `reqwest` over tokio,
//! which panics on an executor with no tokio reactor. So its stream is polled
//! here, on a tokio runtime of its own, and its events cross to the front end
//! on a `futures` channel that belongs to no executor. The request side needs
//! no bridge: an unbounded tokio send never blocks or touches the reactor.
//!
//! **Owned rather than shared with the agent runtime.** The bridge is one long
//! poll and a handful of small requests; kept apart, a channel that misbehaves
//! cannot take worker threads away from the agents. One worker thread, since
//! the work is a request asleep almost all the time.

use futures::channel::mpsc;
use onehand_core::remote::types::{RemoteChannel, RemoteEvent, RemoteRequest, ReqTx};

/// Events buffered between the channel and the front end before the
/// forwarder waits.
const EVENT_BUFFER: usize = 32;

/// A channel that is running.
pub struct Channel {
    rt: tokio::runtime::Runtime,
    tx: ReqTx,
}

impl Channel {
    /// Start `channel`, and the events it reports. Dropping the receiver tells
    /// the channel nobody is listening.
    pub fn start(
        channel: Box<dyn RemoteChannel>,
    ) -> std::io::Result<(Self, mpsc::Receiver<RemoteEvent>)> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("onehand-remote")
            .build()?;
        let (tx, requests) = tokio::sync::mpsc::unbounded_channel();
        let (mut events, event_rx) = mpsc::channel(EVENT_BUFFER);
        rt.spawn(async move {
            use futures::{SinkExt as _, StreamExt as _};
            let stream = channel.connect(requests);
            futures::pin_mut!(stream);
            while let Some(event) = stream.next().await {
                if events.send(event).await.is_err() {
                    break;
                }
            }
        });
        Ok((Self { rt, tx }, event_rx))
    }

    /// Hand `request` to the channel.
    pub fn send(&self, request: RemoteRequest) {
        let _ = self.tx.send(request);
    }

    /// Stop the channel. **Handed off rather than dropped**: dropping a
    /// runtime blocks until its threads come back, a stall on the caller's
    /// thread for a channel that has already stopped working.
    pub fn shut_down(self) {
        self.rt.shutdown_background();
    }
}
