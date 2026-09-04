//! The bridge between a remote channel's tokio world and GPUI's smol world.
//!
//! The same shape, and for the same reason, as the one in front of the ACP
//! adapter. `onehand_core::remote` hands back a channel folded into the stream
//! it returns — it advances only while something polls it, and dropping the
//! stream ends the poll — but that loop is `reqwest` over `tokio`, and GPUI's
//! executor is smol, where awaiting a tokio I/O future panics looking for a
//! reactor nobody started.
//!
//! So the stream is driven on a tokio runtime this module owns and its events
//! are handed across on a plain `futures` channel belonging to neither executor.
//! The request side needs no bridge: the channel out is an unbounded tokio
//! sender, and an unbounded send never blocks or touches the reactor, so a
//! notification can go straight out from the middle of a render's event
//! handler.
//!
//! **One process, one bot.** This lives on the process-global state rather than
//! on a window, because a token identifies one bot and a second poll against the
//! same token is two clients fighting over the same updates. What follows from
//! that is the whole routing problem: a message arriving from outside belongs to
//! no window in particular, so it has to find one.

use crate::state::Shared;
use gpui::{App, BorrowAppContext as _};
use onehand_core::chat::Away;
use onehand_core::config::RemoteConfig;
use onehand_core::remote::types::{Button, Outbound, RemoteEvent, ReqTx};
use onehand_core::remote::{Aim, Press, RemoteCommand, is_silently_ignored};
use onehand_remote_telegram::secret;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// Events buffered between the channel and the app before the forwarder waits.
/// The same buffer core's own stream uses.
const EVENT_BUFFER: usize = 32;

/// The process-wide bridge to whatever channel is configured.
///
/// Holds no channel at all when none is configured, and that is the ordinary
/// state: the bridge is off unless asked for, so the cost of the feature on a
/// machine that does not use it is one `None`.
pub struct RemoteBridge {
    live: Option<Live>,
}

/// A channel that is actually running.
struct Live {
    /// Owned rather than shared with the ACP runtime. Those two have nothing to
    /// say to each other, the bridge is one long poll and a handful of small
    /// requests, and keeping them apart means a channel that misbehaves cannot
    /// take worker threads away from the agents.
    ///
    /// One worker thread, because that is what the work is: a request that is
    /// asleep almost all the time.
    rt: tokio::runtime::Runtime,
    tx: ReqTx,
    /// The chats allowed to reach the app.
    ///
    /// **Permission, not audience, and the two are different lists.** Being here
    /// is what makes a chat able to say anything and able to be told anything —
    /// everything a message says is something the reader is already trusted
    /// with, because that is what being on this list means. What a chat actually
    /// hears about a *session* is `followed`, which is narrower and is the
    /// reader's own choice.
    ///
    /// This list is still the whole audience for what is about the bridge rather
    /// than about a session — the away switch thrown at the keyboard, today the
    /// only one. That has no session to be subscribed to, and a reader who has
    /// asked for nothing yet is exactly who needs telling that the machine has
    /// been left.
    allowed: Vec<String>,
    /// Which session each chat has pointed itself at.
    ///
    /// **A chat is bound by being told to, and never by being guessed at.** One
    /// project root runs as many sessions as it is asked to, so "the active
    /// one" is a moving answer that changes every time somebody at the keyboard
    /// clicks a rail row — a message sent from a train would land wherever the
    /// window happened to be pointing, which is the one failure a prompt cannot
    /// recover from. `/sessions` numbers them and `/use` picks one, and until
    /// that has happened a prompt is answered with how to point it rather than
    /// with a guess.
    ///
    /// In memory only, so a restart forgets it. That is the honest lifetime:
    /// sessions are not persisted either, so a binding that outlived the process
    /// would name a session that no longer exists.
    ///
    /// `RefCell` because the bridge is reached through a global that most
    /// callers read immutably; the alternative is threading mutable access to
    /// that global through everything that merely wants to send a message.
    /// Borrows are taken and released inside single functions here, so none is
    /// ever held across a call.
    bindings: RefCell<HashMap<String, u64>>,
    /// The archive listing each chat was last sent.
    ///
    /// **Kept exactly as it went out, and never read off disk again.** A saved
    /// conversation has no small stable name to put in a message — on disk it is
    /// an agent-chosen session id, too long to type and too long for a button to
    /// carry — so a chat holds a place in a list, and a place is only safe while
    /// the list it counts into cannot move underneath it. Re-scanning on `/open`
    /// would reintroduce exactly the drift that made session numbers uids.
    ///
    /// Replaced whole by the next `/archive`, so it is one listing per chat and
    /// not a history of them.
    archives: RefCell<HashMap<String, Vec<ArchivePick>>>,
    /// Which sessions each chat has asked to hear about.
    ///
    /// **This is the whole audience rule for a session's own news.** `allowed` is
    /// who may reach the app and who may be told anything at all; this is who
    /// asked about what, and a session absent from a chat's set is a session that
    /// chat hears nothing about — not a finished turn, not a parked question, not
    /// an adapter that died.
    ///
    /// **Silence is the default, and that is the change this encodes.** The
    /// arrangement it replaced spoke about everything and was quietened one
    /// session at a time, which makes a chat's contents a consequence of whatever
    /// happens to be open at the far end — something its reader neither chose nor
    /// can see. A machine running eight agents had to be told about seven of them
    /// before it was bearable, and every session opened afterwards reopened the
    /// argument. Asking first inverts that: what arrives is exactly what was
    /// asked for, and the cost is that a chat which asked for nothing gets
    /// nothing, which `/status` exists to say out loud.
    ///
    /// **Per chat, like `bindings` and `archives` and unlike anything global.**
    /// A subscription is by construction a fact about a reader rather than about
    /// a conversation: two people can want different things from one machine, and
    /// `/use` — which subscribes — is already per chat, so a shared set would
    /// have one phone's pointing decide another phone's notifications.
    ///
    /// **The channel only.** Nothing here reaches the desktop notification or the
    /// rail's badge: those obey what is on screen, which the app can see for
    /// itself, and somebody choosing what to hear about on a phone has said
    /// nothing about the machine they are not at.
    ///
    /// A uid is never reused — the salt only counts up — so an entry left behind
    /// by a closed session cannot start announcing a different conversation
    /// later. It is dropped anyway wherever one is found to be gone, so that the
    /// set stays something `/status` can print in full.
    followed: RefCell<HashMap<String, HashSet<u64>>>,
}

