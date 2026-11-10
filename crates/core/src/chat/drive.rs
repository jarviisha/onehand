//! What happens next in one session: the prompt that comes in, the events the
//! agent sends, the save at a turn's end, the wait for a quiet spell, and the
//! stream that ends.
//!
//! **Decided here, carried out elsewhere.** A [`SessionDrive`] owns the
//! session's [`Chat`] and is told what happened: a command (a prompt, a stop),
//! one ACP event at a time, or what came of something it asked for
//! ([`Input`]). It answers with [`Effect`]s, in order, for the front end to
//! carry out: say something to whatever watches the session, write the
//! transcript, arm a timer, send an offloaded project's code first. It has no
//! timer, thread or I/O of its own.
//!
//! **One way in for a prompt.** A person's composer, the remote bridge, a
//! lead's tools, a run and issue work all send through [`SessionDrive::prompt`],
//! each naming its [`Origin`]. What differs between them is one table, in
//! [`SessionDrive::prompt`], rather than a copy of the sending rule at each.
//!
//! **Reads go to the model.** The drive derefs to its [`Chat`]; what a front
//! end draws, it reads there. Every change to the session comes through a
//! method here; a front end has no way to the model's mutators.
//!
//! **A batch of events.** The agent streams many small events, and a front end
//! repaints once per batch rather than once per event. Where a batch may end
//! is behaviour, not drawing: a card is said only once the events that came
//! with it are in, and a batch ends after any event that tells a watcher more
//! than that something was appended. [`SessionDrive::apply`] answers each event
//! with whether the batch may go on ([`Next`]); [`SessionDrive::end_batch`]
//! closes it. How many events a front end takes at most is its own cap.

use super::attention::Attention;
use super::model::{Away, Chat, Link, Now, QueuedPrompt, SubmitBlock, UserAsk};
use super::store::PendingWrite;
use crate::acp::{AcpEvent, AcpRequest};
use crate::attachment::StagedAttachment;
use std::time::Duration;

/// How long an agent writes nothing, between turns, before what it wrote is
/// saved. For an agent that never says its own work between turns has
/// settled: long enough that a burst of output lands as one write, short
/// enough that closing the window soon after loses nothing.
pub const QUIET_SAVE: Duration = Duration::from_secs(3);

/// Who sent a prompt, which decides what a busy session or an offloaded
/// project does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A person, at the composer.
    Person,
    /// A person, from outside the app, over the remote bridge.
    Remote,
    /// onehand's tools, called by a session's agent: a lead to its worker,
    /// or the first prompt of a session opened for an agent.
    Tools,
    /// A run, to the session it is driven on.
    Run,
    /// Issue work, as the session's first prompt.
    Issue,
}

/// Where an offloaded project's code is, as the front end knows it, for a
/// person's prompt. Every other origin ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Code {
    /// Not offloaded, or the host already holds the code.
    Here,
    /// Already on its way up: the prompt waits in the composer.
    Going,
    /// This machine holds the code, and it has to go up before the prompt:
    /// what the session says meanwhile.
    MustGo { line: String },
}

/// What became of a prompt, at once. What comes later arrives as effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intake {
    /// Sent: a turn started.
    Sent,
    /// Held behind the running turn, sent when it ends.
    Queued,
    /// Held until the agent is up, sent then.
    Held,
    /// Neither sent nor taken: the code is on its way up, and the prompt waits
    /// where it was written.
    Waiting,
    /// The code goes up first ([`Effect::SendCode`]); the prompt waits where it
    /// was written and is sent again once the code is there.
    CodeFirst,
    /// Not taken, and why.
    Refused(Refusal),
}

/// Why a prompt was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// What the session says stands in the way.
    Blocked(SubmitBlock),
    /// A prompt already waits behind this turn.
    AlreadyQueued,
    /// Nothing stood in the way, and the agent still did not take it: its
    /// request channel closed as the prompt went.
    NotTaken,
}

