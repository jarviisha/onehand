//! Offloaded projects' drive: who holds each one's code, the code's way up
//! before a prompt and back once the host is done with it, the branch a
//! project on a branch is fetched from and updated with, and the check every
//! [`OFFLOAD_POLL`] that brings the code back with nothing on screen.
//!
//! **Decided here, carried out elsewhere.** An [`OffloadDrive`] takes the
//! front end's commands as methods (track a folder, claim the send before a
//! prompt, check after a turn, check again, take the host's code, update the
//! host, offload a folder, the poll's clock) and is told what came of each
//! [`Job`] it hands out ([`Input`]). It reads no disk, no network and no
//! clock: a job's body ([`Job::run`]) does, off the UI loop, over the attach
//! the front end hands it ([`Reach`]).
//!
//! **One per process.** Two windows holding the same folder hold one copy of
//! its code, so they share whether it is locked and must never copy it back
//! twice at once: one move at a time per folder (`moving`), claimed when it
//! starts.
//!
//! **The host is the key's authority.** What is kept here (`offload.json`,
//! [`OffloadState`]) is a cache of its last word: every move asks the host
//! first, so a cache gone stale can only make a move wait, never send old
//! code up over the host's.
//!
//! **Late answers.** A job's answer is *recorded*: what its body wrote is on
//! disk whatever the drive did since, so it lands on the folder's entry while
//! the folder is kept, and on nothing once it is not. The poll's clock is
//! *dropped* past its use: only one is ever armed.

use super::{Manifest, OffloadState};
use crate::host::wire::Holder;
use crate::host::HostFailure;
use crate::workspace::HostName;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod blocking;
mod branch;
#[cfg(test)]
mod tests;

pub use blocking::{Brought, Job, Reach, Reached};
pub use branch::BranchState;

/// How often an offloaded project whose host holds the code is checked for
/// being done there: a session no window shows sends no turn end here.
pub const OFFLOAD_POLL: Duration = Duration::from_secs(30);

/// One offloaded folder, as the drive knows it.
pub struct Offload {
    pub host: HostName,
    /// As last kept; `None` until read.
    pub state: Option<OffloadState>,
    /// The code on its way now, if it is.
    pub moving: Option<Moving>,
    /// Why the last move did not go, in a person's words, until one does.
    pub failed: Option<String>,
    /// Its host refused the poll's attach (no service it can run, say): not
    /// attached by the poll again until the person checks again.
    pub refused: bool,
    /// On a branch: no key and no lock, and `state` is never kept. `Up` is
    /// the host being updated from here, `Back` its branch being fetched.
    pub branch: Option<BranchState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moving {
    Up,
    Back,
}

/// What the poll does with one folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Due {
    /// Count and fetch its host's branch.
    Branch,
    /// Check its code, the host attached.
    Back,
    /// Attach to its host, then check its code.
    Attach,
}

impl Offload {
    fn new(host: HostName, state: Option<OffloadState>, branch: Option<BranchState>) -> Self {
        Self {
            host,
            state,
            moving: None,
            failed: None,
            refused: false,
            branch,
        }
    }

    /// Whether this machine's writers are off: the host holds the code, it is
    /// on its way up, or what was kept has not been read yet.
    pub fn locked(&self) -> bool {
        self.branch.is_none()
            && (self.moving == Some(Moving::Up)
                || self.state.as_ref().is_none_or(|s| s.key == Holder::Host))
    }

    /// Whether the code is this machine's, settled: what *Run here again*
    /// needs.
    pub fn here(&self) -> bool {
        self.moving.is_none()
            && (self.branch.is_some()
                || self.state.as_ref().is_some_and(|s| s.key == Holder::Laptop))
    }

    /// Whether a prompt to a session of it sends the code up first: this
    /// machine holds it, or as far as anything here knows, and nothing moves
    /// it now. Never on a branch, whose host works on what it has.
    pub fn sends_up(&self) -> bool {
        self.branch.is_none() && self.moving.is_none() && !self.locked()
    }