/// One row of an archive listing, and everything reopening it needs.
///
/// Carries the project as well as the conversation, because minting a session
/// goes to whichever project was last clicked — a conversation reopened from a
/// train would otherwise land on an unrelated checkout.
#[derive(Clone)]
pub struct ArchivePick {
    /// The project root this conversation was had in.
    root: std::path::PathBuf,
    project: String,
    /// The conversation's directory, which is what a resume is given.
    dir: std::path::PathBuf,
    agent: String,
    title: String,
    /// When it was last written, for the listing's own "when".
    updated: u64,
}

impl RemoteBridge {
    /// A bridge with nothing behind it.
    pub fn off() -> Self {
        Self { live: None }
    }

    /// Whether a channel is running.
    ///
    /// Read by the status bar, which offers the away switch only where there is
    /// somewhere for the announcements to go — a control whose whole effect is
    /// on a channel that does not exist is a control that does nothing.
    pub fn is_live(&self) -> bool {
        self.live.is_some()
    }

    /// Stop using the channel.
    ///
    /// Called when the channel reports that it is not coming back. Ending the
    /// runtime is what ends the poll; what this is really for is that
    /// [`announce`] stops handing messages to a sender nobody will read.
    ///
    /// **Handed off rather than dropped**, because this runs on the UI thread:
    /// dropping a runtime blocks until its threads have come back, and however
    /// short that is, it is a stall in the middle of a frame for the sake of a
    /// bridge that has already stopped working.
    fn shut_down(&mut self) {
        if let Some(live) = self.live.take() {
            live.rt.shutdown_background();
        }
    }

    /// Say `out` on the channel, if there is one.
    fn send(&self, out: Outbound) {
        if let Some(live) = &self.live {
            let _ = live.tx.send(onehand_core::remote::RemoteRequest::Send(out));
        }
    }

    /// Tell the channel a press has been dealt with.
    fn ack(&self, press_id: String, text: String) {
        if let Some(live) = &self.live {
            let _ = live.tx.send(onehand_core::remote::RemoteRequest::Ack {
                press_id,
                text: Some(text),
            });
        }
    }
}

/// One session, as somebody reading about it from a phone would need it.
///
/// `uid` is both the session's identity and the number it is listed under. A
/// position in a list would be the wrong thing to print: the list is read, a
/// message is typed, and a session closed in between would slide every number
/// below it onto a different conversation.
pub struct RemoteSession {
    pub uid: u64,
    pub project: String,
    /// `None` until the conversation has earned a name, which is its first
    /// prompt — the same rule the rail falls back from.
    pub conversation: Option<String>,
    pub agent: String,
    /// What the session is doing, in the rail's own words for it. `None` is the
    /// case the rail draws nothing for: connected, idle and already read.
    pub state: Option<&'static str>,
}

impl RemoteSession {
    /// The two lines this session takes in a listing.
    ///
    /// Says nothing about whether the chat reading it follows this session:
    /// that is a fact about the reader rather than about the session, so it is
    /// carried by the mark in the margin that `listing` puts there and not by
    /// this, which is printed in places where there is no margin.
    fn line(&self) -> String {
        let name = self.conversation.as_deref().unwrap_or("no name yet");
        let state = self.state.unwrap_or("Idle");
        format!(
            "{} · {} — {}\n    {} · {}",
            self.uid, self.project, name, self.agent, state
        )
    }
}

/// Every session this process is running, across every window, in one order.
///
/// Windows are walked in the order they were opened and each window's sessions
/// come back sorted by uid, so the whole list is stable between one message and
/// the next — which is the property the numbers in it are only useful because
/// of.
fn sessions(cx: &App) -> Vec<RemoteSession> {
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    shells
        .iter()
        // A window closed between the registry's prune and this walk is not an
        // error, it is one fewer window.
        .filter_map(|shell| shell.upgrade())
        .flat_map(|shell| shell.read(cx).remote_sessions(cx))
        .collect()
}

/// Start whatever channel `cfg` asks for and pump its events into the app.
///
/// Everything here fails by not starting, loudly on stderr and never fatally: a
/// missing token or a channel switched off is the ordinary case, and an app that
/// refused to open a window over a chat bridge would be trading the whole
/// product for one of its conveniences.
pub fn boot(cfg: &RemoteConfig, cx: &mut App) {
    let tg = &cfg.telegram;
    if !tg.enabled {
        return;
    }
    // Said out loud, because both of these look identical from the outside --
    // the bridge is on in the config and nothing ever arrives.
    if tg.allowed_chats.is_empty() {
        eprintln!(
            "onehand: the Telegram bridge is enabled but no chat is allowed, \
             so it would answer nobody. Set remote.telegram.allowed_chats."
        );
        return;
    }
    let Some(token) = secret::telegram_token(tg) else {
        eprintln!(
            "onehand: the Telegram bridge is enabled but no bot token was found. \
             Set ${} or put the token in {}.",
            tg.token_env.as_deref().unwrap_or(secret::DEFAULT_TOKEN_ENV),
            secret::token_file().display()
        );
        return;
    };
    // Said and returned rather than asserted. Every other way this function
    // gives up prints a line and leaves the app running, and a bridge that
    // cannot start is not worth taking the window down for -- the registry is
    // sealed before `Shared` exists, so a contribution missing here means the
    // composition root changed and the app is still perfectly usable without a
    // phone attached to it.
    let Some(factory) = Shared::global(cx)
        .plugins
        .remote_channels()
        .iter()
        .find(|item| item.id == onehand_remote_telegram::CHANNEL_ID)
        .and_then(|item| item.factory)
    else {
        eprintln!(
            "onehand: the Telegram bridge is enabled but no channel is registered under `{}`.",
            onehand_remote_telegram::CHANNEL_ID
        );
        return;
    };

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .thread_name("onehand-remote")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("onehand: could not start the remote bridge: {e}");
            return;
        }
    };

    let (requests, request_rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut events, event_rx) = futures::channel::mpsc::channel(EVENT_BUFFER);
    rt.spawn(async move {
        use futures::{SinkExt as _, StreamExt as _};
        let stream = factory(token).connect(request_rx);
        futures::pin_mut!(stream);
        while let Some(event) = stream.next().await {
            if events.send(event).await.is_err() {
                break;
            }
        }
    });

    cx.update_global::<Shared, _>(|shared, _| {
        shared.remote.live = Some(Live {
            rt,
            tx: requests,
            allowed: tg.allowed_chats.clone(),
            bindings: RefCell::new(HashMap::new()),
            archives: RefCell::new(HashMap::new()),
            followed: RefCell::new(HashMap::new()),
        });
    });

    // Held on the global rather than detached: the pump is what keeps the
    // receiver alive, and a dropped receiver is what tells the channel that
    // nobody is listening. Detaching it would work today and would silently stop
    // working the moment anything wanted to take the bridge down.
    let pump = cx.spawn(async move |cx| {
        use futures::StreamExt as _;
        let mut event_rx = event_rx;
        while let Some(event) = event_rx.next().await {
            cx.update(|cx| receive(event, cx));
        }
    });
    cx.update_global::<Shared, _>(|shared, _| shared._remote_pump = Some(pump));
}