impl Refusal {
    /// What to tell whoever sent the prompt.
    pub fn hint(&self) -> String {
        match self {
            Self::Blocked(block) => block.hint(),
            Self::AlreadyQueued => "A prompt is already waiting behind this turn".to_string(),
            Self::NotTaken => "The prompt was not accepted".to_string(),
        }
    }
}

/// What the session tells whatever watches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// New content landed.
    Appended,
    /// A turn settled.
    TurnEnded,
    /// Work the agent did by itself, between turns, settled.
    CycleEnded,
    /// The agent parked a card and waits on a person.
    AwaitingUser(UserAsk),
    /// The agent's stream ended.
    Disconnected,
    /// The agent refused a pick, put back to what it last confirmed: why.
    PickRefused(String),
    /// The agent would not stop a background item: why.
    StopRefused(String),
    /// The transcript could not be written: why. Said once, at the first of a
    /// run of failures.
    SaveFailed(String),
    /// An offloaded project's code could not go up: why.
    CodeFailed(String),
    /// The conversation's newest name could not be written, and the last
    /// one written is back: why.
    RenameFailed(String),
}

/// One thing the drive asks its front end to do, in the order given.
pub enum Effect {
    /// Tell whatever watches the session.
    Signal(Signal),
    /// The transcript changed: what is derived from it is stale.
    Changed,
    /// What the agent offers (modes, config options) changed: keep it for
    /// the next start of this agent.
    OfferChanged,
    /// Write this, and report [`Input::Saved`].
    ///
    /// A late answer is **recorded**, never dropped: the write either landed or
    /// it did not, and a failed one is handed back so the next save carries it.
    Save(Box<PendingWrite>),
    /// Report [`Input::Elapsed`] with `token` after `wait`, replacing any wait
    /// armed before. A late answer is **dropped**: a newer wait stands.
    Arm { token: u64, wait: Duration },
    /// Call off the wait armed. Its answer, if one is already on its way, is
    /// dropped by its token.
    Disarm,
    /// Send the offloaded project's code up, and report [`Input::CodeSent`]
    /// with `token`. A late answer is **recorded**: the code is up or not
    /// whatever the session has done since, and the line the session said
    /// about the send is taken back.
    SendCode { token: u64 },
    /// The code is up: send the person's prompt again, from where it waits.
    PromptAgain,
    /// Send `request` to the agent on its channel
    /// ([`Chat::agent_channel`]), in the order given.
    ///
    /// Not answered: whether the link was up was asked before this was
    /// queued, and a channel that closed as it went is the agent's stream
    /// ending, which the stream says itself.
    Request(AcpRequest),
    /// Write the conversation's name and settings, after every name written
    /// before it, and report [`Input::TitleSaved`] with `change` and `title`.
    ///
    /// A late answer is **recorded**: the write landed or it did not, and
    /// only the newest change's failure puts a name back, since a newer
    /// write carries the name on screen.
    SaveTitle {
        write: Box<PendingWrite>,
        change: u64,
        title: Option<String>,
    },
    /// News a person may want told: say where they are, as
    /// [`Input::Seen`] with `what`, read as it stands when the news is heard.
    ///
    /// A late answer is **recorded**: the news happened, and it is told as the
    /// answer says, whatever the session has done since. A carrier answers it
    /// as soon as it hears it, so none is late in practice.
    Look(Away),
    /// Say `what` outside the window: on the desktop, on a chat away from
    /// the machine, or both. Not answered.
    Tell {
        what: Away,
        desktop: bool,
        chat: bool,
    },
}

/// What the front end reports back.
pub enum Input {
    /// A [`Effect::Save`] landed, or failed.
    Saved {
        write: Box<PendingWrite>,
        result: Result<(), String>,
    },
    /// The wait armed with `token` ran out.
    Elapsed { token: u64 },
    /// The code sent for `token` is up, or why not; `still` when the prompt it
    /// went ahead of still waits, unchanged, where it was written.
    CodeSent {
        token: u64,
        result: Result<(), String>,
        still: bool,
    },
    /// The agent's stream ended.
    StreamEnded,
    /// The name written for `change`, `title`, landed, or why not.
    TitleSaved {
        change: u64,
        title: Option<String>,
        result: Result<(), String>,
    },
    /// Where the person was when the news [`Effect::Look`] asked about came.
    Seen { what: Away, attention: Attention },
    /// The person is looking at this session's conversation: its news is
    /// read.
    Shown,
}

