//! The run's carrier and the `Tasks` global, held to what they do today: a
//! check run with no session, an agent's run on a session, a queued task
//! stopped or passed over, and a dismiss whose write fails.

use super::{Tasks, add, begin, dismiss, retry, start, start_check, stop_task};
use crate::chat::session::tests::carrier;
use gpui::{
    AnyWindowHandle, AppContext as _, BorrowAppContext as _, IntoElement, Render, TestAppContext,
    Window, div,
};
use gpui_component::Root;
use onehand_core::chat::Chat;
use onehand_core::task::tasks::{Begin, Input, Start};
use onehand_core::task::{Group, Task, Working, queue};
use onehand_core::workflow::{Brief, GateKind, Outcome, Setup, StepKind, StepSpec, Stop, Template};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "onehand-app-task-{name}-{}-{}",
        std::process::id(),
        onehand_core::task::new_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo(name: &str) -> PathBuf {
    let repo = temp_dir(name);
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
    repo
}

fn setup(dir: &Path) -> Setup {
    Setup {
        repo: dir.to_path_buf(),
        dir: dir.to_path_buf(),
        branch: None,
        agent: None,
        check: None,
        mode: None,
        forge: None,
    }
}

/// A window to say things in, and the `Tasks` global writing into `store`.
fn app(cx: &mut TestAppContext, store: &Path) -> AnyWindowHandle {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let store = store.to_path_buf();
    cx.update(|cx| cx.set_global(Tasks::in_dir(store)));
    cx.add_window(|window, cx| Root::new(cx.new(|_| Blank), window, cx))
        .into()
}

/// Keep `task` and ask for its place from `window`, as the drive does once
/// the place is found: whether it got it. What that asks is left undone, so
/// a test drives the task itself.
fn ask(task: Task, window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    let id = task.id.clone();
    let place = queue::place_blocking(&task.setup.dir);
    cx.update(|cx| {
        add(task, cx);
        cx.update_global::<Tasks, _>(|t, _| {
            t.drive.request(&id, window, None);
            t.drive.handle(Input::PlaceFound {
                task: id.clone(),
                place,
                window,
                window_open: true,
            });
            t.drive.queue().holds(&id)
        })
    })
}

/// Keep `task` and give it its place, as a request finding it free does.
fn placed(task: Task, window: AnyWindowHandle, cx: &mut TestAppContext) -> String {
    let id = task.id.clone();
    assert!(ask(task, window, cx));
    id
}

/// Task `id`, holding its place, begun as the tasks' drive begins it.
fn begun(id: &str, cx: &mut TestAppContext) -> Start {
    match cx.update(|cx| begin(id, true, cx)) {
        Begin::Check(start) | Begin::Session { start, .. } => start,
        Begin::Refused { why, .. } => panic!("not begun: {why}"),
    }
}

/// Run check task `id`, which holds its place, saying in `window` how it went.
fn drive_check(id: String, window: AnyWindowHandle, cx: &mut TestAppContext) {
    let start = begun(&id, cx);
    cx.update(|cx| start_check(id, start, window, cx));
}

fn holds(id: &str, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| cx.global::<Tasks>().drive.queue().holds(id))
}

