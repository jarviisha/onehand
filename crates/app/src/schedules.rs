//! The clock schedules run by, while onehand runs.
//!
//! A look once a minute, and one shortly after start, which is the catch-up
//! for what came due while onehand was closed. Each look reads
//! `schedules.toml` again, so a schedule edited in the file holds from the
//! next look; what each schedule does is core's `schedule::decide`; a start
//! goes through what every task off screen goes through (`control::start_task`).
//! A schedule fires in the first window that holds its project, and one that
//! no window holds waits.

use crate::control::Started;
use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Global, Task};
use onehand_core::schedule::{self, Act, Schedule, Starts};
use onehand_core::task::{Grant, Origin};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// How often the clock looks.
const EVERY: Duration = Duration::from_secs(60);

/// How long after start the first look waits, so the first window is up to
/// hold the projects it is about.
const FIRST_LOOK: Duration = Duration::from_secs(5);

/// The clock's state, for the life of the process.
struct Clock {
    /// When onehand opened: a due time before it was missed.
    opened_at: u64,
    /// The last task each schedule started, by schedule id: while it is still
    /// queued, running or waiting, the schedule is busy.
    last_task: HashMap<String, String>,
    /// Schedules whose time came while they were busy and that wait for it
    /// to end.
    queued: HashSet<String>,
    /// The last failure each schedule was notified of, so one reason is said
    /// once.
    told: HashMap<String, String>,
    /// A look is under way; the next waits for it.
    looking: bool,
    _tick: Task<()>,
}

impl Global for Clock {}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Start the clock.
pub(crate) fn boot(cx: &mut App) {
    let tick = cx.spawn(async move |cx| {
        cx.background_executor().timer(FIRST_LOOK).await;
        loop {
            cx.update(look);
            cx.background_executor().timer(EVERY).await;
        }
    });
    cx.set_global(Clock {
        opened_at: now(),
        last_task: HashMap::new(),
        queued: HashSet::new(),
        told: HashMap::new(),
        looking: false,
        _tick: tick,
    });
}

/// What a look starts: a schedule, and the window and shell it fires in.
type Firing = (
    Schedule,
    gpui::AnyWindowHandle,
    gpui::Entity<crate::shell::Shell>,
);

/// One look: read the file, decide each schedule, start what is due, and
/// write back what changed.
fn look(cx: &mut App) {
    let busy = cx.update_global::<Clock, _>(|clock, _| std::mem::replace(&mut clock.looking, true));
    if busy {
        return;
    }
    let path = schedule::path();
    cx.spawn(async move |cx| {
        let read = {
            let path = path.clone();
            cx.background_executor()
                .spawn(async move { schedule::load_blocking(&path) })
                .await
        };
        let mut all = match read {
            Ok(all) => all,
            Err(why) => {
                eprintln!("onehand: the schedules were not read: {why}");
                cx.update(|cx| cx.update_global::<Clock, _>(|c, _| c.looking = false));
                return;
            }
        };
        let now = now();
        let (firing, mut changed) = cx.update(|cx| decide_all(&mut all, now, cx));
        for (s, window, shell) in firing {
            let outcome = start(&s, window, shell, cx).await;
            let said = cx.update(|cx| record(&s, outcome, cx));
            if let Some(kept) = all.iter_mut().find(|kept| kept.id == s.id)
                && kept.said != said
            {
                kept.said = said;
                changed = true;
            }
        }
        if changed {
            // What the clock moved, onto the file as it is now: a start can
            // take a while (a worktree cut), and a hand edit made meanwhile
            // stands.
            let written = cx
                .background_executor()
                .spawn(async move {
                    let mut now_kept = schedule::load_blocking(&path)?;
                    for kept in &mut now_kept {
                        if let Some(looked) = all.iter().find(|s| s.id == kept.id) {
                            kept.last_fired = looked.last_fired;
                            kept.said = looked.said.clone();
                        }
                    }
                    schedule::save_blocking(&path, &now_kept)
                })
                .await;
            if let Err(why) = written {
                eprintln!("onehand: the schedules were not written: {why}");
            }
        }
        cx.update(|cx| cx.update_global::<Clock, _>(|c, _| c.looking = false));
    })
    .detach();
}