/// Whether a batch of events may take another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// The event told a watcher something: the batch ends here.
    Stop,
    /// It may take another, within the front end's cap.
    More,
    /// It parked a card: it takes the event after it even past the cap, which
    /// is the card's answer when a host gave one.
    PastCap,
}

/// A prompt held until the agent is up.
#[derive(Debug, Clone)]
struct Held {
    text: String,
    origin: Origin,
}

/// What a batch has gathered that is said at its end.
#[derive(Debug, Default)]
struct Batch {
    changed: bool,
    quiet: bool,
    asked: Option<UserAsk>,
}

/// One session's drive. See the module header.
pub struct SessionDrive {
    chat: Chat,
    /// The token minted last.
    minted: u64,
    /// The wait armed, by its token.
    armed: Option<u64>,
    /// The code send under way, by its token.
    sending: Option<u64>,
    /// A first prompt held until the agent is up.
    held: Option<Held>,
    /// The last save failed: the next failure is not said again.
    save_failed: bool,
    /// How many saves are on their way and not answered yet.
    saving: usize,
    /// The name shown and the one last written, once a person named the
    /// conversation here.
    title: Option<crate::saving::Saving<Option<String>>>,
    batch: Batch,
    /// What the session waits on its host for, while it does.
    hold: Option<host::Hold>,
    /// A turn ended out of the person's sight and they have not looked since:
    /// the session's row says so.
    unseen: bool,
}

/// **Closing honours the saves still on their way.** The model writes what is
/// left as it goes, from the mark the newest save moved; were a save still on
/// its way to fail after that, its turn would be in neither write. So a
/// session closed with one on its way writes the whole transcript instead,
/// and the store drops the lines of any save built before that write lands.
impl Drop for SessionDrive {
    fn drop(&mut self) {
        if self.saving > 0 {
            self.chat.write_whole();
        }
    }
}

impl std::ops::Deref for SessionDrive {
    type Target = Chat;
    fn deref(&self) -> &Chat {
        &self.chat
    }
}

impl SessionDrive {
    pub fn new(chat: Chat) -> Self {
        Self {
            chat,
            minted: 0,
            armed: None,
            sending: None,
            held: None,
            save_failed: false,
            saving: 0,
            title: None,
            batch: Batch::default(),
            hold: None,
            unseen: false,
        }
    }

    fn mint(&mut self) -> u64 {
        self.minted += 1;
        self.minted
    }

    /// Send `text` with `staged`, from `origin`.
    ///
    /// | Origin | Busy | Offloaded, this machine holds the code |
    /// |---|---|---|
    /// | Person | queued, replacing what waited | the code goes first |
    /// | Remote | queued, unless a prompt already waits | sent as it is |
    /// | Tools, Run, Issue | refused: they never queue | sent as it is |
    ///
    /// Only a person's prompt waits on the code. Every other origin is kept off
    /// an offloaded project before it gets here, where projects are listed or
    /// picked, except the remote bridge's, which reaches every open session.
    pub fn prompt(
        &mut self,
        text: &str,
        staged: &[StagedAttachment],
        origin: Origin,
        code: Code,
        now: Now,
    ) -> (Intake, Vec<Effect>) {
        self.chat.tell_time(now);
        let (intake, fx) = self.intake(text, staged, origin, code);
        (intake, self.sent(fx))
    }

