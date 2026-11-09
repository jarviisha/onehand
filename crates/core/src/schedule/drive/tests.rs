use super::*;
use crate::schedule::{Missed, Overlap};
use chrono::Utc;
use std::collections::BTreeMap;

type Win = u8;
const NOW: u64 = 1_800_000_000;

/// A schedule of every minute on project `/p/<id>`, switched on an hour ago.
fn every_minute(id: &str) -> Schedule {
    Schedule {
        id: id.into(),
        name: format!("Schedule {id}"),
        project: PathBuf::from(format!("/p/{id}")),
        on: true,
        when: "* * * * *".into(),
        agent: None,
        mode: None,
        tools: vec!["list_issues".into()],
        labels: Vec::new(),
        missed: Missed::RunOnce,
        overlap: Overlap::Queue,
        on_since: NOW - 3600,
        last_fired: Some(NOW - 120),
        said: None,
        options: BTreeMap::new(),
        starts: Starts::Prompt {
            prompt: "look".into(),
        },
    }
}

/// A clock opened a while ago, past its first look's arming.
fn clock() -> ScheduleDrive<Win> {
    let (clock, fx) = ScheduleDrive::new(NOW - 600);
    assert_eq!(fx, vec![Effect::Arm(FIRST_LOOK)]);
    clock
}

/// The clock comes round and reads `all`, with each schedule's project held
/// by the window of the same number in `held`, and `working` tasks busy.
fn look(
    clock: &mut ScheduleDrive<Win>,
    all: Vec<Schedule>,
    held: &[(&str, Win)],
    working: &[&str],
) -> Vec<Effect<Win>> {
    let fx = clock.handle(Input::Elapsed, NOW, &Utc);
    assert_eq!(fx, vec![Effect::Read, Effect::Arm(EVERY)]);
    clock.handle(
        Input::Read {
            all: Ok(all),
            holders: held
                .iter()
                .map(|(id, w)| (PathBuf::from(format!("/p/{id}")), *w))
                .collect(),
            working: working.iter().map(|t| t.to_string()).collect(),
            library: Vec::new(),
        },
        NOW,
        &Utc,
    )
}

fn started(clock: &mut ScheduleDrive<Win>, task: &str) -> Vec<Effect<Win>> {
    clock.handle(
        Input::Started(Ok((task.into(), Placed::Started))),
        NOW,
        &Utc,
    )
}

fn start_in(fx: &[Effect<Win>]) -> Option<(&Starting, Win)> {
    fx.iter().find_map(|e| match e {
        Effect::Start { starting, window } => Some((&**starting, *window)),
        _ => None,
    })
}

fn written(fx: &[Effect<Win>]) -> Option<&[Schedule]> {
    fx.iter().find_map(|e| match e {
        Effect::Write { looked } => Some(looked.as_slice()),
        _ => None,
    })
}

/// A look while one is under way waits for it, and the clock is armed again
/// either way.
#[test]
fn a_look_waits_for_the_one_under_way() {
    let mut clock = clock();
    assert_eq!(clock.handle(Input::Elapsed, NOW, &Utc)[0], Effect::Read);
    assert_eq!(
        clock.handle(Input::Elapsed, NOW, &Utc),
        vec![Effect::Arm(EVERY)]
    );
}

/// A due schedule fires in the first window holding its project, one start
/// at a time, as a task that keeps where it came from and what its sessions
/// may use; then `last_fired` is written.
#[test]
fn a_due_schedule_starts_in_the_first_window_that_holds_it() {
    let mut clock = clock();
    let fx = look(
        &mut clock,
        vec![every_minute("a"), every_minute("b")],
        &[("a", 3), ("a", 4), ("b", 5)],
        &[],
    );
    let (starting, window) = start_in(&fx).expect("a starts");
    assert_eq!(window, 3);
    assert_eq!(starting.project, PathBuf::from("/p/a"));
    assert_eq!(starting.brief.body, "look");
    assert_eq!(
        starting.origin,
        Origin::Schedule {
            id: "a".into(),
            name: "Schedule a".into()
        }
    );
    assert_eq!(starting.grant.tools, vec!["list_issues".to_string()]);
    let fx = started(&mut clock, "t-a");
    assert_eq!(
        start_in(&fx).map(|(s, w)| (s.project.clone(), w)),
        Some((PathBuf::from("/p/b"), 5))
    );
    let fx = started(&mut clock, "t-b");
    let looked = written(&fx).expect("what moved is written");
    assert!(looked.iter().all(|s| s.last_fired == Some(NOW)));
    let fx = clock.handle(Input::Written(Ok(looked.to_vec())), NOW, &Utc);
    assert_eq!(fx, vec![Effect::Refresh]);
    assert_eq!(clock.read(), Some(&Ok(())));
    assert_eq!(clock.listed().len(), 2);
}

/// A schedule no window holds waits, unmoved, and nothing is written.
#[test]
fn a_schedule_no_window_holds_waits() {
    let mut clock = clock();
    let fx = look(&mut clock, vec![every_minute("a")], &[], &[]);
    assert_eq!(fx, vec![Effect::Refresh], "nothing to start or write");
    assert_eq!(clock.listed()[0].last_fired, Some(NOW - 120));
}