/// Every background answer landed, and every write the writer was sent.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..50 {
        cx.run_until_parked();
        let writer = cx.update(|cx| cx.global::<Tasks>().writer.clone());
        assert!(writer.flush(Duration::from_secs(5)));
        cx.run_until_parked();
        let idle = cx.update(|cx| {
            let t = cx.global::<Tasks>();
            t.live.is_empty() && t.draining.is_empty()
        });
        if idle {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn kept(id: &str, cx: &mut TestAppContext) -> Task {
    cx.update(|cx| super::task(id, cx)).unwrap()
}

fn group(id: &str, cx: &mut TestAppContext) -> Group {
    cx.update(|cx| {
        let t = cx.global::<Tasks>();
        t.task(id).unwrap().group(t.working(id))
    })
}

fn on_disk(store: &Path, id: &str) -> Task {
    let text = std::fs::read_to_string(store.join(format!("{id}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[gpui::test]
fn a_check_that_passes_ends_done_and_gives_its_place_up(cx: &mut TestAppContext) {
    let (work, store) = (temp_dir("check-pass"), temp_dir("store"));
    let window = app(cx, &store);
    let id = placed(
        Task::check("1".into(), "true".into(), setup(&work)),
        window,
        cx,
    );
    drive_check(id.clone(), window, cx);
    settle(cx);
    assert_eq!(kept(&id, cx).outcome(), Some(Outcome::Done));
    assert_eq!(group(&id, cx), Group::Finished);
    assert!(!holds(&id, cx));
    assert_eq!(on_disk(&store, &id).outcome(), Some(Outcome::Done));
}

#[gpui::test]
fn a_check_that_fails_ends_under_needs_attention(cx: &mut TestAppContext) {
    let (work, store) = (temp_dir("check-fail"), temp_dir("store"));
    let window = app(cx, &store);
    let id = placed(
        Task::check("2".into(), "false".into(), setup(&work)),
        window,
        cx,
    );
    drive_check(id.clone(), window, cx);
    settle(cx);
    let outcome = kept(&id, cx).outcome();
    assert!(
        matches!(
            outcome,
            Some(Outcome::Exhausted { .. } | Outcome::Failed(_))
        ),
        "{outcome:?}"
    );
    assert_eq!(group(&id, cx), Group::Ended);
    assert!(!holds(&id, cx));
}

/// A Stop pressed while the first mark is pinned ends the run once the pin
/// lands, and its command is never started.
#[gpui::test]
fn a_check_stopped_before_its_command_starts_never_runs_it(cx: &mut TestAppContext) {
    let (work, store) = (repo("check-stop"), temp_dir("store"));
    let window = app(cx, &store);
    let touched = work.join("ran");
    let command = format!("touch {}", touched.display());
    let id = placed(Task::check("3".into(), command, setup(&work)), window, cx);
    let start = begun(&id, cx);
    cx.update(|cx| {
        start_check(id.clone(), start, window, cx);
        stop_task(&id, cx);
    });
    settle(cx);
    assert_eq!(
        kept(&id, cx).outcome(),
        Some(Outcome::Stopped(Stop::ByPerson))
    );
    assert!(!touched.exists());
    assert!(!holds(&id, cx));
}

/// A queued task that never ran a step is over, stopped by a person, with no
/// empty run kept; the task holding the place carries on.
#[gpui::test]
fn stop_on_a_queued_task_calls_off_that_start_only(cx: &mut TestAppContext) {
    let (work, store) = (temp_dir("queued-stop"), temp_dir("store"));
    let window = app(cx, &store);
    let first = placed(
        Task::check("4".into(), "true".into(), setup(&work)),
        window,
        cx,
    );
    assert!(!ask(
        Task::check("5".into(), "true".into(), setup(&work)),
        window,
        cx
    ));
    assert_eq!(group("5", cx), Group::Queued);
    // A retry is refused while the task waits for its place.
    let template = kept("5", cx).runs[0].template.clone();
    assert!(!cx.update(|cx| retry("5", template, None, None, cx)));
    cx.update(|cx| stop_task("5", cx));
    settle(cx);
    let stopped = kept("5", cx);
    assert!(stopped.runs.is_empty());
    assert_eq!(stopped.outcome(), Some(Outcome::Stopped(Stop::ByPerson)));
    assert_eq!(group("5", cx), Group::Finished);
    assert!(on_disk(&store, "5").runs.is_empty());
    assert!(holds(&first, cx));
    cx.update(|cx| assert_eq!(cx.global::<Tasks>().working(&first), Some(Working::Running)));
}

/// A place let go passes to the next task waiting; one whose window has no
/// shell to start it in is passed over, reads cut off, and the place passes
/// on.
#[gpui::test]
fn a_queued_task_with_no_window_to_start_in_is_passed_over(cx: &mut TestAppContext) {
    let (work, store) = (temp_dir("passed-over"), temp_dir("store"));
    let window = app(cx, &store);
    let first = placed(
        Task::check("6".into(), "true".into(), setup(&work)),
        window,
        cx,
    );
    assert!(!ask(
        Task::check("7".into(), "true".into(), setup(&work)),
        window,
        cx
    ));
    drive_check(first, window, cx);
    settle(cx);
    assert!(!holds("7", cx));
    cx.update(|cx| assert!(!cx.global::<Tasks>().drive.queue().queued("7")));
    assert_eq!(kept("7", cx).outcome(), None);
    assert_eq!(group("7", cx), Group::Ended);
}

#[gpui::test]
fn a_dismiss_is_kept_once_its_write_lands(cx: &mut TestAppContext) {
    let store = temp_dir("store");
    let window = app(cx, &store);
    let work = temp_dir("dismiss");
    cx.update(|cx| add(Task::check("8".into(), "true".into(), setup(&work)), cx));
    window
        .update(cx, |_, window, cx| dismiss("8", window, cx))
        .unwrap();
    settle(cx);
    assert!(kept("8", cx).dismissed);
    assert_eq!(group("8", cx), Group::Finished);
    assert!(on_disk(&store, "8").dismissed);
}

/// Not kept, so not let go: a dismiss whose write fails goes back.
#[gpui::test]
fn a_dismiss_whose_write_fails_goes_back(cx: &mut TestAppContext) {
    // A store under a file: no directory can be made there.
    let file = temp_dir("blocked").join("file");
    std::fs::write(&file, "").unwrap();
    let window = app(cx, &file.join("store"));
    let work = temp_dir("dismiss-fails");
    cx.update(|cx| add(Task::check("9".into(), "true".into(), setup(&work)), cx));
    window
        .update(cx, |_, window, cx| dismiss("9", window, cx))
        .unwrap();
    settle(cx);
    assert!(!kept("9", cx).dismissed);
    assert_eq!(group("9", cx), Group::Ended);
}

fn one_agent_step() -> Template {
    let mut template = Template::blank("One step");
    template.id = "one-step".into();
    template.steps.push(StepSpec {
        id: "do".into(),
        label: "Do".into(),
        kind: StepKind::Agent {
            prompt: "{brief}".into(),
            gates: vec![GateKind::Answered],
            keep_answer: false,
            mode: None,
            options: Default::default(),
        },
    });
    template
}

fn brief() -> Brief {
    Brief {
        title: "Say done".into(),
        body: String::new(),
        instructions: None,
    }
}

/// An agent's run end to end on a session: its prompt waits for the agent,
/// goes once it is up, the turn's answer passes the gate, the run is kept
/// done with both marks pinned, and the place is given up.
#[gpui::test]
fn an_agent_run_goes_from_its_prompt_to_done(cx: &mut TestAppContext) {
    let (work, store) = (repo("agent-run"), temp_dir("store"));
    let window = app(cx, &store);
    let id = placed(
        Task::new("10".into(), one_agent_step(), brief(), setup(&work)),
        window,
        cx,
    );
    let (session, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let started = begun(&id, cx);
    cx.update(|cx| start(7, &session, window, id.clone(), started, cx));
    cx.run_until_parked();
    // Measured and pinned, the prompt waits for the agent.
    let mut requests = carrier::up(&mut tx);
    cx.run_until_parked();
    let sent = requests.try_recv();
    assert!(
        matches!(sent, Ok(onehand_core::acp::AcpRequest::Prompt { .. })),
        "{sent:?}"
    );
    carrier::turn(&mut tx);
    settle(cx);
    let task = kept(&id, cx);
    assert_eq!(task.outcome(), Some(Outcome::Done));
    // Its prompt went once.
    let more = std::iter::from_fn(|| requests.try_recv().ok())
        .filter(|r| matches!(r, onehand_core::acp::AcpRequest::Prompt { .. }))
        .count();
    assert_eq!(more, 0);
    let visit = &task.runs[0].visits()[0];
    assert!(visit.start.is_some() && visit.end.is_some(), "{visit:?}");
    let refs = git(
        &work,
        &["for-each-ref", "--format=%(refname)", "refs/onehand"],
    );
    assert_eq!(refs.lines().count(), 2, "{refs}");
    cx.update(|cx| {
        let t = cx.global::<Tasks>();
        assert!(!t.drive.queue().holds(&id));
        assert!(t.task_on(7).is_none());
    });
    assert_eq!(on_disk(&store, &id).outcome(), Some(Outcome::Done));
}

/// A Stop on a run whose session's turn is going cancels the turn and ends
/// the run, but the place is held until that turn is over.
#[gpui::test]
fn a_stopped_run_holds_its_place_until_its_turn_is_over(cx: &mut TestAppContext) {
    let (work, store) = (repo("agent-stop"), temp_dir("store"));
    let window = app(cx, &store);
    let id = placed(
        Task::new("11".into(), one_agent_step(), brief(), setup(&work)),
        window,
        cx,
    );
    let (session, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let started = begun(&id, cx);
    cx.update(|cx| start(8, &session, window, id.clone(), started, cx));
    cx.run_until_parked();
    let mut requests = carrier::up(&mut tx);
    cx.run_until_parked();
    assert!(requests.try_recv().is_ok());
    cx.update(|cx| super::stop(8, cx));
    cx.run_until_parked();
    let cancelled = std::iter::from_fn(|| requests.try_recv().ok())
        .any(|r| matches!(r, onehand_core::acp::AcpRequest::Cancel));
    assert!(cancelled);
    assert_eq!(
        kept(&id, cx).outcome(),
        Some(Outcome::Stopped(Stop::ByPerson))
    );
    cx.update(|cx| {
        let t = cx.global::<Tasks>();
        assert!(t.drive.queue().holds(&id));
        assert!(t.draining.contains_key(&id));
    });
    // The agent says the cancelled turn is over: the place goes.
    tx.try_send(onehand_core::acp::AcpEvent::TurnEnded {
        stop_reason: "cancelled".into(),
    })
    .unwrap();
    settle(cx);
    assert!(!holds(&id, cx));
}

/// How many notifications `window` has put up.
fn notes(window: AnyWindowHandle, cx: &mut TestAppContext) -> usize {
    use gpui_component::WindowExt as _;
    cx.update(|cx| {
        window
            .update(cx, |_, window, cx| window.notifications(cx).len())
            .unwrap()
    })
}

/// An agent's run up to its prompt on a session, every effect so far landed.
fn prompted(
    id: &str,
    work: &Path,
    window: AnyWindowHandle,
    uid: u64,
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<crate::chat::session::ChatSession>,
    futures::channel::mpsc::Sender<onehand_core::acp::AcpEvent>,
    tokio::sync::mpsc::UnboundedReceiver<onehand_core::acp::AcpRequest>,
) {
    let task = Task::new(id.into(), one_agent_step(), brief(), setup(work));
    let id = placed(task, window, cx);
    let (session, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let started = begun(&id, cx);
    cx.update(|cx| start(uid, &session, window, id, started, cx));
    cx.run_until_parked();
    let requests = carrier::up(&mut tx);
    cx.run_until_parked();
    (session, tx, requests)
}

/// Every write of a run that fails reaches its drive, which says so once.
#[gpui::test]
fn a_runs_failed_writes_reach_its_drive_and_are_said_once(cx: &mut TestAppContext) {
    let work = repo("unwritable-run");
    let file = temp_dir("blocked-run").join("file");
    std::fs::write(&file, "").unwrap();
    let window = app(cx, &file.join("store"));
    let (_session, _tx, _requests) = prompted("12", &work, window, 9, cx);
    let writer = cx.update(|cx| cx.global::<Tasks>().writer.clone());
    assert!(writer.flush(Duration::from_secs(5)));
    cx.run_until_parked();
    assert_eq!(
        notes(window, cx),
        1,
        "the measure's and the prompt's writes failed, said once"
    );
}

/// A session closed under its run ends the run as cut off, to be resumed,
/// and gives its place up; nothing of the run is left listening.
#[gpui::test]
fn a_session_closed_under_its_run_leaves_it_resumable_and_its_place_free(cx: &mut TestAppContext) {
    let (work, store) = (repo("closed-under"), temp_dir("store"));
    let window = app(cx, &store);
    let (session, tx, _requests) = prompted("13", &work, window, 10, cx);
    drop((session, tx));
    settle(cx);
    let task = kept("13", cx);
    assert_eq!(task.outcome(), Some(Outcome::Stopped(Stop::Closed)));
    assert!(task.resumable());
    assert!(!holds("13", cx));
    cx.update(|cx| {
        let t = cx.global::<Tasks>();
        assert!(t.live.is_empty() && t.draining.is_empty());
    });
}

/// A run that finished hears nothing more from its session: a later turn
/// there moves no run, and nothing of it is still held.
#[gpui::test]
fn a_finished_run_hears_nothing_more_from_its_session(cx: &mut TestAppContext) {
    let (work, store) = (repo("finished-quiet"), temp_dir("store"));
    let window = app(cx, &store);
    let (_session, mut tx, _requests) = prompted("14", &work, window, 11, cx);
    carrier::turn(&mut tx);
    settle(cx);
    let before = kept("14", cx);
    assert_eq!(before.outcome(), Some(Outcome::Done));
    carrier::turn(&mut tx);
    settle(cx);
    let after = kept("14", cx);
    assert_eq!(after.runs[0].history.len(), before.runs[0].history.len());
    cx.update(|cx| assert!(cx.global::<Tasks>().live.is_empty()));
}
