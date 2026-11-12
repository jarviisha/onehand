use super::*;
use crate::config::AgentSpec;
use crate::task::drive::Said;
use crate::task::flows::harness::repo;
use crate::task::tasks::{self, Input as TasksInput, TasksDrive};
use crate::unattended::link::{Joined, Out};
use crate::unattended::TrackerRef;
use crate::workflow::{GateKind, Outcome, Place, StepKind, StepSpec, Stop};
use std::path::Path;

type Win = u8;
const WINDOW: Win = 1;
const PICKED: Win = 2;

/// A worktree workflow called `id`: an agent turn that commits, then the
/// project's check command when `needs_check`.
fn template(id: &str, needs_check: bool) -> Template {
    let mut t = Template::blank(id);
    t.id = id.to_string();
    t.place = Place::Worktree;
    t.steps = vec![StepSpec {
        id: "work".into(),
        label: "Work".into(),
        kind: StepKind::Agent {
            prompt: "go".into(),
            gates: vec![GateKind::Committed],
            keep_answer: false,
            mode: None,
            options: BTreeMap::new(),
        },
    }];
    if needs_check {
        t.steps.push(StepSpec {
            id: "check".into(),
            label: "Check".into(),
            kind: StepKind::Command {
                command: None,
                on_fail: "work".into(),
            },
        });
    }
    t
}

fn config(at_once: u32, waiting: Option<u32>) -> UnattendedConfig {
    UnattendedConfig {
        workflow: "plain".into(),
        at_once,
        waiting,
        ..Default::default()
    }
}

/// The tasks, unattended runs on `cfg`, and the world they read: the
/// workflows `plain` and `cmd` (which runs the check), and one agent.
struct Rig {
    tasks: TasksDrive<Win>,
    u: UnattendedDrive,
    world: World,
}

impl Rig {
    fn new(cfg: UnattendedConfig) -> Self {
        let (u, _) = UnattendedDrive::new::<Win>(&cfg, &[]);
        Self {
            tasks: TasksDrive::default(),
            u,
            world: World {
                library: vec![template("plain", false), template("cmd", true)],
                agents: vec![AgentSpec {
                    name: "a".into(),
                    ..Default::default()
                }],
                offers: Vec::new(),
            },
        }
    }

    /// Give unattended runs `command`, routed through the link.
    fn on<R: Default>(
        &mut self,
        command: impl FnOnce(&mut UnattendedDrive, &Seen<Win>, &World) -> (R, Vec<Effect<Win>>),
    ) -> (R, Out<Win>) {
        let Rig { tasks, u, world } = self;
        let mut joined = Joined::new(tasks, [], Some(u));
        joined.unattended(|u, seen| command(u, seen, world))
    }

    fn fx(
        &mut self,
        command: impl FnOnce(&mut UnattendedDrive, &Seen<Win>, &World) -> Vec<Effect<Win>>,
    ) -> Out<Win> {
        self.on(|u, seen, world| ((), command(u, seen, world))).1
    }

    fn handle(&mut self, input: Input<Win>, roots: Vec<On>) -> Out<Win> {
        self.fx(|u, seen, world| {
            u.handle(
                input,
                seen,
                roots,
                world,
                crate::Now {
                    secs: 0,
                    at: std::time::Instant::now(),
                },
            )
        })
    }

    fn tick(&mut self, roots: Vec<On>) -> Out<Win> {
        self.fx(|u, seen, world| u.tick(None, seen, roots, world))
    }

    /// What run `id` told unattended runs, through the link.
    fn said(&mut self, id: &str, said: Said, roots: Vec<On>) -> Out<Win> {
        let Rig { tasks, u, world } = self;
        let world = said.looks().then_some((roots, &*world));
        Joined::new(tasks, [], Some(u)).run_said(id, said, world)
    }

