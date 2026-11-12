//! One live conversation: the core [`Chat`] model plus the task feeding it.
//!
//! **One entity per session, not one per pane.** Each session polls its own
//! stream in its own `cx.spawn` and updates itself, so no agent event is ever
//! routed: there is no central update to look a window up from, and a session's
//! identity is window-independent because nothing about it names a window.

use crate::state::Shared;
use futures::{FutureExt as _, StreamExt as _};
use gpui::{
    App, AppContext, BorrowAppContext as _, Context, Entity, EventEmitter, Subscription, Task,
};
use gpui_component::input::TextareaState;
use gpui_component::text::TextViewState;
use onehand_core::chat::drive::HostLink;
use onehand_core::chat::drive::{Code, Effect, Input, Intake, Next, Origin, Signal};
use onehand_core::chat::{ChatItem, Md, MdId, SessionDrive, TranscriptItemId};
use onehand_core::completion;
use onehand_core::config::AgentSpec;
use onehand_core::host::client::HostConn;
use onehand_core::workspace::HostName;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) use host::{BeginOnHost, list as host_list};
use host::{Source, host_events, new_chat};

mod cards;
mod folds;
mod host;
#[cfg(test)]
pub(crate) mod tests;

/// Emitted when the transcript changed in a way the hosting pane cares about.
/// Say something on the desktop, outside the app's own window.
///
/// Fire-and-forget on its own thread: `show()` blocks on the notification bus,
/// and this must never be what the UI loop is waiting on. A bus that is absent
/// or refusing is not worth reporting either — the thing being announced is
/// already on screen inside the app, and the announcement is the copy.
fn notify_desktop(summary: String, body: String, urgency: notify_rust::Urgency) {
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .appname("onehand")
            .summary(&summary)
            .body(&body)
            .urgency(urgency)
            .show();
    });
}

/// Desktop notification for a turn that finished out of sight.
///
/// The sentence is core's, for the same reason the parked-ask one below is: two
/// surfaces now speak for a session from outside it, and a wording written at
/// each of them is a wording that drifts.
pub fn notify_turn_ended(agent: String, root: String) {
    notify_desktop(
        onehand_core::chat::Away::TurnEnded.headline(&agent),
        format!("in {root}"),
        // Normal, so it follows the desktop's own timeout: a finished turn is
        // news, and news that has been read stops being worth space.
        notify_rust::Urgency::Normal,
    );
}

/// Desktop notification for a schedule's task that ended: the schedule's name,
/// then how it ended.
pub(crate) fn notify_scheduled(schedule: String, said: String) {
    notify_desktop(schedule, said, notify_rust::Urgency::Normal);
}

/// Desktop notification for an agent that has stopped and is waiting on the
/// user.
///
/// The sentence is core's, so the two things that can park a session are named
/// the same way wherever either is announced.
pub fn notify_awaiting_user(ask: onehand_core::chat::UserAsk, agent: String, root: String) {
    notify_desktop(
        ask.headline(&agent),
        format!("in {root}"),
        // Critical, which on most desktops means it does not fade on its own.
        // A finished turn that is missed costs the time until it is noticed; a
        // question that is missed costs the agent standing still until someone
        // comes back to it, and a notification that expires while the agent is
        // still blocked is the app quietly withdrawing the only thing that said
        // so outside its own window.
        notify_rust::Urgency::Critical,
    );
}

/// Identify an image by its magic bytes.
///
/// ACP delivers image results as raw bytes with no declared type, and gpui
/// decodes by format rather than sniffing. Kept dependency-free and limited to
/// the formats an agent actually returns -- the same call the `base64_encode`
/// helper in core makes about pulling in a crate for a dozen lines.
fn sniff_image_format(bytes: &[u8]) -> Option<gpui::ImageFormat> {
    use gpui::ImageFormat;
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some(ImageFormat::Png),
        [0xFF, 0xD8, 0xFF, ..] => Some(ImageFormat::Jpeg),
        [b'G', b'I', b'F', b'8', ..] => Some(ImageFormat::Gif),
        [b'B', b'M', ..] => Some(ImageFormat::Bmp),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageFormat::Webp),
        _ => None,
    }
}

