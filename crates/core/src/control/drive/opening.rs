//! A worker's open, step by step, from the call to its first prompt: what
//! the world reports at each step and what the open does next. See the
//! drive's header for the steps and what a failure leaves behind.

use super::super::kept::KeptWorker;
use super::super::open::{self, Step};
use super::super::state as said;
use super::super::workers::{sessions_in, Relation, Resolved, Seen, Target};
use super::{Begun, ControlDrive, Effect, Facts, Key, Open, Pending, Phase, World, OPEN_DEADLINE};
use crate::chat::drive::Intake;
use crate::chat::Chat;
use crate::worktree::occupants::Occupants;
use std::path::{Path, PathBuf};

impl ControlDrive {
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
                return Ok(Begun::Said(match uid.and_then(|uid| self.handle_of(uid)) {
                    Some(handle) => {
                        format!("{handle} is still opening; call again with the same request")
                    }
                    None => {
                        "the session is still opening; call again with the same request".to_string()
                    }
                }))
            }
            Seen::Answered(answer) => return Ok(Begun::Said(answer)),
        }
        let target = match super::super::pick_project(projects, &open.project) {
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

    /// Open `key`'s folder was read, as `resolved` says, with `world` as it
    /// stands: refuse it, or reserve it and start.
    pub(super) fn resolved(
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
        // started for nobody, nor counted under a cap it no longer has. A
        // worker coming back needs only its lead open: the lead's tools are
        // off in every new run until the person switches them on.
        let allowed = match pending.open.back {
            Some(_) => world.lead_here,
            None => self.lead_allowed(lead, world.lead_here),
        };
        if !allowed {
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
        if let Err(why) = self.admit_but(lead, &world.opened, Some(key)) {
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
        // A worker coming back keeps the folders it had; its own was read
        // again only to be checked.
        let (dirs, resume) = match &pending.open.back {
            Some(back) => (back.dirs.clone(), Some(back.conversation.clone())),
            None => (resolved.dirs, None),
        };
        pending.phase = Phase::Starting {
            folder: resolved.folder,
            added,
            root: resolved.root.clone(),
            dirs: dirs.clone(),
        };
        vec![Effect::Start {
            key: key.clone(),
            root: resolved.root,
            dirs,
            folder,
            name,
            resume,
        }]
    }

    /// Open `key`'s session started as `uid`, or why not.
    pub(super) fn started(&mut self, key: &Key, started: Result<u64, String>) -> Vec<Effect> {
        let Some(pending) = self.pending.get_mut(key) else {
            // Failed while it started: what it started goes, its project
            // with it when the open added it.
            let (root, added) = self.abandoned.remove(key).unwrap_or_default();
            return match started {
                Ok(uid) => vec![Effect::Drop {
                    key: key.clone(),
                    uid,
                    root,
                    added,
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
        let back = pending.open.back.clone();
        let uid = match started {
            Ok(uid) => uid,
            Err(why) => {
                self.reserved.release(&folder);
                return self.fail(key, why);
            }
        };
        self.reserved.bind(&folder, uid);
        self.opened_one(lead, uid);
        if let Some(back) = back {
            return self.came_back(key, uid, back, root, dirs);
        }
        self.requests.started(lead, &key.1, uid);
        let handle = open::mint_handle();
        self.handles.insert(handle.clone(), uid);
        let pending = self.pending.get_mut(key).expect("looked up above");
        pending.phase = Phase::Up {
            uid,
            root,
            added,
            dirs,
            picked: Vec::new(),
            handle,
        };
        vec![Effect::Changed, Effect::Watch(uid)]
    }

    /// Worker `uid`'s session changed, or is gone (`chat` is `None`);
    /// `lead_here` says whether its lead is still a session of some window.
    pub(super) fn woke(&mut self, uid: u64, chat: Option<&Chat>, lead_here: bool) -> Vec<Effect> {
        if let Some(fx) = self.turn_woke(uid, chat) {
            return fx;
        }
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
    pub(super) fn begun(&mut self, uid: u64, facts: Facts) -> Vec<Effect> {
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
            phase: Phase::Up {
                root, dirs, handle, ..
            },
            token,
            ..
        }) = self.pending.remove(&key)
        else {
            return Vec::new();
        };
        let mut lines = vec![
            format!("session: {handle}"),
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
        // Its first prompt is the lead's first turn, running now.
        let unended = open.prompt.is_some();
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
                handle,
                folder: root,
                agent: open.agent.clone(),
                name: open.name.clone(),
                conversation: self.conversations.get(&uid).cloned(),
                person: false,
                unended,
                seq: token,
            },
        );
        self.requests
            .answered(open.lead, &open.request, answer.clone());
        vec![
            Effect::Unwatch(uid),
            Effect::Changed,
            self.keep(open.lead),
            Effect::Answer {
                key,
                outcome: Ok(answer),
            },
        ]
    }

    /// The wait armed with `token` ran out.
    pub(super) fn elapsed(&mut self, token: u64) -> Vec<Effect> {
        if let Some(fx) = self.turn_elapsed(token) {
            return fx;
        }
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
    pub(super) fn key_of(&self, uid: u64) -> Option<Key> {
        self.pending
            .iter()
            .find_map(|(key, pending)| match &pending.phase {
                Phase::Up { uid: up, .. } if *up == uid => Some(key.clone()),
                Phase::Up { .. } | Phase::Resolving | Phase::Starting { .. } => None,
            })
    }

    /// Fail every open `lead` has in flight: it went, or only its tools did
    /// (`closed` false), which leaves its workers coming back alone.
    pub(super) fn abandon_of(&mut self, lead: u64, closed: bool) -> Vec<Effect> {
        let keys: Vec<Key> = self
            .pending
            .iter()
            .filter(|((of, _), pending)| *of == lead && (closed || pending.open.back.is_none()))
            .map(|(key, _)| key.clone())
            .collect();
        keys.iter()
            .flat_map(|key| self.fail(key, "the lead went before its worker was ready".into()))
            .collect()
    }

    /// Fail open `key` with `why`: close what it started, free its request,
    /// and answer. A worker coming back is answered to nobody: it ends, its
    /// handle reading closed with `why`.
    pub(super) fn fail(&mut self, key: &Key, why: String) -> Vec<Effect> {
        let Some(pending) = self.pending.remove(key) else {
            return Vec::new();
        };
        self.requests.failed(key.0, &key.1);
        let mut fx = Vec::new();
        // Its session may still be starting: what it started and where is
        // read when its start lands.
        if let Phase::Starting { root, added, .. } = &pending.phase {
            self.abandoned.insert(key.clone(), (root.clone(), *added));
        }
        if let Some(back) = &pending.open.back {
            if let Phase::Starting { folder, .. } = &pending.phase {
                self.reserved.release(folder);
            }
            let lead = pending.open.lead;
            self.close_record(back.handle.clone(), lead, Some(why));
            fx.push(self.keep(lead));
            return fx;
        }
        match pending.phase {
            Phase::Up {
                uid,
                root,
                added,
                handle,
                ..
            } => {
                self.handles.remove(&handle);
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

    /// Worker `back` came back as session `uid`, its lead's again: its
    /// relation as it was kept, its handle naming its new session. Nothing is
    /// picked or sent, and no call waits on it.
    fn came_back(
        &mut self,
        key: &Key,
        uid: u64,
        back: KeptWorker,
        root: PathBuf,
        dirs: Vec<PathBuf>,
    ) -> Vec<Effect> {
        let Some(pending) = self.pending.remove(key) else {
            return Vec::new();
        };
        self.handles.insert(back.handle.clone(), uid);
        self.conversations.insert(uid, back.conversation.clone());
        self.workers.insert(
            uid,
            Relation {
                lead: pending.open.lead,
                turn: back.turn,
                text: back.text,
                // Its chat counts its prompts from nothing again.
                sent: 0,
                dirs,
                handle: back.handle,
                folder: root,
                agent: back.agent,
                name: back.name,
                conversation: Some(back.conversation),
                person: back.person,
                unended: back.unended,
                seq: pending.token,
            },
        );
        let lead = pending.open.lead;
        vec![Effect::Changed, self.keep(lead)]
    }
}
