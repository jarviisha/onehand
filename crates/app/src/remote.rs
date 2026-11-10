//! The remote bridge's carrier: what a channel reports goes into
//! `onehand_core::remote::RemoteDrive`, and what the drive answers is carried
//! out here.
//!
//! The channel runs on `onehand_io::remote`'s runtime; its events reach the UI
//! thread on a pump held on the global, one at a time. What a line or a press
//! means, what is said back and which sessions a chat may reach are the
//! drive's; what it asks of one session is `onehand_core::remote::link`'s,
//! called here on the session an effect names.
//!
//! **One process, one bot.** The bridge lives on the process-global state
//! rather than on a window, because a token identifies one bot and a second
//! poll against it is two clients fighting over the same updates. So a message
//! belongs to no window in particular, and each effect that names a session or
//! a project asks every window until the one holding it answers: a map kept
//! here would need correcting on every open, close and restart, and would be
//! wrong in between.

use crate::chat::session::{ChatSession, now};
use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Entity};
use onehand_core::config::RemoteConfig;
use onehand_core::remote::types::{RemoteEvent, RemoteRequest};
use onehand_core::remote::{Effect, Input, RemoteDrive, link};
use onehand_io::remote::Channel;
use onehand_telegram::secret;

pub use onehand_core::remote::{Announcement, Origin, RemoteSession};

/// The process-wide bridge: its drive, and the channel when one runs.
pub struct RemoteBridge {
    pub drive: RemoteDrive,
    channel: Option<Channel>,
}

impl RemoteBridge {
    /// A bridge with nothing behind it, the ordinary state.
    pub fn off() -> Self {
        Self {
            drive: RemoteDrive::off(),
            channel: None,
        }
    }

    fn send(&self, request: RemoteRequest) {
        if let Some(channel) = &self.channel {
            channel.send(request);
        }
    }
}

/// Start whatever channel `cfg` asks for and pump its events into the drive.
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
    // A channel missing here means the composition root changed; the app is
    // still usable without a phone attached to it.
    let Some(factory) = crate::features::remote_channel(onehand_telegram::CHANNEL_ID) else {
        eprintln!(
            "onehand: the Telegram bridge is enabled but no channel is registered under `{}`.",
            onehand_telegram::CHANNEL_ID
        );
        return;
    };
    let (channel, events) = match Channel::start(factory(token)) {
        Ok(started) => started,
        Err(e) => {
            eprintln!("onehand: could not start the remote bridge: {e}");
            return;
        }
    };
    let allowed = tg.allowed_chats.clone();
    cx.update_global::<Shared, _>(|shared, _| {
        shared.remote = RemoteBridge {
            drive: RemoteDrive::new(allowed),
            channel: Some(channel),
        };
    });
    // Held on the global rather than detached: the pump keeps the receiver
    // alive, and a dropped receiver is what tells the channel nobody listens.
    let pump = cx.spawn(async move |cx| {
        use futures::StreamExt as _;
        let mut events = events;
        while let Some(event) = events.next().await {
            cx.update(|cx| receive(event, cx));
        }
    });
    cx.update_global::<Shared, _>(|shared, _| shared._remote_pump = Some(pump));
}

/// One event from the channel, on the UI thread.
fn receive(event: RemoteEvent, cx: &mut App) {
    let input = match event {
        RemoteEvent::Connected { name } => {
            eprintln!("onehand: the remote bridge is up as {name}");
            return;
        }
        RemoteEvent::Disconnected(why) => {
            eprintln!("onehand: the remote bridge stopped: {why}");
            Input::Disconnected
        }
        RemoteEvent::Message { chat, from, text } => Input::Message { chat, from, text },
        RemoteEvent::Unreadable { chat, from } => Input::Unreadable { chat, from },
        RemoteEvent::Pressed {
            chat,
            from,
            press_id,
            data,
        } => Input::Pressed {
            chat,
            from,
            press_id,
            data,
        },
    };
    feed(input, cx);
}

/// Hand `input` to the drive with the sessions open now, and carry out what it
/// answers.
fn feed(input: Input, cx: &mut App) {
    let open = sessions(cx);
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.remote.drive.handle(input, &open));
    carry(fx, cx);
}

