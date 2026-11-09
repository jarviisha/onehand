//! The clock schedules run by: what each look starts, skips or holds, and
//! what it writes back.
//!
//! **Decided here, carried out elsewhere.** A [`ScheduleDrive`] is told when
//! its clock comes round, what the file holds, which window holds each
//! project, which of its last tasks still work and the time; it answers with
//! what to do ([`Effect`]). It reads no file and no clock: one look shortly
//! after start, the catch-up for what came due while onehand was closed,
//! then one every minute, each an [`Effect::Arm`] answered by
//! [`Input::Elapsed`].
//!
//! A look reads `schedules.toml` again, so a schedule edited in the file
//! holds from the next one; decides every schedule switched on whose
//! project a window holds (the first that does), by [`super::decide`];
//! starts what is due, one at a time, each as a task; and writes back only
//! `last_fired` and `said`, onto the file as it is then, so a hand edit made
//! during a look stands.

use super::{decide, Act, Schedule, Starts};
use crate::task::tasks::Placed;
use crate::task::{Grant, Origin};
use crate::workflow::{Brief, Template};
use chrono::TimeZone;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

/// How often the clock looks.
pub const EVERY: Duration = Duration::from_secs(60);

/// How long after start the first look waits, so the first window is up to
/// hold the projects it is about.
pub const FIRST_LOOK: Duration = Duration::from_secs(5);

/// What a schedule starts, made ready for the one way every task off screen
/// starts: its project, its workflow filled with its mode and settings, its
/// brief, and what its task keeps of where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Starting {
    pub project: PathBuf,
    pub template: Template,
    pub brief: Brief,
    pub origin: Origin,
    pub grant: Grant,
    pub agent: Option<String>,
    pub mode: Option<String>,
}

/// What the world found, as the drive is told it.
#[derive(Debug, Clone, PartialEq)]
pub enum Input<W> {
    /// The clock came round.
    Elapsed,
    /// What the file holds, or why it could not be read, with what a look
    /// decides by: the first window holding each schedule's project, which
    /// of the clock's last tasks are still queued, running or waiting, and
    /// the workflows on offer.
    Read {
        all: Result<Vec<Schedule>, String>,
        holders: Vec<(PathBuf, W)>,
        working: HashSet<String>,
        library: Vec<Template>,
    },
    /// Where the start under way landed: its task and its place, or why it
    /// did not start.
    Started(Result<(String, Placed), String>),
    /// What the clock's write left in the file, or why it failed.
    Written(Result<Vec<Schedule>, String>),
    /// Switching schedule `id` on or off was written, or failed (`failed`),
    /// with what the file was read back as after a failure (`kept`).
    Switched {
        id: String,
        on: bool,
        window: W,
        failed: Option<String>,
        kept: Option<Vec<Schedule>>,
    },
}

/// What a person, or the log, is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Signal<W> {
    /// Schedule `name` did not start, for `why`: said on the desktop, once
    /// per reason.
    NotStarted { name: String, why: String },
    /// Switching a schedule on (`on`) or off failed, in `window`.
    SwitchFailed { window: W, on: bool, why: String },
    /// For the log only.
    Log(String),
}

/// What to do, in the order given.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect<W> {
    /// Report [`Input::Elapsed`] after this long. One clock, armed again at
    /// each fire, so nothing supersedes it.
    Arm(Duration),
    /// Read the file and what a look decides by, and report [`Input::Read`].
    /// Awaited: the next look waits for this one.
    Read,
    /// Start `starting` in `window`, and report [`Input::Started`].
    /// Awaited: the next start waits for it.
    Start { starting: Box<Starting>, window: W },
    /// Write each of `looked`'s `last_fired` and `said` onto the file as it
    /// is now ([`super::merge_looked`]), in order with every other write of
    /// it, and report [`Input::Written`]. Awaited.
    Write { looked: Vec<Schedule> },
    /// Write schedule `id` switched on (`on`, since `since`) or off onto the
    /// file as it is ([`super::switch`]), in order with every other write
    /// of it, reading it back if that fails, and report [`Input::Switched`]
    /// with `window`. Recorded: what the file holds is shown whenever it
    /// lands.
    Switch {
        id: String,
        on: bool,
        since: u64,
        window: W,
    },
    /// Tell this.
    Signal(Signal<W>),
    /// What the schedules show changed: draw them again.
    Refresh,
}

