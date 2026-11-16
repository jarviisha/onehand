//! What happens next for onehand's tools: who may call them, what a session's
//! tools opened, and a worker's open from the call to its first prompt.
//!
//! **Decided here, carried out elsewhere.** A [`ControlDrive`] keeps every
//! token handed to an agent, each session's switch and set of tools, the
//! labels its issues get, the sessions its tools opened, the folders workers
//! hold, every lead's requests and each worker's lead. It is told what the
//! world reports and answers with [`Effect`]s; it has no thread, timer or
//! I/O. The socket asks it each connection's standing on every message.
//!
//! **A worker's open, step by step.** [`ControlDrive::open_worker`] checks
//! the call and its request id, and names the folder to resolve off the UI
//! thread. Once resolved ([`Input::Resolved`]) the drive refuses a
//! folder another session or a process works in, or a lead over its cap, and
//! reserves the folder and asks for the session ([`Effect::Start`]). Once
//! started ([`Input::Started`]) every change of the session wakes it
//! ([`Input::Woke`]): the mode and the model go in one pick at a time,
//! each answered, then the first prompt ([`Effect::Begin`]), and only then is
//! the relation written and the open answered ([`Input::Begun`]).
//!
//! **The lead is asked about at every step.** A lead closed or switched off
//! abandons its opens at once ([`ControlDrive::forget`],
//! [`ControlDrive::switch`]), and a wake finding it gone fails the open: its
//! session closed, its first prompt never sent, no relation written.
//!
//! **A failure leaves nothing behind and touches nothing else**: past the
//! deadline, an agent that never came up, a pick refused, a prompt not taken.
//! Only the open's own session is closed ([`Effect::Drop`]), its project with
//! it when the open added it; its folder, its place under the cap and its
//! request id are freed.

use super::kept::{Kept, KeptWorker};
use super::workers::{Asked, Relation, Requests, Reservations, Resolved, Target, Worker};
use super::{Peer, Standing};
use crate::chat::drive::Intake;
use crate::chat::Chat;
use crate::task::Grant;
use crate::workflow::Pick;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

/// How many sessions one session's tools may have open at once.
const OPENED_CAP: usize = 4;

/// How long a worker has to come up, from the call to its first prompt
/// accepted: the agent's start, its handshake, and every pick answered.
pub const OPEN_DEADLINE: Duration = Duration::from_secs(120);

/// An open in flight, by its lead and request id.
pub type Key = (u64, String);

/// What `open_worker` was asked, by the session `lead`.
#[derive(Debug, Clone)]
pub struct Open {
    pub lead: u64,
    pub project: String,
    pub agent: String,
    pub prompt: Option<String>,
    pub dirs: Vec<PathBuf>,
    pub request: String,
    pub turn: Option<u64>,
    pub model: Option<String>,
    pub name: Option<String>,
    /// A worker coming back with its lead's conversation, as it was kept.
    pub back: Option<KeptWorker>,
}

impl Open {
    fn asked(&self) -> Asked {
        Asked {
            project: self.project.clone(),
            agent: self.agent.clone(),
            prompt: self.prompt.clone(),
            dirs: self.dirs.clone(),
            turn: self.turn,
            model: self.model.clone(),
            name: self.name.clone(),
        }
    }
}

/// What an `open_worker` call came to at once.
pub enum Begun {
    /// Answered now, with this.
    Said(String),
    /// Resolve `target` and the further folders off the UI thread
    /// (`workers::resolve_blocking`), and report [`Input::Resolved`] for
    /// `key`; and carry `fx` now, the open's deadline armed at the call. A
    /// late answer for an open already failed is **dropped**: nothing was
    /// reserved or started for it.
    Resolve {
        key: Key,
        target: Target,
        fx: Vec<Effect>,
    },
}