    /// Whether its code is on its way up before a prompt now, which holds
    /// the prompt: never on a branch, whose update sends nothing ahead of
    /// one (the host refuses a prompt while it merges).
    pub fn going_up(&self) -> bool {
        self.branch.is_none() && self.moving == Some(Moving::Up)
    }

    /// What the poll does with this folder now: `seen` is how its host
    /// stands. A branch is counted either way, since a merge here changes
    /// the count with no host; a mirror only when its host holds the code,
    /// attaching first when it is not attached.
    fn due(&self, seen: HostSeen) -> Option<Due> {
        if self.moving.is_some() {
            return None;
        }
        if self.branch.is_some() {
            return Some(Due::Branch);
        }
        let held = self.state.as_ref().is_some_and(|s| s.key == Holder::Host);
        match (held, seen.live) {
            (false, _) => None,
            (true, true) => Some(Due::Back),
            // A refusal waits for the person: trying cannot change it.
            (true, false) if seen.taken || self.refused => None,
            (true, false) => Some(Due::Attach),
        }
    }

    /// Claim the folder's code for `moving`, if nothing moves it now.
    fn claim(&mut self, moving: Moving) -> bool {
        if self.moving.is_some() {
            return false;
        }
        self.moving = Some(moving);
        true
    }
}

/// How a host stands as the poll reads it: attached, and whether another
/// machine holds it.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostSeen {
    pub live: bool,
    pub taken: bool,
}

/// What an offload that landed keeps of the project: what went, for a
/// mirror, or the branch its host works on.
#[derive(Debug)]
pub enum Kept {
    Mirror(OffloadState),
    Branch(String),
}

/// What the drive asks for.
pub enum Effect {
    /// Hand over the poll's clock after this long ([`OffloadDrive::elapsed`]).
    Arm(Duration),
    /// Run this job off the UI loop and hand its answer to
    /// [`OffloadDrive::handle`].
    Run(Job),
    Signal(Signal),
}

/// What whoever asked is told.
#[derive(Debug)]
pub enum Signal {
    /// The code went up before a prompt to a session of `path`, or why not.
    Sent {
        path: PathBuf,
        result: Result<(), String>,
    },
    /// The form's offload of `path` to `host` landed: the mirror and what to
    /// keep of it, or why not.
    Offloaded {
        path: PathBuf,
        host: HostName,
        result: Result<(PathBuf, Kept), String>,
    },
    /// *Take the host's code* landed: `Ok(false)` when the host said this
    /// machine held the code already, so nothing was taken.
    TookOver {
        path: PathBuf,
        result: Result<bool, String>,
    },
}

/// Which way an attach the drive asked for goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    /// The poll's, made by itself: then the code is checked.
    Poll,
    /// The person's *Check again*: whatever stood in the way goes, then the
    /// code is checked.
    Again,
}

/// What a copy back's end leaves: the code settled, `None` while the host
/// still works on it; or why it failed, with what to keep then.
pub type Came = Result<Option<OffloadState>, (String, Option<Box<OffloadState>>)>;

/// What a send up's end leaves: what to keep, or why it stopped, with what
/// to keep then.
pub type Went = Result<OffloadState, (String, Option<Box<OffloadState>>)>;

/// What a job came to.
pub enum Input {
    Loaded {
        path: PathBuf,
        read: Result<Option<OffloadState>, String>,
    },
    Attached {
        path: PathBuf,
        then: Then,
        result: Result<(), HostFailure>,
    },
    /// `rsync` on both ends, before an offload into a mirror.
    Checked {
        path: PathBuf,
        host: HostName,
        result: Result<(), String>,
    },
    Offloaded {
        path: PathBuf,
        host: HostName,
        result: Result<(PathBuf, Kept), String>,
    },
    SentUp {
        path: PathBuf,
        went: Went,
    },
    BroughtBack {
        path: PathBuf,
        /// The poll's, which goes on to its next folder.
        poll: bool,
        /// The person's *Take the host's code*, into this backup.
        took: Option<PathBuf>,
        brought: Brought,
    },
    /// A branch fetched (`Back`) or the host updated from here (`Up`), and
    /// what it has to merge.
    Counted {
        path: PathBuf,
        by: Moving,
        poll: bool,
        counted: Result<usize, String>,
    },
}