    fn request(&mut self, id: &str) -> Out<Win> {
        let Rig { tasks, u, .. } = self;
        Joined::new(tasks, [], Some(u)).request(id, WINDOW)
    }

    /// Task `id`'s place was found free at `place`, as the carrier reports.
    fn found_place(&mut self, id: &str, place: &str) -> Out<Win> {
        let Rig { tasks, u, .. } = self;
        let input = TasksInput::PlaceFound {
            task: id.into(),
            place: PathBuf::from(place),
            window: WINDOW,
            window_open: true,
        };
        Joined::new(tasks, [], Some(u))
            .tasks(|t| ((), t.handle(input)))
            .1
    }

    fn slots(&mut self) -> Slots {
        let Rig { tasks, u, .. } = self;
        u.slots(&Seen::new(tasks, []))
    }
}

/// An issue task numbered `number` of the issues kept in `file`, in `dir`.
fn issue_task(id: &str, number: u64, file: &Path, dir: &Path) -> Task {
    let setup = Setup {
        repo: dir.to_path_buf(),
        dir: dir.to_path_buf(),
        branch: Some(format!("onehand/local-{number}-fix")),
        agent: None,
        check: None,
        mode: None,
        forge: None,
    };
    let brief = crate::workflow::Brief {
        title: format!("#{number}"),
        body: String::new(),
        instructions: None,
    };
    Task::of_issue(
        id.into(),
        template("plain", false),
        brief,
        setup,
        IssueSource {
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
        },
    )
}

/// Every effect's variant by name, a signal's as `Signal::` and its own:
/// matched whole, so a new variant is named here before any test passes it.
fn name<W>(e: &Effect<W>) -> &'static str {
    match e {
        Effect::Arm(_) => "Arm",
        Effect::Check { .. } => "Check",
        Effect::Look(_) => "Look",
        Effect::Claim { .. } => "Claim",
        Effect::Take { .. } => "Take",
        Effect::ClaimPicked { .. } => "ClaimPicked",
        Effect::Tell { .. } => "Tell",
        Effect::Submit { .. } => "Submit",
        Effect::AnswerReview { .. } => "AnswerReview",
        Effect::Send(_) => "Send",
        Effect::Drain { .. } => "Drain",
        Effect::Teardown { .. } => "Teardown",
        Effect::NoteTakenUp { .. } => "NoteTakenUp",
        Effect::Synced(_) => "Synced",
        Effect::Tick => "Tick",
        Effect::Refresh => "Refresh",
        Effect::Signal(Signal::Warn { .. }) => "Signal::Warn",
        Effect::Signal(Signal::Log(_)) => "Signal::Log",
    }
}

fn looks<W>(out: &Out<W>) -> Option<&Look<W>> {
    out.unattended.iter().find_map(|e| match e {
        Effect::Look(look) => Some(&**look),
        Effect::Arm(_)
        | Effect::Check { .. }
        | Effect::Claim { .. }
        | Effect::Take { .. }
        | Effect::ClaimPicked { .. }
        | Effect::Tell { .. }
        | Effect::Submit { .. }
        | Effect::AnswerReview { .. }
        | Effect::Send(_)
        | Effect::Drain { .. }
        | Effect::Teardown { .. }
        | Effect::NoteTakenUp { .. }
        | Effect::Synced(_)
        | Effect::Tick
        | Effect::Signal(_)
        | Effect::Refresh => None,
    })
}

/// The first look in `out`, taken out of it.
fn take_look<W>(out: Out<W>) -> Option<Look<W>> {
    out.unattended.into_iter().find_map(|e| match e {
        Effect::Look(look) => Some(*look),
        Effect::Arm(_)
        | Effect::Check { .. }
        | Effect::Claim { .. }
        | Effect::Take { .. }
        | Effect::ClaimPicked { .. }
        | Effect::Tell { .. }
        | Effect::Submit { .. }
        | Effect::AnswerReview { .. }
        | Effect::Send(_)
        | Effect::Drain { .. }
        | Effect::Teardown { .. }
        | Effect::NoteTakenUp { .. }
        | Effect::Synced(_)
        | Effect::Tick
        | Effect::Signal(_)
        | Effect::Refresh => None,
    })
}