/// One event from the channel, on the UI thread.
fn receive(event: RemoteEvent, cx: &mut App) {
    match event {
        RemoteEvent::Connected { name } => {
            eprintln!("onehand: the remote bridge is up as {name}");
        }
        RemoteEvent::Disconnected(why) => {
            eprintln!("onehand: the remote bridge stopped: {why}");
            cx.update_global::<Shared, _>(|shared, _| {
                shared.remote.shut_down();
                // **Away goes with it, and this is not tidying.** Both ways of
                // turning it off need the channel: the status bar draws its
                // switch only while one is live, and `/here` arrives over the
                // one that just died. Left set, it is a mode with no way out
                // short of restarting the app -- and one that goes on treating
                // every window as unwatched, so a turn ending in the
                // conversation being read still badges it and still interrupts
                // the desktop. Being wrong about the user's presence costs
                // nothing while there is nowhere to announce to.
                shared.away = false;
            });
            cx.refresh_windows();
        }
        RemoteEvent::Message { chat, text } => {
            // The one gate, and it comes before everything: a chat that is not
            // on the list is answered with nothing at all. Not a refusal --
            // a refusal confirms that the bot is real, that it is running right
            // now and that there is a list to get onto, which is three facts
            // more than silence gives away.
            if !allowed(&chat, cx) {
                return;
            }
            if let Some(reply) = answer(&chat, &text, cx) {
                Shared::global(cx).remote.send(Outbound::text(chat, reply));
            }
        }
        RemoteEvent::Unreadable { chat } => {
            // The gate first, as everywhere: a stranger sending a photo learns
            // nothing, including that this went unread.
            if !allowed(&chat, cx) {
                return;
            }
            // Somebody allowed is owed an answer even when the answer is no.
            // They typed something and are watching for a reply, and silence
            // here is indistinguishable from a bridge that has stopped running.
            Shared::global(cx).remote.send(Outbound::text(
                chat,
                "I can only read text — an image or a voice note doesn't travel. \
                 Type what you want done and it will go to the session."
                    .to_string(),
            ));
        }
        RemoteEvent::Pressed {
            chat,
            press_id,
            data,
        } => {
            // The same gate, and it comes first for the same reason. A press
            // from a chat that is not on the list is not even acknowledged: the
            // spinner the client shows is the far side's business, and stopping
            // it would say that something here read the press.
            if !allowed(&chat, cx) {
                return;
            }
            let said = match Press::decode(&data) {
                Some(press) => act_on(press, cx),
                // A button from a build that wrote its data differently, still
                // sitting in somebody's chat history. Saying so beats acting on
                // a guess, which would land on a real option of a real session.
                None => "That button is from an older message.".to_string(),
            };
            let bridge = &Shared::global(cx).remote;
            // Both, and they are not the same thing. The acknowledgement stops
            // the client spinning and shows the answer for a second; the message
            // is what makes the chat a record of what was decided from it, which
            // is exactly what a permission granted from a phone is worth having.
            bridge.ack(press_id, said.clone());
            bridge.send(Outbound::text(chat, said));
        }
    }
}

/// Carry out a press, wherever the session it names lives.
fn act_on(press: Press, cx: &mut App) -> String {
    let uid = press.uid();
    ask_windows(cx, |shell, cx| shell.remote_answer(press.clone(), cx))
        // Not `no_longer_there`: a press carries its own session and never went
        // through a binding, so there is nothing to unpoint and no listing that
        // would help — the message it was pressed on is simply older than the
        // session it was about.
        .unwrap_or_else(|| format!("Session {uid} is gone."))
}

/// Whether this chat may reach the app at all.
fn allowed(chat: &str, cx: &App) -> bool {
    Shared::global(cx)
        .remote
        .live
        .as_ref()
        .is_some_and(|live| !is_silently_ignored(&live.allowed, &chat.to_string()))
}

/// The session `chat` has pointed itself at, if it has.
fn bound(chat: &str, cx: &App) -> Option<u64> {
    let live = Shared::global(cx).remote.live.as_ref()?;
    // Copied out, so the borrow ends with this expression rather than travelling
    // back to a caller that is about to reach for the same map.
    live.bindings.borrow().get(chat).copied()
}

