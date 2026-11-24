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

use crate::chat::session::ChatSession;
use crate::state::Shared;
use gpui::Task;
use gpui::{App, BorrowAppContext as _, Entity};
use onehand_core::config::{RemoteConfig, TelegramConfig};
use onehand_core::remote::types::{RemoteChannel, RemoteEvent, RemoteRequest};
use onehand_core::remote::{Effect, Input, RemoteDrive, link};
use onehand_io::remote::Channel;
use onehand_telegram::secret;

pub use onehand_core::remote::{Announcement, Origin, RemoteSession};

/// The process-wide bridge: its drive, the channel when one runs, and the
/// pump carrying that channel's events, which goes with it.
pub struct RemoteBridge {
    pub drive: RemoteDrive,
    /// What the bridge was set up with, read again at every start: the
    /// token's variable can be renamed.
    telegram: TelegramConfig,
    channel: Option<Channel>,
    /// Held rather than detached: the pump keeps the receiver alive, and a
    /// dropped receiver is what tells the channel nobody listens. Dropped
    /// with its channel, so a replaced channel's late events reach nobody.
    pump: Option<Task<()>>,
}

impl RemoteBridge {
    /// A bridge with nothing behind it, the ordinary state.
    pub fn off() -> Self {
        Self {
            drive: RemoteDrive::off(),
            telegram: TelegramConfig::default(),
            channel: None,
            pump: None,
        }
    }

    fn send(&self, request: RemoteRequest) {
        if let Some(channel) = &self.channel {
            channel.send(request);
        }
    }

    /// Let the channel go, and its pump with it.
    fn stop(&mut self) {
        self.pump = None;
        if let Some(channel) = self.channel.take() {
            channel.shut_down();
        }
    }
}

/// Set the bridge up as `cfg` says, and start it if it is switched on.
///
/// Nothing here is fatal: a missing token or a channel switched off is the
/// ordinary case, and the bridge's state says which, where Settings shows it;
/// an app that refused to open a window over a chat bridge would be trading
/// the whole product for one of its conveniences.
pub fn boot(cfg: &RemoteConfig, cx: &mut App) {
    let tg = cfg.telegram.clone();
    let fx = cx.update_global::<Shared, _>(|shared, _| {
        shared.remote.drive = RemoteDrive::new(tg.allowed_chats.clone(), tg.enabled);
        shared.remote.telegram = tg;
        shared.remote.drive.start()
    });
    carry(fx, cx);
}

/// Look the token up off the UI thread, and tell the drive what was found.
fn look(cx: &mut App) {
    let tg = Shared::global(cx).remote.telegram.clone();
    let tg_env = tg
        .token_env
        .clone()
        .unwrap_or_else(|| secret::DEFAULT_TOKEN_ENV.to_string());
    cx.spawn(async move |cx| {
        let (source, token) = cx
            .background_executor()
            .spawn(async move { secret::token_source(&tg) })
            .await;
        // Said on stderr too, for whoever launched onehand from a terminal.
        if token.is_none() {
            eprintln!(
                "onehand: the Telegram bridge is on but no bot token was found. \
                 Set ${} or put the token in {}.",
                tg_env,
                secret::token_file().display()
            );
        }
        cx.update(|cx| feed(Input::Looked { source, token }, cx));
    })
    .detach();
}

/// Start the channel for `token`, in place of any that runs.
fn connect(token: String, cx: &mut App) {
    // A channel missing here means the composition root changed; the app is
    // still usable without a phone attached to it.
    let Some(factory) = crate::features::remote_channel(onehand_telegram::CHANNEL_ID) else {
        let why = format!(
            "no channel is registered under `{}`",
            onehand_telegram::CHANNEL_ID
        );
        feed(Input::Disconnected(why), cx);
        return;
    };
    connect_with(factory(token), cx);
}

/// Start `channel` in place of any that runs, and pump its events.
fn connect_with(channel: Box<dyn RemoteChannel>, cx: &mut App) {
    cx.update_global::<Shared, _>(|shared, _| shared.remote.stop());
    let (channel, events) = match Channel::start(channel) {
        Ok(started) => started,
        Err(e) => {
            feed(Input::Disconnected(format!("could not start: {e}")), cx);
            return;
        }
    };
    let pump = cx.spawn(async move |cx| {
        use futures::StreamExt as _;
        let mut events = events;
        while let Some(event) = events.next().await {
            cx.update(|cx| receive(event, cx));
        }
    });
    cx.update_global::<Shared, _>(|shared, _| {
        shared.remote.channel = Some(channel);
        shared.remote.pump = Some(pump);
    });
}