fn warned<W>(out: &Out<W>) -> Option<&str> {
    out.unattended.iter().find_map(|e| match e {
        Effect::Signal(Signal::Warn { said, .. }) => Some(said.as_str()),
        Effect::Signal(Signal::Log(_))
        | Effect::Arm(_)
        | Effect::Check { .. }
        | Effect::Look(_)
        | Effect::Claim { .. }
        | Effect::Take { .. }
        | Effect::ClaimPicked { .. }
        | Effect::Tell { .. }
        | Effect::Submit { .. }
        | Effect::AnswerReview { .. }
        | Effect::Send(_)
        | Effect::Drain { .. }
        | Effect::Teardown { .. }
        | Effect::NoteTakenUp { .. }
        | Effect::Synced(_)
        | Effect::Tick
        | Effect::Refresh => None,
    })
}

/// How long an arm waits.
fn armed<W>(e: &Effect<W>) -> Option<Duration> {
    match e {
        Effect::Arm(every) => Some(*every),
        Effect::Check { .. }
        | Effect::Look(_)
        | Effect::Claim { .. }
        | Effect::Take { .. }
        | Effect::ClaimPicked { .. }
        | Effect::Tell { .. }
        | Effect::Submit { .. }
        | Effect::AnswerReview { .. }
        | Effect::Send(_)
        | Effect::Drain { .. }
        | Effect::Teardown { .. }
        | Effect::NoteTakenUp { .. }
        | Effect::Synced(_)
        | Effect::Tick
        | Effect::Signal(_)
        | Effect::Refresh => None,
    }
}

/// A config that cannot work is filed as what stops every run, and the
/// clock still runs: it is also what keeps the rows' answers current.
#[test]
fn a_config_that_cannot_work_stops_every_run_and_the_clock_still_runs() {
    let cfg = UnattendedConfig {
        label: " ".into(),
        ..config(1, None)
    };
    let (u, fx) = UnattendedDrive::new::<Win>(&cfg, &[]);
    assert!(u
        .blocked(&World::default())
        .unwrap()
        .contains("no label is set"));
    assert_eq!(fx.last().and_then(armed), Some(Duration::from_secs(1800)));
    let mut rig = Rig {
        u,
        ..Rig::new(config(1, None))
    };
    let project = repo("blocked", &[("Fix", &["auto"])]);
    let out = rig.handle(Input::Elapsed, vec![(project.clone(), false)]);
    let look = looks(&out).expect("the projects are still looked at");
    assert!(look.search.is_none(), "but nothing is searched");
    assert_eq!(out.unattended.last().map(name), Some("Arm"), "armed again");
    let _ = std::fs::remove_dir_all(project.root);
}

/// Nothing switched on is nothing to look at: a tick asks no connector
/// anything, and only sends what is owed.
#[test]
fn a_tick_with_nothing_switched_on_looks_at_nothing() {
    let mut rig = Rig::new(config(1, None));
    let out = rig.tick(Vec::new());
    assert!(looks(&out).is_none());
    assert!(!rig.u.claiming());
}