/// The poll's pass over every folder: the ones still to look at, and the
/// hosts it could not reach, each tried once a pass since every try could
/// wait out ssh's connect.
#[derive(Default)]
struct Pass {
    queue: VecDeque<(PathBuf, Due)>,
    unreached: HashSet<HostName>,
}

/// The offloaded folders of every window, by their folder here.
#[derive(Default)]
pub struct OffloadDrive {
    roots: HashMap<PathBuf, Offload>,
    /// The poll's clock, armed with the first folder kept.
    armed: bool,
    pass: Option<Pass>,
}

impl std::ops::Deref for OffloadDrive {
    type Target = HashMap<PathBuf, Offload>;
    fn deref(&self) -> &Self::Target {
        &self.roots
    }
}

fn fresh(key: Holder) -> OffloadState {
    OffloadState {
        key,
        manifest: Manifest::default(),
        moved: 0,
        stopped: None,
    }
}

impl OffloadDrive {
    /// Keep `o` for `path`, arming the poll with the first one kept.
    fn keep(&mut self, path: PathBuf, o: Offload) -> Vec<Effect> {
        self.roots.insert(path, o);
        match std::mem::replace(&mut self.armed, true) {
            true => Vec::new(),
            false => vec![Effect::Arm(OFFLOAD_POLL)],
        }
    }

    /// Start keeping `path`, offloaded to `host`: with `state` when it was
    /// just offloaded, else reading what was kept of it. A folder already
    /// kept is left as it is, unless `state` says it anew.
    pub fn track(
        &mut self,
        path: PathBuf,
        host: HostName,
        state: Option<OffloadState>,
    ) -> Vec<Effect> {
        if self.roots.contains_key(&path) && state.is_none() {
            return Vec::new();
        }
        let read = state.is_none();
        let mut fx = self.keep(path.clone(), Offload::new(host.clone(), state, None));
        if read {
            fx.push(Effect::Run(Job::load(path, host)));
        }
        fx
    }

    /// Stop keeping `path`: it runs here again, or left the workspace. What
    /// was kept stays on disk, so a later offload sends only what changed.
    pub fn forget(&mut self, path: &Path) {
        self.roots.remove(path);
    }

    /// Claim the send of `path`'s code before a prompt to a session of it:
    /// `true` when it is offloaded and this machine holds the code, or holds
    /// it as far as anything here knows, and nothing else moves it now.
    /// Claimed at once, so a second press waits on this send rather than
    /// starting one of its own; [`Signal::Sent`] says how it went.
    pub fn claim_send(&mut self, path: &Path) -> (bool, Vec<Effect>) {
        let Some(o) = self.roots.get_mut(path).filter(|o| o.sends_up()) else {
            return (false, Vec::new());
        };
        o.moving = Some(Moving::Up);
        let sent = o
            .state
            .as_ref()
            .map(|s| s.manifest.clone())
            .unwrap_or_default();
        let job = Job::send_up(path.to_path_buf(), o.host.clone(), sent);
        (true, vec![Effect::Run(job)])
    }

    /// Check `path` now, as a turn of a session on it ends: its branch
    /// fetched if it moved, or its code brought back if its host holds it
    /// and nothing works on it there. Nothing while it is already moving, or
    /// the host is not attached: a check never attaches by itself.
    pub fn check(&mut self, path: &Path) -> Vec<Effect> {
        self.check_for(path, false).unwrap_or_default()
    }

    /// The check, `poll` when the poll's pass made it; `None` when there is
    /// nothing to check.
    fn check_for(&mut self, path: &Path, poll: bool) -> Option<Vec<Effect>> {
        match self.roots.get(path)?.branch.is_some() {
            true => self.refresh(path, poll),
            false => self.check_back(path, poll),
        }
    }