/// Files offered to `@`-mention completion: a monorepo has more paths than any
/// popup can be useful about.
const MAX_MENTION_FILES: usize = 5000;

pub enum ChatEvent {
    /// What the session's drive tells whatever watches it: content landed, a
    /// turn settled, a card waits, the stream ended. The pane decides whether
    /// a settled turn or a waiting card is worth a badge or a desktop
    /// notification -- it is the half that knows what is on screen.
    Session(Signal),
    /// A path in the transcript was clicked. The chat has no business opening
    /// files -- it says what was asked for and the shell decides where it goes.
    OpenFile(std::path::PathBuf),
    /// Fork the conversation from the prompt at this place. A new session is
    /// the shell's to make.
    Fork(onehand_core::chat::TranscriptItemId),
    /// The host named the session this one reads.
    HostSession(onehand_core::host::wire::HostSession),
    /// The host runs another build, with sessions mid-turn, so this one was
    /// not attached: it waits for them, and is restarted once they are done.
    HostBusy,
    /// *Restart now*, pressed on the line saying this session waits on the
    /// host named.
    RestartHost(onehand_core::workspace::HostName),
    /// The session has news a person may want told: whoever knows where they
    /// are answers with `Input::Seen` (`ChatPane::look`).
    Look(onehand_core::chat::Away),
}

/// The widget behind one question field's free-text box, and the subscription
/// that carries what is typed into it back to the model.
///
/// The subscription is held rather than detached for the reason the pump is:
/// it writes into this session, so it has no business outliving it.
struct AskInput {
    state: Entity<TextareaState>,
    _typing: Subscription,
}

/// One parked question's claim on a box: which question, which of its fields is
/// on screen, what the empty box invites, and what has been typed into that
/// field already.
type AskBox = (usize, usize, String, String);

/// The most ACP events one update takes, so a flood still yields to a frame.
const EVENT_BATCH: usize = 64;

