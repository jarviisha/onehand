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

use futures::channel::mpsc;
use futures::{SinkExt as _, StreamExt as _};
use onehand_core::acp::{self, AcpEvent};
use onehand_core::config::AgentSpec;
use std::cell::RefCell;
use std::path::PathBuf;

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
    command: String,
    args: Vec<String>,
    cwd: PathBuf,
    events: mpsc::Receiver<AcpEvent>,
}

impl Warm {
    /// Whether this parked adapter is the one `spec` in `cwd` would have
    /// spawned. Compared by what was *executed* rather than by the agent's
    /// name: renaming an agent does not change the process, and two agents
    /// pointed at one command are interchangeable to the adapter.
    fn matches(&self, spec: &AgentSpec, cwd: &PathBuf) -> bool {
        self.command == spec.command && self.args == spec.args && &self.cwd == cwd
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
        let events = self.start(spec, cwd.clone(), None);
        *self.warm.borrow_mut() = Some(Warm {
            command: spec.command.clone(),
            args: spec.args.clone(),
            cwd,
            events,
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
    pub fn connect(
        &self,
        spec: &AgentSpec,
        cwd: PathBuf,
        resume: Option<String>,
    ) -> mpsc::Receiver<AcpEvent> {
        // A resume names a conversation, and the parked adapter has already
        // been through `session/new` on a fresh one -- it is the wrong process
        // to hand this caller, so it stays parked for whoever wants a new
        // conversation instead.
        if resume.is_none()
            && let Some(warm) = self
                .warm
                .borrow_mut()
                .take_if(|warm| warm.matches(spec, &cwd))
        {
            return warm.events;
        }
        self.start(spec, cwd, resume)
    }

    /// Spawn an adapter and forward its events, with no reference to the parked
    /// one either way.
    fn start(
        &self,
        spec: &AgentSpec,
        cwd: PathBuf,
        resume: Option<String>,
    ) -> mpsc::Receiver<AcpEvent> {
        let (mut tx, rx) = mpsc::channel(EVENT_BUFFER);
        let (command, args) = (spec.command.clone(), spec.args.clone());

        self.rt.spawn(async move {
            let stream = acp::connect(command, args, cwd, resume);
            futures::pin_mut!(stream);
            while let Some(event) = stream.next().await {
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        rx
    }
}