/// A look under way: the schedules it read, the starts still to make, and
/// whether anything moved.
struct Looking<W> {
    all: Vec<Schedule>,
    firing: VecDeque<(Schedule, W)>,
    library: Vec<Template>,
    changed: bool,
}

/// The clock's state, for the life of the process.
pub struct ScheduleDrive<W> {
    /// When onehand opened: a due time before it was missed.
    opened_at: u64,
    /// The last task each schedule started, by schedule id: while it is still
    /// queued, running or waiting, the schedule is busy.
    last_task: HashMap<String, String>,
    /// Schedules whose time came while they were busy and that wait for it
    /// to end.
    queued: HashSet<String>,
    /// The last failure each schedule was told of, so one reason is said once.
    told: HashMap<String, String>,
    looking: Option<Looking<W>>,
    /// A look asked for its read and has not had it yet.
    reading: bool,
    /// The schedules as the last look found them, for the pages to show.
    all: Vec<Schedule>,
    /// What the last read of the file came to: nothing yet, or why it failed.
    read: Option<Result<(), String>>,
    /// Switches not yet written, by schedule: a look's list gives way to
    /// them, so a switch does not flip back on screen while it is written.
    pending: HashMap<String, usize>,
}

impl<W: Copy> ScheduleDrive<W> {
    /// The clock of a process that opened at `opened_at`, its first look
    /// armed.
    pub fn new(opened_at: u64) -> (Self, Vec<Effect<W>>) {
        let drive = Self {
            opened_at,
            last_task: HashMap::new(),
            queued: HashSet::new(),
            told: HashMap::new(),
            looking: None,
            reading: false,
            all: Vec::new(),
            read: None,
            pending: HashMap::new(),
        };
        (drive, vec![Effect::Arm(FIRST_LOOK)])
    }

    /// The schedules as the clock last found them.
    pub fn listed(&self) -> &[Schedule] {
        &self.all
    }

    /// What the last read came to: `None` before the first, else why it
    /// failed.
    pub fn read(&self) -> Option<&Result<(), String>> {
        self.read.as_ref()
    }

    /// The tasks the schedules last started, whose work a look asks after.
    pub fn last_tasks(&self) -> impl Iterator<Item = &str> {
        self.last_task.values().map(String::as_str)
    }

    /// What a save or a delete left in the file: the list the pages show,
    /// until the next look reads it again.
    pub fn put(&mut self, all: Vec<Schedule>) -> Vec<Effect<W>> {
        self.all = all;
        vec![Effect::Refresh]
    }

    /// Switch schedule `id` on or off at `now`, asked from `window`: at once
    /// on screen, then in the file. Switched on, nothing due before now is
    /// caught up.
    pub fn set_on(&mut self, id: &str, on: bool, now: u64, window: W) -> Vec<Effect<W>> {
        let Some(s) = self
            .all
            .iter_mut()
            .find(|s| s.id == id)
            .filter(|s| s.on != on)
        else {
            return Vec::new();
        };
        s.on = on;
        if on {
            s.on_since = now;
        }
        *self.pending.entry(id.to_string()).or_default() += 1;
        vec![
            Effect::Refresh,
            Effect::Switch {
                id: id.to_string(),
                on,
                since: now,
                window,
            },
        ]
    }