pub struct ChatSession {
    /// The session's drive, which owns its transcript: read through it as
    /// the transcript, changed through its methods.
    pub chat: SessionDrive,
    /// The agent this session runs, as its offer is kept for the next start.
    agent: AgentSpec,
    /// When this session's adapter was started — for one warmed ahead of the
    /// session, when it was warmed. A restart is a new session, so this is
    /// when the agent now answering started, which is what tells a change to
    /// its plugins apart from one it has already loaded.
    pub started: std::time::Instant,
    /// What this session's agent's proxy to onehand's tools speaks for; let
    /// go with the session, and with it the agent's way in.
    /// None on a host: an agent there has no way to this machine's socket.
    _token: Option<crate::control::Token>,
    /// Whether the agent runs on a host, where a fork is not offered yet.
    pub(crate) on_host: bool,
    /// The host and the connection this session's watch is on, which says
    /// whether the watch ended because the connection was lost.
    host_conn: Option<(HostName, HostConn)>,
    /// Parsed markdown, keyed by [`MdId`]; the `usize` is how many bytes of the
    /// model's `source` are already in the state.
    ///
    /// The model deliberately holds only the source: the parsed form is
    /// framework-shaped, and this is where that shape is allowed to live --
    /// putting it in the model would drag the front end's text widget into a
    /// crate that must not know a front end exists. Growth is spliced in by byte
    /// count rather than re-parsed, so a long answer does not get slower as it
    /// streams.
    md: HashMap<MdId, (usize, Entity<TextViewState>)>,
    /// Decoded-image handles for inline results, keyed by the payload `Arc`'s
    /// pointer -- stable for the item's life, so one handle is reused across
    /// frames instead of re-hashing megabytes on every redraw. `RefCell`
    /// because the render path only ever has `&self`.
    images: RefCell<HashMap<usize, Arc<gpui::Image>>>,
    /// The free-text box of a parked question, keyed by its live item index and
    /// the field it belongs to.
    ///
    /// Here rather than in the model for the reason `md` is: an input's caret,
    /// selection and undo are framework-shaped, and the model is not allowed to
    /// know a front end exists. The *answer* is not kept here — every keystroke
    /// is written into the field's own `custom` slot, so what gets sent is read
    /// off the model like every other part of the card.
    ///
    /// One box per field, so moving between a form's questions does not rewrite
    /// one box under the user.
    /// The project's files, root-relative, for `@`-mention completion: what
    /// the composer offers, read off disk once when the session opens, and
    /// so this side's. None for a session on a host.
    pub files: Vec<String>,
    /// The directories [`Self::files`] passes through, with what is under
    /// each: derived once, where the list changes once, rather than in the
    /// popup on every keystroke and every streamed frame.
    pub folders: Vec<(String, usize)>,
    /// Whether the scan that fills [`Self::files`] has finished. An empty
    /// list is not a list that has not arrived: one means there is nothing
    /// here to name, the other that the popup is still looking.
    pub files_scanned: bool,
    /// Where the reader is on each open question card, by its live index:
    /// which question shows and the keyboard's row. View state, pruned with
    /// the boxes once a card is answered.
    ask_places: HashMap<usize, onehand_core::chat::AskPlace>,
    ask_inputs: HashMap<(usize, usize), AskInput>,
    /// The card of a parked question, keyed by its live item index.
    ///
    /// **What scopes the card's keyboard.** A question answers to number keys,
    /// the arrows, Enter and Esc — every one of them a key somebody is as
    /// likely to be typing into the composer an inch below — so the card takes
    /// them only while the caret is inside it, and this is the handle that
    /// answers "is it". Held here for the reason the boxes above are: it has to
    /// outlive the frame, and the card is rebuilt on every one.
    ask_focus: HashMap<usize, gpui::FocusHandle>,
    /// The card of a parked permission, keyed by its live item index.
    ///
    /// The same thing the handles above are, for the same reason: Enter and
    /// Esc settle a permission, and both are keys somebody is as likely to be
    /// pressing in the composer an inch below, so the card answers them only
    /// while it holds the caret.
    perm_focus: HashMap<usize, gpui::FocusHandle>,
    /// The pinned card the keyboard was last offered to, and whether it held
    /// the keys last frame: a card is offered them once, as it is pinned, and
    /// the next one is when this one held them as it was answered.
    pinned_keys: (Option<usize>, bool),
    /// Activity runs the user opened, keyed by the run's first item.
    ///
    /// **On the session, because a [`TranscriptItemId`] only means anything
    /// inside one transcript.** It is a position — history slot *n* or live
    /// slot *n* — so the same id names a different step in every other
    /// conversation. Held one level up, on the pane, opening a run here quietly
    /// opened whatever happened to sit at that position in the next session the
    /// user switched to.
    ///
    /// Dying with the session is the other half, and it is wanted: a restart
    /// folds the live tail back into history and renumbers everything, so a
    /// fold carried across it would land on an unrelated step.
    ///
    /// Keyed by the run's *first item*, not by the run's ordinal: a new step
    /// joining the run ahead of it renumbers every run below and would silently
    /// move the fold.
    ///
    /// Every fold is the reader's, and so this side's: core says only what
    /// opens by itself (a step running or one that just failed, a thought
    /// still being written). Every change moves one counter, `folds`.
    activity_open: HashMap<TranscriptItemId, bool>,
    /// Sections the user opened inside an opened cluster.
    ///
    /// **A third set, because a cluster and its first section share a key.**
    /// Both are anchored on their first item and the cluster's first item *is*
    /// its first section's, so one set holding both would open a section the
    /// moment the cluster around it opened and there would be no closing it.
    section_open: HashSet<TranscriptItemId>,
    /// Prompts the user opened past their cut.
    ///
    /// **A set of its own, because the section set already keys a turn's
    /// file list by its prompt**, and one press would open both.
    prompt_open: HashSet<TranscriptItemId>,
    /// Which turn summaries the user has decided about, and what they decided.
    ///
    /// **A map and not a set, because the default moves.** A summary opens
    /// itself while its turn is the newest and is a line once another prompt
    /// goes out -- so a set recording "the exception to the default" records a
    /// fact that changes meaning underneath it: a block closed while it was
    /// newest re-opened the moment it stopped being. Absent means untouched and
    /// follows the default; present is the answer the user gave, and it keeps.
    turn_open: HashMap<TranscriptItemId, bool>,
    /// Which file rows of a turn's closing summary are showing their diff,
    /// keyed by the turn's prompt and the path.
    ///
    /// **A pair and not an item id, because a row is not an item.** A file
    /// there stands for every edit the turn made to it, which is one row for
    /// what may be four transcript items -- and the same path in two turns is
    /// two rows that open independently.
    /// Keyed by the turn's prompt and the path, holding the diff each row
    /// shows -- taken once when the row opens rather than on every frame it is
    /// drawn. See `toggle_file`.
    file_open: HashMap<(TranscriptItemId, String), Vec<onehand_core::diff::Row>>,
    /// Subagent cards the reader opened, by the subagent's id.
    ///
    /// **Kept across a renumbering**, unlike the folds above: an id names one
    /// subagent wherever its card sits, so it cannot land on another.
    subagent_open: HashSet<String>,
    /// Which steps, thoughts, plans, and answered permissions and questions
    /// the reader opened or closed, and how; absent follows what the item
    /// does by itself.
    ///
    /// **One map for every kind**, because an id names one item, and an item
    /// is one kind.
    item_open: HashMap<TranscriptItemId, bool>,
    /// A step's output wells the reader opened past their cut, by the
    /// section's place in the step's content.
    out_open: HashSet<(TranscriptItemId, usize)>,
    /// How many times a fold was toggled. Apart from the transcript's
    /// revision because folding changes no item, yet the run layout reads the
    /// folds to choose how far a run stands from its neighbours.
    folds: u64,
    /// The chat's numbering the folds above were keyed under.
    numbering: u64,
    /// The event pump.
    ///
    /// Held rather than detached so the adapter's lifetime is tied to this
    /// entity's: dropping the session drops the task, which drops the event
    /// receiver, which makes the bridge's forwarder stop polling and kill the
    /// child (see [`onehand_io::acp::AcpRuntime::connect`]). Nothing else has to
    /// remember to shut an agent down.
    _pump: Task<()>,
    /// The wait the drive armed; replacing it restarts it, dropping it calls
    /// it off.
    _armed: Option<Task<()>>,
}