/// One event from the channel, on the UI thread.
fn receive(event: RemoteEvent, cx: &mut App) {
    let input = match event {
        RemoteEvent::Connected { name } => {
            eprintln!("onehand: the remote bridge is up as {name}");
            Input::Connected { name }
        }
        RemoteEvent::Retrying(why) => Input::Retrying(why),
        RemoteEvent::Disconnected(why) => {
            eprintln!("onehand: the remote bridge stopped: {why}");
            Input::Disconnected(why)
        }
        RemoteEvent::Message {
            chat,
            from,
            who,
            text,
        } => Input::Message {
            chat,
            from,
            who,
            text,
        },
        RemoteEvent::Unreadable { chat, from, who } => Input::Unreadable { chat, from, who },
        RemoteEvent::Pressed {
            chat,
            from,
            who,
            press_id,
            data,
        } => Input::Pressed {
            chat,
            from,
            who,
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
    // What the bridge is doing is drawn in Settings, and away in every window.
    cx.refresh_windows();
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
                cx.update_global::<Shared, _>(|shared, _| shared.remote.stop());
            }
            Effect::Start => look(cx),
            Effect::Connect(token) => connect(token, cx),
            Effect::Away(away) => back_or_away(away, cx),
            Effect::Prompt { chat, uid, text } => {
                let handled = on_session(uid, cx, |session, cx| {
                    session.update(cx, |session, cx| {
                        session.drive(cx, |chat, now| link::prompt(chat, &text, now))
                    })
                });
                feed(Input::Prompted { chat, uid, handled }, cx);
            }
            Effect::Stop { chat, uid } => {
                let stopped = on_session(uid, cx, |session, cx| {
                    session.update(cx, |session, cx| {
                        session.drive(cx, |chat, _| link::stop(chat))
                    })
                });
                feed(Input::Stopped { chat, uid, stopped }, cx);
            }
            Effect::Options { chat, uid } => {
                let offered = on_session(uid, cx, |session, cx| {
                    link::options(session.read(cx).chat(), uid)
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
                        session.drive(cx, |chat, _| link::press(chat, press))
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
    crate::windows::live(cx)
        .into_iter()
        .flat_map(|(_, shell)| shell.read(cx).remote_sessions(cx))
        .collect()
}

/// Run `act` on session `uid` in whichever window holds it; `None` when none
/// does, which the drive says as a session gone.
fn on_session<R>(
    uid: u64,
    cx: &mut App,
    act: impl FnOnce(&Entity<ChatSession>, &mut App) -> R,
) -> Option<R> {
    let session = crate::windows::session(uid, cx)?;
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
    let active = cx.active_window();
    for (handle, _) in crate::windows::live(cx) {
        if Some(handle) == active {
            // Later: a caller can already be holding the active window's
            // shell.
            crate::windows::later(handle, cx, |shell, _, cx| shell.mark_active_seen(cx));
        }
    }
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
        cx.update(|cx| {
            feed(
                Input::Scanned {
                    chat,
                    found,
                    total,
                    now: onehand_core::Now::read(),
                },
                cx,
            )
        });
    })
    .detach();
}

/// Every project root on this machine that runs its sessions here, once each:
/// two windows can hold one folder.
fn roots(cx: &App) -> Vec<(onehand_core::chat::Scope, String)> {
    let mut out: Vec<(onehand_core::chat::Scope, String)> = Vec::new();
    for (_, shell) in crate::windows::live(cx) {
        for (path, name) in shell.read(cx).remote_roots() {
            if !out.iter().any(|(scope, _)| scope.folder == path) {
                out.push((crate::projects::scope_of(&path, cx), name));
            }
        }
    }
    out
}

/// Reopen `pick` in the window holding its project: showing the new session is
/// what starts its agent.
fn reopen(pick: &onehand_core::remote::chats::ArchiveRow, cx: &mut App) -> Option<u64> {
    let agent = gpui::SharedString::from(pick.agent.clone());
    crate::windows::live(cx)
        .into_iter()
        .find_map(|(handle, _)| {
            let (root, dir, agent) = (pick.root.clone(), pick.dir.clone(), agent.clone());
            crate::windows::with_shell(handle, cx, |shell, window, cx| {
                shell.remote_open(&root, dir, Some(agent), window, cx)
            })
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

#[cfg(test)]
mod tests;