    fn intake(
        &mut self,
        text: &str,
        staged: &[StagedAttachment],
        origin: Origin,
        code: Code,
    ) -> (Intake, Vec<Effect>) {
        // Read before sending: a send that went clears what refused it.
        let blocker = self.chat.submit_blocker(text, staged);
        if origin == Origin::Person {
            match code {
                Code::Going => return (Intake::Waiting, Vec::new()),
                Code::MustGo { line } if blocker.is_none() => {
                    let token = self.mint();
                    self.sending = Some(token);
                    self.chat.say_host_wait(line);
                    return (
                        Intake::CodeFirst,
                        vec![
                            Effect::Signal(Signal::Appended),
                            Effect::Changed,
                            Effect::SendCode { token },
                        ],
                    );
                }
                Code::Here | Code::MustGo { .. } => {}
            }
        }
        if blocker.is_none() {
            if self.chat.submit(text, staged) {
                return (Intake::Sent, vec![Effect::Changed]);
            }
            return (Intake::Refused(Refusal::NotTaken), Vec::new());
        }
        let queues = matches!(origin, Origin::Person | Origin::Remote);
        if queues && blocker == Some(SubmitBlock::Busy) {
            if origin == Origin::Remote && self.chat.queued.is_some() {
                return (Intake::Refused(Refusal::AlreadyQueued), Vec::new());
            }
            if self.chat.queue(text, staged) {
                return (Intake::Queued, Vec::new());
            }
        }
        let why = blocker.map_or(Refusal::NotTaken, Refusal::Blocked);
        (Intake::Refused(why), Vec::new())
    }

    /// Send `text` from `origin` once the agent is up: at once when it already
    /// is, dropped if it never comes up.
    ///
    /// Issue work's prompt goes only to a session nobody has prompted yet: a
    /// person who wrote first is driving it.
    pub fn prompt_when_up(
        &mut self,
        text: &str,
        origin: Origin,
        now: Now,
    ) -> (Intake, Vec<Effect>) {
        self.chat.tell_time(now);
        match self.chat.link {
            Link::Connected => self.prompt_held(
                Held {
                    text: text.to_string(),
                    origin,
                },
                now,
            ),
            Link::Connecting => {
                self.held = Some(Held {
                    text: text.to_string(),
                    origin,
                });
                (Intake::Held, Vec::new())
            }
            Link::Lost => (
                Intake::Refused(Refusal::Blocked(SubmitBlock::NotConnected)),
                Vec::new(),
            ),
        }
    }

    fn prompt_held(&mut self, held: Held, now: Now) -> (Intake, Vec<Effect>) {
        let untouched =
            !self.chat.busy && self.chat.prompts_sent == 0 && self.chat.queued.is_none();
        if held.origin == Origin::Issue && !untouched {
            return (
                Intake::Refused(Refusal::Blocked(SubmitBlock::Busy)),
                Vec::new(),
            );
        }
        self.prompt(&held.text, &[], held.origin, Code::Here, now)
    }

    /// Send the last prompt again, after a turn that failed, as a person's.
    ///
    /// Never queued: an error can be said before the turn it ends has
    /// closed, and a resend queued then would replace a prompt already
    /// waiting, which the person wrote and did not take back.
    pub fn resend(&mut self, now: Now) -> (Intake, Vec<Effect>) {
        self.chat.tell_time(now);
        let Some(text) = self.chat.resend_text().map(str::to_string) else {
            return (Intake::Refused(Refusal::NotTaken), Vec::new());
        };
        if self.chat.busy {
            return (
                Intake::Refused(Refusal::Blocked(SubmitBlock::Busy)),
                Vec::new(),
            );
        }
        self.prompt(&text, &[], Origin::Person, Code::Here, now)
    }

    /// Take the queued prompt back, for a front end putting it back where it
    /// was written.
    pub fn unqueue(&mut self) -> Option<QueuedPrompt> {
        self.chat.unqueue()
    }

    /// Stop the running turn.
    pub fn stop(&mut self) -> Vec<Effect> {
        self.chat.cancel_turn();
        self.sent(Vec::new())
    }

    /// `fx`, then what the model queued for the agent meanwhile.
    pub(crate) fn sent(&mut self, mut fx: Vec<Effect>) -> Vec<Effect> {
        fx.extend(self.chat.take_requests().into_iter().map(Effect::Request));
        fx
    }

