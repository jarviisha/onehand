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
use onehand_core::config::RemoteConfig;
use onehand_core::remote::types::{Outbound, RemoteEvent, ReqTx};
use onehand_core::remote::{ArchiveRow, Chats, Press, RemoteCommand};
use onehand_remote_telegram::secret;
use std::cell::RefCell;

/// What the bridge remembers about each chat, and every reply that reports or
/// changes it, is `onehand_core::remote::chats`. Re-exported so the front end
/// keeps naming these where it always did.
pub use onehand_core::remote::{Announcement, Origin, RemoteSession};

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
    /// Where each chat types and what each chat has asked to hear about, plus
    /// the list of who may reach the app at all.
    ///
    /// **Every rule over these is decided in core**, which is what makes them
    /// testable: they were never GUI-shaped, only unreachable, and while they
    /// sat here "this session has closed" grew four different answers depending
    /// on which command happened to notice.
    ///
    /// **A chat is bound by being told to, and never by being guessed at.** One
    /// project root runs as many sessions as it is asked to, so "the active
    /// one" is a moving answer that changes every time somebody at the keyboard
    /// clicks a rail row — a message sent from a train would land wherever the
    /// window happened to be pointing, which is the one failure a prompt cannot
    /// recover from.
    ///
    /// In memory only, so a restart forgets it. That is the honest lifetime:
    /// sessions are not persisted either, so a binding that outlived the process
    /// would name a session that no longer exists.
    ///
    /// `RefCell` because the bridge is reached through a global that most
    /// callers read immutably; the alternative is threading mutable access to
    /// that global through everything that merely wants to send a message.
    /// Borrows are taken and released inside single functions here, so none is
    /// ever held across a call — and never across a window walk, which is what
    /// would deadlock a reply that has to ask the shells something first.
    chats: RefCell<Chats>,
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
            chats: RefCell::new(Chats::new(tg.allowed_chats.clone())),
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

/// Reach what the bridge remembers about its chats, if there is a bridge.
///
/// **The borrow is taken and dropped inside `act`**, which is why every caller
/// gathers what it needs — the open sessions, whether the user is away — before
/// calling rather than inside: a window walk from in here would be a second
/// borrow of the same cell and a panic rather than an error.
///
/// `None` is a bridge that is off, which is the ordinary state and never an
/// error: the caller has nobody to answer anyway.
fn with_chats<R>(cx: &App, act: impl FnOnce(&mut Chats) -> R) -> Option<R> {
    let live = Shared::global(cx).remote.live.as_ref()?;
    let mut chats = live.chats.borrow_mut();
    Some(act(&mut chats))
}

/// Whether this chat may reach the app at all.
fn allowed(chat: &str, cx: &App) -> bool {
    with_chats(cx, |chats| chats.allows(chat)).unwrap_or(false)
}