    /// Something the world found at `now`, in time zone `tz`: what to do.
    pub fn handle(&mut self, input: Input<W>, now: u64, tz: &impl TimeZone) -> Vec<Effect<W>> {
        match input {
            Input::Elapsed => {
                let mut fx = match self.reading || self.looking.is_some() {
                    true => Vec::new(),
                    false => {
                        self.reading = true;
                        vec![Effect::Read]
                    }
                };
                fx.push(Effect::Arm(EVERY));
                fx
            }
            Input::Read {
                all,
                holders,
                working,
                library,
            } => {
                self.reading = false;
                match all {
                    // Said on the pages that list them, not only in the log.
                    Err(why) => {
                        self.read = Some(Err(why));
                        vec![Effect::Refresh]
                    }
                    Ok(all) => {
                        let looking = self.decide_all(all, &holders, &working, library, now, tz);
                        self.looking = Some(looking);
                        self.next()
                    }
                }
            }
            Input::Started(placed) => {
                let mut fx = Vec::new();
                if let Some(looking) = &mut self.looking {
                    if let Some((s, _)) = looking.firing.pop_front() {
                        let outcome = placed.and_then(|(task, placed)| match placed {
                            Placed::Started | Placed::Queued(_) => Ok(task),
                            Placed::Refused(why) => Err(why),
                        });
                        let said =
                            record(&mut self.last_task, &mut self.told, &s, outcome, &mut fx);
                        set_said(looking, &s.id, said);
                    }
                }
                fx.extend(self.next());
                fx
            }
            Input::Written(written) => {
                let Some(looking) = self.looking.take() else {
                    return Vec::new();
                };
                let mut fx = Vec::new();
                let all = match written {
                    Ok(now_kept) => now_kept,
                    Err(why) => {
                        fx.push(Effect::Signal(Signal::Log(format!(
                            "the schedules were not written: {why}"
                        ))));
                        looking.all
                    }
                };
                fx.extend(self.settle(all));
                fx
            }
            Input::Switched {
                id,
                on,
                window,
                failed,
                kept,
            } => {
                if let Some(n) = self.pending.get_mut(&id) {
                    *n = n.saturating_sub(1);
                }
                let back = kept
                    .as_ref()
                    .and_then(|all| all.iter().find(|s| s.id == id));
                if let (Some(back), Some(shown)) = (back, self.all.iter_mut().find(|s| s.id == id))
                {
                    shown.on = back.on;
                    shown.on_since = back.on_since;
                }
                match failed {
                    Some(why) => vec![
                        Effect::Signal(Signal::SwitchFailed { window, on, why }),
                        Effect::Refresh,
                    ],
                    None => Vec::new(),
                }
            }
        }
    }

    /// Decide every schedule in `all` at `now`, moving `last_fired` and
    /// `said` as each comes to, and keep what to start.
    fn decide_all(
        &mut self,
        mut all: Vec<Schedule>,
        holders: &[(PathBuf, W)],
        working: &HashSet<String>,
        library: Vec<Template>,
        now: u64,
        tz: &impl TimeZone,
    ) -> Looking<W> {
        let mut firing = VecDeque::new();
        let mut changed = false;
        // A time held for a schedule since switched off, or gone, is let go.
        self.queued
            .retain(|id| all.iter().any(|s| s.on && &s.id == id));
        for s in all.iter_mut().filter(|s| s.on) {
            let Some(window) = holders
                .iter()
                .find(|(project, _)| *project == s.project)
                .map(|(_, window)| *window)
            else {
                continue;
            };
            let busy = self
                .last_task
                .get(&s.id)
                .is_some_and(|task| working.contains(task));
            // A time that came while it was busy goes once the last one is done.
            if !busy && self.queued.remove(&s.id) {
                firing.push_back((s.clone(), window));
                continue;
            }
            match decide(s, now, self.opened_at, busy, tz) {
                Act::Wait => continue,
                Act::Start { .. } => firing.push_back((s.clone(), window)),
                Act::Skip(why) => s.said = Some(why),
                Act::Queue => {
                    self.queued.insert(s.id.clone());
                }
            }
            s.last_fired = Some(now);
            changed = true;
        }
        Looking {
            all,
            firing,
            library,
            changed,
        }
    }