    /// Fold one event in.
    pub fn apply(&mut self, event: AcpEvent, now: Now) -> (Vec<Effect>, Next) {
        self.chat.tell_time(now);
        let outcome = self.chat.apply(event);
        let mut fx = vec![Effect::Signal(Signal::Appended)];
        // The turn's save is built before what waited behind it goes: built
        // after, it would write the next prompt as a turn still running,
        // and the file, written only at its end, would never hear when that
        // turn finished.
        let turn_save = outcome.turn_ended.then(|| self.save()).flatten();
        // Last of a turn's end, so a prompt written mid-turn opens its own
        // turn against a conversation that has finished closing the last.
        if outcome.turn_ended {
            if let Some(QueuedPrompt { text, attachments }) = self.chat.unqueue() {
                // A failed send leaves the prompt queued rather than dropping
                // it: the adapter dying is not the person's cue to retype it.
                if !self.chat.submit(&text, &attachments) {
                    self.chat.queued = Some(QueuedPrompt { text, attachments });
                }
            }
        }
        if let Some(held) = self.held.take_if(|_| self.chat.link != Link::Connecting) {
            if self.chat.link == Link::Connected {
                fx.extend(self.prompt_held(held, now).1);
            }
        }
        self.batch.changed |= outcome.transcript_changed;
        self.batch.quiet |= self.chat.starts_quiet_save(&outcome);
        // A cycle ending inside a person's turn is that turn's, and the turn's
        // end saves it; one that wrote nothing, or nothing settled, says
        // nothing.
        let cycle_ended = outcome.cycle_ended && self.chat.unsaved_between_turns();
        let told = outcome.turn_ended
            || cycle_ended
            || outcome.pick_refused.is_some()
            || outcome.stop_refused.is_some();
        // A card is said once the events after it are in, as a card the host
        // answered comes with its answer and waits on nobody; but before
        // anything said of this event, as it was asked first.
        self.batch.asked = outcome.asked_user.or(self.batch.asked);
        if let Some(ask) = self
            .batch
            .asked
            .take_if(|_| told)
            .filter(|_| self.chat.awaiting_permission())
        {
            self.say(&mut fx, Signal::AwaitingUser(ask));
        }
        if outcome.turn_ended {
            fx.extend(turn_save);
        } else if cycle_ended {
            fx.extend(self.save());
        }
        if outcome.turn_ended {
            self.say(&mut fx, Signal::TurnEnded);
        }
        if cycle_ended {
            self.say(&mut fx, Signal::CycleEnded);
        }
        if let Some(why) = outcome.stop_refused {
            fx.push(Effect::Signal(Signal::StopRefused(why)));
        }
        if let Some(why) = outcome.pick_refused {
            fx.push(Effect::Signal(Signal::PickRefused(why)));
        }
        if outcome.offer_changed {
            fx.push(Effect::OfferChanged);
        }
        let next = match (told, outcome.asked_user.is_some()) {
            (true, _) => Next::Stop,
            (false, true) => Next::PastCap,
            (false, false) => Next::More,
        };
        (self.sent(fx), next)
    }

    /// Close the batch [`Self::apply`] has been folding.
    pub fn end_batch(&mut self) -> Vec<Effect> {
        let batch = std::mem::take(&mut self.batch);
        let mut fx = Vec::new();
        if let Some(ask) = batch.asked.filter(|_| self.chat.awaiting_permission()) {
            self.say(&mut fx, Signal::AwaitingUser(ask));
        }
        if batch.changed {
            fx.push(Effect::Changed);
        }
        // Every batch restarts the wait: a silence is counted from the agent's
        // last word.
        if self.chat.busy {
            fx.extend(self.disarm());
        } else if let Some(wait) = self.chat.own_cycle_wait() {
            fx.push(self.arm(wait));
        } else if batch.quiet {
            fx.push(self.arm(QUIET_SAVE));
        }
        fx
    }