    /// Bring `path`'s code back if its host holds it, over an attach already
    /// up; `None` when its host does not hold it, or the code moves now.
    fn check_back(&mut self, path: &Path, poll: bool) -> Option<Vec<Effect>> {
        let o = self.roots.get_mut(path)?;
        let sent = o
            .state
            .as_ref()
            .filter(|s| s.key == Holder::Host)?
            .manifest
            .clone();
        o.claim(Moving::Back).then_some(())?;
        let job = Job::check_back(path.to_path_buf(), o.host.clone(), sent, poll);
        Some(vec![Effect::Run(job)])
    }

    /// Check `path` again because the person asked, attaching to its host if
    /// it is not attached: why not, on its page, when that fails.
    pub fn check_again(&mut self, path: &Path) -> Vec<Effect> {
        let Some(o) = self.roots.get(path) else {
            return Vec::new();
        };
        let job = Job::attach(path.to_path_buf(), o.host.clone(), Then::Again);
        vec![Effect::Run(job)]
    }

    /// Take the host's code over what changed here since it went, once the
    /// person said so: what changed is copied into `into` first, then the
    /// code comes and the key comes back as on any copy back. Why not is
    /// said on the project's page too, nothing here written over.
    pub fn take_over(&mut self, path: &Path, into: PathBuf) -> Vec<Effect> {
        let claimed = self.roots.get_mut(path).and_then(|o| {
            let sent = o
                .state
                .as_ref()
                .filter(|s| s.key == Holder::Host)?
                .manifest
                .clone();
            o.claim(Moving::Back).then(|| (o.host.clone(), sent))
        });
        let Some((host, sent)) = claimed else {
            let why = "The code was moving; check again, then take it".to_string();
            if let Some(o) = self.roots.get_mut(path) {
                o.failed = Some(why.clone());
            }
            return vec![Effect::Signal(Signal::TookOver {
                path: path.to_path_buf(),
                result: Err(why),
            })];
        };
        vec![Effect::Run(Job::take_over(
            path.to_path_buf(),
            host,
            sent,
            into,
        ))]
    }

    /// Offload `path` to `host`, for the form: `rsync` checked on both ends
    /// first, unless on a branch, which needs none. [`Signal::Offloaded`]
    /// says how it went; the folder is kept only once the window keeps it.
    pub fn offload(&mut self, path: PathBuf, host: HostName, on_branch: bool) -> Vec<Effect> {
        let job = match on_branch {
            true => Job::offload(path, host, true),
            false => Job::check(path, host),
        };
        vec![Effect::Run(job)]
    }

    /// The poll's clock came round: every folder whose host holds the code
    /// is checked, attaching to the host by itself when it is not attached,
    /// so the code comes back with nothing of the project on screen; each one
    /// on a branch is counted, and its branch fetched if it moved while the
    /// host is attached. One at a time, then the clock again.
    ///
    /// The attach made here never takes a host another machine holds, and
    /// one the host refuses stands on the project's page, not tried again
    /// until the person checks again: trying cannot change it.
    pub fn elapsed(&mut self, seen: impl Fn(&HostName) -> HostSeen) -> Vec<Effect> {
        if self.pass.is_some() {
            return Vec::new();
        }
        let mut queue: Vec<(PathBuf, Due)> = self
            .roots
            .iter()
            .filter_map(|(path, o)| Some((path.clone(), o.due(seen(&o.host))?)))
            .collect();
        // In the folders' order, so a pass is the same whatever the map's.
        queue.sort_by(|a, b| a.0.cmp(&b.0));
        self.pass = Some(Pass {
            queue: queue.into(),
            unreached: HashSet::new(),
        });
        self.next()
    }