/// Point `chat` at `uid`, replacing whatever it was pointed at.
///
/// **Pointing at a session subscribes to it**, and that is what keeps a channel
/// that says nothing by default from being a channel that appears broken.
/// Pointing a chat somewhere is the gesture that says "this is the one I am
/// attending to" — it is what somebody does before walking away from the
/// machine — so requiring a second command afterwards would mean the ordinary
/// path ends in silence, and silence is the one outcome a reader cannot tell
/// apart from a crash. Unpointing does not unsubscribe: a chat can follow more
/// than one session and only types into one, so dropping the subscription on the
/// way past would take away something that was asked for separately.
///
/// The one exception is a session that has gone, where the caller passes `None`
/// after finding it closed and prunes the subscription itself.
fn bind(chat: &str, uid: Option<u64>, cx: &App) {
    let Some(live) = Shared::global(cx).remote.live.as_ref() else {
        return;
    };
    match uid {
        Some(uid) => {
            live.bindings.borrow_mut().insert(chat.to_string(), uid);
            live.followed
                .borrow_mut()
                .entry(chat.to_string())
                .or_default()
                .insert(uid);
        }
        None => {
            live.bindings.borrow_mut().remove(chat);
        }
    }
}

/// Whether `chat` has asked to hear about `uid`.
fn is_followed(chat: &str, uid: u64, cx: &App) -> bool {
    let Some(live) = Shared::global(cx).remote.live.as_ref() else {
        return false;
    };
    live.followed
        .borrow()
        .get(chat)
        .is_some_and(|set| set.contains(&uid))
}

/// Start or stop `chat` hearing about `uid`.
///
/// Hands back whether this changed anything, so the reply can tell "now
/// following" from "already were" — a command that answers the same way whether
/// or not it did something teaches the user to distrust it.
fn set_followed(chat: &str, uid: u64, on: bool, cx: &App) -> bool {
    let Some(live) = Shared::global(cx).remote.live.as_ref() else {
        return false;
    };
    let mut followed = live.followed.borrow_mut();
    if on {
        followed.entry(chat.to_string()).or_default().insert(uid)
    } else {
        // The chat's own set and not every chat's: one reader losing interest
        // says nothing about what anybody else asked for.
        followed.get_mut(chat).is_some_and(|set| set.remove(&uid))
    }
}

/// Every session `chat` follows that is still open, in the listing's order.
///
/// **Filtered against what is running rather than returned raw**, because a
/// subscription outlives the session it names: uids only count up, so a stale
/// entry can never start announcing something else, but it can be printed, and a
/// list of things you are following that includes conversations that ended is a
/// list nobody can act on.
fn followed_open(chat: &str, sessions: &[RemoteSession], cx: &App) -> Vec<u64> {
    sessions
        .iter()
        .map(|s| s.uid)
        .filter(|uid| is_followed(chat, *uid, cx))
        .collect()
}

/// Drop `uid` from `chat`'s subscription and binding both.
///
/// For the moment a command finds that a session has closed. Left in place the
/// entry is harmless — the uid will not come round again — but it is unprintable
/// and so unremovable, and `/status` promising a full list has to be able to
/// keep the promise.
fn forget(chat: &str, uid: u64, cx: &App) {
    if bound(chat, cx) == Some(uid) {
        bind(chat, None, cx);
    }
    set_followed(chat, uid, false, cx);
}

/// Say whether the user is at the machine, and hand back the sentence that
/// says so.
///
/// **One function for both ways in**, the switch in the status bar and the
/// command from the chat, because a mode with two setters is a mode that ends up
/// meaning two things. What differs is only who is told: the caller from the
/// window broadcasts the sentence, since the phone is where the consequences
/// land; the caller from the chat returns it as its own reply, since saying it
/// twice in the same conversation is once too many.
///
/// Every window is refreshed rather than one: this is global, and the switch
/// draws in each of their status bars.
pub fn set_away(away: bool, cx: &mut App) -> String {
    cx.update_global::<Shared, _>(|shared, _| shared.away = away);
    cx.refresh_windows();
    if !away {
        // **Coming back clears the badge on what is being looked at.** While
        // away every turn is treated as unwatched, including one that ended in
        // the conversation on screen — and the badge is otherwise only cleared
        // when a window *becomes* active, which never happens for one that was
        // focused the whole time somebody was in the next room. The result was a
        // dot on the single conversation they were reading.
        //
        // Only the window that is actually in front: a background window's
        // badges were not read, and clearing them would be this rule lying about
        // a different window than the one it fixed.
        let active: Vec<_> = Shared::global(cx)
            .windows
            .iter()
            .filter(|w| cx.active_window() == Some(w.handle))
            .map(|w| w.shell.clone())
            .collect();
        // **Deferred, and that is load-bearing.** One of the two ways in here is
        // a click on the status bar, and a click handler is already holding the
        // shell it was called on — the same shell this is about to reach into,
        // since the switch that was clicked is drawn in the active window.
        // Updating an entity that is already being updated is a panic, not a
        // borrow error, so it takes the whole app down on the second press of a
        // control whose entire job is to be pressed twice. Deferring runs this
        // once that handler has let go, and costs a frame nobody can see.
        cx.defer(move |cx| {
            for shell in active.into_iter().filter_map(|shell| shell.upgrade()) {
                shell.update(cx, |shell, cx| shell.mark_active_seen(cx));
            }
        });
    }
    if away {
        "Away. Everything gets announced here now, whatever is on screen."
    } else {
        "Back at the machine. Notifications go quiet again while you're looking."
    }
    .to_string()
}

/// Whether the user has said they are away.
pub fn is_away(cx: &App) -> bool {
    Shared::global(cx).away
}

/// Say something to every chat allowed to hear it, with nothing to press.
///
/// For what is about the *bridge* rather than about a session — the away switch
/// being thrown at the keyboard is the only thing so far. A session's own news
/// goes through [`announce`], which names which session it is about.
pub fn broadcast(text: String, cx: &App) {
    let bridge = &Shared::global(cx).remote;
    let Some(live) = &bridge.live else {
        return;
    };
    for chat in &live.allowed {
        bridge.send(Outbound::text(chat.clone(), text.clone()));
    }
}

/// How many saved conversations one `/archive` offers.
///
/// A flat handful across every project rather than a browsable index: the
/// question somebody asks from a phone is "put back the one I was in", and a
/// month of work would otherwise draw a column of hundreds for the sake of the
/// two anybody came for. What the cap cut off is said on screen rather than
/// silently dropped.
const MAX_ARCHIVES: usize = 10;