    /// The look's next start, or its write, or its end.
    fn next(&mut self) -> Vec<Effect<W>> {
        let mut fx = Vec::new();
        let Some(looking) = &mut self.looking else {
            return fx;
        };
        while let Some((s, window)) = looking.firing.front().cloned() {
            match starting(&s, &looking.library) {
                Ok(starting) => {
                    fx.push(Effect::Start {
                        starting: Box::new(starting),
                        window,
                    });
                    return fx;
                }
                // A workflow gone from the library is a start that failed.
                Err(why) => {
                    looking.firing.pop_front();
                    let said = record(&mut self.last_task, &mut self.told, &s, Err(why), &mut fx);
                    set_said(looking, &s.id, said);
                }
            }
        }
        if looking.changed {
            fx.push(Effect::Write {
                looked: looking.all.clone(),
            });
            return fx;
        }
        let all = self.looking.take().map(|l| l.all).unwrap_or_default();
        fx.extend(self.settle(all));
        fx
    }

    /// The look is over: `all` is what the pages show, a switch still being
    /// written standing over what was read.
    fn settle(&mut self, mut all: Vec<Schedule>) -> Vec<Effect<W>> {
        self.read = Some(Ok(()));
        for s in &mut all {
            if self.pending.get(&s.id).is_some_and(|n| *n > 0) {
                if let Some(shown) = self.all.iter().find(|shown| shown.id == s.id) {
                    s.on = shown.on;
                    s.on_since = shown.on_since;
                }
            }
        }
        self.all = all;
        // Every look, changed or not: a row's next time moves with the clock.
        vec![Effect::Refresh]
    }
}

/// What schedule `s` starts: its workflow, or the one-step workflow of its
/// prompt, filled with its mode and settings, as a task that keeps where it
/// came from and what its sessions may use.
fn starting(s: &Schedule, library: &[Template]) -> Result<Starting, String> {
    let template = match &s.starts {
        Starts::Prompt { .. } => super::prompt_template(),
        Starts::Workflow { workflow, .. } => library
            .iter()
            .find(|t| &t.id == workflow)
            .cloned()
            .ok_or_else(|| format!("no workflow with id {workflow} in the library"))?,
    };
    Ok(Starting {
        project: s.project.clone(),
        template: super::fill(template, s.mode.as_deref(), &s.options),
        brief: super::brief(s),
        origin: Origin::Schedule {
            id: s.id.clone(),
            name: s.name.clone(),
        },
        grant: Grant {
            tools: s.tools.clone(),
            labels: s.labels.clone(),
        },
        agent: s.agent.clone(),
        mode: s.mode.clone(),
    })
}

/// Keep what a start of `s` came to: the task it made, or why none, told
/// once per reason. Answers what the schedule's row says now.
fn record<W>(
    last_task: &mut HashMap<String, String>,
    told: &mut HashMap<String, String>,
    s: &Schedule,
    outcome: Result<String, String>,
    fx: &mut Vec<Effect<W>>,
) -> Option<String> {
    match outcome {
        Ok(task) => {
            last_task.insert(s.id.clone(), task);
            told.remove(&s.id);
            None
        }
        Err(why) => {
            if told.insert(s.id.clone(), why.clone()).as_ref() != Some(&why) {
                fx.push(Effect::Signal(Signal::NotStarted {
                    name: s.name.clone(),
                    why: why.clone(),
                }));
            }
            Some(format!("did not start: {why}"))
        }
    }
}

/// Schedule `id`'s row now says `said`: a change to write.
fn set_said<W>(looking: &mut Looking<W>, id: &str, said: Option<String>) {
    if let Some(kept) = looking.all.iter_mut().find(|kept| kept.id == id) {
        if kept.said != said {
            kept.said = said;
            looking.changed = true;
        }
    }
}

#[cfg(test)]
mod tests;
