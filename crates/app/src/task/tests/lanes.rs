//! A run whose step names its own agent, as its carrier carries it: the
//! step's session opened when the run reaches it, the run found by either
//! session's uid, and a step whose agent is gone failing the run there.

use super::{app, begun, brief, holds, kept, placed, repo, settle, setup, temp_dir};
use crate::chat::session::ChatSession;
use crate::chat::session::tests::carrier;
use crate::task::{Tasks, start};
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Entity, Global, TestAppContext};
use onehand_core::acp::AcpRequest;
use onehand_core::chat::Chat;
use onehand_core::config::AgentSpec;
use onehand_core::config::drive::SettingsDrive;
use onehand_core::task::Task;
use onehand_core::workflow::{GateKind, Outcome, StepKind, StepSpec, Template};
use std::path::PathBuf;

/// The sessions a test hands the lanes its runs open, in order, and the
/// agent each lane asked for.
#[derive(Default)]
struct Lanes {
    ready: Vec<(u64, Entity<ChatSession>)>,
    asked: Vec<String>,
    /// The tools each lane was granted as it was opened.
    granted: Vec<Vec<String>>,
}

impl Global for Lanes {}

/// Open a lane's session as the shell would, on a session the test made:
/// an agent not in the agent list opens none.
pub(in crate::task) fn open_lane(
    _: AnyWindowHandle,
    _: PathBuf,
    agent: &str,
    grant: &onehand_core::task::Grant,
    cx: &mut App,
) -> Result<(u64, Entity<ChatSession>), String> {
    crate::task::facts::lane_spec(agent, cx)?;
    cx.update_default_global::<Lanes, _>(|lanes, _| {
        lanes.asked.push(agent.to_string());
        lanes.granted.push(grant.tools.clone());
        match lanes.ready.is_empty() {
            true => Err("no session to open".to_string()),
            false => Ok(lanes.ready.remove(0)),
        }
    })
}

fn step(id: &str, label: &str, agent: Option<&str>) -> StepSpec {
    StepSpec {
        limit: None,
        id: id.into(),
        label: label.into(),
        kind: StepKind::Agent {
            prompt: format!("{label} it."),
            gates: vec![GateKind::Answered],
            keep_answer: false,
            mode: None,
            options: Default::default(),
            agent: agent.map(str::to_string),
        },
    }
}

/// Do on the run's agent, then review on `b`.
fn reviewed() -> Template {
    let mut template = Template::blank("Reviewed");
    template.id = "reviewed".into();
    template.steps = vec![step("do", "Do", None), step("review", "Review", Some("b"))];
    template
}

/// `agents` configured, by name.
fn configure(agents: &[&str], cx: &mut TestAppContext) {
    let agents = agents
        .iter()
        .map(|name| AgentSpec {
            name: (*name).into(),
            command: "/nonexistent/onehand-test-agent".into(),
            ..Default::default()
        })
        .collect();
    cx.update(|cx| {
        cx.update_global::<crate::state::Shared, _>(|shared, _| {
            shared.settings = SettingsDrive::new(agents, Default::default(), Vec::new());
        })
    });
}

fn prompts(requests: &mut tokio::sync::mpsc::UnboundedReceiver<AcpRequest>) -> usize {
    std::iter::from_fn(|| requests.try_recv().ok())
        .filter(|r| matches!(r, AcpRequest::Prompt { .. }))
        .count()
}

fn task_on(uid: u64, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| cx.global::<Tasks>().task_on(uid))
}