/// The tick runs end to end: the search finds the oldest labelled issue,
/// claims it, keeps it as a task that asks for its place, counted against
/// the cap until it has, and looks again at once for the next slot.
#[test]
fn a_tick_claims_an_issue_and_its_task_asks_for_its_place() {
    let mut rig = Rig::new(config(2, None));
    let project = repo("flow", &[("Fix the bug", &["auto"])]);
    let roots = vec![(project.clone(), false)];
    let out = rig.tick(roots.clone());
    assert!(rig.u.claiming(), "a second tick waits for this one");
    assert!(looks(&rig.tick(roots.clone())).unwrap().search.is_none());
    let look = take_look(out);
    let looked = look.unwrap().run_blocking();
    let out = rig.handle(Input::Looked(Box::new(looked)), roots.clone());
    let (found, label) = out
        .unattended
        .into_iter()
        .find_map(|e| match e {
            Effect::Claim { found, label, .. } => Some((found, label)),
            Effect::Arm(_)
            | Effect::Check { .. }
            | Effect::Look(_)
            | Effect::Take { .. }
            | Effect::ClaimPicked { .. }
            | Effect::Tell { .. }
            | Effect::Submit { .. }
            | Effect::AnswerReview { .. }
            | Effect::Send(_)
            | Effect::Drain { .. }
            | Effect::Teardown { .. }
            | Effect::NoteTakenUp { .. }
            | Effect::Synced(_)
            | Effect::Tick
            | Effect::Signal(_)
            | Effect::Refresh => None,
        })
        .expect("the issue found is claimed");
    let (begun, synced) = blocking::claim_found(*found, &label);
    let out = rig.handle(
        Input::Claimed {
            begun,
            synced,
            asked_from: None,
            held: None,
            holding: vec![(WINDOW, None)],
            new_task: "t1".into(),
        },
        roots,
    );
    let task = rig.tasks.task("t1").expect("kept as a task");
    assert_eq!(task.issue().unwrap().number, 1);
    assert!(!task.issue().unwrap().picked);
    assert_eq!(task.setup.agent.as_deref(), Some("a"));
    let find = tasks::Effect::FindPlace {
        task: "t1".into(),
        dir: task_dir(&rig, "t1"),
        window: WINDOW,
    };
    assert!(out.tasks.contains(&find), "{:?}", out.tasks);
    assert_eq!(
        out.unattended.last().map(name),
        Some("Tick"),
        "and looks again"
    );
    assert!(!rig.u.claiming());
    assert_eq!(
        rig.slots().starting,
        1,
        "counted until it asked for its place"
    );
    rig.found_place("t1", &project.root.display().to_string());
    assert_eq!(
        rig.slots().starting,
        0,
        "placed: it holds its slot as a run"
    );
    assert_eq!(rig.slots().holders.len(), 1);
    let _ = std::fs::remove_dir_all(task_dir(&rig, "t1"));
    let _ = std::fs::remove_dir_all(project.root);
}

/// A sync a claim made of a file kept in step is told that file's drive
/// before anything else the claim comes to, and a claim that synced nothing
/// tells nothing.
#[test]
fn a_claims_sync_is_told_its_files_drive_first() {
    use crate::issues::sync::{OutsideSync, Report};
    let mut rig = Rig::new(config(1, None));
    let outside = OutsideSync {
        file: "/p/issues.json".into(),
        kept: crate::issues::Issues::default(),
        report: Report::default(),
    };
    let claimed = |synced| Input::Claimed {
        begun: None,
        synced,
        asked_from: None,
        held: None,
        holding: Vec::new(),
        new_task: "t1".into(),
    };
    let out = rig.handle(claimed(vec![outside.clone()]), Vec::new());
    let first = out.unattended.first().expect("an effect");
    assert_eq!(name(first), "Synced", "told first");
    assert_eq!(synced_of(first), Some(&outside));
    let out = rig.handle(claimed(Vec::new()), Vec::new());
    assert!(!out.unattended.iter().map(name).any(|n| n == "Synced"));
}

/// The sync `effect` tells, when it tells one.
fn synced_of<W>(effect: &Effect<W>) -> Option<&crate::issues::sync::OutsideSync> {
    match effect {
        Effect::Synced(outside) => Some(outside),
        Effect::Arm(_)
        | Effect::Check { .. }
        | Effect::Look(_)
        | Effect::Claim { .. }
        | Effect::Take { .. }
        | Effect::ClaimPicked { .. }
        | Effect::Tell { .. }
        | Effect::Submit { .. }
        | Effect::AnswerReview { .. }
        | Effect::Send(_)
        | Effect::Drain { .. }
        | Effect::Teardown { .. }
        | Effect::NoteTakenUp { .. }
        | Effect::Tick
        | Effect::Signal(_)
        | Effect::Refresh => None,
    }
}

