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

use super::open::{self, Step};
use super::state::{self as said, Held};
use super::turns::Last;
use super::workers::{
    offered, sessions_in, whose, Asked, Relation, Requests, Reservations, Resolved, Seen, Target,
    Whose, Worker,
};
use super::{Peer, Standing};
use crate::chat::drive::Intake;
use crate::chat::Chat;
use crate::task::Grant;
use crate::workflow::Pick;
use crate::worktree::occupants::Occupants;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
    },
    /// Report [`Input::Woke`] for `uid` whenever its session changes, and
    /// once now. A late report, for a worker no open holds any more, is
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
    /// answer is **dropped** by its token: the open it timed is over.
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
    /// The lead each closed worker had, so it alone reads it as closed.
    closed_workers: HashMap<u64, u64>,
    pending: HashMap<Key, Pending>,
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
                fx.extend(self.abandon_of(uid));
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
        let mut fx = self.abandon_of(uid);
        if let Some(key) = self.key_of(uid) {
            fx.extend(self.fail(&key, "the session was closed before it was ready".into()));
        }
        self.on.remove(&uid);
        self.opened.remove(&uid);
        self.labels.remove(&uid);
        self.only.remove(&uid);
        self.reserved.release_worker(uid);
        self.requests.forget_lead(uid);
        if let Some(relation) = self.workers.remove(&uid) {
            self.closed_workers.insert(uid, relation.lead);
        }
        self.workers.retain(|_, relation| relation.lead != uid);
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

    // What the person's view reads.

    /// The lead of session `uid`, while that is a worker: from the moment its
    /// session starts, not only once its relation is written.
    pub fn lead_of(&self, uid: u64) -> Option<u64> {
        self.workers
            .get(&uid)
            .map(|relation| relation.lead)
            .or_else(|| self.requests.lead_opening(uid))
    }

    /// Lead `lead`'s workers, opening or open, oldest first.
    pub fn workers_of(&self, lead: u64) -> Vec<Worker> {
        let open = self
            .workers
            .iter()
            .filter(|(_, relation)| relation.lead == lead)
            .map(|(&uid, relation)| Worker {
                uid,
                turn: relation.turn,
                sent: Some(relation.sent),
            });
        let opening = self
            .requests
            .opening_of(lead)
            .into_iter()
            .filter(|uid| !self.workers.contains_key(uid))
            .map(|uid| Worker {
                uid,
                turn: None,
                sent: None,
            });
        let mut workers: Vec<Worker> = open.chain(opening).collect();
        workers.sort_by_key(|worker| worker.uid);
        workers
    }

    /// Whether *Give back to the lead* is offered for worker `uid`, which has
    /// sent `prompts_sent` prompts, `lead_open` saying its lead is still a
    /// session of some window.
    pub fn give_back_offered(&self, uid: u64, prompts_sent: usize, lead_open: bool) -> bool {
        let relation = self.workers.get(&uid);
        let on = relation.is_some_and(|relation| self.on.contains(&relation.lead));
        offered(relation, prompts_sent, on, lead_open)
    }

    /// Give worker `uid` back to its lead: what the person sent so far counts
    /// as the lead's. False when it is no longer offered.
    pub fn give_back(&mut self, uid: u64, prompts_sent: usize, lead_open: bool) -> bool {
        if !self.give_back_offered(uid, prompts_sent, lead_open) {
            return false;
        }
        if let Some(relation) = self.workers.get_mut(&uid) {
            relation.sent = prompts_sent;
        }
        true
    }

    // A lead's calls on its workers.

    /// The caller's worker `handle` names, `open` saying whether a window
    /// holds its session; anything that is not the caller's own worker, open
    /// or closed, gets the same refusal.
    pub fn find(&self, caller: u64, uid: u64, handle: &str, open: bool) -> Result<Found, String> {
        let relation = self.workers.get(&uid);
        let closed = self.closed_workers.get(&uid).copied();
        match whose(relation, closed, caller, open) {
            Whose::Mine if open => Ok(Found::Open { uid }),
            Whose::Mine | Whose::Closed => Ok(Found::Closed),
            Whose::NotYours => Err(format!("{handle} is not a worker you opened")),
        }
    }

    /// What the lead last sent worker `uid`, for `turns`' rules.
    pub fn last(&self, uid: u64) -> Option<Last> {
        self.workers.get(&uid).map(|relation| Last {
            turn: relation.turn,
            text: relation.text.clone(),
            sent: relation.sent,
        })
    }

    /// What `session_state` reads of worker `uid`'s relation.
    pub fn held(&self, uid: u64) -> Option<Held> {
        self.workers.get(&uid).map(|relation| Held {
            turn: relation.turn,
            sent: relation.sent,
            dirs: relation.dirs.clone(),
        })
    }

    /// The lead's turn `turn`, `text`, was handed to worker `uid` and came
    /// to `intake`, the worker having sent `sent` prompts since: what the
    /// lead is answered. Only a prompt the agent took moves the relation on.
    pub fn sent(
        &mut self,
        uid: u64,
        turn: u64,
        text: &str,
        intake: &Intake,
        sent: usize,
    ) -> Result<String, String> {
        if *intake != Intake::Sent {
            return Err("the prompt was not accepted".to_string());
        }
        if let Some(relation) = self.workers.get_mut(&uid) {
            relation.turn = Some(turn);
            relation.text = Some(text.to_string());
            relation.sent = sent;
        }
        Ok(format!("turn {turn} accepted"))
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

    // A worker's open.

    /// Begin `open`, the window's projects being `projects` and `mode` the
    /// one its agent is put in.
    pub fn open_worker(
        &mut self,
        open: Open,
        projects: &[(String, PathBuf)],
        mode: Option<String>,
    ) -> Result<Begun, String> {
        match (&open.prompt, open.turn) {
            (Some(_), None) => return Err("`turn` is required with a prompt".to_string()),
            (None, Some(_)) => return Err("`turn` goes with a prompt; there is none".to_string()),
            _ => {}
        }
        let key = (open.lead, open.request.clone());
        match self.requests.see(open.lead, &open.request, &open.asked())? {
            Seen::New => {}
            Seen::Opening(uid) => {
                return Ok(Begun::Said(match uid {
                    Some(uid) => format!(
                        "{} is still opening; call again with the same request",
                        open::handle(&this_run(), uid)
                    ),
                    None => {
                        "the session is still opening; call again with the same request".to_string()
                    }
                }))
            }
            Seen::Answered(answer) => return Ok(Begun::Said(answer)),
        }
        let target = match super::pick_project(projects, &open.project) {
            Ok(dir) => Target::Project(dir.to_path_buf()),
            Err(_) if Path::new(&open.project).is_absolute() => {
                Target::Worktree(PathBuf::from(&open.project))
            }
            Err(why) => {
                self.requests.failed(open.lead, &open.request);
                return Err(why);
            }
        };
        self.minted += 1;
        let token = self.minted;
        self.pending.insert(
            key.clone(),
            Pending {
                open,
                mode,
                phase: Phase::Resolving,
                token,
            },
        );
        Ok(Begun::Resolve {
            key,
            target,
            fx: vec![Effect::Arm {
                token,
                wait: OPEN_DEADLINE,
            }],
        })
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
        }
    }

    /// Open `key`'s folder was read, as `resolved` says, with `world` as it
    /// stands: refuse it, or reserve it and start.
    fn resolved(
        &mut self,
        key: &Key,
        resolved: Result<Resolved, String>,
        world: &World,
    ) -> Vec<Effect> {
        let Some(pending) = self.pending.get(key) else {
            return Vec::new();
        };
        let lead = pending.open.lead;
        let resolved = match resolved {
            Ok(resolved) => resolved,
            Err(why) => return self.fail(key, why),
        };
        // The lead may have gone while the folder was read: nothing is
        // started for nobody, nor counted under a cap it no longer has.
        if !self.lead_allowed(lead, world.lead_here) {
            return self.fail(key, "the lead went before its worker was ready".into());
        }
        let roots: Vec<(PathBuf, PathBuf, usize)> = resolved
            .canonical
            .iter()
            .map(|(path, real)| {
                let count = world
                    .counts
                    .iter()
                    .filter(|(root, _)| root == path)
                    .map(|(_, count)| count)
                    .sum();
                (path.clone(), real.clone(), count)
            })
            .collect();
        if let Some(root) = sessions_in(&resolved.folder, &roots).first() {
            let why = format!(
                "{} already has a session open on it; a worker never shares its folder",
                root.display()
            );
            return self.fail(key, why);
        }
        // A window closing ends its sessions without closing them one by one.
        self.reserved.prune(|uid| world.live.contains(&uid));
        if let Err(why) = self.reserved.take(&resolved.folder) {
            return self.fail(key, why);
        }
        let refused = match &resolved.occupants {
            Occupants::None => None,
            Occupants::Found(found) => Some(format!(
                "{} processes work in {}: {}; the person has to stop them before a worker opens there",
                found.len(),
                resolved.folder.display(),
                found.join(", ")
            )),
            Occupants::Unknown(unread) => Some(format!(
                "could not tell whether anything works in {}: {}",
                resolved.folder.display(),
                unread.join(", ")
            )),
        };
        if let Some(why) = refused {
            self.reserved.release(&resolved.folder);
            return self.fail(key, why);
        }
        if let Err(why) = self.admit(lead, &world.opened) {
            self.reserved.release(&resolved.folder);
            return self.fail(key, why);
        }
        // Named for what it works on, else for its folder, never for its
        // prompt, which is the lead's instructions.
        let folder = resolved.root.file_name().map_or_else(
            || resolved.root.display().to_string(),
            |last| last.to_string_lossy().into_owned(),
        );
        let added = resolved.same.is_empty();
        let pending = self.pending.get_mut(key).expect("looked up above");
        let name = pending.open.name.clone();
        pending.phase = Phase::Starting {
            folder: resolved.folder,
            added,
            root: resolved.root.clone(),
            dirs: resolved.dirs.clone(),
        };
        vec![Effect::Start {
            key: key.clone(),
            root: resolved.root,
            dirs: resolved.dirs,
            folder,
            name,
        }]
    }

    /// Open `key`'s session started as `uid`, or why not.
    fn started(&mut self, key: &Key, started: Result<u64, String>) -> Vec<Effect> {
        let Some(pending) = self.pending.get_mut(key) else {
            // Failed while it started: what it started goes.
            return match started {
                Ok(uid) => vec![Effect::Drop {
                    key: key.clone(),
                    uid,
                    root: PathBuf::new(),
                    added: false,
                }],
                Err(_) => Vec::new(),
            };
        };
        let Phase::Starting {
            folder,
            added,
            root,
            dirs,
        } = std::mem::replace(&mut pending.phase, Phase::Resolving)
        else {
            return Vec::new();
        };
        let lead = pending.open.lead;
        let uid = match started {
            Ok(uid) => uid,
            Err(why) => {
                self.reserved.release(&folder);
                return self.fail(key, why);
            }
        };
        self.reserved.bind(&folder, uid);
        self.opened_one(lead, uid);
        self.requests.started(lead, &key.1, uid);
        let pending = self.pending.get_mut(key).expect("looked up above");
        pending.phase = Phase::Up {
            uid,
            root,
            added,
            dirs,
            picked: Vec::new(),
        };
        vec![Effect::Changed, Effect::Watch(uid)]
    }

    /// Worker `uid`'s session changed, or is gone (`chat` is `None`);
    /// `lead_here` says whether its lead is still a session of some window.
    fn woke(&mut self, uid: u64, chat: Option<&Chat>, lead_here: bool) -> Vec<Effect> {
        let Some(key) = self.key_of(uid) else {
            return vec![Effect::Unwatch(uid)];
        };
        let pending = &self.pending[&key];
        if !self.lead_allowed(pending.open.lead, lead_here) {
            return self.fail(&key, "the lead went before its worker was ready".into());
        }
        let Some(chat) = chat else {
            return self.fail(&key, "the session was closed before it was ready".into());
        };
        let picked = match &pending.phase {
            Phase::Up { picked, .. } => picked,
            Phase::Resolving | Phase::Starting { .. } => return Vec::new(),
        };
        let next = open::step(
            chat,
            pending.mode.as_deref(),
            pending.open.model.as_deref(),
            picked,
        );
        match next {
            Step::Wait => Vec::new(),
            Step::Refused(why) => self.fail(&key, why),
            Step::Ready => vec![Effect::Begin {
                uid,
                prompt: pending.open.prompt.clone(),
            }],
            Step::Pick(pick) => {
                match self.pending.get_mut(&key).map(|pending| &mut pending.phase) {
                    Some(Phase::Up { picked, .. }) => picked.push(pick.group().to_string()),
                    Some(Phase::Resolving | Phase::Starting { .. }) | None => {}
                }
                vec![Effect::Pick { uid, pick }]
            }
        }
    }

    /// Worker `uid`'s first prompt went as `facts` say: write the relation and
    /// answer, or fail.
    fn begun(&mut self, uid: u64, facts: Facts) -> Vec<Effect> {
        let Some(key) = self.key_of(uid) else {
            return Vec::new();
        };
        match facts.intake {
            None | Some(Intake::Sent) => {}
            Some(Intake::Refused(why)) => return self.fail(&key, why.hint()),
            Some(Intake::Queued | Intake::Held | Intake::Waiting | Intake::CodeFirst) => {
                return self.fail(&key, "the prompt was not accepted".into())
            }
        }
        let Some(Pending {
            open,
            phase: Phase::Up { root, dirs, .. },
            ..
        }) = self.pending.remove(&key)
        else {
            return Vec::new();
        };
        let mut lines = vec![
            format!("session: {}", open::handle(&this_run(), uid)),
            format!("name: {}", facts.name),
            format!("project: {}", root.display()),
            format!("mode: {}", facts.mode),
            format!("model: {}", facts.model),
            format!("dirs: {}", said::dirs_said(&dirs)),
        ];
        if let Some(turn) = open.turn {
            lines.push(format!("turn: {turn} accepted"));
        }
        let answer = lines.join("\n");
        // Written last, once the prompt went, so a lead forgotten in between
        // can never have one made again after it.
        self.workers.insert(
            uid,
            Relation {
                lead: open.lead,
                turn: open.turn,
                text: open.prompt,
                sent: facts.sent,
                dirs,
            },
        );
        self.requests
            .answered(open.lead, &open.request, answer.clone());
        vec![
            Effect::Unwatch(uid),
            Effect::Changed,
            Effect::Answer {
                key,
                outcome: Ok(answer),
            },
        ]
    }

    /// The wait armed with `token` ran out.
    fn elapsed(&mut self, token: u64) -> Vec<Effect> {
        let key = self
            .pending
            .iter()
            .find(|(_, pending)| pending.token == token)
            .map(|(key, _)| key.clone());
        match key {
            Some(key) => self.fail(
                &key,
                format!(
                    "the agent was not ready within {} seconds",
                    OPEN_DEADLINE.as_secs()
                ),
            ),
            None => Vec::new(),
        }
    }

    /// The open in flight whose session is `uid`.
    fn key_of(&self, uid: u64) -> Option<Key> {
        self.pending
            .iter()
            .find_map(|(key, pending)| match &pending.phase {
                Phase::Up { uid: up, .. } if *up == uid => Some(key.clone()),
                Phase::Up { .. } | Phase::Resolving | Phase::Starting { .. } => None,
            })
    }

    /// Fail every open `lead` has in flight: it went.
    fn abandon_of(&mut self, lead: u64) -> Vec<Effect> {
        let keys: Vec<Key> = self
            .pending
            .keys()
            .filter(|(of, _)| *of == lead)
            .cloned()
            .collect();
        keys.iter()
            .flat_map(|key| self.fail(key, "the lead went before its worker was ready".into()))
            .collect()
    }

    /// Fail open `key` with `why`: close what it started, free its request,
    /// and answer.
    fn fail(&mut self, key: &Key, why: String) -> Vec<Effect> {
        let Some(pending) = self.pending.remove(key) else {
            return Vec::new();
        };
        self.requests.failed(key.0, &key.1);
        let mut fx = Vec::new();
        match pending.phase {
            Phase::Up {
                uid, root, added, ..
            } => {
                fx.push(Effect::Unwatch(uid));
                fx.push(Effect::Drop {
                    key: key.clone(),
                    uid,
                    root,
                    added,
                });
                fx.push(Effect::Changed);
            }
            Phase::Starting { folder, .. } => self.reserved.release(&folder),
            Phase::Resolving => {}
        }
        fx.push(Effect::Answer {
            key: key.clone(),
            outcome: Err(why),
        });
        fx
    }
}

/// This run of the app, as a worker's handle names it: uids count from 1 in
/// every run, so a handle from an earlier one must never reach whichever
/// session took its number. Never written anywhere.
pub fn this_run() -> String {
    static RUN: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| super::new_token()[..8].to_string());
    RUN.clone()
}

#[cfg(test)]
mod tests;