/// Decide every schedule at `now`, moving `last_fired` and `said` as each
/// comes to, and hand back what to start and whether anything changed.
fn decide_all(all: &mut [Schedule], now: u64, cx: &mut App) -> (Vec<Firing>, bool) {
    let windows: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .filter_map(|w| Some((w.handle, w.shell.upgrade()?)))
        .collect();
    let mut firing = Vec::new();
    let mut changed = false;
    // A time held for a schedule since switched off, or gone, is let go.
    cx.update_global::<Clock, _>(|c, _| {
        c.queued
            .retain(|id| all.iter().any(|s| s.on && &s.id == id))
    });
    for s in all.iter_mut().filter(|s| s.on) {
        let Some((window, shell)) = windows
            .iter()
            .find(|(_, shell)| shell.read(cx).holds_root(&s.project))
            .cloned()
        else {
            continue;
        };
        let clock = cx.global::<Clock>();
        let busy = clock
            .last_task
            .get(&s.id)
            .is_some_and(|task| crate::task::is_working(task, cx));
        // A time that came while it was busy goes once the last one is done.
        if !busy && clock.queued.contains(&s.id) {
            cx.update_global::<Clock, _>(|c, _| c.queued.remove(&s.id));
            firing.push((s.clone(), window, shell));
            continue;
        }
        let opened_at = clock.opened_at;
        match schedule::decide(s, now, opened_at, busy, &chrono::Local) {
            Act::Wait => continue,
            Act::Start { .. } => firing.push((s.clone(), window, shell)),
            Act::Skip(why) => s.said = Some(why),
            Act::Queue => {
                cx.update_global::<Clock, _>(|c, _| c.queued.insert(s.id.clone()));
            }
        }
        s.last_fired = Some(now);
        changed = true;
    }
    (firing, changed)
}

/// Start schedule `s` in `window`: its workflow, or the one-step workflow of
/// its prompt, filled with its mode and settings, as a task that keeps where
/// it came from and what its sessions may use. Answers the task's id.
async fn start(
    s: &Schedule,
    window: gpui::AnyWindowHandle,
    shell: gpui::Entity<crate::shell::Shell>,
    cx: &mut gpui::AsyncApp,
) -> Result<String, String> {
    let template = match &s.starts {
        Starts::Prompt { .. } => Ok(schedule::prompt_template()),
        Starts::Workflow { workflow, .. } => cx.update(|cx| {
            crate::workflow::templates(cx)
                .into_iter()
                .find_map(|entry| entry.template.ok().filter(|t| &t.id == workflow))
                .ok_or_else(|| format!("no workflow with id {workflow} in the library"))
        }),
    }?;
    let template = schedule::fill(template, s.mode.as_deref(), &s.options);
    let started = Started {
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
    };
    let brief = schedule::brief(s);
    let project = s.project.clone();
    match crate::control::start_task(window, shell, project, template, brief, started, cx).await? {
        (id, crate::task::Placed::Started | crate::task::Placed::Queued(_)) => Ok(id),
        (_, crate::task::Placed::Refused(why)) => Err(why),
    }
}

/// Keep what a start came to: the task it made, or why none, notified once
/// per reason. Answers what the schedule's row says now.
fn record(s: &Schedule, outcome: Result<String, String>, cx: &mut App) -> Option<String> {
    match outcome {
        Ok(task) => {
            cx.update_global::<Clock, _>(|c, _| {
                c.last_task.insert(s.id.clone(), task);
                c.told.remove(&s.id);
            });
            None
        }
        Err(why) => {
            let new = cx.update_global::<Clock, _>(|c, _| {
                c.told.insert(s.id.clone(), why.clone()).as_ref() != Some(&why)
            });
            if new {
                crate::chat::session::notify_scheduled(
                    s.name.clone(),
                    format!("did not start: {why}"),
                );
            }
            Some(format!("did not start: {why}"))
        }
    }
}