fn task_dir(rig: &Rig, id: &str) -> PathBuf {
    rig.tasks.task(id).unwrap().setup.dir.clone()
}

/// A search asked for now says what came of it: nothing switched on, the
/// cap reached, or no labelled issue found.
#[test]
fn a_search_asked_for_now_always_says_what_came_of_it() {
    let mut rig = Rig::new(config(1, None));
    let out = rig.fx(|u, seen, world| u.look_now(WINDOW, seen, Vec::new(), world));
    assert_eq!(
        warned(&out),
        Some("No project is switched on for unattended runs.")
    );
    let project = repo("now", &[("Unlabelled", &[])]);
    let roots = vec![(project.clone(), false)];
    let out = rig.fx(|u, seen, world| u.look_now(WINDOW, seen, roots.clone(), world));
    let look = take_look(out);
    let out = rig.handle(Input::Looked(Box::new(look.unwrap().run_blocking())), roots);
    assert_eq!(
        warned(&out),
        Some("No open issue of yours labelled `auto` in the projects switched on.")
    );
    assert!(!rig.u.claiming());
    let _ = std::fs::remove_dir_all(project.root);
}

/// An issue's task kept by a claim counts against the cap until it has
/// asked for its place, and a Resume or Retry of another is held to it.
#[test]
fn a_start_is_held_to_the_cap_and_a_counted_one_never_is() {
    let mut rig = Rig::new(config(1, None));
    let file = PathBuf::from("/kept/issues.json");
    rig.tasks.add(issue_task("a", 1, &file, Path::new("/w/a")));
    rig.tasks.add(issue_task("b", 2, &file, Path::new("/w/b")));
    rig.u.starting.insert("a".into());
    let Rig { tasks, u, .. } = &mut rig;
    assert_eq!(u.over_cap("a", &Seen::new(tasks, [])), None);
    let why = u.over_cap("b", &Seen::new(tasks, [])).unwrap();
    assert!(why.contains("Slots: 1 of 1"), "{why}");
    let out = rig.request("b");
    let refused = tasks::Effect::Signal(tasks::Signal::Refused {
        window: WINDOW,
        why,
    });
    assert!(out.tasks.contains(&refused), "{:?}", out.tasks);
    // Asking for its place lets the count go.
    let out = rig.request("a");
    let find = tasks::Effect::FindPlace {
        task: "a".into(),
        dir: PathBuf::from("/w/a"),
        window: WINDOW,
    };
    assert!(out.tasks.contains(&find), "{:?}", out.tasks);
    rig.found_place("a", "/w/a");
    let slots = rig.slots();
    assert_eq!(
        (slots.starting, slots.holders.len()),
        (0, 1),
        "it holds its slot as a run"
    );
}