/// What the world holds when an open's folder is resolved.
pub struct World {
    /// The lead is still a session of some window.
    pub lead_here: bool,
    /// Every window's project roots, with how many sessions are open on each.
    pub counts: Vec<(PathBuf, usize)>,
    /// Every session open in any window.
    pub live: Vec<u64>,
    /// The sessions the lead's tools opened that are still open, with the
    /// project each is on.
    pub opened: Vec<(u64, PathBuf)>,
}

/// What the person's view of a worker's first moments needs to answer the
/// open, read once its first prompt went.
pub struct Facts {
    /// What its first prompt came to, when it had one.
    pub intake: Option<Intake>,
    /// The worker's prompts sent.
    pub sent: usize,
    /// The mode and model in force, as an answer says them.
    pub mode: String,
    pub model: String,
    /// Its name as the rail shows it.
    pub name: String,
}

/// What the world reports back.
pub enum Input<'a> {
    /// Open `key`'s folder was read, with the world as it stands.
    Resolved {
        key: Key,
        resolved: Result<Resolved, String>,
        world: World,
    },
    /// Open `key`'s session started as this uid, or why not.
    Started {
        key: Key,
        started: Result<u64, String>,
    },
    /// Worker `uid`'s session changed, or is gone (`chat` is `None`);
    /// `lead_here` says whether its lead is still a session of some window.
    Woke {
        uid: u64,
        chat: Option<&'a Chat>,
        lead_here: bool,
    },
    /// Worker `uid`'s first prompt went as `facts` say.
    Begun { uid: u64, facts: Facts },
    /// The wait armed with `token` ran out.
    Elapsed { token: u64 },
    /// Session `uid`'s conversation is `session_id`, and it has sent
    /// `prompts_sent` prompts: said when either changes.
    Conversation {
        uid: u64,
        session_id: String,
        prompts_sent: usize,
    },
}

/// What the front end carries out.
#[derive(Debug)]
pub enum Effect {
    /// Tell every connection of session `uid`'s agent that its tools changed.
    ToolsChanged(u64),
    /// Start the worker's session on `root` with `dirs`, off screen, and
    /// name it for `folder` (or `name`); report [`Input::Started`]. A late
    /// answer is **cleaned up**: a session started for an open that failed
    /// meanwhile is closed ([`Effect::Drop`]).
    Start {
        key: Key,
        root: PathBuf,
        dirs: Vec<PathBuf>,
        folder: String,
        name: Option<String>,
        /// A worker coming back: its session goes on this conversation, by
        /// its session id, and connects only once shown or called on.
        resume: Option<String>,
    },
    /// Report [`Input::Woke`] for `uid` whenever its session changes, and
    /// once now. A late report, for a worker no open nor turn holds any more, is
    /// **dropped**: the drive answers it by unwatching.
    Watch(u64),
    /// Stop watching `uid`.
    Unwatch(u64),
    /// Put `pick` to worker `uid`'s agent. Its answer is the session's
    /// change, reported as [`Input::Woke`]; one for an open already over is
    /// **dropped** with it.
    Pick { uid: u64, pick: Pick },
    /// Send worker `uid` its first prompt, if it has one, from onehand's
    /// tools, then report [`Input::Begun`] in the same pass. A late answer,
    /// for an open that failed meanwhile, is **dropped**: the session it went
    /// to is being closed.
    Begin { uid: u64, prompt: Option<String> },
    /// Report [`Input::Elapsed`] with `token` after `wait`. A late
    /// answer is **dropped** by its token: the open or turn it timed is over.
    Arm { token: u64, wait: Duration },
    /// Close worker `uid`'s session, open `key`'s, and its project when the
    /// open added it and nothing else stands on it.
    Drop {
        key: Key,
        uid: u64,
        root: PathBuf,
        added: bool,
    },
    /// Answer the open `key` with `outcome`.
    Answer {
        key: Key,
        outcome: Result<String, String>,
    },
    /// What the person's view reads of the workers changed.
    Changed,
    /// Write `kept` beside lead `lead`'s conversation, replacing what was
    /// there, off the UI thread. A lead with no conversation yet writes
    /// nothing. Late answers do not exist: a later `Keep` for the same lead
    /// is the whole of it again, and the last asked for is what stays.
    Keep { lead: u64, kept: Kept },
    /// Send worker `uid` the lead's turn `turn`, `text`, from onehand's
    /// tools, as a turn without a model goes, then tell the drive it was
    /// sent (`ControlDrive::sent`), and answer the turn's call with what
    /// that says. A session gone meanwhile answers that the worker is
    /// closed.
    Send { uid: u64, turn: u64, text: String },
    /// Answer the turn worker `uid` was given with `outcome`. One for a call
    /// already answered, or never asked, is **dropped**.
    TurnAnswer {
        uid: u64,
        outcome: Result<String, String>,
    },
}

