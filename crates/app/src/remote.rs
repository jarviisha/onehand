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
use onehand_core::remote::types::{Button, Outbound, RemoteChannel as _, RemoteEvent, ReqTx};
use onehand_core::remote::{Press, RemoteCommand, Telegram, is_silently_ignored, secret};
use std::cell::RefCell;
use std::collections::HashMap;

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
    /// **This is also the audience for everything the app says out**, and the
    /// two being one list is deliberate rather than a shortcut. A notification
    /// exists to reach somebody who is not at the machine, including on a chat
    /// that has not asked this bridge for anything yet — so narrowing the
    /// audience to whoever spoke last would silence the bridge exactly when it
    /// has been quiet, which is when it matters. Everything a message says is
    /// something the reader is already trusted with, because that is what being
    /// on this list means.
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
        let stream = Telegram::new(token).connect(request_rx);
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
            cx.update_global::<Shared, _>(|shared, _| shared.remote.shut_down());
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
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    shells
        .into_iter()
        .filter_map(|shell| shell.upgrade())
        .find_map(|shell| shell.update(cx, |shell, cx| shell.remote_answer(press, cx)))
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
fn bind(chat: &str, uid: Option<u64>, cx: &App) {
    let Some(live) = Shared::global(cx).remote.live.as_ref() else {
        return;
    };
    let mut bindings = live.bindings.borrow_mut();
    match uid {
        Some(uid) => bindings.insert(chat.to_string(), uid),
        None => bindings.remove(chat),
    };
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
        RemoteCommand::Away => Some(set_away(true, cx)),
        RemoteCommand::Here => Some(set_away(false, cx)),
        RemoteCommand::Prompt(prompt) => Some(send_prompt(chat, &prompt, cx)),
    }
}

/// What this chat can do, in the order somebody arriving would need it.
const HELP: &str = "\
/sessions — every session onehand is running, and what each is doing
/use <number> — point this chat at one of them
/away — you've left the machine; announce everything here
/here — you're back; go quiet again while you're looking
/help — this

Anything else is sent as a prompt to the session this chat is pointed at.

Notifications arrive here on their own: a turn that finished, an agent waiting \
on you, an agent that stopped answering. While you're at the keyboard the ones \
you can already see are held back — /away is how you say you can't.";

/// The `/sessions` reply, marking the one this chat is pointed at.
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
        // The mark is on the line rather than in a sentence underneath, because
        // the question it answers -- where does what I type go -- is asked while
        // reading the list, not after it.
        out.push_str(if here == Some(session.uid) {
            "→ "
        } else {
            "  "
        });
        out.push_str(&session.line());
        out.push('\n');
    }
    out.push_str("\n/use <number> points this chat at one.");
    out
}

/// Answer `/use`.
fn point_at(chat: &str, uid: u64, cx: &mut App) -> String {
    let Some(session) = sessions(cx).into_iter().find(|s| s.uid == uid) else {
        return format!("There's no session {uid}.\n\n{}", listing(chat, cx));
    };
    bind(chat, Some(uid), cx);
    format!(
        "Pointed at {}.\nAnything you type here now goes to it as a prompt.",
        session.line().trim_start()
    )
}

/// Send a prompt to whatever this chat is pointed at.
fn send_prompt(chat: &str, prompt: &str, cx: &mut App) -> String {
    let Some(uid) = bound(chat, cx) else {
        return format!(
            "This chat isn't pointed at a session, so there is nowhere to send that.\n\n{}",
            listing(chat, cx)
        );
    };

    // Every window is asked, and the one holding that session answers. A map of
    // uid to window kept here would have to be corrected on every open, close
    // and restart, and would be wrong in between -- while the windows themselves
    // cannot be wrong about which sessions they hold.
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    let handled = shells
        .into_iter()
        .filter_map(|shell| shell.upgrade())
        .find_map(|shell| shell.update(cx, |shell, cx| shell.remote_prompt(uid, prompt, cx)));

    match handled {
        Some(Handled::Sent) => format!("Sent to {uid}."),
        Some(Handled::Queued) => {
            format!("{uid} is mid-turn — this goes in the moment that one ends.")
        }
        Some(Handled::Refused(why)) => format!("{uid} didn't take it: {why}"),
        // The session was closed since this chat was pointed at it. The binding
        // goes with it rather than being left to fail again on the next message,
        // and the list comes back so the next one can land somewhere.
        None => {
            bind(chat, None, cx);
            format!("Session {uid} is gone.\n\n{}", listing(chat, cx))
        }
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
/// Silence is the caller's decision, not this function's. Whether a finished
/// turn or a parked question is worth announcing depends on what is on screen,
/// and the pane is the only thing that knows — the same split that already
/// decides whether the desktop hears about it.
pub fn announce(origin: &Origin, what: Announcement, cx: &App) {
    let bridge = &Shared::global(cx).remote;
    let Some(live) = &bridge.live else {
        return;
    };
    let mut text = format!(
        "{}\n{}",
        what.away.headline(&origin.agent),
        origin.context()
    );
    if let Some(detail) = &what.detail {
        text.push_str("\n\n");
        text.push_str(detail);
    }
    for chat in &live.allowed {
        bridge.send(Outbound {
            chat: chat.clone(),
            text: text.clone(),
            buttons: what.buttons.clone(),
        });
    }
}
