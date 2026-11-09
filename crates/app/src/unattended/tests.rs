//! What unattended runs decide today in the app, held before it moves: the
//! cap and the runs still starting, the pause, a config that cannot work, and
//! a report delivered once.

use super::{Unattended, blocked, deliver, over_cap, pause, placed, slots};
use crate::state::Shared;
use crate::task::Tasks;
use gpui::TestAppContext;
use onehand_core::config::UnattendedConfig;
use onehand_core::task::{Source, Task};
use onehand_core::unattended::{IssueSource, PendingReport, TrackerRef};
use onehand_core::workflow::Setup;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("onehand-unattended-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The unattended state as `boot` files it for a config that works, with
/// room for `at_once` runs and no clock.
fn unattended(at_once: u32) -> Unattended {
    Unattended {
        label: "auto".into(),
        timeout: "45m".into(),
        mode: None,
        agent: None,
        workflow: "builtin:issue".into(),
        workflows: BTreeMap::new(),
        at_once,
        waiting: None,
        claiming: false,
        blocked: None,
        paused: None,
        delivering: HashSet::new(),
        starting: HashSet::new(),
        accounts: None,
        checked_at: None,
        checks_out: 0,
        problems: HashMap::new(),
        _tick: gpui::Task::ready(()),
    }
}

/// The process's state and the tasks' global writing into `store`, with
/// unattended runs as `u` has them.
fn app(cx: &mut TestAppContext, store: &Path, u: Option<Unattended>) {
    cx.executor().allow_parking();
    let store = store.to_path_buf();
    cx.update(|cx| {
        let mut shared = Shared::for_test();
        shared.unattended = u;
        cx.set_global(shared);
        cx.set_global(Tasks::in_dir(store));
    });
}

/// A task of the issue numbered `number` kept in `file`, working in `dir`.
fn issue_task(id: &str, number: u64, file: &Path, dir: &Path) -> Task {
    let mut task = Task::check(id.into(), "true".into(), setup(dir));
    task.source = Source::Issue(IssueSource {
        tracker: TrackerRef::Local {
            file: file.to_path_buf(),
        },
        number,
        forge_ref: None,
        forge: None,
        base: "main".into(),
        picked: false,
        unsent: Vec::new(),
        notes: Vec::new(),
    });
    task
}

fn setup(dir: &Path) -> Setup {
    Setup {
        repo: dir.to_path_buf(),
        dir: dir.to_path_buf(),
        branch: Some("onehand/local-1-fix".into()),
        agent: None,
        check: None,
        mode: None,
        forge: None,
    }
}

fn issue_of(id: &str, cx: &mut TestAppContext) -> IssueSource {
    cx.update(|cx| crate::task::task(id, cx))
        .and_then(|task| task.issue().cloned())
        .unwrap()
}

/// A task kept by the tick or a pick counts against the cap until it has
/// asked for its place, so a tick landing in between cannot start one more;
/// it is never refused by its own count, and a task of no issue never is.
#[gpui::test]
fn a_task_still_starting_holds_a_slot_until_it_asks_for_its_place(cx: &mut TestAppContext) {
    let dir = temp_dir("cap");
    app(cx, &dir.join("tasks"), Some(unattended(1)));
    let file = dir.join("issues.json");
    cx.update(|cx| {
        crate::task::add(issue_task("a", 1, &file, &dir), cx);
        crate::task::add(issue_task("b", 2, &file, &dir), cx);
        crate::task::add(Task::check("c".into(), "true".into(), setup(&dir)), cx);
        super::with(cx, |u| u.starting.insert("a".into()));
    });
    cx.update(|cx| {
        assert_eq!(over_cap("a", cx), None, "its own count never refuses it");
        let full = over_cap("b", cx).expect("the one slot is taken");
        assert!(full.contains("Slots: 1 of 1"), "{full}");
        assert_eq!(over_cap("c", cx), None, "only an issue's task is capped");
        assert_eq!(slots(cx).unwrap().starting, 1);
    });
    cx.update(|cx| placed("a", cx));
    cx.update(|cx| {
        assert_eq!(slots(cx).unwrap().starting, 0);
        assert_eq!(over_cap("b", cx), None, "nothing holds the slot now");
    });
}

/// With unattended runs not set up, nothing is capped.
#[gpui::test]
fn nothing_is_capped_with_unattended_runs_off(cx: &mut TestAppContext) {
    let dir = temp_dir("off");
    app(cx, &dir.join("tasks"), None);
    let file = dir.join("issues.json");
    cx.update(|cx| crate::task::add(issue_task("a", 1, &file, &dir), cx));
    cx.update(|cx| {
        assert_eq!(over_cap("a", cx), None);
        assert!(slots(cx).is_none());
    });
}

/// A run that failed on a setting pauses every later one until onehand is
/// restarted: it is what stops every run, said with what to fix.
#[gpui::test]
fn a_pause_stops_every_run_and_says_what_to_fix(cx: &mut TestAppContext) {
    let dir = temp_dir("pause");
    app(cx, &dir.join("tasks"), Some(unattended(1)));
    cx.update(|cx| {
        pause(
            "The agent does not offer mode `x`",
            "unattended.mode is fixed",
            cx,
        )
    });
    let why = cx.update(|cx| blocked(cx)).expect("every run is stopped");
    assert_eq!(
        why,
        "The agent does not offer mode `x`. Unattended runs are paused until \
         unattended.mode is fixed and onehand restarted."
    );
}

/// A config that cannot work is filed as what stops every run, whatever the
/// workflows say: no label, or an interval that does not parse.
#[gpui::test]
fn boot_files_why_a_config_cannot_work(cx: &mut TestAppContext) {
    let dir = temp_dir("boot");
    app(cx, &dir.join("tasks"), None);
    let cases = [
        (
            UnattendedConfig {
                label: "  ".into(),
                ..Default::default()
            },
            "no label is set",
        ),
        (
            UnattendedConfig {
                every: "soon".into(),
                ..Default::default()
            },
            "take a number and a unit",
        ),
        (
            UnattendedConfig {
                timeout: "forever".into(),
                ..Default::default()
            },
            "take a number and a unit",
        ),
    ];
    for (cfg, said) in cases {
        cx.update(|cx| super::boot(&cfg, cx));
        let filed = cx.update(|cx| {
            Shared::global(cx)
                .unattended
                .as_ref()
                .and_then(|u| u.blocked.clone())
        });
        assert!(
            filed.as_deref().is_some_and(|why| why.contains(said)),
            "{filed:?}"
        );
    }
    cx.update(|cx| super::boot(&UnattendedConfig::default(), cx));
    let filed = cx.update(|cx| {
        Shared::global(cx)
            .unattended
            .as_ref()
            .unwrap()
            .blocked
            .clone()
    });
    assert_eq!(filed, None);
}

/// A report goes to its issue once, however many times delivery is asked
/// for while it is on its way, and is dropped from the task once it lands.
#[gpui::test]
fn a_report_is_sent_once_and_dropped_once_it_lands(cx: &mut TestAppContext) {
    let dir = temp_dir("deliver");
    app(cx, &dir.join("tasks"), Some(unattended(1)));
    let file = dir.join("issues.json");
    onehand_core::issues::update_blocking(&file, |kept| {
        kept.create(
            onehand_core::issues::Draft {
                title: "Fix it".into(),
                ..Default::default()
            },
            1,
        )
    })
    .unwrap();
    let mut task = issue_task("a", 1, &file, &dir);
    if let Source::Issue(issue) = &mut task.source {
        issue.unsent.push(PendingReport {
            run: "a".into(),
            outcome: None,
            started: true,
            ended_on: None,
            asked: None,
            notes: Vec::new(),
        });
    }
    cx.update(|cx| crate::task::add(task, cx));
    cx.update(|cx| {
        deliver("a".into(), cx);
        deliver("a".into(), cx);
        super::deliver_all(cx);
    });
    for _ in 0..100 {
        cx.run_until_parked();
        let out = cx.update(|cx| {
            Shared::global(cx)
                .unattended
                .as_ref()
                .is_some_and(|u| !u.delivering.is_empty())
        });
        if !out {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        issue_of("a", cx).unsent.is_empty(),
        "dropped once it landed"
    );
    let kept = onehand_core::issues::load_blocking(&file).unwrap();
    let notes = &kept.get(1).unwrap().notes;
    assert_eq!(notes.len(), 1, "told once: {notes:?}");
    assert!(notes[0].text.contains("cut off"), "{}", notes[0].text);
    let _ = std::fs::remove_dir_all(dir);
}
