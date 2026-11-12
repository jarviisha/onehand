//! What happens next on the remote bridge: a line or a press from a chat, what
//! it asks of a session, and what is said back.
//!
//! **Decided here, carried out elsewhere.** A [`RemoteDrive`] owns what the
//! bridge remembers of its chats ([`Chats`]) and whether the person said they
//! are away. It is told what the channel reported and what came of what it
//! asked for ([`Input`]), each with the sessions open across every window,
//! and answers with [`Effect`]s: a message to send, a press to acknowledge, a
//! session to prompt, stop, read or answer, an archive to scan or reopen. What
//! a session does with a prompt, a stop or a press is the session's drive's,
//! through [`super::link`].
//!
//! **Only sessions that run here are reached.** A session whose project runs
//! on a host, or is offloaded to one, is left out of every list, so no chat
//! can point at, follow, prompt or answer it, and none hears of it. That is
//! decided once, where the open sessions come in ([`RemoteDrive::handle`]).
//!
//! **Every binding and subscription is reconciled against the open sessions
//! first, once per line.** Wherever the live list is in hand, anything naming
//! a session that has closed goes; deciding it per command is how the bridge
//! once had four answers to "this session has closed". A press carries its own
//! session and never went through a binding, so it is not reconciled.

use super::chats::{ArchiveRow, Chats, RemoteSession};
use super::command::{self, RemoteCommand, HELP};
use super::press::Press;
use super::types::{Button, ChatId, Outbound};
use crate::chat::first_line_trunc;

/// How much of a taken-back prompt is quoted back at whoever wrote it: enough
/// to recognise it by, not enough to make a stop's confirmation a wall of text.
const QUOTED_PROMPT_MAX: usize = 120;

/// What the channel reported, and what came of what the drive asked for.
pub enum Input {
    /// Someone wrote to the bot.
    Message {
        chat: ChatId,
        from: ChatId,
        text: String,
    },
    /// Something arrived that this build cannot read.
    Unreadable { chat: ChatId, from: ChatId },
    /// A button was pressed; `data` is what it carried.
    Pressed {
        chat: ChatId,
        from: ChatId,
        press_id: String,
        data: String,
    },
    /// The channel is not coming back.
    Disconnected,
    /// The prompt [`Effect::Prompt`] sent to `uid`, or `None` when no window
    /// holds it any more.
    Prompted {
        chat: ChatId,
        uid: u64,
        handled: Option<Handled>,
    },
    /// The stop [`Effect::Stop`] asked of `uid`.
    Stopped {
        chat: ChatId,
        uid: u64,
        stopped: Option<Stopped>,
    },
    /// What `uid`'s agent lets a chat change ([`Effect::Options`]).
    Offered {
        chat: ChatId,
        uid: u64,
        offered: Option<(String, Vec<Vec<Button>>)>,
    },
    /// What the press [`Effect::Press`] did, in the presser's words.
    Answered {
        chat: ChatId,
        press_id: String,
        uid: u64,
        said: Option<String>,
    },
    /// The archive scan [`Effect::Scan`] found `found`, of `total`, landing
    /// at `now`, which its listing's ages are counted from.
    Scanned {
        chat: ChatId,
        found: Vec<ArchiveRow>,
        total: usize,
        now: crate::Now,
    },
    /// The conversation [`Effect::Open`] reopened, as session `uid`, or
    /// `None` when no window holds its project any more.
    Opened {
        chat: ChatId,
        pick: ArchiveRow,
        uid: Option<u64>,
    },
}