impl EventEmitter<ChatEvent> for ChatSession {}

impl ChatSession {
    /// Stop waiting for the host, taking back the line that said so:
    /// `true` when it was waiting.
    pub(crate) fn forget_host_wait(&mut self, cx: &mut Context<Self>) -> bool {
        let (was, fx) = self.chat.forget_host_wait();
        self.perform(fx, cx);
        was
    }

    /// No longer out of reach of its host, taking back the line that said
    /// so: `true` when it was.
    pub(crate) fn forget_host_offline(&mut self, cx: &mut Context<Self>) -> bool {
        let (was, fx) = self.chat.forget_host_away();
        self.perform(fx, cx);
        was
    }

    /// How the connection the session's watch rides stands now.
    fn host_link(&self) -> Option<HostLink> {
        self.host_conn.as_ref().map(|(host, conn)| HostLink {
            host: host.clone(),
            closed: conn.is_closed(),
            taken: conn.taken(),
        })
    }

    /// The wait on the host's busy sessions ended on `why`.
    pub(crate) fn wait_failed(&mut self, why: &str, cx: &mut Context<Self>) {
        let fx = self.chat.wait_failed(why, now());
        self.perform(fx, cx);
        cx.notify();
    }

    /// The wait for the host to be back ended on `why`.
    pub(crate) fn back_failed(&mut self, why: &str, cx: &mut Context<Self>) {
        let fx = self.chat.back_failed(why, now());
        self.perform(fx, cx);
        cx.notify();
    }
}

/// The time now, read here for the session's drive: the model reads no clock
/// of its own, so every event and command that may stamp or time something
/// carries this.
pub(crate) fn now() -> onehand_core::chat::Now {
    onehand_core::chat::Now {
        secs: onehand_core::chat::now_secs(),
        at: std::time::Instant::now(),
    }
}

/// Add a line to session's transcript in onehand's own voice: what a run
/// driving the session did, for the person reading it.
pub(crate) fn note(session: &Entity<ChatSession>, text: String, cx: &mut App) {
    session.update(cx, |session, cx| {
        let fx = session.chat.note(text);
        session.perform(fx, cx);
        cx.notify();
    });
}