    /// The pass's next folder, or the clock again once none is left.
    fn next(&mut self) -> Vec<Effect> {
        while let Some(pass) = self.pass.as_mut() {
            let Some((path, due)) = pass.queue.pop_front() else {
                self.pass = None;
                return vec![Effect::Arm(OFFLOAD_POLL)];
            };
            let fx = match due {
                Due::Branch => self.refresh(&path, true),
                Due::Back => self.check_back(&path, true),
                Due::Attach => {
                    let host = match self.roots.get(&path) {
                        Some(o) if !pass.unreached.contains(&o.host) => o.host.clone(),
                        _ => continue,
                    };
                    Some(vec![Effect::Run(Job::attach(path, host, Then::Poll))])
                }
            };
            if let Some(fx) = fx {
                return fx;
            }
        }
        Vec::new()
    }

    /// What a job came to.
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Loaded { path, read } => {
                if let Some(o) = self.roots.get_mut(&path) {
                    match read {
                        // Nothing kept: the host is asked before any move, so
                        // taking the key for this machine's can only make the
                        // first prompt ask.
                        Ok(state) => o.state = Some(state.unwrap_or_else(|| fresh(Holder::Laptop))),
                        // Taken for the host's, which keeps it locked and lets
                        // the next check ask the host: a copy back held against
                        // nothing kept stops on every file rather than writing
                        // over one.
                        Err(why) => {
                            o.state = Some(fresh(Holder::Host));
                            o.failed = Some(format!("Could not read what was kept: {why}"));
                        }
                    }
                }
                Vec::new()
            }
            Input::Attached { path, then, result } => self.attached(path, then, result),
            Input::Checked { path, host, result } => match result {
                Ok(()) => vec![Effect::Run(Job::offload(path, host, false))],
                Err(why) => vec![Effect::Signal(Signal::Offloaded {
                    path,
                    host,
                    result: Err(why),
                })],
            },
            Input::Offloaded { path, host, result } => {
                vec![Effect::Signal(Signal::Offloaded { path, host, result })]
            }
            Input::SentUp { path, went } => {
                if let Some(o) = self.roots.get_mut(&path) {
                    o.moving = None;
                    match &went {
                        Ok(state) => {
                            o.state = Some(state.clone());
                            o.failed = None;
                        }
                        Err((why, state)) => {
                            if let Some(state) = state {
                                o.state = Some((**state).clone());
                            }
                            o.failed = Some(why.clone());
                        }
                    }
                }
                let result = went.map(drop).map_err(|(why, _)| why);
                vec![Effect::Signal(Signal::Sent { path, result })]
            }
            Input::BroughtBack {
                path,
                poll,
                took,
                brought,
            } => {
                let mut fx = Vec::new();
                let Brought { came, here, backed } = brought;
                match took {
                    None => {
                        if let Some(o) = self.roots.get_mut(&path) {
                            landed(o, came);
                        }
                    }
                    Some(into) => {
                        let host = self.roots.get(&path).map(|o| o.host.clone());
                        let host = host.unwrap_or_else(|| HostName("the host".into()));
                        let said = took_over(&came, here, backed, &host, &into);
                        // Said once, on the project's page where it was pressed.
                        if let Some(o) = self.roots.get_mut(&path) {
                            landed(o, came);
                            if let Err(why) = &said {
                                o.failed = Some(why.clone());
                            }
                        }
                        fx.push(Effect::Signal(Signal::TookOver {
                            path: path.clone(),
                            result: said,
                        }));
                    }
                }
                if poll {
                    fx.extend(self.next());
                }
                fx
            }
            Input::Counted {
                path,
                by,
                poll,
                counted,
            } => {
                if let Some(o) = self.roots.get_mut(&path) {
                    branch::landed_on(o, by, counted);
                }
                match poll {
                    true => self.next(),
                    false => Vec::new(),
                }
            }
        }
    }

    fn attached(
        &mut self,
        path: PathBuf,
        then: Then,
        result: Result<(), HostFailure>,
    ) -> Vec<Effect> {
        match (then, result) {
            (Then::Again, Ok(())) => {
                // Attached: whatever stood in the way of reaching the host
                // is gone.
                if let Some(o) = self.roots.get_mut(&path) {
                    o.failed = None;
                    o.refused = false;
                }
                self.check_back(&path, false).unwrap_or_default()
            }
            (Then::Again, Err(failure)) => {
                if let Some(o) = self.roots.get_mut(&path) {
                    o.failed = Some(failure.said());
                }
                Vec::new()
            }
            (Then::Poll, Ok(())) => match self.check_back(&path, true) {
                Some(fx) => fx,
                None => self.next(),
            },
            (Then::Poll, Err(HostFailure::Refused(why))) => {
                if let Some(o) = self.roots.get_mut(&path) {
                    o.failed = Some(why);
                    o.refused = true;
                }
                self.next()
            }
            // Out of reach, busy or taken: the next poll tries again, or
            // finds it taken and leaves it.
            (
                Then::Poll,
                Err(HostFailure::Other(_) | HostFailure::Busy { .. } | HostFailure::Taken(_)),
            ) => {
                let host = self.roots.get(&path).map(|o| o.host.clone());
                if let (Some(pass), Some(host)) = (self.pass.as_mut(), host) {
                    pass.unreached.insert(host);
                }
                self.next()
            }
        }
    }
}

