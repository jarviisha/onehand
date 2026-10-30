//! A worker's open, waited for: from its session starting to its first
//! prompt accepted, within one deadline.
//!
//! **The lead is asked about at every step.** The open runs for seconds,
//! and a lead closed or switched off meanwhile has nobody to answer to: the
//! open is abandoned, its session closed, its first prompt never sent and no
//! relation written. The relation is written last, once the prompt went, so
//! a lead forgotten in between can never have one made again after it.
//!
//! **A failure leaves nothing behind and touches nothing else.** Only this
//! open's session is closed; its project goes only when this open added it
//! and nothing else stands on it. Closing the session frees its folder and
//! its place under the cap with it.

use super::requests::Relation;
use crate::chat::session::ChatSession;
use futures::StreamExt as _;
use futures::future::select;
use gpui::{App, AsyncApp, WeakEntity};
use onehand_core::control::open::{Step, step};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long a worker has to come up, from the call to its first prompt
/// accepted: the agent's start, its handshake, and every pick answered.
pub(super) const OPEN_DEADLINE: Duration = Duration::from_secs(120);

/// One open, its session started.
pub(super) struct Opening {
    pub(super) lead: u64,
    pub(super) request: String,
    pub(super) uid: u64,
    /// Held weakly: a person closing it ends it, whatever this open waits on.
    pub(super) session: WeakEntity<ChatSession>,
    /// The window it opens in, to put the person back on something to see
    /// when the session they were watching fails.
    pub(super) window: gpui::AnyWindowHandle,
    /// The project the session was started on, and whether the open added it.
    pub(super) root: PathBuf,
    pub(super) added: bool,
    pub(super) mode: Option<String>,
    pub(super) model: Option<String>,
    pub(super) prompt: Option<String>,
    pub(super) turn: Option<u64>,
    pub(super) dirs: Vec<PathBuf>,
    pub(super) deadline: Instant,
}

/// What a wake decided.
enum Flow {
    Wait,
    Again,
    Ready,
    Fail(String),
}

/// Drive `open` until its worker is up, or it fails; answer either way.
pub(super) async fn wait_open(open: Opening, cx: &mut AsyncApp) -> Result<String, String> {
    let (tx, mut changed) = futures::channel::mpsc::unbounded::<()>();
    // The lead closing or switched off wakes the open too: nothing in the
    // worker changes when it goes. A channel of its own, so the worker's
    // ending when its session is dropped is still heard.
    let (poke, mut poked) = futures::channel::mpsc::unbounded::<()>();
    crate::control::poke_on(open.lead, poke);
    let watch = cx.update(|cx| {
        let session = open.session.upgrade()?;
        Some(cx.observe(&session, move |_, _| {
            let _ = tx.unbounded_send(());
        }))
    });
    let session = open.session.clone();
    let mut picked: Vec<String> = Vec::new();
    let outcome = loop {
        let flow = cx.update(|cx| wake(&open, &session, &mut picked, cx));
        match flow {
            Flow::Again => continue,
            Flow::Ready => break cx.update(|cx| ready(&open, &session, cx)),
            Flow::Fail(why) => break Err(why),
            Flow::Wait => {
                let left = open.deadline.saturating_duration_since(Instant::now());
                let timer = cx.background_executor().timer(left);
                futures::pin_mut!(timer);
                // Whatever woke it, the next wake reads the session, the lead
                // and the deadline afresh.
                let _ = select(select(changed.next(), poked.next()), timer).await;
            }
        }
    };
    drop(watch);
    if outcome.is_err() {
        cx.update(|cx| {
            crate::state::with_shell(open.window, cx, |shell, window, cx| {
                shell.drop_opened(open.uid, &open.root, open.added, window, cx)
            });
            super::sessions().requests.failed(open.lead, &open.request);
        });
    }
    outcome
}

/// One wake: the lead, the deadline, then the open's next step.
fn wake(
    open: &Opening,
    session: &WeakEntity<ChatSession>,
    picked: &mut Vec<String>,
    cx: &mut App,
) -> Flow {
    if !lead_here(open.lead, cx) {
        return Flow::Fail("the lead went before its worker was ready".to_string());
    }
    if Instant::now() >= open.deadline {
        return Flow::Fail(format!(
            "the agent was not ready within {} seconds",
            OPEN_DEADLINE.as_secs()
        ));
    }
    let Some(session) = session.upgrade() else {
        return Flow::Fail("the session was closed before it was ready".to_string());
    };
    let next = step(
        &session.read(cx).chat,
        open.mode.as_deref(),
        open.model.as_deref(),
        picked.as_slice(),
    );
    match next {
        Step::Wait => Flow::Wait,
        Step::Refused(why) => Flow::Fail(why),
        Step::Ready => Flow::Ready,
        Step::Pick(pick) => {
            picked.push(pick.group().to_string());
            session.update(cx, |session, cx| {
                session.chat.pick(&pick);
                cx.notify();
            });
            Flow::Again
        }
    }
}

/// Whether `lead` is still a session of some window, allowed to open one.
pub(super) fn lead_here(lead: u64, cx: &App) -> bool {
    let allowed = {
        let sessions = super::sessions();
        sessions.on.contains(&lead)
            || sessions
                .only
                .get(&lead)
                .is_some_and(|tools| tools.iter().any(|tool| tool == "open_session"))
    };
    allowed
        && crate::state::Shared::global(cx)
            .windows
            .iter()
            .filter_map(|window| window.shell.upgrade())
            .any(|shell| shell.read(cx).session_project(lead).is_some())
}

/// The worker is up: send its first prompt, write the relation, answer.
fn ready(
    open: &Opening,
    session: &WeakEntity<ChatSession>,
    cx: &mut App,
) -> Result<String, String> {
    let session = session
        .upgrade()
        .ok_or("the session was closed before it was ready")?;
    if let Some(prompt) = &open.prompt {
        session.update(cx, |session, cx| {
            // Read before sending: a send that went clears what refused it.
            let blocker = session.chat.submit_blocker(prompt, &[]);
            match session.submit(prompt, &[], cx) {
                true => Ok(()),
                false => Err(blocker
                    .map(|why| why.hint())
                    .unwrap_or_else(|| "the prompt was not accepted".to_string())),
            }
        })?;
    }
    let chat = &session.read(cx).chat;
    let relation = Relation {
        lead: open.lead,
        turn: open.turn,
        text: open.prompt.clone(),
        sent: chat.prompts_sent,
        dirs: open.dirs.clone(),
    };
    let mode = onehand_core::control::state::mode_said(chat);
    let model = onehand_core::control::state::model_said(chat);
    let name = crate::state::with_shell(open.window, cx, |shell, _, cx| {
        shell.session_label(open.uid, cx)
    })
    .unwrap_or_default();
    let dirs = onehand_core::control::state::dirs_said(&open.dirs);
    let mut said = vec![
        format!(
            "session: {}",
            onehand_core::control::open::handle(&super::super::RUN, open.uid)
        ),
        format!("name: {name}"),
        format!("project: {}", open.root.display()),
        format!("mode: {mode}"),
        format!("model: {model}"),
        format!("dirs: {dirs}"),
    ];
    if let Some(turn) = open.turn {
        said.push(format!("turn: {turn} accepted"));
    }
    let answer = said.join("\n");
    let mut sessions = super::sessions();
    sessions.workers.insert(open.uid, relation);
    sessions
        .requests
        .answered(open.lead, &open.request, answer.clone());
    Ok(answer)
}