/// Due while its last task still works: skipped and said, or held and
/// started once that task is done; a held time is let go once the schedule
/// is switched off.
#[test]
fn a_busy_schedule_is_skipped_or_held_until_its_task_is_done() {
    let mut clock = clock();
    let mut skip = every_minute("s");
    skip.overlap = Overlap::Skip;
    look(
        &mut clock,
        vec![every_minute("q"), skip.clone()],
        &[("q", 1), ("s", 1)],
        &[],
    );
    started(&mut clock, "tq");
    let fx = started(&mut clock, "ts");
    clock.handle(
        Input::Written(Ok(written(&fx).unwrap().to_vec())),
        NOW,
        &Utc,
    );
    // Both still working at the next time.
    let mut q = every_minute("q");
    q.last_fired = Some(NOW - 60);
    skip.last_fired = Some(NOW - 60);
    let fx = look(
        &mut clock,
        vec![q.clone(), skip.clone()],
        &[("q", 1), ("s", 1)],
        &["tq", "ts"],
    );
    assert!(start_in(&fx).is_none());
    let looked = written(&fx).unwrap();
    assert_eq!(
        looked[1].said.as_deref(),
        Some("the last run was still going")
    );
    assert_eq!(looked[0].said, None, "held, not said");
    clock.handle(Input::Written(Ok(looked.to_vec())), NOW, &Utc);
    // Its task done, the held time goes even with nothing newly due.
    q.last_fired = Some(NOW);
    let fx = look(&mut clock, vec![q.clone()], &[("q", 1)], &[]);
    assert_eq!(
        start_in(&fx).map(|(s, _)| s.project.clone()),
        Some(PathBuf::from("/p/q"))
    );
    // Held again, then switched off: let go.
    started(&mut clock, "tq2");
    let mut busy = every_minute("q");
    busy.last_fired = Some(NOW - 60);
    let fx = look(&mut clock, vec![busy.clone()], &[("q", 1)], &["tq2"]);
    clock.handle(
        Input::Written(Ok(written(&fx).unwrap().to_vec())),
        NOW,
        &Utc,
    );
    busy.on = false;
    look(&mut clock, vec![busy.clone()], &[("q", 1)], &[]);
    busy.on = true;
    busy.last_fired = Some(NOW);
    let fx = look(&mut clock, vec![busy], &[("q", 1)], &[]);
    assert!(
        start_in(&fx).is_none(),
        "the held time went with the switch"
    );
}

/// A start that cannot go says why on its row and on the desktop, once per
/// reason, and is tried again at its next time.
#[test]
fn a_start_that_cannot_go_is_said_once_per_reason() {
    let mut clock = clock();
    let mut s = every_minute("a");
    s.starts = Starts::Workflow {
        workflow: "gone".into(),
        title: "t".into(),
        body: "b".into(),
    };
    let told = |fx: &[Effect<Win>]| {
        fx.iter()
            .filter(|e| matches!(e, Effect::Signal(Signal::NotStarted { .. })))
            .count()
    };
    let fx = look(&mut clock, vec![s.clone()], &[("a", 1)], &[]);
    assert_eq!(told(&fx), 1);
    let looked = written(&fx).unwrap();
    assert_eq!(
        looked[0].said.as_deref(),
        Some("did not start: no workflow with id gone in the library")
    );
    clock.handle(Input::Written(Ok(looked.to_vec())), NOW, &Utc);
    s.last_fired = Some(NOW - 60);
    let fx = look(&mut clock, vec![s.clone()], &[("a", 1)], &[]);
    assert_eq!(told(&fx), 0, "the same reason is said once");
    clock.handle(
        Input::Written(Ok(written(&fx).unwrap().to_vec())),
        NOW,
        &Utc,
    );
    // A refusal is another reason.
    let mut p = every_minute("a");
    p.last_fired = Some(NOW - 60);
    look(&mut clock, vec![p], &[("a", 1)], &[]);
    let fx = clock.handle(
        Input::Started(Ok(("t".into(), Placed::Refused("busy".into())))),
        NOW,
        &Utc,
    );
    assert_eq!(told(&fx), 1);
}

/// A write that fails is logged and the look's own list is shown; a switch
/// still being written stands over what a look read.
#[test]
fn a_switch_being_written_stands_over_a_look() {
    let mut clock = clock();
    let mut off = every_minute("a");
    off.on = false;
    look(&mut clock, vec![off.clone()], &[], &[]);
    let fx = clock.set_on("a", true, NOW, 9);
    assert!(matches!(&fx[1], Effect::Switch { id, on: true, since: NOW, window: 9 } if id == "a"));
    assert!(clock.listed()[0].on, "shown at once");
    // A look lands before the switch is written: it still reads off.
    look(&mut clock, vec![off.clone()], &[], &[]);
    assert!(clock.listed()[0].on, "the switch stands");
    let fx = clock.handle(
        Input::Switched {
            id: "a".into(),
            on: true,
            window: 9,
            failed: Some("disk full".into()),
            kept: Some(vec![off]),
        },
        NOW,
        &Utc,
    );
    assert!(matches!(
        &fx[0],
        Effect::Signal(Signal::SwitchFailed {
            window: 9,
            on: true,
            ..
        })
    ));
    assert!(!clock.listed()[0].on, "what the file holds is put back");
}

/// What the clock writes moves only `last_fired` and `said`, onto the file
/// as it is then.
#[test]
fn a_look_writes_only_what_it_moved() {
    let mut kept = vec![every_minute("a")];
    kept[0].name = "Edited by hand".into();
    let mut looked = every_minute("a");
    looked.last_fired = Some(NOW);
    looked.said = Some("skipped".into());
    crate::schedule::merge_looked(&mut kept, &[looked]);
    assert_eq!(kept[0].name, "Edited by hand");
    assert_eq!(kept[0].last_fired, Some(NOW));
    assert_eq!(kept[0].said.as_deref(), Some("skipped"));
    let mut all = vec![every_minute("a")];
    assert!(crate::schedule::switch(&mut all, "b", true, NOW).is_err());
    crate::schedule::switch(&mut all, "a", false, NOW).unwrap();
    assert!(!all[0].on);
}