/// Every project root this process holds, deduplicated.
///
/// Two windows can hold the same root — one workspace per window, and nothing
/// stops a folder being in both — so a conversation in it would otherwise be
/// listed twice under the same number.
fn roots(cx: &App) -> Vec<(std::path::PathBuf, String)> {
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    let mut out: Vec<(std::path::PathBuf, String)> = Vec::new();
    for shell in shells.into_iter().filter_map(|shell| shell.upgrade()) {
        for root in shell.read(cx).remote_roots() {
            if !out.iter().any(|(path, _)| *path == root.0) {
                out.push(root);
            }
        }
    }
    out
}

/// Read what is on disk for `roots`, newest first.
///
/// **Blocking, and called from the background executor only.** It opens the
/// conversation store and reads a small file per conversation in it, which is
/// hundreds of reads on a machine that has been used for a while — on the UI
/// thread that is a visible stall, in the middle of a frame, for a message
/// nobody is watching the screen for.
fn scan_archives(roots: Vec<(std::path::PathBuf, String)>) -> (Vec<ArchivePick>, usize) {
    let store = onehand_core::chat::conversations_dir();
    let mut found: Vec<ArchivePick> = Vec::new();
    for (root, project) in roots {
        // Every agent, not one: the listing is about what was said, and which
        // adapter said it is a detail below that.
        for meta in onehand_core::chat::list_conversations(&store, &root, None) {
            found.push(ArchivePick {
                root: root.clone(),
                project: project.clone(),
                dir: meta.dir,
                agent: meta.agent,
                title: meta.title,
                updated: meta.updated,
            });
        }
    }
    // Newest first, which is what makes a cap of ten worth having.
    found.sort_by_key(|pick| std::cmp::Reverse(pick.updated));
    // The total travels with the page, because a listing that quietly stops at
    // ten reads as a machine that has had ten conversations.
    let total = found.len();
    found.truncate(MAX_ARCHIVES);
    (found, total)
}

/// Answer `/archive`, off the UI thread.
///
/// Nothing is returned: the scan cannot finish inside this call, so the reply is
/// sent when it lands rather than handed back. Detached, because the task's
/// whole life is the one message it ends with — and a chat that loses its answer
/// because the window it was scanning for closed has lost nothing that mattered.
fn list_archive(chat: &str, cx: &mut App) {
    let roots = roots(cx);
    let chat = chat.to_string();
    cx.spawn(async move |cx| {
        let (found, total) = cx
            .background_executor()
            .spawn(async move { scan_archives(roots) })
            .await;
        cx.update(|cx| {
            let text = archive_listing(&found, total);
            if let Some(live) = Shared::global(cx).remote.live.as_ref() {
                live.archives
                    .borrow_mut()
                    .insert(chat.clone(), found.clone());
            }
            Shared::global(cx).remote.send(Outbound::text(chat, text));
        });
    })
    .detach();
}

/// The `/archive` reply.
fn archive_listing(found: &[ArchivePick], total: usize) -> String {
    if found.is_empty() {
        return "Nothing saved yet — a conversation is written at the end of its first turn."
            .to_string();
    }
    let now = onehand_core::chat::now_secs();
    let mut out = String::from("Archive\n\n");
    for (place, pick) in found.iter().enumerate() {
        out.push_str(&format!(
            "{} · {} — {}\n    {} · {}\n",
            place + 1,
            pick.project,
            pick.title,
            pick.agent,
            crate::chat::pane::rel_time(now, pick.updated),
        ));
    }
    // Said rather than silently dropped: a list that stops at ten with no word
    // about it reads as a machine that has had ten conversations.
    if total > found.len() {
        out.push_str(&format!(
            "\n{} newest of {total}. The rest are in the app.\n",
            found.len()
        ));
    }
    out.push_str("\n/open <number> puts one back.");
    out
}

/// Answer `/open`.
fn open_archive(chat: &str, place: usize, cx: &mut App) -> String {
    let pick = Shared::global(cx).remote.live.as_ref().and_then(|live| {
        live.archives.borrow().get(chat).and_then(|list| {
            // Counted from one, as the listing prints it.
            list.get(place - 1).cloned()
        })
    });
    let Some(pick) = pick else {
        return "Ask for /archive first — the numbers count into that listing.".to_string();
    };

    // Every window is asked and the one holding that project answers, the same
    // handshake the prompt and press paths use. A window is needed here and not
    // there: showing the new session is what spawns its adapter.
    let windows: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| (w.handle, w.shell.clone()))
        .collect();
    let agent = gpui::SharedString::from(pick.agent.clone());
    let opened = windows.into_iter().find_map(|(handle, shell)| {
        let shell = shell.upgrade()?;
        let (root, dir, agent) = (pick.root.clone(), pick.dir.clone(), agent.clone());
        handle
            .update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.remote_open(&root, dir, Some(agent), window, cx)
                })
            })
            .ok()
            .flatten()
    });

    match opened {
        Some(uid) => {
            // Pointed at what was just asked for. Not a guess: naming the
            // conversation to reopen is naming where the next prompt goes, and
            // making them say it twice would be the bridge pretending not to
            // have understood.
            bind(chat, Some(uid), cx);
            format!(
                "Opened {} — {} as session {uid}, and pointed this chat at it.",
                pick.project, pick.title
            )
        }
        // The project was removed from every workspace since the listing was
        // sent, so there is nowhere to put the conversation back.
        None => format!("{} is no longer open as a project.", pick.project),
    }
}

/// Put a question to every window until one answers it.
///
/// **The windows are asked rather than indexed.** A map of session to window
/// kept on the bridge would need correcting on every open, close and restart and
/// would be wrong in between; a window cannot be wrong about which sessions it
/// holds. `None` from all of them means the session is gone, which is a real
/// answer and the one every caller here turns into a sentence.
fn ask_windows<R>(
    cx: &mut App,
    mut act: impl FnMut(&mut crate::shell::Shell, &mut gpui::Context<crate::shell::Shell>) -> Option<R>,
) -> Option<R> {
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    shells
        .into_iter()
        // A window closed between the registry's prune and this walk is not an
        // error, it is one fewer window.
        .filter_map(|shell| shell.upgrade())
        .find_map(|shell| shell.update(cx, |shell, cx| act(shell, cx)))
}