impl ChatSession {
    /// Spawn `spec` against `root` and start folding its events into a fresh
    /// transcript.
    ///
    /// On a host, the events come from the host's session once the host is
    /// attached; the folder is the host's, so nothing here scans it, writes
    /// its transcript or hands its agent a way into onehand.
    pub(crate) fn spawn(
        uid: u64,
        root: PathBuf,
        spec: &AgentSpec,
        opening: onehand_core::acp::Opening,
        dirs: Vec<PathBuf>,
        on_host: Option<BeginOnHost>,
        cx: &mut App,
    ) -> Entity<Self> {
        let host = on_host.is_some();
        let (source, started, token) = match on_host {
            None => {
                // A test's agent, played over a channel, when one waits.
                #[cfg(test)]
                let canned = cx.update_global::<Shared, _>(|shared, _| shared.canned.pop_front());
                #[cfg(not(test))]
                let canned = None;
                let (events, started, token) = match canned {
                    Some(events) => (
                        events,
                        std::time::Instant::now(),
                        crate::control::Token::mint(),
                    ),
                    None => Shared::global(cx)
                        .acp
                        .connect(spec, root.clone(), opening, dirs),
                };
                token.claim(uid);
                (Source::Local(events), started, Some(token))
            }
            Some(on_host) => {
                // The host's folder, which for an offloaded project is its
                // mirror rather than `root`.
                let cwd = on_host.cwd.clone();
                let begin = (on_host, cwd, spec.clone(), opening);
                (
                    Source::Host(Box::new(begin)),
                    std::time::Instant::now(),
                    None,
                )
            }
        };

        let chat = new_chat(uid, root.clone(), spec, host);
        cx.new(|cx| Self::begin(chat, spec, host, source, started, token, cx))
    }

    /// The session itself, folding `source`'s events into `chat`: what
    /// [`Self::spawn`] makes once it has an agent to listen to.
    fn begin(
        chat: onehand_core::chat::Chat,
        spec: &AgentSpec,
        host: bool,
        source: Source,
        started: std::time::Instant,
        token: Option<crate::control::Token>,
        cx: &mut Context<Self>,
    ) -> Self {
        // `@`-mention candidates. Bounded and off the UI loop: a deep tree
        // must not stall the first frame.
        if !host {
            cx.spawn({
                let root = chat.root.clone();
                async move |session, cx| {
                    let files = cx
                        .background_executor()
                        .spawn(async move { completion::scan_files(&root, MAX_MENTION_FILES) })
                        .await;
                    let _ = session.update(cx, |session: &mut Self, cx| {
                        session.folders = completion::folders(&files);
                        session.files = files;
                        session.files_scanned = true;
                        cx.notify();
                    });
                }
            })
            .detach();
        }

        let chat = SessionDrive::new(chat);
        Self {
            chat,
            agent: spec.clone(),
            started,
            on_host: host,
            host_conn: None,
            _token: token,
            md: HashMap::new(),
            images: RefCell::new(HashMap::new()),
            ask_inputs: HashMap::new(),
            ask_places: HashMap::new(),
            files: Vec::new(),
            folders: Vec::new(),
            files_scanned: false,
            ask_focus: HashMap::new(),
            perm_focus: HashMap::new(),
            pinned_keys: (None, false),
            activity_open: HashMap::new(),
            section_open: HashSet::new(),
            prompt_open: HashSet::new(),
            turn_open: HashMap::new(),
            file_open: HashMap::new(),
            subagent_open: HashSet::new(),
            item_open: HashMap::new(),
            out_open: HashSet::new(),
            folds: 0,
            numbering: 0,
            _armed: None,
            _pump: cx.spawn(async move |session, cx| {
                let mut events = match source {
                    Source::Local(events) => events,
                    Source::Host(begin) => host_events(*begin, &session, cx).await,
                };
                while let Some(first) = events.next().await {
                    let delivered = session.update(cx, |session: &mut Self, cx| {
                        // **Every event already waiting, in one update.** A
                        // streamed answer is many small chunks, and a render
                        // plan rebuilt and a frame asked for at each one cost
                        // the UI thread most when several agents stream at
                        // once. Where a batch may end is the drive's; how
                        // many events one update takes at most is ours.
                        let mut next = Some(first);
                        let mut taken = 0;
                        while let Some(event) = next.take() {
                            taken += 1;
                            let link = session.host_link();
                            let (fx, more) =
                                session.chat.apply_watched(event, link.as_ref(), now());
                            session.perform(fx, cx);
                            let more = match more {
                                Next::Stop => false,
                                Next::More => taken < EVENT_BATCH,
                                Next::PastCap => true,
                            };
                            if more {
                                // A stream that has ended answers `None`
                                // again to the loop's own next wait.
                                next = events.next().now_or_never().flatten();
                            }
                        }
                        let fx = session.chat.end_batch();
                        session.perform(fx, cx);
                        cx.notify();
                    });
                    // The entity is gone (session closed, window closed): stop
                    // pumping so the receiver drops and the adapter follows.
                    if delivered.is_err() {
                        return;
                    }
                }

                // The stream ended, which only this loop knows.
                let _ = session.update(cx, |session: &mut Self, cx| {
                    let fx = session.chat.handle(Input::StreamEnded, now());
                    session.perform(fx, cx);
                    cx.notify();
                });
            }),
        }
    }

