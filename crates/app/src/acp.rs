//! The bridge between the ACP client's tokio world and GPUI's smol world.
//!
//! `onehand_core::acp::connect` folds its serve loop *into* the stream it
//! returns — the adapter only advances while something polls it, and dropping
//! the stream drops the child process. That is exactly the shape a front end
//! wants, with one catch: the loop underneath is `tokio::process` +
//! `tokio::io`, so polling it requires a tokio reactor. GPUI's executor is
//! smol, and awaiting a tokio I/O future there panics looking for a reactor
//! that was never started.
//!
//! So the stream is driven on a tokio runtime of our own and its events are
//! handed across on a plain `futures` channel, which belongs to neither
//! executor. That is the shape every external integration here takes: own your
//! runtime, emit events out, accept requests over a channel.
//!
//! The **request** side needs no bridge: `ReqTx` is an unbounded tokio channel,
//! and unbounded sends never block or touch the reactor, so the UI thread can
//! send a prompt straight into the running adapter.

use futures::channel::{mpsc, oneshot};
use futures::{SinkExt as _, Stream, StreamExt as _};
use onehand_core::acp::{self, AcpEvent};
use onehand_core::config::AgentSpec;
use std::cell::RefCell;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

/// Events buffered between the adapter and the UI before the forwarder waits.
/// Matches the buffer core's own stream uses; a turn that out-runs the UI slows
/// the adapter down rather than growing without bound.
const EVENT_BUFFER: usize = 64;

/// The process-wide tokio runtime every ACP adapter runs on.
///
/// One runtime for the whole process rather than one per session: adapters are
/// I/O-bound and idle most of the time, so per-session runtimes would buy
/// nothing but thread-pool duplication. Lives in [`crate::state::Shared`].
pub struct AcpRuntime {
    rt: tokio::runtime::Runtime,
    /// At most one adapter started ahead of the session that will use it. See
    /// [`AcpRuntime::warm`].
    ///
    /// `RefCell` because this is reached through a shared global that every
    /// caller reads immutably; the alternative is threading mutable access to
    /// that global through `ChatSession::spawn`, which has no other reason to
    /// want it. Borrows are taken and dropped inside single methods here, so
    /// there is no path that holds one across a call.
    warm: RefCell<Option<Warm>>,
}

/// An adapter that is already booting for a session nobody has minted yet.
struct Warm {
    /// The agent it runs, nameless: see [`Warm::matches`].
    spec: AgentSpec,
    cwd: PathBuf,
    events: Events,
    /// What its agent's proxy to onehand's tools speaks for, handed to the
    /// session that claims it.
    token: crate::control::Token,
    /// When its process was started, which is when the agent read what it
    /// loads at start — not when a session came to adopt it.
    started: Instant,
}

impl Warm {
    /// Whether this parked adapter is the one `spec` in `cwd` would have
    /// spawned. Compared by what was *executed* rather than by the agent's
    /// name: renaming an agent does not change the process, and two agents
    /// pointed at one command are interchangeable to the adapter.
    fn matches(&self, spec: &AgentSpec, cwd: &PathBuf) -> bool {
        self.spec == nameless(spec) && &self.cwd == cwd
    }
}

/// `spec` without its name, which is everything that decides the process.
fn nameless(spec: &AgentSpec) -> AgentSpec {
    AgentSpec {
        name: String::new(),
        ..spec.clone()
    }
}