/// Where an open in flight stands.
enum Phase {
    /// Its folder is being read.
    Resolving,
    /// Its folder is reserved and its session asked for.
    Starting {
        folder: PathBuf,
        added: bool,
        root: PathBuf,
        dirs: Vec<PathBuf>,
    },
    /// Its session is up or coming up.
    Up {
        uid: u64,
        root: PathBuf,
        added: bool,
        dirs: Vec<PathBuf>,
        picked: Vec<String>,
        /// The handle minted for it as it started.
        handle: String,
    },
}

struct Pending {
    open: Open,
    mode: Option<String>,
    phase: Phase,
    /// The deadline's wait, armed at the call.
    token: u64,
}

/// A worker of the caller's, found by its handle.
pub enum Found {
    /// Its session is open, `uid`.
    Open { uid: u64 },
    /// Its session is gone.
    Closed,
}

/// A worker gone, as its handle still reads to the lead it had.
struct Closed {
    lead: u64,
    /// Why it did not come back, when that is what ended it.
    why: Option<String>,
}

/// onehand's tools' drive. See the module header.
#[derive(Default)]
pub struct ControlDrive {
    /// Each token handed to an agent, and the session holding it once one
    /// does.
    tokens: HashMap<String, Option<u64>>,
    /// A program outside onehand may use the tools.
    outside: bool,
    on: HashSet<u64>,
    /// The tools each session not switched on may use: its schedule's.
    only: HashMap<u64, Vec<String>>,
    /// The labels an issue each session's agent writes gets.
    labels: HashMap<u64, Vec<String>>,
    /// The sessions each session's tools opened.
    opened: HashMap<u64, Vec<u64>>,
    reserved: Reservations,
    requests: Requests,
    /// Each worker's lead, by the worker's uid.
    workers: HashMap<u64, Relation>,
    /// The uid each worker's handle names while its session is open, from
    /// the moment it starts.
    handles: HashMap<String, u64>,
    /// What each closed worker's handle still answers, by the handle.
    closed_workers: HashMap<String, Closed>,
    /// The conversation each session was last said to be on.
    conversations: HashMap<u64, String>,
    pending: HashMap<Key, Pending>,
    /// Each worker's turn waiting on its model's pick.
    turning: HashMap<u64, turning::Turning>,
    minted: u64,
}

impl ControlDrive {
    // Tokens and standing.

    /// A new token, known and held by nobody yet: its agent is connected,
    /// offered nothing, and told when a session claims it and is switched on.
    pub fn mint(&mut self) -> String {
        let token = super::new_token();
        self.tokens.insert(token.clone(), None);
        token
    }

    /// Session `uid` holds `token` from now on.
    pub fn claim(&mut self, token: &str, uid: u64) {
        self.tokens.insert(token.to_string(), Some(uid));
    }

    /// `token` went with whatever held it.
    pub fn release(&mut self, token: &str) {
        self.tokens.remove(token);
    }

    /// The session holding `token`, once one does.
    pub fn holder(&self, token: &str) -> Option<u64> {
        self.tokens.get(token).copied().flatten()
    }