/// The run's first step on session 7, its review's on session 8, opened
/// when the run gets there: either finds the run, each is prompted once,
/// and the run ends done.
#[gpui::test]
fn a_step_lane_is_found_by_its_uid(cx: &mut TestAppContext) {
    let (work, store) = (repo("lanes"), temp_dir("store"));
    let window = app(cx, &store);
    configure(&["a", "b"], cx);
    let id = placed(
        Task::new("20".into(), reviewed(), brief(), setup(&work)),
        window,
        cx,
    );
    let (first, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let (review, mut review_tx, _review_seen) = carrier::driven(cx, Chat::default());
    cx.update(|cx| {
        cx.set_global(Lanes {
            ready: vec![(8, review.clone())],
            ..Default::default()
        })
    });
    let started = begun(&id, cx);
    cx.update(|cx| start(7, &first, window, id.clone(), started, cx));
    cx.run_until_parked();
    let mut requests = carrier::up(&mut tx);
    cx.run_until_parked();
    assert_eq!(prompts(&mut requests), 1, "Do's prompt");
    assert_eq!(task_on(7, cx), Some(id.clone()));

    carrier::turn(&mut tx);
    let mut review_requests = carrier::up(&mut review_tx);
    for _ in 0..100 {
        cx.run_until_parked();
        if task_on(8, cx).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.run_until_parked();
    assert_eq!(task_on(8, cx), Some(id.clone()), "the review's lane");
    assert_eq!(task_on(7, cx), Some(id.clone()), "and still the run's own");
    cx.update(|cx| {
        assert_eq!(cx.global::<Lanes>().asked, ["b"]);
        let t = cx.global::<Tasks>();
        let mut uids = t.driven(&id).unwrap().uids();
        uids.sort();
        assert_eq!(uids, [7, 8]);
        assert_eq!(t.driven(&id).unwrap().uid(), Some(8));
    });
    assert_eq!(prompts(&mut review_requests), 1, "the review's prompt");
    assert_eq!(prompts(&mut requests), 0, "nothing more to the run's own");
    let named = cx.update(|cx| review.read(cx).chat().custom_title.clone());
    assert_eq!(named.as_deref(), Some("Say done · Review"));

    carrier::turn(&mut review_tx);
    settle(cx);
    assert_eq!(kept(&id, cx).outcome(), Some(Outcome::Done));
    assert!(task_on(8, cx).is_none());
}

/// The review's agent left the agent list after the run started: the run
/// fails when it reaches the review, in the lane's words, and lets its
/// place go.
#[gpui::test]
fn a_missing_step_agent_fails_the_run_when_reached(cx: &mut TestAppContext) {
    let (work, store) = (repo("lane-gone"), temp_dir("store"));
    let window = app(cx, &store);
    configure(&["a"], cx);
    let id = placed(
        Task::new("21".into(), reviewed(), brief(), setup(&work)),
        window,
        cx,
    );
    let (first, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let started = begun(&id, cx);
    cx.update(|cx| start(9, &first, window, id.clone(), started, cx));
    cx.run_until_parked();
    let _requests = carrier::up(&mut tx);
    cx.run_until_parked();
    carrier::turn(&mut tx);
    settle(cx);
    let task = kept(&id, cx);
    assert_eq!(
        task.outcome(),
        Some(Outcome::Failed(
            "The agent of Review, b, did not start".into()
        ))
    );
    assert_eq!(
        task.runs().last().and_then(|run| run.failure),
        Some(onehand_core::workflow::Failure::Configuration),
        "an agent gone from the list is the configuration's"
    );
    let waits = cx.update(|cx| crate::task::rows(std::slice::from_ref(&work), cx));
    assert_eq!(
        waits[0].waits.as_deref(),
        Some("The agent of Review, b, did not start"),
        "said on its row as it is in its detail"
    );
    assert!(!holds(&id, cx), "its place let go");
}

/// The run's own session disconnects while the review works in its own:
/// the run forgets it, and coming back opens a new one, so nothing more
/// lands in the one that went and no uid of it finds the run.
#[gpui::test]
fn a_lane_whose_session_went_is_forgotten_and_opened_anew(cx: &mut TestAppContext) {
    let (work, store) = (repo("lane-went"), temp_dir("store"));
    let window = app(cx, &store);
    configure(&["a", "b"], cx);
    let mut template = reviewed();
    template.steps.push(step("wrap", "Wrap", None));
    let id = placed(
        Task::new("22".into(), template, brief(), setup(&work)),
        window,
        cx,
    );
    let (first, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let (review, mut review_tx, _review_seen) = carrier::driven(cx, Chat::default());
    let (again, mut again_tx, _again_seen) = carrier::driven(cx, Chat::default());
    cx.update(|cx| {
        cx.set_global(Lanes {
            ready: vec![(8, review.clone()), (9, again.clone())],
            ..Default::default()
        })
    });
    let started = begun(&id, cx);
    cx.update(|cx| start(7, &first, window, id.clone(), started, cx));
    cx.run_until_parked();
    let _requests = carrier::up(&mut tx);
    cx.run_until_parked();
    carrier::turn(&mut tx);
    let _review_requests = carrier::up(&mut review_tx);
    let mut again_requests = carrier::up(&mut again_tx);
    until(cx, |cx| task_on(8, cx).is_some());
    // The run's own session goes, its agent's stream over, while the
    // review works.
    drop(tx);
    until(cx, |cx| task_on(7, cx).is_none());
    let before = cx.update(|cx| first.read(cx).chat().items.len());

    carrier::turn(&mut review_tx);
    until(cx, |cx| task_on(9, cx).is_some());
    cx.run_until_parked();
    assert_eq!(prompts(&mut again_requests), 1, "Wrap's prompt, anew");
    let after = cx.update(|cx| first.read(cx).chat().items.len());
    assert_eq!(after, before, "nothing more in the session that went");
    assert!(task_on(7, cx).is_none());
    cx.update(|cx| {
        assert_eq!(cx.global::<Lanes>().asked, ["b", ""]);
        // The forgotten session closes with the rest when they close.
        let mut uids = cx.global::<Tasks>().driven(&id).unwrap().uids();
        uids.sort();
        assert_eq!(uids, [7, 8, 9]);
    });
}

/// A lane that comes back before its old session was forgotten replaces it,
/// and the replaced session stays among the run's, so it closes with them.
#[gpui::test]
fn a_replaced_lane_session_is_kept_to_close(cx: &mut TestAppContext) {
    let (work, store) = (repo("lane-replaced"), temp_dir("store"));
    let window = app(cx, &store);
    configure(&["a"], cx);
    let id = placed(
        Task::new("23".into(), reviewed(), brief(), setup(&work)),
        window,
        cx,
    );
    let (first, _tx, _seen) = carrier::driven(cx, Chat::default());
    let started = begun(&id, cx);
    cx.update(|cx| start(7, &first, window, id.clone(), started, cx));
    cx.run_until_parked();
    let (second, _tx2, _seen2) = carrier::driven(cx, Chat::default());
    cx.update(|cx| {
        crate::task::carrier::hold_again(&id, onehand_core::task::drive::Lane::Run, 9, &second, cx);
        let t = cx.global::<Tasks>();
        let mut uids = t.driven(&id).unwrap().uids();
        uids.sort();
        assert_eq!(uids, [7, 9], "the replaced session is still the run's");
    });
}

/// Let background work land until `done`.
fn until(cx: &mut TestAppContext, done: impl Fn(&mut TestAppContext) -> bool) {
    for _ in 0..200 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never got there");
}

/// Do on the run's agent, then a review of it on `b`.
fn with_a_review() -> Template {
    let mut template = Template::blank("Judged");
    template.id = "judged".into();
    template.steps = vec![
        step("do", "Do", None),
        StepSpec {
            limit: None,
            id: "review".into(),
            label: "Review".into(),
            kind: StepKind::Review {
                of: "do".into(),
                on_reject: None,
                agent: Some("b".into()),
                prompt: "Review it.".into(),
                mode: None,
                options: Default::default(),
            },
        },
    ];
    template
}

/// What a test holds of a run's two sessions, so they stay open.
type Held = (
    Entity<ChatSession>,
    futures::channel::mpsc::Sender<onehand_core::acp::AcpEvent>,
    carrier::Seen,
    Entity<ChatSession>,
    carrier::Seen,
);

/// A run of [`with_a_review`] on sessions 7 and 8, at its review's turn,
/// with what sends to the review's session.
fn at_the_review(
    task: &str,
    cx: &mut TestAppContext,
) -> (
    String,
    futures::channel::mpsc::Sender<onehand_core::acp::AcpEvent>,
    Held,
) {
    let (work, store) = (repo(&format!("verdict-{task}")), temp_dir("store"));
    let window = app(cx, &store);
    configure(&["a", "b"], cx);
    let id = placed(
        Task::new(task.into(), with_a_review(), brief(), setup(&work)),
        window,
        cx,
    );
    let (first, mut tx, seen) = carrier::driven(cx, Chat::default());
    let (review, mut review_tx, review_seen) = carrier::driven(cx, Chat::default());
    cx.update(|cx| {
        cx.set_global(Lanes {
            ready: vec![(8, review.clone())],
            ..Default::default()
        })
    });
    let started = begun(&id, cx);
    cx.update(|cx| start(7, &first, window, id.clone(), started, cx));
    cx.run_until_parked();
    let _requests = carrier::up(&mut tx);
    cx.run_until_parked();
    carrier::turn(&mut tx);
    let mut review_requests = carrier::up(&mut review_tx);
    for _ in 0..100 {
        cx.run_until_parked();
        if task_on(8, cx).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.run_until_parked();
    assert_eq!(prompts(&mut review_requests), 1, "the review's prompt");
    // The review's lane was granted its verdict as it was opened.
    let granted = cx.update(|cx| cx.global::<Lanes>().granted.clone());
    assert_eq!(granted, [vec!["review_verdict".to_string()]]);
    (id, review_tx, (first, tx, seen, review, review_seen))
}

/// The review's agent approves from its own session: the run holds the
/// verdict, and the review's turn ending carries it on to done.
#[gpui::test]
fn a_verdict_from_the_review_lane_reaches_its_run(cx: &mut TestAppContext) {
    use onehand_core::workflow::Verdict;
    let (id, mut review_tx, _held) = at_the_review("30", cx);
    let said = cx.update(|cx| crate::task::verdict(8, Verdict::Approve, cx));
    assert_eq!(
        said,
        Ok("Approved. The review ends when this turn does.".into())
    );
    carrier::turn(&mut review_tx);
    settle(cx);
    assert_eq!(kept(&id, cx).outcome(), Some(Outcome::Done));
}

/// A verdict from the session that wrote the work, or from one no run
/// holds, is refused and changes nothing.
#[gpui::test]
fn a_verdict_from_the_developer_lane_is_refused(cx: &mut TestAppContext) {
    use onehand_core::workflow::Verdict;
    let (id, _review_tx, _held) = at_the_review("31", cx);
    let refused = Err("No review is waiting on this session.".to_string());
    for uid in [7, 999] {
        let said = cx.update(|cx| crate::task::verdict(uid, Verdict::Approve, cx));
        assert_eq!(said, refused);
    }
    let run = cx.update(|cx| cx.global::<Tasks>().driven(&id).unwrap().run().clone());
    assert_eq!(run.current().unwrap().id, "review");
    assert!(run.awaiting_turn());
}