/// What to say when a chat asked for something and is pointed at nothing.
fn not_pointed(what: &str, chat: &str, cx: &App) -> String {
    format!(
        "This chat isn't pointed at a session, so there is nothing to {what}.\n\n{}",
        listing(chat, cx)
    )
}

/// What to say when the session a chat was pointed at has since closed.
///
/// The binding goes with it rather than being left to fail again on the next
/// message, and the list comes back so the next one can land somewhere.
fn no_longer_there(uid: u64, chat: &str, cx: &mut App) -> String {
    bind(chat, None, cx);
    format!("Session {uid} is gone.\n\n{}", listing(chat, cx))
}

/// Answer `/options`: what the bound session's agent lets you change, with a
/// button per value.
fn send_options(chat: &str, cx: &mut App) {
    let out = match bound(chat, cx) {
        None => Outbound::text(chat.to_string(), not_pointed("change options on", chat, cx)),
        Some(uid) => match ask_windows(cx, |shell, cx| shell.remote_options(uid, cx)) {
            Some((text, buttons)) => Outbound {
                chat: chat.to_string(),
                text,
                buttons,
            },
            None => Outbound::text(chat.to_string(), no_longer_there(uid, chat, cx)),
        },
    };
    Shared::global(cx).remote.send(out);
}

/// Answer `/status`: what reaches this chat, and what does not.
///
/// **The command silence makes necessary.** Nothing announces itself unless a
/// chat asked for it, so a bridge working perfectly and a bridge whose process
/// died last night look the same from a phone — as does one whose subscriptions
/// were forgotten in a restart. This is the question that separates them, which
/// is why it names what it follows rather than counting it: the whole point is
/// to be able to check the list against what you believe you asked for.
///
/// **Not a second `/sessions`.** That one answers what onehand is running and
/// marks these rows in passing; this one answers what will reach you, and leads
/// with the two facts no session carries — whether the user has said they are
/// away, and where this chat is pointed.
///
/// The one thing it changes rather than reads: a binding or a subscription onto
/// a session that has since closed is dropped as it is reported. Saying "pointed
/// at 7" about a session that is gone is the exact confusion this command exists
/// to end, and an entry naming a closed session cannot be printed, so it could
/// never afterwards be removed.
fn status(chat: &str, cx: &mut App) -> String {
    let sessions = sessions(cx);
    let mut out = String::from(if is_away(cx) {
        "Away is on — everything you follow gets announced here, whatever is on screen.\n"
    } else {
        "Away is off — while you're at the keyboard, what you could already see is held back.\n"
    });

    match bound(chat, cx) {
        None => out.push_str("This chat isn't pointed at a session — /use <number> picks one.\n"),
        Some(uid) => match sessions.iter().find(|s| s.uid == uid) {
            Some(session) => out.push_str(&format!("Pointed at {}\n", session.line().trim_start())),
            None => {
                forget(chat, uid, cx);
                out.push_str(&format!(
                    "Session {uid} closed, so this chat is pointed at nothing — /use <number> picks another.\n"
                ));
            }
        },
    }

    if sessions.is_empty() {
        out.push_str("\nNo sessions are open.");
        return out;
    }
    let following = followed_open(chat, &sessions, cx);
    if following.is_empty() {
        out.push_str(&format!(
            "\nYou're following nothing, so nothing will reach this chat.\n\
             {} session{} open — /use <number> points at one and follows it, or \
             /follow <number> follows one without pointing at it.",
            sessions.len(),
            if sessions.len() == 1 { " is" } else { "s are" }
        ));
        return out;
    }
    out.push_str(&format!(
        "\nYou'll hear about {} of {} open session{}:\n",
        following.len(),
        sessions.len(),
        if sessions.len() == 1 { "" } else { "s" }
    ));
    for session in sessions.iter().filter(|s| following.contains(&s.uid)) {
        out.push_str("  ");
        out.push_str(&session.line());
        out.push('\n');
    }
    out.push_str("\n/unfollow <number> drops one. /sessions lists them all.");
    out
}

/// Answer `/follow` and `/unfollow`.
///
/// **The session has to still be open.** Subscribing to one that has closed is a
/// message saying it worked about something that does not exist, and the entry
/// would then sit in the set unprintable — `/sessions` and `/status` only show
/// sessions that are running, so a subscription neither can name is one nobody
/// can find to drop.
fn follow(chat: &str, aim: Aim, on: bool, cx: &mut App) -> String {
    let word = if on { "follow" } else { "unfollow" };
    let uid = match aim {
        // Not read as the bound session: the user named something, it just was
        // not a number. On `/unfollow` that would silence a conversation nobody
        // typed, which is the mistake whose symptom is nothing happening.
        Aim::Unreadable => {
            return format!(
                "Which session? /{word} <number>, or /{word} on its own for the one this chat is pointed at.\n\n{}",
                listing(chat, cx)
            );
        }
        Aim::Bound => match bound(chat, cx) {
            Some(uid) => uid,
            None => return not_pointed(word, chat, cx),
        },
        Aim::Session(uid) => uid,
    };
    if !sessions(cx).iter().any(|s| s.uid == uid) {
        forget(chat, uid, cx);
        return format!("There's no session {uid}.\n\n{}", listing(chat, cx));
    }
    let changed = set_followed(chat, uid, on, cx);
    match (on, changed) {
        (true, true) => format!(
            "Following {uid}. You'll hear here when it finishes a turn, when it stops to \
             ask you something, and if the agent dies. /unfollow {uid} to stop."
        ),
        (true, false) => format!("You were already following {uid}."),
        // Named rather than left to be inferred: this is a command whose whole
        // effect is that nothing happens afterwards, so the reply is the only
        // evidence it did anything at all.
        (false, true) => format!(
            "Stopped following {uid}. Nothing about it reaches this chat now — not a \
             finished turn, not a question it stops on, not the agent dying. It keeps \
             running, and /sessions still shows it."
        ),
        (false, false) => format!("You weren't following {uid}, so nothing changed."),
    }
}

/// Answer `/stop`.
fn stop_session(chat: &str, cx: &mut App) -> String {
    let Some(uid) = bound(chat, cx) else {
        return not_pointed("stop", chat, cx);
    };
    match ask_windows(cx, |shell, cx| shell.remote_stop(uid, cx)) {
        Some(said) => said,
        None => no_longer_there(uid, chat, cx),
    }
}