    /// The tokens session `uid` holds.
    pub fn tokens_of(&self, uid: u64) -> Vec<String> {
        self.tokens
            .iter()
            .filter(|(_, held)| **held == Some(uid))
            .map(|(token, _)| token.clone())
            .collect()
    }

    /// What a connection may do now: read on every message it sends.
    pub fn standing(&self, peer: &Peer) -> Standing {
        match peer {
            Peer::Outside if self.outside => Standing::On,
            Peer::Outside => Standing::Refused(
                "outside control is off: switch it on in Settings ▸ Connections".to_string(),
            ),
            Peer::Session(token) => match self.tokens.get(token) {
                None => Standing::Refused("no such session".to_string()),
                Some(Some(uid)) if self.on.contains(uid) => Standing::On,
                Some(Some(uid)) if self.only.contains_key(uid) => {
                    Standing::Only(self.only[uid].clone())
                }
                // Not claimed yet (a warm adapter's), or switched off.
                Some(_) => Standing::Off,
            },
        }
    }

    /// Whether a program outside onehand may use the tools.
    pub fn outside(&self) -> bool {
        self.outside
    }

    /// Let a program outside onehand use the tools, or stop it, from its next
    /// message on. Whether anything changed.
    pub fn set_outside(&mut self, on: bool) -> bool {
        std::mem::replace(&mut self.outside, on) != on
    }

    // A session's switch and grant.

    /// Whether session `uid`'s agent may use every tool.
    pub fn switched_on(&self, uid: u64) -> bool {
        self.on.contains(&uid)
    }

    /// Switch session `uid`'s tools on or off. Off abandons its opens.
    pub fn switch(&mut self, uid: u64, on: bool) -> Vec<Effect> {
        let changed = match on {
            true => self.on.insert(uid),
            false => self.on.remove(&uid),
        };
        let mut fx = Vec::new();
        if changed {
            fx.push(Effect::ToolsChanged(uid));
            if !on && !self.only_has(uid, "open_worker") {
                fx.extend(self.abandon_of(uid, false));
            }
        }
        fx
    }

    /// Give session `uid` what its task was given: the tools it may use while
    /// nobody switches it on, and the labels its issues get. A task given
    /// nothing changes nothing.
    pub fn grant(&mut self, uid: u64, grant: &Grant) -> Vec<Effect> {
        let mut fx = Vec::new();
        // An agent may have listed its tools before the session held its
        // token (a warm adapter's): told, it asks again.
        if !grant.tools.is_empty()
            && self.only.insert(uid, grant.tools.clone()).as_ref() != Some(&grant.tools)
        {
            fx.push(Effect::ToolsChanged(uid));
        }
        if !grant.labels.is_empty() {
            self.labels.insert(uid, grant.labels.clone());
        }
        fx
    }

    /// The labels an issue session `uid`'s agent writes gets.
    pub fn labels(&self, uid: u64) -> Vec<String> {
        self.labels.get(&uid).cloned().unwrap_or_default()
    }