    /// Carry out what the drive said, in order, and hand back what only the
    /// caller can do: send an offloaded project's code first, and send the
    /// person's prompt again from where it waits once it is up.
    pub(crate) fn perform(&mut self, fx: Vec<Effect>, cx: &mut Context<Self>) -> Vec<Effect> {
        let mut left = Vec::new();
        for effect in fx {
            match effect {
                Effect::Signal(signal) => cx.emit(ChatEvent::Session(signal)),
                Effect::Changed => self.sync_md(cx),
                // What the agent offers is kept for every start that asks
                // before it brings an agent up.
                Effect::OfferChanged => {
                    let (spec, offer) = (self.agent.clone(), self.chat.offer());
                    cx.update_global::<Shared, _>(|s, _| s.settings.saw_offer(spec, offer));
                }
                Effect::Save(write) => {
                    cx.spawn(async move |session, cx| {
                        let (result, write) = cx
                            .background_executor()
                            .spawn(async move {
                                let result = onehand_core::chat::commit(&write);
                                (result.map_err(|e| e.to_string()), write)
                            })
                            .await;
                        // A session closed while its own write was in flight
                        // has nowhere to file the answer: its drop wrote the
                        // whole transcript, and the store keeps this write's
                        // lines from standing twice.
                        let _ = session.update(cx, |session: &mut Self, cx| {
                            let fx = session.chat.handle(Input::Saved { write, result }, now());
                            session.perform(fx, cx);
                        });
                    })
                    .detach();
                }
                Effect::Arm { token, wait } => {
                    self._armed = Some(cx.spawn(async move |session, cx| {
                        cx.background_executor().timer(wait).await;
                        let _ = session.update(cx, |session: &mut Self, cx| {
                            let fx = session.chat.handle(Input::Elapsed { token }, now());
                            session.perform(fx, cx);
                            cx.notify();
                        });
                    }));
                }
                Effect::Disarm => self._armed = None,
                // A channel that closed as it went is the stream ending, which
                // the pump hears and says.
                Effect::Request(request) => {
                    if let Some(tx) = self.chat.agent_channel() {
                        let _ = tx.send(request);
                    }
                }
                // Through the one writer every shown-then-saved change goes
                // through, so a name is written after every name before it.
                Effect::SaveTitle {
                    write,
                    change,
                    title,
                } => {
                    let written = Shared::global(cx).writer.write(move || {
                        onehand_core::chat::commit(&write).map_err(|why| why.to_string())
                    });
                    cx.spawn(async move |session, cx| {
                        let result = written.await;
                        let _ = session.update(cx, |session: &mut Self, cx| {
                            let fx = session.chat.handle(
                                Input::TitleSaved {
                                    change,
                                    title,
                                    result,
                                },
                                now(),
                            );
                            session.perform(fx, cx);
                            cx.notify();
                        });
                    })
                    .detach();
                }
                Effect::Look(what) => cx.emit(ChatEvent::Look(what)),
                Effect::SendCode { .. } | Effect::PromptAgain | Effect::Tell { .. } => {
                    left.push(effect)
                }
            }
        }
        left
    }

