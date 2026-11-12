//! A run's forge calls as its carrier makes them, against a forge that
//! records what it was asked: each call once, in the order the run's drive
//! asked, a call that fails, or has no forge to go to, said to the drive, and
//! the status checks looked at on the clock until they say something.

use super::{app, begun, brief, git, kept, placed, repo, settle, temp_dir};
use crate::chat::session::tests::carrier;
use crate::task::Tasks;
use crate::task::start;
use gpui::TestAppContext;
use onehand_core::acp::AcpRequest;
use onehand_core::chat::Chat;
use onehand_core::connector::{
    Check, CheckState, Connector, PrState, PullRequest, RemoteIssue, SyncListing,
};
use onehand_core::issues::Snapshot;
use onehand_core::task::Task;
use onehand_core::unattended::IssueRow;
use onehand_core::workflow::{GateKind, Outcome, Place, Setup, StepKind, StepSpec, Template};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// A forge that keeps what it was asked, by the branch it was asked about:
/// the tests share it, and each works a branch of its own. A branch named
/// `refused…` is refused its push; one named `failing…` has a status check
/// that fails, and any other's pass, each at the second look at them.
pub(in crate::task) struct Recording {
    asked: Mutex<Vec<(String, String)>>,
    /// The work each branch was pushed from: a call that names no branch is
    /// kept by the branch of the work it was made in.
    works: Mutex<Vec<(PathBuf, String)>>,
}

static FORGE: Recording = Recording {
    asked: Mutex::new(Vec::new()),
    works: Mutex::new(Vec::new()),
};

/// The connectors a run's forge is looked up among in a test.
pub(in crate::task) fn connectors() -> &'static [&'static dyn Connector] {
    static CONNECTORS: [&dyn Connector; 1] = [&FORGE];
    &CONNECTORS
}

impl Recording {
    fn note(&self, branch: &str, said: impl Into<String>) {
        let mut asked = self.asked.lock().unwrap();
        asked.push((branch.to_string(), said.into()));
    }

    /// The branch pushed from `dir`.
    fn branch_of(&self, dir: &Path) -> String {
        let works = self.works.lock().unwrap();
        let found = works.iter().rev().find(|(work, _)| work == dir);
        found.map(|(_, branch)| branch.clone()).unwrap_or_default()
    }

    /// Forget what was asked about `branch`, which a test starts from.
    fn forget(&self, branch: &str) {
        self.asked.lock().unwrap().retain(|(on, _)| on != branch);
    }

    /// What was asked about `branch`, in order.
    fn asked_of(&self, branch: &str) -> Vec<String> {
        let asked = self.asked.lock().unwrap();
        asked
            .iter()
            .filter(|(on, _)| on == branch)
            .map(|(_, said)| said.clone())
            .collect()
    }
}