/// What the front end carries out.
///
/// Every answered effect's late answer is **recorded**: what it asked of a
/// session was done, or the scan or reopen ran, whatever the bridge heard
/// since, and the answer is said as it is.
pub enum Effect {
    /// Say this on the channel.
    Send(Outbound),
    /// Tell the channel the press `press_id` was dealt with, showing `text`.
    Ack { press_id: String, text: String },
    /// Stop using the channel: it is not coming back. Away went with it, so
    /// whatever decides what is told reads [`RemoteDrive::away`] again.
    ShutDown,
    /// The person said they are away, or back: whatever decides what is told
    /// reads [`RemoteDrive::away`] again. Back, the conversation in the window
    /// in front is read.
    Away(bool),
    /// Send `text` to `uid` as a prompt from outside the app
    /// ([`super::link::prompt`]); answered by [`Input::Prompted`].
    Prompt {
        chat: ChatId,
        uid: u64,
        text: String,
    },
    /// Stop `uid`'s turn ([`super::link::stop`]); answered by
    /// [`Input::Stopped`].
    Stop { chat: ChatId, uid: u64 },
    /// Read what `uid`'s agent lets a chat change
    /// ([`super::link::options`]); answered by [`Input::Offered`].
    Options { chat: ChatId, uid: u64 },
    /// Carry out `press` on its session ([`super::link::press`]); answered by
    /// [`Input::Answered`].
    Press {
        chat: ChatId,
        press_id: String,
        press: Press,
    },
    /// Scan the stored conversations of the projects that run here, off the
    /// UI thread ([`super::scan_archives`]); answered by [`Input::Scanned`].
    Scan { chat: ChatId },
    /// Reopen `pick` on its project, in the window holding it; answered by
    /// [`Input::Opened`].
    Open { chat: ChatId, pick: ArchiveRow },
}

/// What one prompt from outside did, wherever it landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    /// It went in and a turn started.
    Sent,
    /// A turn was already running, so it goes the moment that one ends.
    Queued,
    /// The session would not take it, in its own words.
    Refused(String),
}

/// What a stop from outside did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// Nothing was running.
    Idle,
    /// The turn was cancelled; the prompt waiting behind it was taken back,
    /// not sent.
    Cancelled { taken_back: Option<String> },
}

/// The remote bridge's drive. See the module header.
pub struct RemoteDrive {
    chats: Chats,
    /// The person said they are not at the machine.
    ///
    /// **The one fact a screen cannot stand in for**: a focused window in
    /// front of an empty chair reports every turn as read. In memory only: a
    /// launch that came up believing the person was elsewhere would message
    /// them about every turn while they sat in front of it.
    away: bool,
    /// A channel is running. A bridge that is off, or whose channel stopped,
    /// tells nobody anything.
    live: bool,
}

impl std::ops::Deref for RemoteDrive {
    type Target = Chats;
    fn deref(&self) -> &Chats {
        &self.chats
    }
}

impl RemoteDrive {
    /// A bridge with no channel behind it, the ordinary state.
    pub fn off() -> Self {
        Self {
            chats: Chats::new(Vec::new()),
            away: false,
            live: false,
        }
    }

    /// A bridge whose channel is running, for the chats on `allowed`.
    pub fn new(allowed: Vec<ChatId>) -> Self {
        Self {
            chats: Chats::new(allowed),
            away: false,
            live: true,
        }
    }

    /// Whether the person has said they are away.
    pub fn away(&self) -> bool {
        self.away
    }

