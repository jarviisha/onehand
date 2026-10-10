//! One live conversation: the core [`Chat`] model plus the task feeding it.
//!
//! **One entity per session, not one per pane.** Each session polls its own
//! stream in its own `cx.spawn` and updates itself, so no agent event is ever
//! routed: there is no central update to look a window up from, and a session's
//! identity is window-independent because nothing about it names a window.

use crate::state::Shared;
use futures::StreamExt as _;
use gpui::{
    App, AppContext, BorrowAppContext as _, Context, Entity, EventEmitter, Subscription, Task,
};
use gpui_component::input::InputState;
use gpui_component::text::TextViewState;
use onehand_core::chat::{Chat, ChatItem, Md, MdId, TranscriptItemId};
use onehand_core::completion;
use onehand_core::config::AgentSpec;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

mod cards;

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
    /// New content landed; a follower should stay pinned to the tail.
    Appended,
    /// A turn settled. The pane decides whether that is worth a badge or a
    /// desktop notification -- it is the half that knows what is on screen.
    TurnEnded,
    /// The agent parked a permission or a question and is waiting on the user.
    /// The pane decides whether it needs saying outside the window, for the same
    /// reason it decides that about a finished turn.
    AwaitingUser(onehand_core::chat::UserAsk),
    /// The adapter went away. The session stays on screen as read-only history.
    Disconnected,
    /// A path in the transcript was clicked. The chat has no business opening
    /// files -- it says what was asked for and the shell decides where it goes.
    OpenFile(std::path::PathBuf),
}

/// The widget behind one question field's free-text box, and the subscription
/// that carries what is typed into it back to the model.
///
/// The subscription is held rather than detached for the reason the pump is:
/// it writes into this session, so it has no business outliving it.
struct AskInput {
    state: Entity<InputState>,
    _typing: Subscription,
}

/// One parked question's claim on a box: which question, which of its fields is
/// on screen, what the empty box invites, and what has been typed into that
/// field already.
type AskBox = (usize, usize, String, String);

pub struct ChatSession {
    pub chat: Chat,
    /// When this session's adapter was started — for one warmed ahead of the
    /// session, when it was warmed. A restart is a new session, so this is
    /// when the agent now answering started, which is what tells a change to
    /// its plugins apart from one it has already loaded.
    pub started: std::time::Instant,
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
    activity_open: HashMap<TranscriptItemId, bool>,
    /// Sections the user opened inside an opened cluster.
    ///
    /// **A third set, because a cluster and its first section share a key.**
    /// Both are anchored on their first item and the cluster's first item *is*
    /// its first section's, so one set holding both would open a section the
    /// moment the cluster around it opened and there would be no closing it.
    section_open: HashSet<TranscriptItemId>,
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
    /// How many times a fold has been toggled.
    ///
    /// The run layout is built partly from these folds, so it has to be rebuilt
    /// when one changes -- and a set of ids gives a viewport no cheap way to
    /// ask "is this still the same set?". The counter is that answer.
    folds_revision: u64,
    /// The event pump.
    ///
    /// Held rather than detached so the adapter's lifetime is tied to this
    /// entity's: dropping the session drops the task, which drops the event
    /// receiver, which makes the bridge's forwarder stop polling and kill the
    /// child (see [`crate::acp::AcpRuntime::connect`]). Nothing else has to
    /// remember to shut an agent down.
    _pump: Task<()>,
}

impl EventEmitter<ChatEvent> for ChatSession {}

/// Add a line to session's transcript in onehand's own voice: what a run
/// driving the session did, for the person reading it.
pub(crate) fn note(session: &Entity<ChatSession>, text: String, cx: &mut App) {
    session.update(cx, |session, cx| {
        session.chat.items.push(ChatItem::notice(text));
        cx.emit(ChatEvent::Appended);
        cx.notify();
    });
}