impl Connector for Recording {
    fn name(&self) -> &'static str {
        "Recording"
    }
    fn account_blocking(&self) -> Result<String, String> {
        unreachable!()
    }
    fn serves_blocking(&self, _: &Path) -> Result<(), String> {
        unreachable!()
    }
    fn open_issues_blocking(&self, _: &Path, _: usize) -> Result<Vec<IssueRow>, String> {
        unreachable!()
    }
    fn my_labelled_issues_blocking(&self, _: &Path, _: &str) -> Result<Vec<IssueRow>, String> {
        unreachable!()
    }
    fn remove_label_blocking(&self, _: &Path, _: u64, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn comment_blocking(&self, _: &Path, _: u64, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn default_branch_blocking(&self, _: &Path) -> Result<String, String> {
        unreachable!()
    }
    fn push_blocking(&self, dir: &Path, commit: &str, branch: &str) -> Result<(), String> {
        let mut works = self.works.lock().unwrap();
        works.push((dir.to_path_buf(), branch.to_string()));
        drop(works);
        self.note(branch, format!("push {commit}"));
        match branch.starts_with("refused") {
            true => Err("the forge refused the push".into()),
            false => Ok(()),
        }
    }
    /// None until one was opened on the branch, then that draft.
    fn pull_request_for_blocking(
        &self,
        _: &Path,
        branch: &str,
    ) -> Result<Option<PullRequest>, String> {
        let asked = self.asked_of(branch);
        let Some(opened) = asked.iter().position(|said| said.starts_with("open")) else {
            self.note(branch, "find");
            return Ok(None);
        };
        let pushed = asked.iter().find_map(|said| said.strip_prefix("push "));
        // The read back after the open, then the looks at its status checks.
        let looks = asked[opened..]
            .iter()
            .filter(|said| *said == "find")
            .count();
        let state = match (looks, branch.starts_with("failing")) {
            (0, _) => None,
            (1, _) => Some(CheckState::Pending),
            (_, true) => Some(CheckState::Failed),
            (_, false) => Some(CheckState::Passed),
        };
        self.note(branch, "find");
        Ok(Some(PullRequest {
            url: format!("https://forge.test/{branch}"),
            number: 7,
            state: PrState::Open,
            draft: true,
            head: pushed.unwrap_or_default().to_string(),
            conflicting: false,
            checks: state
                .map(|state| Check {
                    name: "build".into(),
                    state,
                    link: None,
                })
                .into_iter()
                .collect(),
        }))
    }
    fn open_pull_request_blocking(
        &self,
        _: &Path,
        branch: &str,
        title: &str,
        _: &str,
    ) -> Result<(), String> {
        self.note(branch, format!("open {title}"));
        Ok(())
    }
    fn mark_ready_blocking(&self, dir: &Path, number: u64) -> Result<(), String> {
        self.note(&self.branch_of(dir), format!("ready {number}"));
        Ok(())
    }
    fn check_log_blocking(&self, dir: &Path, check: &Check) -> Result<String, String> {
        self.note(&self.branch_of(dir), format!("log {}", check.name));
        Ok("the build broke at line 3".into())
    }
    fn issues_for_sync_blocking(
        &self,
        _: &Path,
        _: Option<u64>,
        _: usize,
    ) -> Result<SyncListing, String> {
        unreachable!()
    }
    fn issue_blocking(&self, _: &Path, _: &str) -> Result<Option<RemoteIssue>, String> {
        unreachable!()
    }
    fn create_issue_blocking(&self, _: &Path, _: &Snapshot) -> Result<RemoteIssue, String> {
        unreachable!()
    }
    fn update_issue_blocking(
        &self,
        _: &Path,
        _: &str,
        _: &Snapshot,
        _: &Snapshot,
    ) -> Result<(), String> {
        unreachable!()
    }
    fn read_review_with(&self, _: u64) -> String {
        unreachable!()
    }
}

/// An agent's step that commits, a command that passes, the push and the pull request.
fn to_a_pull_request() -> Template {
    let mut template = Template::blank("To a pull request");
    template.id = "to-a-pull-request".into();
    template.place = Place::Worktree;
    let step = |id: &str, kind| StepSpec {
        id: id.into(),
        label: id.into(),
        kind,
    };
    template.steps = vec![
        step(
            "do",
            StepKind::Agent {
                prompt: "{brief}".into(),
                gates: vec![GateKind::Committed],
                keep_answer: false,
                mode: None,
                options: Default::default(),
            },
        ),
        step(
            "check",
            StepKind::Command {
                command: Some("true".into()),
                on_fail: "do".into(),
            },
        ),
        step("push", StepKind::Push),
        step("pull-request", StepKind::PullRequest),
    ];
    template
}

/// [`to_a_pull_request`], then the wait for its status checks, a failure
/// sent back to the agent's step.
fn through_status_checks() -> Template {
    let mut template = to_a_pull_request();
    template.id = "through-status-checks".into();
    template.steps.push(StepSpec {
        id: "status-checks".into(),
        label: "status-checks".into(),
        kind: StepKind::StatusChecks {
            on_fail: "do".into(),
            wait: "1h".into(),
        },
    });
    template
}

/// A run under way on a branch of its own.
struct Going {
    id: String,
    branch: String,
    work: PathBuf,
    requests: tokio::sync::mpsc::UnboundedReceiver<AcpRequest>,
    _session: gpui::Entity<crate::chat::session::ChatSession>,
    _tx: futures::channel::mpsc::Sender<onehand_core::acp::AcpEvent>,
}

impl Going {
    /// Let the run go on until the forge was asked `calls` things about
    /// its branch.
    fn until_asked(&self, calls: usize, cx: &mut TestAppContext) {
        for _ in 0..250 {
            cx.run_until_parked();
            let writer = cx.update(|cx| cx.global::<Tasks>().writer.clone());
            assert!(writer.flush(Duration::from_secs(5)));
            cx.run_until_parked();
            if FORGE.asked_of(&self.branch).len() >= calls {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("asked only {:?}", FORGE.asked_of(&self.branch));
    }

    /// The clock comes round to the run's next look at its status checks,
    /// which is not made a moment before.
    fn next_look(&self, cx: &mut TestAppContext) {
        let asked = FORGE.asked_of(&self.branch).len();
        cx.executor().advance_clock(JUST_SHORT);
        cx.run_until_parked();
        assert_eq!(FORGE.asked_of(&self.branch).len(), asked, "looked early");
        cx.executor().advance_clock(THE_REST);
    }

    /// The prompts that reached the agent since last asked.
    fn prompts(&mut self) -> Vec<String> {
        std::iter::from_fn(|| self.requests.try_recv().ok())
            .filter_map(|request| match request {
                AcpRequest::Prompt { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }
}

/// A run's wait between looks at its status checks, a second short of it,
/// and the second that brings the clock round.
const JUST_SHORT: Duration = Duration::from_secs(59);
const THE_REST: Duration = Duration::from_secs(1);

/// Start a task of `template` on `branch` of a repository of its own, its
/// forge the connector called `forge`, and let its agent's turn end on a
/// commit.
fn begin_on(
    template: Template,
    branch: &str,
    forge: &str,
    id: &str,
    cx: &mut TestAppContext,
) -> Going {
    FORGE.forget(branch);
    let (work, store) = (repo(branch), temp_dir("store"));
    let window = app(cx, &store);
    // A session that goes on past its turn writes its conversation's name
    // through the process's writer.
    cx.update(|cx| cx.set_global(crate::state::Shared::for_test()));
    let setup = Setup {
        branch: Some(branch.into()),
        forge: Some(forge.into()),
        ..super::setup(&work)
    };
    let id = placed(Task::new(id.into(), template, brief(), setup), window, cx);
    let (session, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let started = begun(&id, cx);
    cx.update(|cx| start(7, &session, window, id.clone(), started, cx));
    cx.run_until_parked();
    let requests = carrier::up(&mut tx);
    cx.run_until_parked();
    // The agent's turn leaves a commit, which its gate asks for.
    std::fs::write(work.join("b.txt"), "b").unwrap();
    git(&work, &["add", "b.txt"]);
    let by = ["-c", "user.name=T", "-c", "user.email=t@t"];
    git(&work, &[&by[..], &["commit", "-q", "-m", "work"]].concat());
    carrier::turn(&mut tx);
    Going {
        id,
        branch: branch.into(),
        work,
        requests,
        _session: session,
        _tx: tx,
    }
}

/// Run a task of [`to_a_pull_request`] on `branch` of a repository of its
/// own, its forge the connector called `forge`, to its end: the task as
/// kept, and the commit its command passed on.
fn run_on(branch: &str, forge: &str, id: &str, cx: &mut TestAppContext) -> (Task, String) {
    let going = begin_on(to_a_pull_request(), branch, forge, id, cx);
    settle(cx);
    (
        kept(&going.id, cx),
        git(&going.work, &["rev-parse", "HEAD"]),
    )
}

/// The push, the look for a pull request, the open and the read back each
/// reach the forge once, in the order the run's drive asked for them, and
/// the run ends done on what the forge answered.
#[gpui::test]
fn a_runs_forge_calls_go_once_and_in_order(cx: &mut TestAppContext) {
    let (task, commit) = run_on("in-order", "Recording", "20", cx);
    assert_eq!(task.outcome(), Some(Outcome::Done));
    let kept = task.runs()[0]
        .pull_request
        .as_ref()
        .expect("its pull request");
    assert_eq!(
        (kept.number, kept.url.as_str()),
        (7, "https://forge.test/in-order")
    );
    assert_eq!(
        FORGE.asked_of("in-order"),
        [
            format!("push {commit}"),
            "find".to_string(),
            "open Say done".to_string(),
            "find".to_string(),
        ]
    );
}

/// A push the forge refuses reaches the run's drive: the run does not go on
/// to a pull request, and does not end done.
#[gpui::test]
fn a_refused_push_reaches_the_run_and_opens_nothing(cx: &mut TestAppContext) {
    let (task, commit) = run_on("refused-push", "Recording", "21", cx);
    assert_eq!(FORGE.asked_of("refused-push"), [format!("push {commit}")]);
    let outcome = task.outcome().expect("the run ended");
    assert_ne!(outcome, Outcome::Done);
    assert!(
        format!("{outcome:?}").contains("the forge refused the push"),
        "{outcome:?}"
    );
}

/// A run whose forge is not built in is told so, and nothing is asked of any
/// other forge on its behalf.
#[gpui::test]
fn a_run_with_no_such_forge_is_told_why(cx: &mut TestAppContext) {
    let (task, _) = run_on("no-forge", "Elsewhere", "22", cx);
    assert!(FORGE.asked_of("no-forge").is_empty());
    let outcome = task.outcome().expect("the run ended");
    assert!(
        format!("{outcome:?}").contains("no connector called Elsewhere"),
        "{outcome:?}"
    );
}

/// A run waiting on its status checks looks again each time its clock comes
/// round, and not before; once they pass, its draft is taken out of draft,
/// once, and the run ends done.
#[gpui::test]
fn status_checks_are_looked_at_on_the_clock_until_they_pass(cx: &mut TestAppContext) {
    let going = begin_on(through_status_checks(), "passing", "Recording", "23", cx);
    // The push, the look, the open and the read back.
    going.until_asked(4, cx);
    cx.run_until_parked();
    assert_eq!(
        FORGE.asked_of("passing").len(),
        4,
        "no look before the clock"
    );
    going.next_look(cx);
    going.until_asked(5, cx);
    assert_eq!(FORGE.asked_of("passing")[4], "find");
    assert_eq!(kept(&going.id, cx).outcome(), None, "still running");
    going.next_look(cx);
    going.until_asked(7, cx);
    settle(cx);
    assert_eq!(FORGE.asked_of("passing")[5..], ["find", "ready 7"]);
    assert_eq!(kept(&going.id, cx).outcome(), Some(Outcome::Done));
}

/// A status check that fails has its log read, once, and the agent is sent
/// back to its step with what the log said.
#[gpui::test]
fn a_failing_status_checks_log_goes_back_to_the_agent(cx: &mut TestAppContext) {
    let mut going = begin_on(through_status_checks(), "failing", "Recording", "24", cx);
    going.until_asked(4, cx);
    assert_eq!(going.prompts().len(), 1, "the step's own prompt");
    going.next_look(cx);
    going.until_asked(5, cx);
    going.next_look(cx);
    going.until_asked(7, cx);
    cx.run_until_parked();
    assert_eq!(FORGE.asked_of("failing")[5..], ["find", "log build"]);
    let repairs = going.prompts();
    assert_eq!(repairs.len(), 1, "{repairs:?}");
    assert!(
        repairs[0].contains("the build broke at line 3"),
        "{repairs:?}"
    );
}