/// What one prompt did, wherever it landed.
pub enum Handled {
    /// It went in and a turn started.
    Sent,
    /// A turn was already running, so it goes the moment that one ends.
    Queued,
    /// The session would not take it, in its own words.
    Refused(String),
}

/// What to say back to a line from an allowed chat, if anything.
fn answer(chat: &str, text: &str, cx: &mut App) -> Option<String> {
    match onehand_core::remote::command::parse(text) {
        RemoteCommand::List => Some(listing(chat, cx)),
        RemoteCommand::Help => Some(HELP.to_string()),
        // Nothing was asked, so nothing is said. A client that sends an empty
        // message is not owed an answer, and answering one is how a bridge
        // starts talking to itself.
        RemoteCommand::Nothing => None,
        RemoteCommand::Unknown(word) => Some(format!("I don't know /{word}.\n\n{HELP}")),
        RemoteCommand::UseWhich => Some(format!(
            "Which one? /sessions lists them, then /use <number>.\n\n{}",
            listing(chat, cx)
        )),
        RemoteCommand::Use(uid) => Some(point_at(chat, uid, cx)),
        // The one command with nothing to say yet: reading the store is a file
        // per conversation, so it goes to the background and sends its own reply
        // when it lands.
        RemoteCommand::Archive => {
            list_archive(chat, cx);
            None
        }
        RemoteCommand::Open(place) => Some(open_archive(chat, place, cx)),
        RemoteCommand::Stop => Some(stop_session(chat, cx)),
        // Sends its own message, like `/archive` and for a different reason:
        // this one carries buttons, and a reply that is only ever a string has
        // nowhere to put them.
        RemoteCommand::Options => {
            send_options(chat, cx);
            None
        }
        RemoteCommand::OpenWhich => {
            Some("Which one? /archive lists them, then /open <number>.".to_string())
        }
        RemoteCommand::Away => Some(set_away(true, cx)),
        RemoteCommand::Here => Some(set_away(false, cx)),
        RemoteCommand::Follow(aim) => Some(follow(chat, aim, true, cx)),
        RemoteCommand::Unfollow(aim) => Some(follow(chat, aim, false, cx)),
        RemoteCommand::Status => Some(status(chat, cx)),
        RemoteCommand::Prompt(prompt) => Some(send_prompt(chat, &prompt, cx)),
    }
}

/// What this chat can do, in the order somebody arriving would need it.
const HELP: &str = "\
/sessions — every session onehand is running, and what each is doing
/use <number> — point this chat at one of them, and follow it
/follow [number] — hear about a session without pointing at it
/unfollow [number] — stop hearing about one
/status — what reaches this chat right now
/stop — cancel the turn running on the one this chat is pointed at
/options — the agent's own pickers: mode, model, effort
/archive — conversations saved on disk, newest first
/open <number> — put one of them back and point this chat at it
/away — you've left the machine; announce everything you follow
/here — you're back; go quiet again while you're looking
/help — this

Anything else is sent as a prompt to the session this chat is pointed at.
//<command> sends the agent's own slash command — //compact, //clear.

Nothing reaches you here until you ask for it. A session you follow announces \
three things on its own: a turn that finished, an agent waiting on you, an \
agent that stopped answering — everything else stays quiet, and a session you \
don't follow says nothing at all while still running normally. /use follows \
what it points at, so the short way in is /sessions then /use <number>. While \
you're at the keyboard what you could already see is held back; /away is how \
you say you can't see it. If it seems too quiet, /status says why.";

/// The `/sessions` reply, marking the one this chat is pointed at and the ones
/// it will hear about.
///
/// **Two marks and one column**, because they are two different questions asked
/// while reading the same row — where does what I type go, and which of these
/// will tell me anything — and answering the second in a sentence underneath
/// would mean counting rows to use it. The pointed-at session is always followed
/// too, so its arrow stands for both; what the dot is really for is the rows that
/// are followed without being pointed at, which are otherwise indistinguishable
/// from the silent ones.
fn listing(chat: &str, cx: &App) -> String {
    let sessions = sessions(cx);
    if sessions.is_empty() {
        // Not an error and not an empty list: onehand is running and has nothing
        // open, which is a different thing from onehand not being there.
        return "No sessions are open.".to_string();
    }
    let here = bound(chat, cx);
    let mut out = String::from("Sessions\n\n");
    for session in &sessions {
        out.push_str(if here == Some(session.uid) {
            "→ "
        } else if is_followed(chat, session.uid, cx) {
            "• "
        } else {
            "  "
        });
        out.push_str(&session.line());
        out.push('\n');
    }
    out.push_str(
        "\n→ is where what you type goes · is one you'll hear about\n\
         /use <number> points this chat at one and follows it.",
    );
    out
}

/// Answer `/use`.
fn point_at(chat: &str, uid: u64, cx: &mut App) -> String {
    let Some(session) = sessions(cx).into_iter().find(|s| s.uid == uid) else {
        return format!("There's no session {uid}.\n\n{}", listing(chat, cx));
    };
    let already = is_followed(chat, uid, cx);
    bind(chat, Some(uid), cx);
    // The subscription is said out loud rather than left to be discovered,
    // because it is the half of this command nobody asked for: `/use` reads as
    // "send my typing here", and a chat that then starts announcing turns
    // without having said it would would be the bridge doing something unasked.
    format!(
        "Pointed at {}.\nAnything you type here now goes to it as a prompt{}",
        session.line().trim_start(),
        if already {
            ", and you were already following it."
        } else {
            ", and you'll hear about it here — /unfollow if you'd rather not."
        }
    )
}

/// Send a prompt to whatever this chat is pointed at.
fn send_prompt(chat: &str, prompt: &str, cx: &mut App) -> String {
    let Some(uid) = bound(chat, cx) else {
        return not_pointed("send that to", chat, cx);
    };
    match ask_windows(cx, |shell, cx| shell.remote_prompt(uid, prompt, cx)) {
        Some(Handled::Sent) => format!("Sent to {uid}."),
        Some(Handled::Queued) => {
            format!("{uid} is mid-turn — this goes in the moment that one ends.")
        }
        Some(Handled::Refused(why)) => format!("{uid} didn't take it: {why}"),
        None => no_longer_there(uid, chat, cx),
    }
}