    /// What the front end reports back.
    pub fn handle(&mut self, input: Input, now: Now) -> Vec<Effect> {
        self.chat.tell_time(now);
        match input {
            Input::Saved { write, result } => self.saved(&write, result),
            Input::Elapsed { token } => self.elapsed(token),
            Input::CodeSent {
                token,
                result,
                still,
            } => self.code_sent(token, result, still),
            Input::StreamEnded => {
                self.chat.stream_ended();
                let mut fx = self.disarm();
                self.say(&mut fx, Signal::Disconnected);
                fx
            }
            Input::TitleSaved {
                change,
                title,
                result,
            } => self.title_saved(change, title, result),
            Input::Seen { what, attention } => self.seen(what, attention),
            Input::Shown => self.shown(),
        }
    }

    /// Write what is not on disk yet, if anything: `None` while the session
    /// has nowhere to write, no id, or nothing new, which keeps a session
    /// nobody used out of the list of conversations that were had.
    ///
    /// The transcript is written at the end of *every* turn rather than only
    /// when the session is dropped, so a crash, or any exit that runs no
    /// drops, costs the turn in flight instead of the whole conversation.
    /// Building the write moves the conversation's mark, so two saves in
    /// flight at once cannot both carry the same turn; the writing itself is
    /// the front end's, off its UI thread.
    fn save(&mut self) -> Option<Effect> {
        let write = self.chat.flush()?;
        self.saving += 1;
        Some(Effect::Save(Box::new(write)))
    }

    /// Write the conversation's name and settings alone: a rename, which can
    /// come mid-turn, when a line written would be a step not yet finished.
    pub fn save_meta(&mut self) -> Vec<Effect> {
        let Some(write) = self.chat.flush_meta() else {
            return Vec::new();
        };
        self.saving += 1;
        vec![Effect::Save(Box::new(write))]
    }

    fn saved(&mut self, write: &PendingWrite, result: Result<(), String>) -> Vec<Effect> {
        self.saving = self.saving.saturating_sub(1);
        match result {
            Ok(()) => {
                self.save_failed = false;
                Vec::new()
            }
            Err(why) => {
                // The mark moved when the save was built; handed back, the
                // next save carries what this one could not.
                self.chat.unflush(write);
                // Only the edge. A directory gone read-only fails at the end of
                // every turn, and one message per turn is how the first is lost.
                let first = !self.save_failed;
                self.save_failed = true;
                match first {
                    true => vec![Effect::Signal(Signal::SaveFailed(why))],
                    false => Vec::new(),
                }
            }
        }
    }

    fn arm(&mut self, wait: Duration) -> Effect {
        let token = self.mint();
        self.armed = Some(token);
        Effect::Arm { token, wait }
    }

    fn disarm(&mut self) -> Vec<Effect> {
        match self.armed.take() {
            Some(_) => vec![Effect::Disarm],
            None => Vec::new(),
        }
    }

    /// The quiet spell ran out: save what the agent wrote between turns if any
    /// of it is unsaved and settled, end the cycle it ran by itself, and look
    /// again after its wait when a step it left in flight holds the cycle.
    fn elapsed(&mut self, token: u64) -> Vec<Effect> {
        if self.armed != Some(token) {
            return Vec::new();
        }
        self.armed = None;
        let mut fx = Vec::new();
        if self.chat.unsaved_between_turns() {
            fx.extend(self.save());
            self.say(&mut fx, Signal::CycleEnded);
        }
        if self.chat.quiet_spell() {
            fx.push(Effect::Changed);
        }
        if let Some(wait) = self.chat.own_cycle_wait() {
            fx.push(self.arm(wait));
        }
        fx
    }

    fn code_sent(&mut self, token: u64, result: Result<(), String>, still: bool) -> Vec<Effect> {
        self.chat.unsay_host_wait();
        let mut fx = vec![Effect::Signal(Signal::Appended), Effect::Changed];
        if self.sending.take_if(|t| *t == token).is_none() {
            return fx;
        }
        match result {
            Ok(()) if still => fx.push(Effect::PromptAgain),
            Ok(()) => {}
            Err(why) => fx.push(Effect::Signal(Signal::CodeFailed(why))),
        }
        fx
    }
}

mod cards;
mod host;
pub use host::HostLink;
mod life;
mod settings;
mod telling;
mod titles;

#[cfg(test)]
mod tests;