    /// Send `text` with `staged` from `origin`; what became of it, and what
    /// only the caller can do (see [`Self::perform`]).
    pub(crate) fn prompt(
        &mut self,
        text: &str,
        staged: &[onehand_core::attachment::StagedAttachment],
        origin: Origin,
        code: Code,
        cx: &mut Context<Self>,
    ) -> (Intake, Vec<Effect>) {
        let (intake, fx) = self.chat.prompt(text, staged, origin, code, now());
        let left = self.perform(fx, cx);
        cx.notify();
        (intake, left)
    }

    /// Send `text` from `origin` once the agent is up; see
    /// [`SessionDrive::prompt_when_up`].
    pub(crate) fn prompt_when_up(
        &mut self,
        text: &str,
        origin: Origin,
        cx: &mut Context<Self>,
    ) -> Intake {
        let (intake, fx) = self.chat.prompt_when_up(text, origin, now());
        self.perform(fx, cx);
        cx.notify();
        intake
    }

    /// Send the last prompt again, after a turn that failed; see
    /// [`SessionDrive::resend`].
    pub(crate) fn resend(&mut self, cx: &mut Context<Self>) {
        let (_, fx) = self.chat.resend(now());
        self.perform(fx, cx);
        cx.notify();
    }

    /// Stop the running turn.
    pub(crate) fn stop(&mut self, cx: &mut Context<Self>) {
        let fx = self.chat.stop();
        self.perform(fx, cx);
        cx.notify();
    }

    /// Name the conversation `title`; whether anything changed. See
    /// [`SessionDrive::rename`].
    pub(crate) fn rename(&mut self, title: &str, cx: &mut Context<Self>) -> bool {
        let (renamed, fx) = self.chat.rename(title);
        self.perform(fx, cx);
        cx.notify();
        renamed
    }

    /// Drop the name a person gave, back to the automatic one.
    pub(crate) fn reset_title(&mut self, cx: &mut Context<Self>) {
        let fx = self.chat.reset_title();
        self.perform(fx, cx);
        cx.notify();
    }

    /// Write the conversation's name and settings alone.
    pub(crate) fn save_meta(&mut self, cx: &mut Context<Self>) {
        let fx = self.chat.save_meta();
        self.perform(fx, cx);
    }

    /// A cached handle for an inline image result.
    ///
    /// `None` for bytes this build cannot identify: gpui decodes by declared
    /// format, so guessing wrong renders a broken image rather than nothing,
    /// and nothing is the more honest of the two.
    pub fn image(&self, bytes: &Arc<Vec<u8>>) -> Option<Arc<gpui::Image>> {
        let key = Arc::as_ptr(bytes) as usize;
        if let Some(handle) = self.images.borrow().get(&key) {
            return Some(handle.clone());
        }
        let format = sniff_image_format(bytes)?;
        let handle = Arc::new(gpui::Image::from_bytes(format, bytes.as_ref().clone()));
        self.images.borrow_mut().insert(key, handle.clone());
        Some(handle)
    }

    /// The parsed view state for `md`.
    ///
    /// `None` only while a block exists that [`Self::sync_md`] has not seen.
    /// Both routes that put a block in the transcript sync in the same update
    /// as the change: the event pump for a streamed one, [`Self::adopt`] for a
    /// whole archived conversation.
    pub fn md_view(&self, md: &Md) -> Option<&Entity<TextViewState>> {
        self.md.get(&md.id).map(|(_, state)| state)
    }

    /// Take an archived conversation as this session's read-only history.
    ///
    /// **The parse is part of adopting, not a step after it.** The transcript
    /// falls back to drawing a block's raw source when the cache has not seen
    /// it, and the cache was filled only by the event pump — so a resumed or
    /// restarted conversation rendered as unparsed markdown, every heading and
    /// fence and asterisk of it, until the adapter's first content event
    /// happened to arrive and sync the whole transcript as a side effect. That
    /// is a `npx` spawn plus a handshake away, seconds of it, and a stale
    /// resume that replays nothing never healed at all.
    ///
    /// Keeping `sync_md` private and putting the two calls here is what makes
    /// that unrepeatable: there is no way to adopt a conversation without
    /// parsing what it holds.
    pub fn adopt(
        &mut self,
        snapshot: onehand_core::chat::ConversationSnapshot,
        cx: &mut Context<Self>,
    ) {
        let fx = self.chat.resume(snapshot);
        self.forget_folds();
        self.perform(fx, cx);
        cx.notify();
    }