/// Keep what a copy back `came` to, the code no longer moving.
fn landed(o: &mut Offload, came: Came) {
    o.moving = None;
    match came {
        // Still stopped: nothing moved, and why the last try failed stands.
        Ok(Some(mut state)) if state.stopped.is_some() => {
            if let Some(was) = &o.state {
                state.moved = was.moved;
            }
            o.state = Some(state);
        }
        Ok(Some(state)) => {
            o.state = Some(state);
            o.failed = None;
            o.refused = false;
        }
        Ok(None) => {}
        Err((why, state)) => {
            if let Some(state) = state {
                o.state = Some(*state);
            }
            o.failed = Some(why);
        }
    }
}

/// What *Take the host's code* says it came to: `here` when the host said
/// this machine held the code before the copy, `backed` when the backup
/// folder `into` is there. Once the backup is made, a failure after it still
/// says where it is: this machine's versions may now be only there.
fn took_over(
    came: &Came,
    here: bool,
    backed: bool,
    host: &HostName,
    into: &Path,
) -> Result<bool, String> {
    let said = match came {
        Ok(Some(state)) if state.key == Holder::Laptop => Ok(!here),
        // Files changed here after the backup was made: kept as they are.
        Ok(Some(state)) if state.stopped.is_some() => Err(
            "Files changed here while the code came back; they stay as this machine has them"
                .to_string(),
        ),
        Ok(Some(_)) => Err(format!(
            "{host} kept the code; the next check brings it back"
        )),
        Ok(None) => Err(format!(
            "{host} is working on the code again; wait for it to finish"
        )),
        Err((why, _)) => Err(why.clone()),
    };
    said.map_err(|why| match backed {
        true => format!("{why}; this machine's versions are in {}", into.display()),
        false => why,
    })
}

/// Where this machine's versions of what changed go before the host's code
/// is taken over them: a folder of their own in the project's storage, named
/// for `when` (a local date and time).
pub fn backup_dir(path: &Path, when: &str) -> PathBuf {
    crate::workspace::storage_for(path)
        .join("backups")
        .join(when)
}

impl OffloadDrive {
    /// A session running on a host (`on_host`) in folder `root` gave
    /// `signal`: a turn of it, or work it did between turns, ended, so an
    /// offloaded project's code may come back, or its branch have moved.
    pub fn session_said(
        &mut self,
        root: &Path,
        on_host: bool,
        signal: &crate::chat::drive::Signal,
    ) -> Vec<Effect> {
        use crate::chat::drive::Signal as Said;
        match signal {
            Said::TurnEnded | Said::CycleEnded if on_host => self.check(root),
            Said::TurnEnded
            | Said::CycleEnded
            | Said::Appended
            | Said::AwaitingUser(_)
            | Said::Disconnected
            | Said::PickRefused(_)
            | Said::StopRefused(_)
            | Said::SaveFailed(_)
            | Said::CodeFailed(_)
            | Said::RenameFailed(_) => Vec::new(),
        }
    }
}