impl ChatSession {
    /// Spawn `spec` against `root` and start folding its events into a fresh
    /// transcript.
    pub fn spawn(
        uid: u64,
        root: PathBuf,
        spec: &AgentSpec,
        resume: Option<String>,
        cx: &mut App,
    ) -> Entity<Self> {
        let (events, started) = Shared::global(cx).acp.connect(spec, root.clone(), resume);
        let seen = spec.clone();

        cx.new(|cx| {
            // `@`-mention candidates. Bounded and off the UI loop: a deep tree
            // must not stall the first frame.
            cx.spawn({
                let root = root.clone();
                async move |session, cx| {
                    let files = cx
                        .background_executor()
                        .spawn(async move { completion::scan_files(&root, MAX_MENTION_FILES) })
                        .await;
                    let _ = session.update(cx, |session: &mut Self, cx| {
                        session.chat.folders = completion::folders(&files);
                        session.chat.files = files;
                        session.chat.files_scanned = true;
                        cx.notify();
                    });
                }
            })
            .detach();

            Self {
                chat: Chat::new(
                    uid,
                    root,
                    spec.name.clone(),
                    Some(onehand_core::chat::conversations_dir()),
                ),
                started,
                md: HashMap::new(),
                images: RefCell::new(HashMap::new()),
                ask_inputs: HashMap::new(),
                ask_focus: HashMap::new(),
                perm_focus: HashMap::new(),
                activity_open: HashMap::new(),
                section_open: HashSet::new(),
                turn_open: HashMap::new(),
                file_open: HashMap::new(),
                folds_revision: 0,
                _pump: cx.spawn(async move |session, cx| {
                    let mut events = events;
                    while let Some(event) = events.next().await {
                        let delivered = session.update(cx, |session: &mut Self, cx| {
                            // What the event did is the reducer's answer, not a
                            // second match on the event here -- two copies of
                            // "did the turn settle" is one copy too many.
                            let outcome = session.chat.apply(event);
                            if outcome.transcript_changed {
                                session.sync_md(cx);
                            }
                            cx.emit(ChatEvent::Appended);
                            if outcome.turn_ended {
                                cx.emit(ChatEvent::TurnEnded);
                            }
                            if let Some(ask) = outcome.asked_user {
                                cx.emit(ChatEvent::AwaitingUser(ask));
                            }
                            // What the agent offers is kept for every start
                            // that asks before it brings an agent up.
                            if outcome.modes_offered {
                                let modes =
                                    session.chat.modes.iter().map(|m| m.id.clone()).collect();
                                let spec = seen.clone();
                                cx.update_global::<Shared, _>(|s, _| s.saw_modes(spec, modes));
                            }
                            cx.notify();
                        });
                        // The entity is gone (session closed, window closed): stop
                        // pumping so the receiver drops and the adapter follows.
                        if delivered.is_err() {
                            return;
                        }
                    }

                    // The stream ended: core's `connect` always emits
                    // `Disconnected` before it does, so the model already knows.
                    // What it cannot know is that no further event will arrive, and
                    // a turn left mid-flight would otherwise show Stop forever.
                    //
                    // `link` is set here too rather than trusted from that
                    // event: this is the one place that knows the stream is
                    // *over*, and a rail dot that depends on an event having
                    // been emitted is a dot that goes missing the day it is not.
                    let _ = session.update(cx, |session: &mut Self, cx| {
                        session.chat.busy = false;
                        session.chat.link = onehand_core::chat::Link::Lost;
                        cx.emit(ChatEvent::Disconnected);
                        cx.notify();
                    });
                }),
            }
        })
    }

    /// Whether the activity run anchored at `anchor` is showing its steps:
    /// the reader's choice once they made one, else `default`.
    pub fn activity_is_open(&self, anchor: TranscriptItemId, default: bool) -> bool {
        self.activity_open.get(&anchor).copied().unwrap_or(default)
    }

    /// Hold the activity run anchored at `anchor` open or closed, whatever
    /// it goes on to do.
    pub fn set_activity_open(&mut self, anchor: TranscriptItemId, open: bool) {
        self.activity_open.insert(anchor, open);
        self.folds_revision = self.folds_revision.wrapping_add(1);
    }

    /// Whether the section anchored at `anchor` is showing its steps.
    pub fn section_is_open(&self, anchor: TranscriptItemId) -> bool {
        self.section_open.contains(&anchor)
    }

    /// Fold or unfold the section anchored at `anchor`.
    pub fn toggle_section(&mut self, anchor: TranscriptItemId) {
        if !self.section_open.remove(&anchor) {
            self.section_open.insert(anchor);
        }
        self.folds_revision = self.folds_revision.wrapping_add(1);
    }

    /// Whether the turn summary anchored at `anchor` is open, given what it
    /// would do if nobody had said.
    pub fn turn_is_open(&self, anchor: TranscriptItemId, default: bool) -> bool {
        self.turn_open.get(&anchor).copied().unwrap_or(default)
    }

    /// Open or close the turn summary anchored at `anchor`.
    pub fn toggle_turn(&mut self, anchor: TranscriptItemId, default: bool) {
        let now = self.turn_is_open(anchor, default);
        self.turn_open.insert(anchor, !now);
        self.folds_revision = self.folds_revision.wrapping_add(1);
    }

    /// Whether `path`'s diff is showing under the turn anchored at `anchor`.
    pub fn file_is_open(&self, anchor: TranscriptItemId, path: &str) -> bool {
        // **Built and asked, not scanned.** This is answered once per file row
        // per frame, so walking the set turns a turn's worth of rows into a
        // quadratic over what the whole conversation has open.
        self.file_open.contains_key(&(anchor, path.to_string()))
    }

    /// Show or hide `path`'s diff under the turn anchored at `anchor`.
    ///
    /// **The diff is computed here, once, and kept.** Taking it in the renderer
    /// instead put an LCS over two whole files inside a render pass -- which
    /// runs on every frame the row is on screen, for as long as it is open, and
    /// the table that diff builds is megabytes wide on a large file. The row
    /// changes what it draws only when somebody clicks it, so this is the one
    /// moment the work is actually needed.
    pub fn toggle_file(&mut self, anchor: TranscriptItemId, path: &str, body: &[TranscriptItemId]) {
        let key = (anchor, path.to_string());
        if self.file_open.remove(&key).is_none() {
            let rows = self.with_turn_items(body, |items| {
                onehand_core::chat::turn_file_diff(items, path)
            });
            self.file_open.insert(key, rows);
        }
        self.folds_revision = self.folds_revision.wrapping_add(1);
    }