    /// Start as a fork of another conversation, seeded with its history up to
    /// the point; parsed as an adopted conversation is.
    pub fn adopt_fork(&mut self, seed: onehand_core::chat::ForkSeed, cx: &mut Context<Self>) {
        let fx = self.chat.fork(seed);
        self.forget_folds();
        self.perform(fx, cx);
        cx.notify();
    }

    /// Reparse whatever grew, drop whatever went away.
    ///
    /// Markdown only ever grows at its tail, so an unchanged block costs one
    /// length comparison and a streaming one costs `push_str` of the delta —
    /// never a reparse of the whole answer on every chunk.
    fn sync_md(&mut self, cx: &mut Context<Self>) {
        // Here because this runs after everything that moves the transcript:
        // a dropped replay renumbers it under the folds.
        if self.chat.numbering() != self.numbering {
            self.forget_folds();
        }
        // The blocks are walked twice rather than copied once. Collecting them
        // as owned strings copied *every* answer in the conversation on *every*
        // streamed chunk -- work proportional to the whole transcript, to
        // discover that all but one block was unchanged. What is copied here is
        // only the text actually going into a widget.
        let mut live: HashSet<MdId> = HashSet::new();
        let mut edits: Vec<(MdId, Edit)> = Vec::new();
        let mut sources = Vec::new();
        sources_of(&self.chat.history, &mut sources);
        sources_of(&self.chat.items, &mut sources);
        for md in sources {
            live.insert(md.id);
            let edit = match self.md.get(&md.id) {
                Some((parsed, _)) if md.source.len() > *parsed => {
                    Edit::Append(md.source[*parsed..].to_string())
                }
                // Shrunk or rewritten -- only a full reparse is safe.
                Some((parsed, _)) if md.source.len() < *parsed => Edit::Replace(md.source.clone()),
                Some(_) => continue,
                None => Edit::Create(md.source.clone()),
            };
            edits.push((md.id, edit));
        }

        for (id, edit) in edits {
            match (edit, self.md.get_mut(&id)) {
                (Edit::Append(delta), Some((parsed, state))) => {
                    *parsed += delta.len();
                    state.update(cx, |state, cx| state.push_str(&delta, cx));
                }
                (Edit::Replace(source), Some((parsed, state))) => {
                    *parsed = source.len();
                    state.update(cx, |state, cx| state.set_text(&source, cx));
                }
                (Edit::Create(source), _) => {
                    let state = cx.new(|cx| TextViewState::markdown(&source, cx));
                    self.md.insert(id, (source.len(), state));
                }
                (Edit::Append(_) | Edit::Replace(_), None) => {}
            }
        }

        self.md.retain(|id, _| live.contains(id));
    }
}

/// The markdown the transcript items carry, a subagent card's own included:
/// the card draws its prose as the transcript does.
fn sources_of<'a>(items: &'a [ChatItem], out: &mut Vec<&'a Md>) {
    for item in items {
        match item {
            ChatItem::Agent(md) => out.push(md),
            ChatItem::Thought(th) => out.push(&th.md),
            ChatItem::Subagent(card) => sources_of(&card.items, out),
            ChatItem::User(_)
            | ChatItem::Tool(_)
            | ChatItem::Plan(_)
            | ChatItem::Permission(_)
            | ChatItem::Ask(_)
            | ChatItem::Notice { .. } => {}
        }
    }
}

/// What one markdown block needs doing to it, decided while the transcript is
/// borrowed and carried out once it is not.
enum Edit {
    /// The block grew; only the new tail has to be parsed.
    Append(String),
    /// The block shrank or was rewritten, so the old parse is worthless.
    Replace(String),
    Create(String),
}
