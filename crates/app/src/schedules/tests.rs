//! What the clock decides today in the app, held before it moves: a
//! schedule whose project no window holds waits, and a time held for one
//! since switched off is let go.

use super::{Clock, decide_all};
use crate::state::Shared;
use gpui::TestAppContext;
use onehand_core::schedule::{Missed, Overlap, Schedule, Starts};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A schedule of every minute, switched on an hour before `now`.
fn every_minute(id: &str, now: u64) -> Schedule {
    Schedule {
        id: id.into(),
        name: format!("Schedule {id}"),
        project: "/nowhere/onehand-test".into(),
        on: true,
        when: "* * * * *".into(),
        agent: None,
        mode: None,
        tools: Vec::new(),
        labels: Vec::new(),
        missed: Missed::RunOnce,
        overlap: Overlap::Queue,
        on_since: now - 3600,
        last_fired: None,
        said: None,
        options: BTreeMap::new(),
        starts: Starts::Prompt {
            prompt: "look".into(),
        },
    }
}

/// The process's state, and a clock holding a time for each of `queued`.
fn app(cx: &mut TestAppContext, opened_at: u64, queued: &[&str]) {
    let queued: HashSet<String> = queued.iter().map(|id| id.to_string()).collect();
    cx.update(|cx| {
        cx.set_global(Shared::for_test());
        cx.set_global(Clock {
            opened_at,
            last_task: HashMap::new(),
            queued,
            told: HashMap::new(),
            looking: false,
            all: Vec::new(),
            read: None,
            pending: HashMap::new(),
            _tick: gpui::Task::ready(()),
        });
    });
}

/// Due, but no window holds its project: it waits, its last time unmoved,
/// rather than firing or being said as skipped.
#[gpui::test]
fn a_schedule_no_window_holds_waits(cx: &mut TestAppContext) {
    let now = 1_800_000_000;
    app(cx, now - 60, &[]);
    let mut all = vec![every_minute("a", now)];
    let (firing, changed) = cx.update(|cx| decide_all(&mut all, now, cx));
    assert!(firing.is_empty() && !changed);
    assert_eq!(all[0].last_fired, None);
    assert_eq!(all[0].said, None);
}

/// A time held for a schedule that is then switched off, or gone from the
/// file, is let go; one still on keeps its.
#[gpui::test]
fn a_held_time_is_let_go_when_its_schedule_is_switched_off(cx: &mut TestAppContext) {
    let now = 1_800_000_000;
    app(cx, now - 60, &["on", "off", "gone"]);
    let mut off = every_minute("off", now);
    off.on = false;
    let mut all = vec![every_minute("on", now), off];
    cx.update(|cx| decide_all(&mut all, now, cx));
    let held = cx.update(|cx| cx.global::<Clock>().queued.clone());
    assert_eq!(held, HashSet::from(["on".to_string()]));
}
