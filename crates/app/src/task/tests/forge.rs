//! A run's forge calls as its carrier makes them, against a forge that
//! records what it was asked: each call once, in the order the run's drive
//! asked, and a call that fails, or has no forge to go to, said to the drive.

use super::{app, begun, brief, git, kept, placed, repo, settle, temp_dir};
use crate::chat::session::tests::carrier;
use crate::task::start;
use gpui::TestAppContext;
use onehand_core::chat::Chat;
use onehand_core::connector::{Connector, PrState, PullRequest, RemoteIssue, SyncListing};
use onehand_core::issues::Snapshot;
use onehand_core::task::Task;
use onehand_core::unattended::IssueRow;
use onehand_core::workflow::{GateKind, Outcome, Place, Setup, StepKind, StepSpec, Template};
use std::path::Path;
use std::sync::Mutex;

/// A forge that keeps what it was asked, by the branch it was asked about:
/// the tests share it, and each works a branch of its own. A branch named
/// `refused…` is refused its push.
pub(in crate::task) struct Recording {
    asked: Mutex<Vec<(String, String)>>,
}

static FORGE: Recording = Recording {
    asked: Mutex::new(Vec::new()),
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
    fn push_blocking(&self, _: &Path, commit: &str, branch: &str) -> Result<(), String> {
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
        let opened = self
            .asked_of(branch)
            .iter()
            .any(|said| said.starts_with("open"));
        self.note(branch, "find");
        Ok(opened.then(|| PullRequest {
            url: format!("https://forge.test/{branch}"),
            number: 7,
            state: PrState::Open,
            draft: true,
            head: String::new(),
            conflicting: false,
            checks: Vec::new(),
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
    fn mark_ready_blocking(&self, _: &Path, _: u64) -> Result<(), String> {
        unreachable!()
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

/// Run a task of [`to_a_pull_request`] on `branch` of a repository of its
/// own, its forge the connector called `forge`, to its end: the task as
/// kept, and the commit its command passed on.
fn run_on(branch: &str, forge: &str, id: &str, cx: &mut TestAppContext) -> (Task, String) {
    let (work, store) = (repo(branch), temp_dir("store"));
    let window = app(cx, &store);
    let setup = Setup {
        branch: Some(branch.into()),
        forge: Some(forge.into()),
        ..super::setup(&work)
    };
    let id = placed(
        Task::new(id.into(), to_a_pull_request(), brief(), setup),
        window,
        cx,
    );
    let (session, mut tx, _seen) = carrier::driven(cx, Chat::default());
    let started = begun(&id, cx);
    cx.update(|cx| start(7, &session, window, id.clone(), started, cx));
    cx.run_until_parked();
    let _requests = carrier::up(&mut tx);
    cx.run_until_parked();
    // The agent's turn leaves a commit, which its gate asks for.
    std::fs::write(work.join("b.txt"), "b").unwrap();
    git(&work, &["add", "b.txt"]);
    let by = ["-c", "user.name=T", "-c", "user.email=t@t"];
    git(&work, &[&by[..], &["commit", "-q", "-m", "work"]].concat());
    carrier::turn(&mut tx);
    settle(cx);
    (kept(&id, cx), git(&work, &["rev-parse", "HEAD"]))
}

/// The push, the look for a pull request, the open and the read back each
/// reach the forge once, in the order the run's drive asked for them, and
/// the run ends done on what the forge answered.
#[gpui::test]
fn a_runs_forge_calls_go_once_and_in_order(cx: &mut TestAppContext) {
    let (task, commit) = run_on("in-order", "Recording", "20", cx);
    assert_eq!(task.outcome(), Some(Outcome::Done));
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