    /// What the channel reported, or what came of an effect, with every
    /// session open across the windows.
    pub fn handle(&mut self, input: Input, open: &[RemoteSession]) -> Vec<Effect> {
        let open: Vec<RemoteSession> = open.iter().filter(|s| s.here).cloned().collect();
        let open = &open[..];
        match input {
            // The one gate, and it comes before everything: a chat that is not
            // on the list is answered with nothing at all. A refusal would
            // confirm that the bot is real, running, and has a list to get
            // onto.
            Input::Message { chat, from, text } if self.chats.allows(&chat, &from) => {
                self.answer(chat, &text, open)
            }
            // Somebody allowed is owed an answer even when it is no: silence is
            // indistinguishable from a bridge that stopped.
            Input::Unreadable { chat, from } if self.chats.allows(&chat, &from) => {
                vec![Effect::Send(Outbound::text(
                    chat,
                    "I can only read text — an image or a voice note doesn't travel. \
                     Type what you want done and it will go to the session.",
                ))]
            }
            // Not even acknowledged from a chat off the list: stopping the far
            // side's spinner would say something here read the press.
            Input::Pressed {
                chat,
                from,
                press_id,
                data,
            } if self.chats.allows(&chat, &from) => match Press::decode(&data) {
                Some(press) if open.iter().any(|s| s.uid == press.uid()) => {
                    vec![Effect::Press {
                        chat,
                        press_id,
                        press,
                    }]
                }
                // Not "no longer there" with a listing: a press never went
                // through a binding, and the message it was pressed on is
                // simply older than the session it was about.
                Some(press) => said_to_press(chat, press_id, gone_alone(press.uid())),
                // A button from a build that wrote its data differently. Saying
                // so beats acting on a guess, which would land on a real option
                // of a real session.
                None => said_to_press(
                    chat,
                    press_id,
                    "That button is from an older message.".into(),
                ),
            },
            Input::Message { .. } | Input::Unreadable { .. } | Input::Pressed { .. } => Vec::new(),
            Input::Disconnected => {
                self.live = false;
                // **Away goes with it.** The only way out of it, `/here`,
                // arrives over the channel that just died; left set, every
                // window would go on being treated as unwatched.
                self.away = false;
                vec![Effect::ShutDown]
            }
            Input::Prompted { chat, uid, handled } => {
                let text = match handled {
                    Some(Handled::Sent) => format!("Sent to {uid}."),
                    Some(Handled::Queued) => {
                        format!("{uid} is mid-turn — this goes in the moment that one ends.")
                    }
                    Some(Handled::Refused(why)) => format!("{uid} didn't take it: {why}"),
                    None => self.gone(uid, &chat, open),
                };
                vec![Effect::Send(Outbound::text(chat, text))]
            }
            Input::Stopped { chat, uid, stopped } => {
                let text = match stopped {
                    // Not "stopped": saying so would have the reader believe
                    // they had cut something short.
                    Some(Stopped::Idle) => format!("Nothing is running on {uid}."),
                    Some(Stopped::Cancelled { taken_back: None }) => format!("Stopped {uid}."),
                    Some(Stopped::Cancelled {
                        taken_back: Some(text),
                    }) => format!(
                        "Stopped {uid}. The prompt waiting behind it was taken back, not sent:\n\n{}",
                        first_line_trunc(&text, QUOTED_PROMPT_MAX)
                    ),
                    None => self.gone(uid, &chat, open),
                };
                vec![Effect::Send(Outbound::text(chat, text))]
            }
            Input::Offered { chat, uid, offered } => vec![Effect::Send(match offered {
                Some((text, buttons)) => Outbound {
                    chat,
                    text,
                    buttons,
                },
                None => {
                    let text = self.gone(uid, &chat, open);
                    Outbound::text(chat, text)
                }
            })],
            Input::Answered {
                chat,
                press_id,
                uid,
                said,
            } => said_to_press(chat, press_id, said.unwrap_or_else(|| gone_alone(uid))),
            Input::Scanned {
                chat,
                found,
                total,
                now,
            } => {
                let text = super::archive_listing(&found, total, now.secs);
                self.chats.remember_archive(&chat, found);
                vec![Effect::Send(Outbound::text(chat, text))]
            }
            Input::Opened { chat, pick, uid } => {
                let text = match uid {
                    // Pointed at what was just asked for: naming the
                    // conversation to reopen is naming where the next prompt
                    // goes.
                    Some(uid) => {
                        self.chats.point_at(&chat, uid, open);
                        format!(
                            "Opened {} — {} as session {uid}, and pointed this chat at it.",
                            pick.project, pick.title
                        )
                    }
                    // The project left every workspace since the listing went.
                    None => format!("{} is no longer open as a project.", pick.project),
                };
                vec![Effect::Send(Outbound::text(chat, text))]
            }
        }
    }

    /// What to say about `origin`'s news to every chat that follows it.
    ///
    /// An empty audience is the ordinary case, and the whole of the silence
    /// rule here; whether the news is worth telling at all is the session's
    /// drive's. A session that does not run here has none: no chat can follow
    /// a session it is never listed.
    pub fn announce(&self, origin: &super::Origin, what: &super::Announcement) -> Vec<Effect> {
        if !self.live {
            return Vec::new();
        }
        self.chats
            .announcement(origin, what)
            .into_iter()
            .map(Effect::Send)
            .collect()
    }