/// Which session an announcement is about, in the words a reader who is not
/// looking at the app would need.
///
/// A desktop notification can afford to be vague, because the window it is about
/// is one keystroke away. A message on a phone is the only thing its reader has,
/// so it names the project and the conversation as well as the agent — and it
/// carries the uid, which is what a button pressed on it has to come back with.
pub struct Origin {
    pub uid: u64,
    pub agent: String,
    pub project: String,
    /// The conversation's name, or `None` while it has not earned one.
    pub conversation: Option<String>,
}

impl Origin {
    /// The line under the headline: which session this is, where, and which
    /// conversation.
    ///
    /// The number leads, spelled the same way the listing spells it, because it
    /// is what a reply is typed against. Without it a notification about a
    /// waiting agent means opening `/sessions` and matching a name to a row
    /// before anything can be said back to it.
    fn context(&self) -> String {
        match &self.conversation {
            Some(title) => format!("{} · {} — {title}", self.uid, self.project),
            None => format!("{} · {}", self.uid, self.project),
        }
    }
}

/// One thing to say about one session, outside the app.
pub struct Announcement {
    /// Which of the three moments this is.
    pub away: Away,
    /// The line a reader on the far side needs and a reader at the window does
    /// not: what the agent is asking permission for, what the question was.
    ///
    /// `None` where the headline is the whole of it. Somebody looking at the
    /// window can read the card; somebody holding a phone has only this.
    pub detail: Option<String>,
    /// What can be answered from the message itself.
    pub buttons: Vec<Vec<Button>>,
}

impl Announcement {
    /// News with nothing to add and nothing to press: the headline and where it
    /// happened are the whole of it.
    pub fn plain(away: Away) -> Self {
        Self {
            away,
            detail: None,
            buttons: Vec::new(),
        }
    }
}

/// Say something about a session outside the app, to everyone allowed to hear
/// it.
///
/// Silence is the caller's decision, not this function's — with one exception,
/// below. Whether a finished turn or a parked question is worth announcing
/// depends on what is on screen, and the pane is the only thing that knows — the
/// same split that already decides whether the desktop hears about it.
///
/// **Who hears it is decided here and not there, because it is about this
/// channel and nothing else.** The pane's rules are about the screen and hold
/// for the desktop notification as much as for the chat; which sessions a chat
/// subscribed to is a fact only the bridge has, and pushing it up into the pane
/// would quieten the desktop over instructions that never mentioned it. It is
/// also the one gate that has to catch all three moments at once, and this is
/// where all three arrive.
///
/// **The audience is the chats that asked, which is narrower than `allowed`.**
/// That list is who may be told anything; a session's own news goes only to the
/// chats following it, so a bridge nobody has subscribed from is a bridge that
/// says nothing. What that costs is a channel which can look dead while working
/// perfectly, and `/status` is what answers for it.
pub fn announce(origin: &Origin, what: Announcement, cx: &App) {
    let bridge = &Shared::global(cx).remote;
    let Some(live) = &bridge.live else {
        return;
    };
    let audience: Vec<String> = {
        let followed = live.followed.borrow();
        live.allowed
            .iter()
            .filter(|chat| {
                followed
                    .get(*chat)
                    .is_some_and(|set| set.contains(&origin.uid))
            })
            .cloned()
            .collect()
    };
    if audience.is_empty() {
        return;
    }
    let mut text = format!(
        "{}\n{}",
        what.away.headline(&origin.agent),
        origin.context()
    );
    if let Some(detail) = &what.detail {
        text.push_str("\n\n");
        text.push_str(detail);
    }
    for chat in audience {
        bridge.send(Outbound {
            chat,
            text: text.clone(),
            buttons: what.buttons.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command word [`HELP`] offers, as the reader would type it.
    ///
    /// Lines beginning with a doubled slash are skipped: that entry is the
    /// escape for the agent's own commands, and `<command>` there is a
    /// placeholder rather than a word this build knows.
    fn words_help_offers() -> Vec<&'static str> {
        HELP.lines()
            .filter(|line| line.starts_with('/') && !line.starts_with("//"))
            .filter_map(|line| line.split_whitespace().next())
            .collect()
    }

    /// A command named in the help and not in the parser answers "I don't know
    /// that" while pointing at itself, which is the worst reply the bridge has.
    /// It is the failure a rename produces and the one nobody would go looking
    /// for, since the help is the last place anybody suspects.
    #[test]
    fn help_offers_nothing_the_parser_cannot_read() {
        for word in words_help_offers() {
            let read = onehand_core::remote::command::parse(word);
            assert!(
                !matches!(read, RemoteCommand::Unknown(_)),
                "{word} is offered in the help and the parser does not know it"
            );
        }
    }

    /// The other direction, for the three the subscription model turns on. A
    /// bridge that says nothing until asked is only usable if the asking is
    /// findable, and `/status` is what a reader reaches for when it seems
    /// broken — every one of them has to be in the first thing a new chat is
    /// sent.
    #[test]
    fn subscribing_is_offered_in_the_help() {
        let offered = words_help_offers();
        assert!(offered.contains(&"/follow"), "{HELP}");
        assert!(offered.contains(&"/unfollow"), "{HELP}");
        assert!(offered.contains(&"/status"), "{HELP}");
    }

    /// Silence being the default is the one thing a new chat cannot work out by
    /// waiting, so the help has to say it rather than only listing the commands
    /// that change it. Pinned by the words a reader would scan for: without them
    /// the first symptom is a bridge that looks dead.
    #[test]
    fn the_help_says_that_nothing_arrives_unasked() {
        assert!(
            HELP.contains("Nothing reaches you here until you ask for it"),
            "{HELP}"
        );
        // And the short way past it, since the listing alone does not say that
        // pointing a chat at a session is also subscribing to it.
        assert!(HELP.contains("/use follows what it points at"), "{HELP}");
    }
}