    /// Session `uid` closed: its switch and what its tools opened go with it,
    /// its opens are abandoned, and an open whose session it was fails.
    pub fn forget(&mut self, uid: u64) -> Vec<Effect> {
        let mut fx = self.abandon_of(uid, true);
        fx.extend(self.turn_forgotten(uid));
        if let Some(key) = self.key_of(uid) {
            fx.extend(self.fail(&key, "the session was closed before it was ready".into()));
        }
        self.on.remove(&uid);
        self.opened.remove(&uid);
        self.labels.remove(&uid);
        self.only.remove(&uid);
        self.reserved.release_worker(uid);
        self.requests.forget_lead(uid);
        self.conversations.remove(&uid);
        if let Some(relation) = self.workers.remove(&uid) {
            let closed = Closed {
                lead: relation.lead,
                why: None,
            };
            self.closed_workers.insert(relation.handle, closed);
            fx.push(self.keep(relation.lead));
        }
        // Its own handle goes, and its workers' with their relations: the
        // person closing a lead lets its workers go for good, so none of
        // them comes back with its conversation.
        let mut gone = vec![uid];
        let mut ended = Vec::new();
        self.workers.retain(|&worker, relation| {
            let mine = relation.lead == uid;
            if mine {
                gone.push(worker);
                ended.push(relation.handle.clone());
            }
            !mine
        });
        self.handles.retain(|_, worker| !gone.contains(worker));
        for handle in ended {
            let why = Some("its lead was closed".to_string());
            self.closed_workers
                .insert(handle, Closed { lead: uid, why });
        }
        // What it keeps is written once, whole, last: one written on the
        // way, by an open failing, still lists workers this lets go.
        fx.retain(|effect| match effect {
            Effect::Keep { lead, .. } => *lead != uid,
            Effect::ToolsChanged(_)
            | Effect::Start { .. }
            | Effect::Watch(_)
            | Effect::Unwatch(_)
            | Effect::Pick { .. }
            | Effect::Begin { .. }
            | Effect::Arm { .. }
            | Effect::Drop { .. }
            | Effect::Answer { .. }
            | Effect::Changed
            | Effect::Send { .. }
            | Effect::TurnAnswer { .. } => true,
        });
        if self.keeps(uid) {
            fx.push(self.keep(uid));
        }
        fx
    }

    fn only_has(&self, uid: u64, tool: &str) -> bool {
        self.only
            .get(&uid)
            .is_some_and(|tools| tools.iter().any(|t| t == tool))
    }

    /// Whether `lead` may still open a worker, `here` saying it is still a
    /// session of some window.
    fn lead_allowed(&self, lead: u64, here: bool) -> bool {
        here && (self.on.contains(&lead) || self.only_has(lead, "open_worker"))
    }

    // The cap.

    /// Whether `lead`'s tools may open one more session, `open` being the
    /// sessions they opened that are still open, with their projects. Keeps
    /// only those.
    pub fn admit(&mut self, lead: u64, open: &[(u64, PathBuf)]) -> Result<(), String> {
        if open.len() >= OPENED_CAP {
            let said: Vec<String> = open
                .iter()
                .map(|(uid, at)| format!("session {uid} on {}", at.display()))
                .collect();
            return Err(format!(
                "this session's tools already have {OPENED_CAP} sessions open: {}; \
                 one has to close first",
                said.join(", ")
            ));
        }
        self.opened
            .insert(lead, open.iter().map(|(uid, _)| *uid).collect());
        Ok(())
    }

    /// `lead`'s tools opened session `uid`.
    pub fn opened_one(&mut self, lead: u64, uid: u64) {
        self.opened.entry(lead).or_default().push(uid);
    }

    /// The sessions `lead`'s tools opened.
    pub fn opened_by(&self, lead: u64) -> Vec<u64> {
        self.opened.get(&lead).cloned().unwrap_or_default()
    }

    /// What the world reports back.
    pub fn handle(&mut self, input: Input<'_>) -> Vec<Effect> {
        match input {
            Input::Resolved {
                key,
                resolved,
                world,
            } => self.resolved(&key, resolved, &world),
            Input::Started { key, started } => self.started(&key, started),
            Input::Woke {
                uid,
                chat,
                lead_here,
            } => self.woke(uid, chat, lead_here),
            Input::Begun { uid, facts } => self.begun(uid, facts),
            Input::Elapsed { token } => self.elapsed(token),
            Input::Conversation {
                uid,
                session_id,
                prompts_sent,
            } => self.conversation(uid, session_id, prompts_sent),
        }
    }
}

/// This run of the app, as handles of earlier builds named it: a handle in
/// that shape is told apart by its length, and refused. Never written
/// anywhere.
pub fn this_run() -> String {
    static RUN: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| super::new_token()[..8].to_string());
    RUN.clone()
}

mod opening;
mod relations;
mod turning;

#[cfg(test)]
mod tests;