    /// A line from an allowed chat.
    fn answer(&mut self, chat: ChatId, text: &str, open: &[RemoteSession]) -> Vec<Effect> {
        self.chats.reconcile(open);
        let reply = |chat: ChatId, text: String| vec![Effect::Send(Outbound::text(chat, text))];
        let bound = self.chats.bound(&chat);
        match command::parse(text) {
            // Nothing was asked, so nothing is said: answering an empty message
            // is how a bridge starts talking to itself.
            RemoteCommand::Nothing => Vec::new(),
            RemoteCommand::List => {
                let text = self.chats.listing(&chat, open);
                reply(chat, text)
            }
            RemoteCommand::Help => reply(chat, HELP.to_string()),
            RemoteCommand::Unknown(word) => reply(chat, format!("I don't know /{word}.\n\n{HELP}")),
            RemoteCommand::UseWhich => {
                let text = format!(
                    "Which one? /sessions lists them, then /use <number>.\n\n{}",
                    self.chats.listing(&chat, open)
                );
                reply(chat, text)
            }
            RemoteCommand::Use(uid) => {
                let text = self.chats.point_at(&chat, uid, open);
                reply(chat, text)
            }
            // Reading the store is a file per conversation, so it goes to the
            // background and its listing is said when it lands.
            RemoteCommand::Archive => vec![Effect::Scan { chat }],
            RemoteCommand::Open(place) => match self.chats.archive_at(&chat, place) {
                Some(pick) => vec![Effect::Open { chat, pick }],
                None => reply(
                    chat,
                    "Ask for /archive first — the numbers count into that listing.".into(),
                ),
            },
            RemoteCommand::OpenWhich => reply(
                chat,
                "Which one? /archive lists them, then /open <number>.".into(),
            ),
            RemoteCommand::Stop => match bound {
                Some(uid) => vec![Effect::Stop { chat, uid }],
                None => self.not_pointed(chat, "stop", open),
            },
            RemoteCommand::Options => match bound {
                Some(uid) => vec![Effect::Options { chat, uid }],
                None => self.not_pointed(chat, "change options on", open),
            },
            RemoteCommand::Prompt(text) => match bound {
                Some(uid) => vec![Effect::Prompt { chat, uid, text }],
                None => self.not_pointed(chat, "send that to", open),
            },
            RemoteCommand::Away => self.set_away(chat, true),
            RemoteCommand::Here => self.set_away(chat, false),
            RemoteCommand::Follow(aim) => {
                let text = self.chats.follow(&chat, aim, true, open);
                reply(chat, text)
            }
            RemoteCommand::Unfollow(aim) => {
                let text = self.chats.follow(&chat, aim, false, open);
                reply(chat, text)
            }
            RemoteCommand::Status => {
                let text = self.chats.status(&chat, self.away, open);
                reply(chat, text)
            }
        }
    }

    /// Say the person is away or back, and the sentence that says so.
    fn set_away(&mut self, chat: ChatId, away: bool) -> Vec<Effect> {
        self.away = away;
        let text = if away {
            "Away. Everything gets announced here now, whatever is on screen."
        } else {
            "Back at the machine. Notifications go quiet again while you're looking."
        };
        vec![Effect::Away(away), Effect::Send(Outbound::text(chat, text))]
    }

    /// A chat asked for something and is pointed at nothing.
    fn not_pointed(&self, chat: ChatId, what: &str, open: &[RemoteSession]) -> Vec<Effect> {
        let text = self.chats.not_pointed(&chat, what, open);
        vec![Effect::Send(Outbound::text(chat, text))]
    }

    /// The session a chat was pointed at answered from no window.
    ///
    /// **It says so and changes nothing**: the binding goes at the reconcile
    /// of the chat's next line, the one place that decides it.
    fn gone(&self, uid: u64, chat: &str, open: &[RemoteSession]) -> String {
        format!("{}\n\n{}", gone_alone(uid), self.chats.listing(chat, open))
    }
}

fn gone_alone(uid: u64) -> String {
    format!("Session {uid} is gone.")
}

/// A press's answer, both ways: the acknowledgement stops the far side
/// spinning, and the message makes the chat a record of what was decided
/// from it.
fn said_to_press(chat: ChatId, press_id: String, said: String) -> Vec<Effect> {
    vec![
        Effect::Ack {
            press_id,
            text: said.clone(),
        },
        Effect::Send(Outbound::text(chat, said)),
    ]
}

#[cfg(test)]
mod tests;