impl AcpRuntime {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            rt: tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("onehand-acp")
                .build()?,
            warm: RefCell::new(None),
        })
    }

    /// Start `spec` against `cwd` now, and park it for the session that has not
    /// been asked for yet.
    ///
    /// Bringing an agent up is expensive in a way none of it is our work: the
    /// package manager resolves the adapter, node boots, and the agent's own
    /// SDK reads its settings and starts whatever tool servers it was
    /// configured with. Measured together that is seconds, and every one of
    /// them is spent *after* the user asks for a session — so the wait lands on
    /// the one action that has to feel immediate.
    ///
    /// Doing it early works because the handshake needs nothing from the user:
    /// a project root is all `session/new` takes, and the root is chosen long
    /// before the session is. The events it produces meanwhile queue in the
    /// channel and are drained in order by whichever session claims it, so a
    /// claimed adapter is indistinguishable from one spawned on the spot.
    ///
    /// Idempotent: asking again for an adapter already parked keeps the one
    /// that has had time to boot. Asking for a *different* one drops the old,
    /// which kills its process — the point is to hold one spare, not a pool.
    pub fn warm(&self, spec: &AgentSpec, cwd: PathBuf) {
        if self
            .warm
            .borrow()
            .as_ref()
            .is_some_and(|w| w.matches(spec, &cwd))
        {
            return;
        }
        let token = crate::control::Token::mint();
        let events = self.start(spec, cwd.clone(), acp::Opening::Fresh, token.servers());
        *self.warm.borrow_mut() = Some(Warm {
            spec: nameless(spec),
            cwd,
            events,
            token,
            started: Instant::now(),
        });
    }

    /// Drop the parked adapter, killing its process.
    ///
    /// Called when the next session is no longer the likely next action, so an
    /// agent nobody is going to talk to does not sit there holding a node
    /// process and whatever tool servers it started.
    pub fn drop_warm(&self) {
        self.warm.borrow_mut().take();
    }

    /// Spawn an adapter for `spec` and return its event stream.
    ///
    /// Dropping the returned receiver tears the whole thing down: the forwarder
    /// sees a closed channel, stops polling, drops the stream, and core's
    /// `connect` kills the child. So a caller holds the receiver for exactly as
    /// long as it wants the agent alive — there is no separate shutdown to
    /// remember.
    ///
    /// Also handed back: when the adapter was started. For one spawned here
    /// that is now; for a parked one it is when it was parked, since an agent
    /// reads what it loads at start and a change made while it sat parked is
    /// one it has not seen.
    pub fn connect(
        &self,
        spec: &AgentSpec,
        cwd: PathBuf,
        opening: acp::Opening,
    ) -> (Events, Instant, crate::control::Token) {
        // A resume or a fork names a conversation, and the parked adapter has
        // already been through `session/new` on a fresh one -- it is the wrong
        // process to hand this caller, so it stays parked for whoever wants a new
        // conversation instead.
        if opening == acp::Opening::Fresh
            && let Some(warm) = self
                .warm
                .borrow_mut()
                .take_if(|warm| warm.matches(spec, &cwd))
        {
            return (warm.events, warm.started, warm.token);
        }
        let token = crate::control::Token::mint();
        let events = self.start(spec, cwd, opening, token.servers());
        (events, Instant::now(), token)
    }

    /// Spawn an adapter for a look at what it offers, never the parked one:
    /// it is brought up as far as its session and dropped, so it must not
    /// take the spare a session is about to claim.
    pub fn probe(&self, spec: &AgentSpec, cwd: PathBuf) -> Events {
        // No session will hold it, so its agent is given no way to onehand.
        self.start(spec, cwd, acp::Opening::Fresh, Vec::new())
    }

    /// Spawn an adapter and forward its events, with no reference to the parked
    /// one either way.
    fn start(
        &self,
        spec: &AgentSpec,
        cwd: PathBuf,
        opening: acp::Opening,
        mcp_servers: Vec<serde_json::Value>,
    ) -> Events {
        let (rx, mut tx, alive) = events();
        // A built-in agent whose binary is gone starts nothing and says so on
        // the stream its session already reads, as any other failed start does.
        let launch = spec.launch(std::env::var_os("PATH").as_deref());

        self.rt.spawn(async move {
            let launch = match launch {
                Ok(launch) => launch,
                Err(why) => {
                    let _ = tx.send(AcpEvent::Disconnected(why)).await;
                    return;
                }
            };
            let opts = acp::SessionOpts {
                cwd,
                opening,
                mcp_servers,
            };
            forward(acp::connect(launch, opts), tx, alive).await;
        });

        rx
    }
}

/// An adapter's events, as the UI reads them. Dropping them is what ends the
/// adapter: the forwarder notices at once and drops core's stream.
pub struct Events {
    rx: mpsc::Receiver<AcpEvent>,
    /// Held only to be dropped with the events, which the forwarder hears.
    _held: oneshot::Receiver<()>,
}

impl Stream for Events {
    type Item = AcpEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<AcpEvent>> {
        self.rx.poll_next_unpin(cx)
    }
}

/// A new pair: the events the UI holds, and the forwarder's two ends.
fn events() -> (Events, mpsc::Sender<AcpEvent>, oneshot::Sender<()>) {
    let (tx, rx) = mpsc::channel(EVENT_BUFFER);
    let (alive, held) = oneshot::channel();
    (Events { rx, _held: held }, tx, alive)
}

/// Hand `stream`'s events to `tx` until either side ends.
///
/// **The UI letting go ends it at once**, not at the next event: a closed
/// session's agent is usually idle, and an idle agent says nothing, so a
/// forwarder that only noticed a failed send would keep the stream, and with
/// it the adapter's whole process group, alive for as long as onehand runs.
/// `alive` hears the events dropped.
async fn forward(
    stream: impl Stream<Item = AcpEvent>,
    mut tx: mpsc::Sender<AcpEvent>,
    mut alive: oneshot::Sender<()>,
) {
    use futures::future::{Either, select};
    futures::pin_mut!(stream);
    loop {
        let event = match select(alive.cancellation(), stream.next()).await {
            Either::Left(_) | Either::Right((None, _)) => return,
            Either::Right((Some(event), _)) => event,
        };
        match select(alive.cancellation(), tx.send(event)).await {
            Either::Left(_) | Either::Right((Err(_), _)) => return,
            Either::Right((Ok(()), _)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{events, forward};
    use futures::StreamExt as _;
    use onehand_core::acp::AcpEvent;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Set once the stream it is moved into is dropped, as core's stream
    /// ends its adapter when dropped.
    struct Dropped(Arc<AtomicBool>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// **An adapter with nothing to say is still let go.** A session closed
    /// while its agent is idle drops the events; the forwarder must drop the
    /// stream then, not at an event that never comes.
    #[test]
    fn the_forwarder_lets_go_once_nobody_holds_the_events() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let gone = Arc::new(AtomicBool::new(false));
        let guard = Dropped(gone.clone());
        let silent = futures::stream::pending::<AcpEvent>().map(move |event| {
            let _ = &guard;
            event
        });
        let (held, tx, alive) = events();
        rt.block_on(async move {
            let task = tokio::spawn(forward(silent, tx, alive));
            tokio::time::sleep(Duration::from_millis(20)).await;
            drop(held);
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .expect("the forwarder kept an idle adapter after its events were dropped")
                .unwrap();
        });
        assert!(gone.load(Ordering::SeqCst));
    }
}