/// A pick is refused before any claim, in this order: the cap, a claim
/// already under way, what stops every run, what this project lacks.
#[test]
fn a_pick_is_refused_before_the_claim() {
    let pick = |workflow: &str, has_check| Pick {
        repo: PathBuf::from("/r"),
        tracker: Tracker::Local(PathBuf::from("/r/issues.json")),
        row: IssueRow {
            issue: crate::unattended::Issue::new(3, "Fix".into(), String::new()),
            author: String::new(),
            labels: Vec::new(),
        },
        workflow: workflow.into(),
        instructions: String::new(),
        has_check,
    };
    let mut rig = Rig::new(config(1, None));
    let refused = |rig: &mut Rig, p: Pick| {
        let Rig { tasks, u, world } = rig;
        u.pick(p, PICKED, &Seen::new(tasks, []), world).err()
    };
    assert!(refused(&mut rig, pick("cmd", false))
        .unwrap()
        .starts_with("Cannot start on this project"));
    assert_eq!(refused(&mut rig, pick("plain", false)), None);
    assert!(rig.u.claiming());
    assert_eq!(
        refused(&mut rig, pick("plain", false)).as_deref(),
        Some("An unattended run is starting — one at a time.")
    );
    rig.u.claiming = false;
    rig.fx(|u, _, _| {
        u.pause(
            "The agent does not offer mode `x`",
            "unattended.mode is fixed",
        )
    });
    assert!(refused(&mut rig, pick("plain", false))
        .unwrap()
        .starts_with("Nothing can be started"));
    rig.u.starting.insert("other".into());
    assert!(refused(&mut rig, pick("plain", false))
        .unwrap()
        .contains("Slots: 1 of 1"));
}

/// A run waiting on a person gives its slot up, so an issue's looks for the
/// next issue at once; an answer does under a cap on waiting runs only.
#[test]
fn waiting_and_answering_look_again_only_where_they_free_room() {
    let project = repo("waiting", &[]);
    let roots = vec![(project.clone(), false)];
    let file = project.issues.clone().unwrap();
    for cap in [None, Some(2)] {
        let mut rig = Rig::new(config(1, cap));
        rig.tasks.add(issue_task("i", 1, &file, &project.root));
        rig.tasks.add(Task::check(
            "c".into(),
            "true".into(),
            issue_task("x", 1, &file, &project.root).setup,
        ));
        let out = rig.said("i", Said::Waiting, roots.clone());
        assert!(looks(&out).is_some_and(|l| l.search.is_some()));
        rig.u.claiming = false;
        let out = rig.said("c", Said::Waiting, roots.clone());
        assert!(looks(&out).is_none(), "a task of no issue holds no slot");
        let out = rig.said("i", Said::Answered, roots.clone());
        assert_eq!(looks(&out).is_some(), cap.is_some(), "{cap:?}");
    }
    let _ = std::fs::remove_dir_all(project.root);
}

/// A run's end drops its project unless a person has it, picked by hand or
/// taken over, then sends its reports once; what landed leaves its task.
#[test]
fn an_ended_run_drops_its_project_unless_a_person_has_it_and_is_told_once() {
    for (picked, outcome, keep) in [
        (false, Outcome::Done, false),
        (true, Outcome::Done, true),
        (false, Outcome::Stopped(Stop::TakenOver), true),
    ] {
        let mut rig = Rig::new(config(1, None));
        let mut task = issue_task("a", 1, Path::new("/kept/issues.json"), Path::new("/w/a"));
        match &mut task.source {
            crate::task::Source::Issue(issue) => issue.picked = picked,
            source @ (crate::task::Source::Workflow | crate::task::Source::Check) => {
                panic!("{source:?}")
            }
        }
        task.runs[0].outcome = Some(outcome.clone());
        let run = task.runs[0].clone();
        match &mut task.source {
            crate::task::Source::Issue(issue) => issue.keep_report(&run, None),
            source @ (crate::task::Source::Workflow | crate::task::Source::Check) => {
                panic!("{source:?}")
            }
        }
        rig.tasks.add(task);
        let out = rig.fx(|u, seen, _| u.ended("a", seen));
        match &out.unattended[0] {
            Effect::Teardown { keep: k, .. } => assert_eq!(*k, keep, "{picked} {outcome:?}"),
            e @ (Effect::Arm(_)
            | Effect::Check { .. }
            | Effect::Look(_)
            | Effect::Claim { .. }
            | Effect::Take { .. }
            | Effect::ClaimPicked { .. }
            | Effect::Tell { .. }
            | Effect::Submit { .. }
            | Effect::AnswerReview { .. }
            | Effect::Send(_)
            | Effect::Drain { .. }
            | Effect::NoteTakenUp { .. }
            | Effect::Synced(_)
            | Effect::Tick
            | Effect::Signal(_)
            | Effect::Refresh) => panic!("{} first: {picked} {outcome:?}", name(e)),
        }
        assert_eq!(name(&out.unattended[1]), "Send");
        let again = rig.fx(|u, seen, _| u.deliver("a", seen));
        assert!(again.unattended.is_empty(), "sent once while it is out");
        let out = rig.handle(
            Input::Sent {
                task: "a".into(),
                landed: 1,
            },
            Vec::new(),
        );
        assert!(rig
            .tasks
            .task("a")
            .unwrap()
            .issue()
            .unwrap()
            .unsent
            .is_empty());
        let saved = tasks::Effect::Save(Box::new(rig.tasks.task("a").unwrap().clone()));
        assert!(out.tasks.contains(&saved), "{:?}", out.tasks);
    }
}