    /// The diff showing under `path`'s row, if it is showing.
    pub fn file_diff(
        &self,
        anchor: TranscriptItemId,
        path: &str,
    ) -> Option<&[onehand_core::diff::Row]> {
        self.file_open
            .get(&(anchor, path.to_string()))
            .map(Vec::as_slice)
    }

    /// Show every file of the turn anchored at `anchor`, or hide them all if
    /// they are already showing.
    ///
    /// One control for the whole block, since opening eight rows one at a time
    /// to read a turn's diff is the block asking to be scrolled rather than
    /// read.
    pub fn toggle_every_file(
        &mut self,
        anchor: TranscriptItemId,
        paths: &[String],
        body: &[TranscriptItemId],
    ) {
        let all_open = paths.iter().all(|path| self.file_is_open(anchor, path));
        self.file_open.retain(|(at, _), _| *at != anchor);
        if !all_open {
            let rows: Vec<(String, Vec<onehand_core::diff::Row>)> =
                self.with_turn_items(body, |items| {
                    paths
                        .iter()
                        .map(|path| {
                            (
                                path.clone(),
                                onehand_core::chat::turn_file_diff(items, path),
                            )
                        })
                        .collect()
                });
            self.file_open
                .extend(rows.into_iter().map(|(path, diff)| ((anchor, path), diff)));
        }
        self.folds_revision = self.folds_revision.wrapping_add(1);
    }

    /// Run `f` over the transcript items `body` names.
    ///
    /// The summary block holds a turn's items by id rather than by value, so
    /// this is how it reaches them when a row is opened -- and it is a closure
    /// rather than a returned `Vec<&ChatItem>` because those borrow the chat
    /// this session owns.
    pub fn with_turn_items<T>(
        &self,
        body: &[TranscriptItemId],
        f: impl FnOnce(&[&onehand_core::chat::ChatItem]) -> T,
    ) -> T {
        let items: Vec<&onehand_core::chat::ChatItem> = body
            .iter()
            .filter_map(|target| match target {
                TranscriptItemId::History(i) => self.chat.history.get(*i),
                TranscriptItemId::Live(i) => self.chat.items.get(*i),
            })
            .collect();
        f(&items)
    }

    /// How many folds have been toggled. See the field.
    pub fn folds_revision(&self) -> u64 {
        self.folds_revision
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
        self.chat.resume_from(snapshot);
        // Folds are keyed by position, and the transcript is numbered afresh,
        // so a choice kept from before would land on an unrelated step.
        self.activity_open.clear();
        self.section_open.clear();
        self.turn_open.clear();
        self.file_open.clear();
        self.folds_revision = self.folds_revision.wrapping_add(1);
        self.sync_md(cx);
        cx.notify();
    }

    /// Reparse whatever grew, drop whatever went away.
    ///
    /// Markdown only ever grows at its tail, so an unchanged block costs one
    /// length comparison and a streaming one costs `push_str` of the delta —
    /// never a reparse of the whole answer on every chunk.
    fn sync_md(&mut self, cx: &mut Context<Self>) {
        // The blocks are walked twice rather than copied once. Collecting them
        // as owned strings copied *every* answer in the conversation on *every*
        // streamed chunk -- work proportional to the whole transcript, to
        // discover that all but one block was unchanged. What is copied here is
        // only the text actually going into a widget.
        let mut live: HashSet<MdId> = HashSet::new();
        let mut edits: Vec<(MdId, Edit)> = Vec::new();
        for md in self
            .chat
            .history
            .iter()
            .chain(self.chat.items.iter())
            .filter_map(source_of)
        {
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

    /// Send the composer's contents as a prompt. Returns whether a turn started.
    ///
    /// The guard and the ordering live in [`Chat::submit`], so the view cannot
    /// come to a different answer than the model about when Send is allowed.
    pub fn submit(
        &mut self,
        text: &str,
        attachments: &[onehand_core::attachment::StagedAttachment],
        cx: &mut Context<Self>,
    ) -> bool {
        let sent = self.chat.submit(text, attachments);
        if sent {
            // A prompt is also what closes a resumed session's replay window,
            // and closing it can put a whole conversation back on screen -- the
            // one the adapter started re-delivering and never finished. Those
            // blocks were taken out of the transcript when the replay began, so
            // the parse cache dropped them; coming back, they have to be parsed
            // again or every answer in the conversation draws as its own
            // markdown source. The event path is covered because the pump syncs
            // whenever the transcript moved; this path has no event.
            self.sync_md(cx);
            cx.notify();
        }
        sent
    }
}

/// The markdown a transcript item carries, if it carries any.
fn source_of(item: &ChatItem) -> Option<&Md> {
    match item {
        ChatItem::Agent(md) => Some(md),
        ChatItem::Thought(th) => Some(&th.md),
        _ => None,
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
