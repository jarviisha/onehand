//! The unattended carrier against its contract: its clock comes round and
//! ticks again, a start over the cap is refused where it was asked, a pause
//! stops every run, a config that cannot work is filed, and a report goes to
//! its issue once and leaves its task once it has landed.

use crate::state::Shared;
use crate::task::Tasks;
use gpui::{AnyWindowHandle, AppContext as _, TestAppContext};
use gpui_component::Root;
use onehand_core::config::UnattendedConfig;
use onehand_core::task::{Source, Task};
use onehand_core::unattended::{IssueSource, PendingReport, TrackerRef};
use onehand_core::workflow::Setup;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Blank;

impl gpui::Render for Blank {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("onehand-unattended-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The process's state, the tasks writing into `store`, and unattended runs
/// on `cfg` when given; a window to say things in.
fn app(cx: &mut TestAppContext, store: &Path, cfg: Option<UnattendedConfig>) -> AnyWindowHandle {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let store = store.to_path_buf();
    cx.update(|cx| {
        cx.set_global(Shared::for_test());
        cx.set_global(Tasks::in_dir(store));
        if let Some(cfg) = cfg {
            super::boot(&cfg, cx);
        }
    });
    cx.add_window(|window, cx| Root::new(cx.new(|_| Blank), window, cx))
        .into()
}

/// A task of the issue numbered `number` kept in `file`, working in `dir`,
/// owing it `unsent`.
fn issue_task(id: &str, number: u64, file: &Path, dir: &Path, unsent: Vec<PendingReport>) -> Task {
    let setup = Setup {
        repo: dir.to_path_buf(),
        dir: dir.to_path_buf(),
        branch: Some(format!("onehand/local-{number}-fix")),
        agent: None,
        check: None,
        mode: None,
        forge: None,
    };
    let mut task = Task::check(id.into(), "true".into(), setup);
    task.source = Source::Issue(IssueSource {
        tracker: TrackerRef::Local {
            file: file.to_path_buf(),
        },
        number,
        forge_ref: None,
        forge: None,
        base: "main".into(),
        picked: false,
        unsent,
        notes: Vec::new(),
    });
    task
}

/// A report of a run cut off and let go.
fn report() -> PendingReport {
    PendingReport {
        run: "a".into(),
        outcome: None,
        started: true,
        ended_on: None,
        asked: None,
        notes: Vec::new(),
    }
}

/// An issues file in `dir` holding issues numbered 1 and 2.
fn issues(dir: &Path) -> PathBuf {
    let file = dir.join("issues.json");
    for title in ["Fix it", "And this"] {
        let draft = onehand_core::issues::Draft {
            title: title.into(),
            ..Default::default()
        };
        onehand_core::issues::drive::write_blocking(
            &file,
            onehand_core::issues::drive::Change::Create(draft),
            1,
        )
        .unwrap();
    }
    file
}

fn issue_of(id: &str, cx: &mut TestAppContext) -> IssueSource {
    cx.update(|cx| crate::task::task(id, cx))
        .and_then(|task| task.issue().cloned())
        .unwrap()
}

/// Run what is under way until task `id` owes its issue nothing.
fn delivered(id: &str, cx: &mut TestAppContext) {
    for _ in 0..100 {
        cx.run_until_parked();
        if issue_of(id, cx).unsent.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("never delivered");
}

/// The notes left on issue `number` of `file`.
fn notes(file: &Path, number: u64) -> Vec<String> {
    let kept = onehand_core::issues::load_blocking(file).unwrap();
    kept.get(number)
        .unwrap()
        .notes
        .iter()
        .map(|note| note.text.clone())
        .collect()
}

/// A report goes to its issue once, however many times delivery is asked
/// for while it is on its way, and leaves its task once it has landed.
#[gpui::test]
fn a_report_is_sent_once_and_dropped_once_it_lands(cx: &mut TestAppContext) {
    let dir = temp_dir("deliver");
    app(cx, &dir.join("tasks"), Some(UnattendedConfig::default()));
    let file = issues(&dir);
    let task = issue_task("a", 1, &file, &dir, vec![report()]);
    cx.update(|cx| crate::task::add(task, cx));
    cx.update(|cx| {
        for _ in 0..2 {
            crate::task::with_unattended(cx, |u, seen| ((), u.deliver("a", seen)));
        }
    });
    delivered("a", cx);
    let said = notes(&file, 1);
    assert_eq!(said.len(), 1, "told once: {said:?}");
    assert!(said[0].contains("cut off"), "{}", said[0]);
    let _ = std::fs::remove_dir_all(dir);
}

/// The clock comes round on its own and is armed again, and a tick sends
/// what is owed even with nothing switched on.
#[gpui::test]
fn the_clock_ticks_and_a_tick_sends_what_is_owed(cx: &mut TestAppContext) {
    let dir = temp_dir("clock");
    let cfg = UnattendedConfig {
        every: "1m".into(),
        ..Default::default()
    };
    app(cx, &dir.join("tasks"), Some(cfg));
    let file = issues(&dir);
    cx.update(|cx| crate::task::add(issue_task("a", 1, &file, &dir, vec![report()]), cx));
    cx.run_until_parked();
    assert_eq!(
        issue_of("a", cx).unsent.len(),
        1,
        "nothing before the clock"
    );
    cx.executor().advance_clock(Duration::from_secs(61));
    delivered("a", cx);
    assert_eq!(notes(&file, 1).len(), 1);
    cx.update(|cx| crate::task::add(issue_task("b", 2, &file, &dir, vec![report()]), cx));
    cx.executor().advance_clock(Duration::from_secs(61));
    delivered("b", cx);
    assert_eq!(notes(&file, 2).len(), 1, "the next fire came");
    let _ = std::fs::remove_dir_all(dir);
}

/// A start of an issue's task with every slot taken is refused through the
/// one way every start goes, and answered to whoever asked.
#[gpui::test]
fn a_start_over_the_cap_is_refused_where_it_was_asked(cx: &mut TestAppContext) {
    let dir = temp_dir("cap");
    let cfg = UnattendedConfig {
        at_once: 0,
        ..Default::default()
    };
    let window = app(cx, &dir.join("tasks"), Some(cfg));
    let file = dir.join("issues.json");
    let task = issue_task("a", 1, &file, &dir, Vec::new());
    let told = cx.update(|cx| crate::task::submit_told(task, window, cx));
    cx.run_until_parked();
    let placed = futures::executor::block_on(told).unwrap();
    assert!(
        matches!(&placed, crate::task::Placed::Refused(why) if why.contains("capped at none")),
        "{placed:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// With unattended runs not set up, nothing is capped.
#[gpui::test]
fn nothing_is_capped_with_unattended_runs_off(cx: &mut TestAppContext) {
    let dir = temp_dir("off");
    let window = app(cx, &dir.join("tasks"), None);
    let file = dir.join("issues.json");
    let task = issue_task("a", 1, &file, &dir, Vec::new());
    let told = cx.update(|cx| crate::task::submit_told(task, window, cx));
    cx.run_until_parked();
    assert!(cx.update(|cx| super::slots(cx)).is_none());
    // Past the cap to its place: refused only because this window holds no
    // shell to drive it in.
    let placed = futures::executor::block_on(told).unwrap();
    assert_eq!(
        placed,
        crate::task::Placed::Refused("the window closed".into())
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A run that failed on a setting pauses every later one until onehand is
/// restarted: it is what stops every run, said with what to fix.
#[gpui::test]
fn a_pause_stops_every_run_and_says_what_to_fix(cx: &mut TestAppContext) {
    let dir = temp_dir("pause");
    app(cx, &dir.join("tasks"), Some(UnattendedConfig::default()));
    cx.update(|cx| {
        super::run_said(
            "t",
            onehand_core::task::drive::Said::Pause {
                why: "The agent does not offer mode `x`".into(),
                fix: "unattended.mode is fixed".into(),
            },
            cx,
        )
    });
    let why = cx
        .update(|cx| super::blocked(cx))
        .expect("every run is stopped");
    assert_eq!(
        why,
        "The agent does not offer mode `x`. Unattended runs are paused until \
         unattended.mode is fixed and onehand restarted."
    );
}

/// A config that cannot work is filed as what stops every run: no label, or
/// an interval that does not parse.
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
        let filed = cx.update(|cx| super::blocked(cx));
        assert!(
            filed.as_deref().is_some_and(|why| why.contains(said)),
            "{filed:?}"
        );
    }
}

/// A run's note that it took its issue up is written once, through the
/// file's drive, and shows in a view open on the file without a rescan.
#[gpui::test]
fn a_runs_taken_up_note_is_written_once(cx: &mut TestAppContext) {
    let dir = temp_dir("taken-up");
    app(cx, &dir.join("tasks"), None);
    let (storage, root) = (dir.join("storage"), dir.join("project"));
    std::fs::create_dir_all(&root).unwrap();
    let file = onehand_core::issues::file_for(&storage, &root);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let draft = onehand_core::issues::Draft {
        title: "Fix it".into(),
        ..Default::default()
    };
    let create = onehand_core::issues::drive::Change::Create(draft);
    onehand_core::issues::drive::write_blocking(&file, create, 1).unwrap();
    let view = cx.update(|cx| onehand_issues::testing::OpenView::new(&storage, &root, cx));
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.update(|cx| {
        super::perform(
            vec![onehand_core::unattended::drive::Effect::NoteTakenUp {
                root: root.clone(),
                file: file.clone(),
                number: 1,
                said: "Taken up by an unattended run".into(),
                session: "s-1".into(),
            }],
            cx,
        )
    });
    for _ in 0..50 {
        cx.run_until_parked();
        if !notes(&file, 1).is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    cx.run_until_parked();
    assert_eq!(notes(&file, 1), ["Taken up by an unattended run"]);
    let kept = onehand_core::issues::load_blocking(&file).unwrap();
    let session = kept.get(1).unwrap().notes[0].session.clone();
    assert_eq!(session.as_deref(), Some("s-1"));
    let shown = cx.update(|cx| view.shown(cx)).unwrap();
    let said: Vec<_> = shown
        .get(1)
        .unwrap()
        .notes
        .iter()
        .map(|n| n.text.clone())
        .collect();
    assert_eq!(said, ["Taken up by an unattended run"]);
    let _ = std::fs::remove_dir_all(dir);
}