/// A run's agent coming up notes on an issue kept in onehand which session
/// works it; an issue only on a forge has nowhere to keep it.
#[test]
fn a_run_coming_up_notes_its_session_on_an_issue_kept_here() {
    let mut rig = Rig::new(config(1, None));
    rig.tasks.add(issue_task(
        "a",
        4,
        Path::new("/kept/issues.json"),
        Path::new("/w/a"),
    ));
    let came_up = |session: Option<&str>| Said::CameUp {
        session: session.map(str::to_string),
    };
    let out = rig.said("a", came_up(Some("s-1")), Vec::new());
    match &out.unattended[..] {
        [Effect::NoteTakenUp {
            number, session, ..
        }] => assert_eq!((*number, session.as_str()), (4, "s-1")),
        [e @ (Effect::Arm(_)
        | Effect::Check { .. }
        | Effect::Look(_)
        | Effect::Claim { .. }
        | Effect::Take { .. }
        | Effect::ClaimPicked { .. }
        | Effect::Tell { .. }
        | Effect::Submit { .. }
        | Effect::AnswerReview { .. }
        | Effect::Send(_)
        | Effect::Drain { .. }
        | Effect::Teardown { .. }
        | Effect::Tick
        | Effect::Signal(_)
        | Effect::Refresh)] => panic!("{}", name(e)),
        fx => panic!(
            "one note, not {:?}",
            fx.iter().map(name).collect::<Vec<_>>()
        ),
    }
    let out = rig.said("a", came_up(None), Vec::new());
    assert!(out.unattended.is_empty());
}

/// A check counts as under way until it answers, and a look of every
/// project lets go of what was kept for one since switched off.
#[test]
fn a_check_waits_until_it_answers_and_prunes_what_is_switched_off() {
    let mut rig = Rig::new(config(1, None));
    let (on, off) = (PathBuf::from("/on"), PathBuf::from("/off"));
    let project = |root: &PathBuf| Project {
        root: root.clone(),
        issues: None,
    };
    rig.fx(|u, _, _| u.check(vec![project(&off)], false));
    assert!(rig.u.checking());
    rig.handle(
        Input::Checked {
            accounts: Vec::new(),
            checked: vec![(off.clone(), Err("no forge".into()))],
            prune: false,
        },
        Vec::new(),
    );
    assert!(!rig.u.checking());
    assert_eq!(rig.u.problem(&off), Some("no forge"));
    rig.fx(|u, _, _| u.check(vec![project(&on)], true));
    rig.handle(
        Input::Checked {
            accounts: Vec::new(),
            checked: vec![(on.clone(), Err("not a git repository".into()))],
            prune: true,
        },
        vec![(project(&on), false)],
    );
    assert_eq!(rig.u.problem(&off), None, "switched off since");
    assert_eq!(rig.u.problem(&on), Some("not a git repository"));
    assert_eq!(rig.u.checked_at(), Some(0));
}