/// Carry out the drive's effects, in order, each once.
///
/// Never inside an update of the global: an effect walks the windows, and a
/// window reads the bridge.
fn carry(fx: Vec<Effect>, cx: &mut App) {
    for effect in fx {
        match effect {
            Effect::Send(out) => Shared::global(cx).remote.send(RemoteRequest::Send(out)),
            Effect::Ack { press_id, text } => Shared::global(cx).remote.send(RemoteRequest::Ack {
                press_id,
                text: Some(text),
            }),
            Effect::ShutDown => {
                let channel =
                    cx.update_global::<Shared, _>(|shared, _| shared.remote.channel.take());
                if let Some(channel) = channel {
                    channel.shut_down();
                }
                cx.refresh_windows();
            }
            Effect::Away(away) => back_or_away(away, cx),
            Effect::Prompt { chat, uid, text } => {
                let handled = on_session(uid, cx, |session, cx| {
                    session.update(cx, |session, cx| {
                        let (handled, fx) = link::prompt(&mut session.chat, &text, now());
                        session.perform(fx, cx);
                        cx.notify();
                        handled
                    })
                });
                feed(Input::Prompted { chat, uid, handled }, cx);
            }
            Effect::Stop { chat, uid } => {
                let stopped = on_session(uid, cx, |session, cx| {
                    session.update(cx, |session, cx| {
                        let (stopped, fx) = link::stop(&mut session.chat);
                        session.perform(fx, cx);
                        cx.notify();
                        stopped
                    })
                });
                feed(Input::Stopped { chat, uid, stopped }, cx);
            }
            Effect::Options { chat, uid } => {
                let offered = on_session(uid, cx, |session, cx| {
                    link::options(&session.read(cx).chat, uid)
                });
                feed(Input::Offered { chat, uid, offered }, cx);
            }
            Effect::Press {
                chat,
                press_id,
                press,
            } => {
                let uid = press.uid();
                let said = on_session(uid, cx, |session, cx| {
                    session.update(cx, |session, cx| {
                        let (said, fx) = link::press(&mut session.chat, press);
                        session.perform(fx, cx);
                        cx.notify();
                        said
                    })
                });
                feed(
                    Input::Answered {
                        chat,
                        press_id,
                        uid,
                        said,
                    },
                    cx,
                );
            }
            Effect::Scan { chat } => scan(chat, cx),
            Effect::Open { chat, pick } => {
                let uid = reopen(&pick, cx);
                feed(Input::Opened { chat, pick, uid }, cx);
            }
        }
    }
}

/// Every session this process is running, across every window, in one order:
/// windows in the order they were opened, each one's by uid, so the numbers in
/// a listing stay put between one message and the next.
fn sessions(cx: &App) -> Vec<RemoteSession> {
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    shells
        .iter()
        // A window closed between the registry's prune and this walk is one
        // fewer window.
        .filter_map(|shell| shell.upgrade())
        .flat_map(|shell| shell.read(cx).remote_sessions(cx))
        .collect()
}

/// Run `act` on session `uid` in whichever window holds it; `None` when none
/// does, which the drive says as a session gone.
fn on_session<R>(
    uid: u64,
    cx: &mut App,
    act: impl FnOnce(&Entity<ChatSession>, &mut App) -> R,
) -> Option<R> {
    let shells: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| w.shell.clone())
        .collect();
    let session = shells
        .into_iter()
        .filter_map(|shell| shell.upgrade())
        .find_map(|shell| shell.read(cx).session_entity(uid, cx))?;
    Some(act(&session, cx))
}

/// The person said they are away or back: every window decides again what it
/// tells. Back, the conversation in the window in front is read: it was
/// treated as unwatched the whole time, and a window focused throughout never
/// becomes active again to clear it.
fn back_or_away(away: bool, cx: &mut App) {
    cx.refresh_windows();
    if away {
        return;
    }
    let active: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .filter(|w| cx.active_window() == Some(w.handle))
        .map(|w| w.shell.clone())
        .collect();
    // Deferred: a caller can already be holding the active window's shell, and
    // updating it again would panic.
    cx.defer(move |cx| {
        for shell in active.into_iter().filter_map(|shell| shell.upgrade()) {
            shell.update(cx, |shell, cx| shell.mark_active_seen(cx));
        }
    });
}

/// Scan the stored conversations of every project here, off the UI thread.
///
/// Detached: the task's whole life is the one answer it ends with.
fn scan(chat: String, cx: &mut App) {
    let roots = roots(cx);
    cx.spawn(async move |cx| {
        let (found, total) = cx
            .background_executor()
            .spawn(async move { onehand_core::remote::scan_archives(roots) })
            .await;
        cx.update(|cx| feed(Input::Scanned { chat, found, total }, cx));
    })
    .detach();
}

/// Every project root on this machine that runs its sessions here, once each:
/// two windows can hold one folder.
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

/// Reopen `pick` in the window holding its project: showing the new session is
/// what starts its agent.
fn reopen(pick: &onehand_core::remote::chats::ArchiveRow, cx: &mut App) -> Option<u64> {
    let windows: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| (w.handle, w.shell.clone()))
        .collect();
    let agent = gpui::SharedString::from(pick.agent.clone());
    windows.into_iter().find_map(|(handle, shell)| {
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
    })
}

/// Say a session's news to every chat following it.
pub fn announce(origin: &Origin, what: Announcement, cx: &mut App) {
    let fx = Shared::global(cx).remote.drive.announce(origin, &what);
    carry(fx, cx);
}

/// Whether the person has said they are away.
pub fn is_away(cx: &App) -> bool {
    Shared::global(cx).remote.drive.away()
}