/// The session `chat` has pointed itself at, if it has.
fn pointed_at(chat: &str, cx: &App) -> Option<u64> {
    with_chats(cx, |chats| chats.bound(chat)).flatten()
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
    let Some(everyone) = with_chats(cx, |chats| chats.everyone().to_vec()) else {
        return;
    };
    let bridge = &Shared::global(cx).remote;
    for chat in everyone {
        bridge.send(Outbound::text(chat, text.clone()));
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
fn scan_archives(roots: Vec<(std::path::PathBuf, String)>) -> (Vec<ArchiveRow>, usize) {
    let store = onehand_core::chat::conversations_dir();
    let mut found: Vec<ArchiveRow> = Vec::new();
    for (root, project) in roots {
        // Every agent, not one: the listing is about what was said, and which
        // adapter said it is a detail below that.
        for meta in onehand_core::chat::list_conversations(&store, &root, None) {
            found.push(ArchiveRow {
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
            with_chats(cx, |chats| chats.remember_archive(&chat, found));
            Shared::global(cx).remote.send(Outbound::text(chat, text));
        });
    })
    .detach();
}

/// The `/archive` reply.
fn archive_listing(found: &[ArchiveRow], total: usize) -> String {
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
    let Some(pick) = with_chats(cx, |chats| chats.archive_at(chat, place)).flatten() else {
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
            //
            // Walked again rather than reusing the list this message arrived
            // with: the session being pointed at is the one just minted, so it
            // is in this walk and was in no earlier one.
            let open = sessions(cx);
            with_chats(cx, |chats| chats.point_at(chat, uid, &open));
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
fn not_pointed(what: &str, chat: &str, open: &[RemoteSession], cx: &App) -> String {
    with_chats(cx, |chats| chats.not_pointed(chat, what, open)).unwrap_or_default()
}

/// What to say when the session a chat was pointed at answered from no window.
///
/// **It says so and changes nothing.** The binding is dropped by the reconcile
/// at the top of the next message, which is the one place that decides this:
/// answering it a second way here is how the four separate cleanups this
/// replaced came about. The window is a race — the session was open when the
/// message arrived and had closed by the time a shell was asked — so the entry
/// cannot outlive the next thing this chat says.
fn gone(uid: u64, chat: &str, open: &[RemoteSession], cx: &App) -> String {
    format!(
        "Session {uid} is gone.\n\n{}",
        with_chats(cx, |chats| chats.listing(chat, open)).unwrap_or_default()
    )
}

/// Answer `/options`: what the bound session's agent lets you change, with a
/// button per value.
fn send_options(chat: &str, open: &[RemoteSession], cx: &mut App) {
    let out = match pointed_at(chat, cx) {
        None => Outbound::text(
            chat.to_string(),
            not_pointed("change options on", chat, open, cx),
        ),
        Some(uid) => match ask_windows(cx, |shell, cx| shell.remote_options(uid, cx)) {
            Some((text, buttons)) => Outbound {
                chat: chat.to_string(),
                text,
                buttons,
            },
            None => Outbound::text(chat.to_string(), gone(uid, chat, open, cx)),
        },
    };
    Shared::global(cx).remote.send(out);
}

/// Answer `/stop`.
fn stop_session(chat: &str, open: &[RemoteSession], cx: &mut App) -> String {
    let Some(uid) = pointed_at(chat, cx) else {
        return not_pointed("stop", chat, open, cx);
    };
    match ask_windows(cx, |shell, cx| shell.remote_stop(uid, cx)) {
        Some(said) => said,
        None => gone(uid, chat, open, cx),
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
///
/// **Every binding and subscription is reconciled against the open sessions
/// first, once, here.** That is the whole of the rule: wherever the live list is
/// in hand, anything naming a session that has closed goes. Deciding it per
/// command is how this came to have four different answers — one dropped the
/// binding and the subscription, one dropped only the binding, one dropped
/// neither — and which one a chat got depended on what it happened to type.
///
/// A button press deliberately does not come through here. It carries its own
/// session and never went through a binding, so there is nothing to reconcile
/// and no listing that would help: the message it was pressed on is simply older
/// than the session it was about.
fn answer(chat: &str, text: &str, cx: &mut App) -> Option<String> {
    let command = onehand_core::remote::command::parse(text);
    // Walked once and threaded through, rather than asked for again inside each
    // reply: every window is visited to build it, and the borrow of the chat
    // state below must not be held across that walk.
    let open = sessions(cx);
    with_chats(cx, |chats| chats.reconcile(&open));
    match command {
        RemoteCommand::List => Some(with_chats(cx, |chats| chats.listing(chat, &open))?),
        RemoteCommand::Help => Some(HELP.to_string()),
        // Nothing was asked, so nothing is said. A client that sends an empty
        // message is not owed an answer, and answering one is how a bridge
        // starts talking to itself.
        RemoteCommand::Nothing => None,
        RemoteCommand::Unknown(word) => Some(format!("I don't know /{word}.\n\n{HELP}")),
        RemoteCommand::UseWhich => Some(format!(
            "Which one? /sessions lists them, then /use <number>.\n\n{}",
            with_chats(cx, |chats| chats.listing(chat, &open))?
        )),
        RemoteCommand::Use(uid) => Some(with_chats(cx, |chats| chats.point_at(chat, uid, &open))?),
        // The one command with nothing to say yet: reading the store is a file
        // per conversation, so it goes to the background and sends its own reply
        // when it lands.
        RemoteCommand::Archive => {
            list_archive(chat, cx);
            None
        }
        RemoteCommand::Open(place) => Some(open_archive(chat, place, cx)),
        RemoteCommand::Stop => Some(stop_session(chat, &open, cx)),
        // Sends its own message, like `/archive` and for a different reason:
        // this one carries buttons, and a reply that is only ever a string has
        // nowhere to put them.
        RemoteCommand::Options => {
            send_options(chat, &open, cx);
            None
        }
        RemoteCommand::OpenWhich => {
            Some("Which one? /archive lists them, then /open <number>.".to_string())
        }
        RemoteCommand::Away => Some(set_away(true, cx)),
        RemoteCommand::Here => Some(set_away(false, cx)),
        RemoteCommand::Follow(aim) => Some(with_chats(cx, |chats| {
            chats.follow(chat, aim, true, &open)
        })?),
        RemoteCommand::Unfollow(aim) => Some(with_chats(cx, |chats| {
            chats.follow(chat, aim, false, &open)
        })?),
        RemoteCommand::Status => {
            let away = is_away(cx);
            Some(with_chats(cx, |chats| chats.status(chat, away, &open))?)
        }
        RemoteCommand::Prompt(prompt) => Some(send_prompt(chat, &prompt, &open, cx)),
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

/// Send a prompt to whatever this chat is pointed at.
fn send_prompt(chat: &str, prompt: &str, open: &[RemoteSession], cx: &mut App) -> String {
    let Some(uid) = pointed_at(chat, cx) else {
        return not_pointed("send that to", chat, open, cx);
    };
    match ask_windows(cx, |shell, cx| shell.remote_prompt(uid, prompt, cx)) {
        Some(Handled::Sent) => format!("Sent to {uid}."),
        Some(Handled::Queued) => {
            format!("{uid} is mid-turn — this goes in the moment that one ends.")
        }
        Some(Handled::Refused(why)) => format!("{uid} didn't take it: {why}"),
        None => gone(uid, chat, open, cx),
    }
}

/// Say something about a session outside the app, to every chat that asked.
///
/// Silence is the caller's decision, not this function's. Whether a finished
/// turn or a parked question is worth announcing depends on what is on screen,
/// and the pane is the only thing that knows — the same split that already
/// decides whether the desktop hears about it.
///
/// **Who hears it is decided in core and not here**, because it is a fact about
/// this channel and nothing else: the pane's rules are about the screen and hold
/// for the desktop notification as much as for the chat, so pushing a
/// subscription up into the pane would quieten the desktop over instructions
/// that never mentioned it. An empty audience is the ordinary case, and it is
/// the whole of the silence rule.
pub fn announce(origin: &Origin, what: Announcement, cx: &App) {
    let Some(messages) = with_chats(cx, |chats| chats.announcement(origin, &what)) else {
        return;
    };
    let bridge = &Shared::global(cx).remote;
    for out in messages {
        bridge.send(out);
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
