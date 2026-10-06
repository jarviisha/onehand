use super::*;
use crate::workflow::{Brief, Setup, StepSpec, Template, Transition, Visit};
use std::path::PathBuf;

fn step(id: &str, kind: StepKind) -> StepSpec {
    StepSpec {
        id: id.into(),
        label: {
            let mut label = id.to_string();
            label[..1].make_ascii_uppercase();
            label
        },
        kind,
    }
}

fn agent() -> StepKind {
    StepKind::Agent {
        prompt: "do it".into(),
        gates: Vec::new(),
        keep_answer: true,
    }
}

/// Plan, approve, implement, verify, push, a pull request, its checks.
fn forge_flow() -> Template {
    let mut template = Template::blank("Issue");
    template.steps = vec![
        step("plan", agent()),
        step("approve", StepKind::Approval { of: "plan".into() }),
        step("implement", agent()),
        step(
            "verify",
            StepKind::Command {
                command: Some("make test".into()),
                on_fail: "implement".into(),
            },
        ),
        step("push", StepKind::Push),
        step("pr", StepKind::PullRequest),
        step(
            "checks",
            StepKind::StatusChecks {
                on_fail: "implement".into(),
                wait: "1h".into(),
            },
        ),
    ];
    template
}

/// Implement and verify, the branch being the result.
fn branch_flow() -> Template {
    let mut template = forge_flow();
    template.steps.truncate(4);
    template
}

fn task(id: &str, template: Template, forge: Option<&str>) -> Task {
    Task::new(
        id.into(),
        template,
        Brief {
            title: format!("task {id}"),
            body: String::new(),
            instructions: None,
        },
        Setup {
            repo: PathBuf::from("/repo"),
            dir: PathBuf::from("/repo-wt"),
            branch: Some("onehand/local-1".into()),
            agent: None,
            check: None,
            mode: None,
            forge: forge.map(str::to_string),
        },
    )
}

/// `task`'s run moved to step `at`, with an open visit there begun at `since`.
fn at(mut task: Task, at: usize, since: u64) -> Task {
    let run = task.runs.last_mut().unwrap();
    let id = run.template.steps[at].id.clone();
    run.step = at;
    run.visits.push(Visit {
        id: run.visits.len() as u32 + 1,
        step: id.clone(),
        started_at: since,
        ended_at: None,
        start: None,
        end: None,
        output: None,
        why: None,
    });
    run.history.push(Transition {
        at: since,
        from: String::new(),
        to: id,
        why: String::new(),
    });
    task
}

fn ended(mut task: Task, outcome: Outcome, why: Option<&str>) -> Task {
    let run = task.runs.last_mut().unwrap();
    if let Some(visit) = run.visits.last_mut() {
        visit.ended_at = Some(visit.started_at + 1);
        visit.why = why.map(str::to_string);
    }
    run.outcome = Some(outcome);
    task
}

fn open() -> Around<'static> {
    Around {
        open: true,
        can_start: true,
        pr: PrSeen::Unread,
        now: 1_000,
    }
}

fn closed() -> Around<'static> {
    Around {
        open: false,
        ..open()
    }
}

fn pr(state: PrState, draft: bool) -> PullRequest {
    PullRequest {
        url: "https://forge/pr/7".into(),
        number: 7,
        state,
        draft,
        head: "abc".into(),
        conflicting: false,
        checks: Vec::new(),
    }
}

fn next_of(task: &Task, working: Option<Working>, around: Around<'_>) -> Next {
    next_action(Some(&Work::of(task, working, None)), around)
}

fn acts(next: &Next) -> (Option<Act>, Vec<Act>) {
    (next.primary, next.secondary.clone())
}

#[test]
fn no_run_recorded_offers_a_run_workflow() {
    let next = next_action(None, open());
    assert_eq!(next.said.as_deref(), Some("No run recorded"));
    assert_eq!(
        acts(&next),
        (Some(Act::RunWorkflow), vec![Act::WorkHere, Act::Edit])
    );
    // Where no run can start, nothing is put in the primary place instead.
    let next = next_action(
        None,
        Around {
            can_start: false,
            ..open()
        },
    );
    assert_eq!(acts(&next), (None, vec![Act::WorkHere, Act::Edit]));
}

#[test]
fn a_closed_issue_with_no_run_offers_no_start() {
    let next = next_action(None, closed());
    assert_eq!(acts(&next), (None, vec![Act::ReopenIssue, Act::Edit]));
}

#[test]
fn running_says_the_step_and_how_long_and_needs_nobody() {
    let task = at(task("1", forge_flow(), Some("GitHub")), 2, 880);
    let work = Work::of(&task, Some(Working::Running), None);
    assert_eq!(work.said, "Running");
    assert_eq!(
        work.step,
        Some(StepAt {
            label: "Implement".into(),
            at: 3,
            of: 7
        })
    );
    let next = next_action(Some(&work), open());
    assert_eq!(
        next.said.as_deref(),
        Some("Working on Implement, started 2m ago")
    );
    assert_eq!(
        acts(&next),
        (None, vec![Act::OpenSession, Act::Stop, Act::Edit])
    );
}

#[test]
fn waiting_on_status_checks_waits_on_the_forge_not_on_a_person() {
    let task = at(task("1", forge_flow(), Some("GitHub")), 6, 900);
    let next = next_of(&task, Some(Working::Running), open());
    assert_eq!(
        next.said.as_deref(),
        Some("Waiting for the forge's checks, not for you")
    );
    assert_eq!(
        acts(&next),
        (None, vec![Act::OpenPullRequest, Act::Stop, Act::Edit])
    );
}

#[test]
fn queued_names_the_task_holding_its_place() {
    let task = task("1", forge_flow(), None);
    let work = Work::of(&task, Some(Working::Queued), Some("Fix login".into()));
    let next = next_action(Some(&work), open());
    assert_eq!(
        next.said.as_deref(),
        Some("Waiting for its place, held by \u{201c}Fix login\u{201d}")
    );
    assert_eq!(acts(&next), (None, vec![Act::ShowTask, Act::Edit]));
}

#[test]
fn an_approval_is_reviewed_and_says_what_approving_starts() {
    let task = at(task("1", forge_flow(), None), 1, 900);
    let work = Work::of(&task, Some(Working::Waiting), None);
    assert_eq!(work.said, "Waiting for approval");
    let next = next_action(Some(&work), open());
    assert_eq!(next.said.as_deref(), Some("Approving starts Implement"));
    assert_eq!(
        acts(&next),
        (Some(Act::Review), vec![Act::OpenSession, Act::Edit])
    );
}

#[test]
fn a_card_is_answered_in_the_session() {
    let task = at(task("1", forge_flow(), None), 2, 900);
    let work = Work::of(&task, Some(Working::Waiting), None);
    assert_eq!(work.said, "Waiting for an answer");
    let next = next_action(Some(&work), open());
    assert_eq!(
        acts(&next),
        (Some(Act::AnswerInSession), vec![Act::Stop, Act::Edit])
    );
}

#[test]
fn a_run_its_agent_or_a_restart_cut_off_resumes() {
    for task in [
        ended(
            at(task("1", forge_flow(), None), 2, 900),
            Outcome::Stopped(Stop::LinkLost),
            None,
        ),
        ended(
            at(task("1", forge_flow(), None), 2, 900),
            Outcome::Stopped(Stop::Closed),
            None,
        ),
        // Cut off: no outcome, nothing driving it.
        at(task("1", forge_flow(), None), 2, 900),
    ] {
        let next = next_of(&task, None, open());
        assert_eq!(
            acts(&next),
            (Some(Act::Resume), vec![Act::Retry, Act::Edit])
        );
    }
}

#[test]
fn an_exhausted_or_timed_out_run_says_where_and_what_its_last_visit_ended_on() {
    let task = ended(
        at(task("1", forge_flow(), None), 3, 900),
        Outcome::Exhausted {
            step: "verify".into(),
        },
        Some("make test failed"),
    );
    let next = next_of(&task, None, open());
    assert_eq!(
        next.said.as_deref(),
        Some("Too many misses at Verify: make test failed")
    );
    assert_eq!(
        acts(&next),
        (Some(Act::Retry), vec![Act::ShowTask, Act::Edit])
    );
    let task = ended(
        at(self::task("1", forge_flow(), None), 2, 900),
        Outcome::Stopped(Stop::TimedOut),
        None,
    );
    let timed_out = next_of(&task, None, open());
    assert_eq!(timed_out.said.as_deref(), Some("Timed out at Implement"));
    assert_eq!(timed_out.primary, Some(Act::Retry));
}

#[test]
fn a_failed_run_says_why_and_offers_a_retry() {
    let task = ended(
        at(task("1", forge_flow(), None), 0, 900),
        Outcome::Failed("the agent is not configured".into()),
        None,
    );
    let next = next_of(&task, None, open());
    assert_eq!(next.said.as_deref(), Some("the agent is not configured"));
    assert_eq!(
        acts(&next),
        (Some(Act::Retry), vec![Act::ShowTask, Act::Edit])
    );
}

#[test]
fn a_done_run_with_its_pull_request_open_is_reviewed_on_the_forge() {
    let task = ended(
        at(task("1", forge_flow(), Some("GitHub")), 6, 900),
        Outcome::Done,
        None,
    );
    let open_pr = pr(PrState::Open, true);
    let next = next_of(
        &task,
        None,
        Around {
            pr: PrSeen::Read(Some(&open_pr)),
            ..open()
        },
    );
    assert_eq!(next.said.as_deref(), Some("Review it on GitHub"));
    assert_eq!(acts(&next), (Some(Act::OpenPullRequest), vec![Act::Edit]));
    assert_eq!(pr_said(&open_pr), "open, draft");
}

#[test]
fn a_merged_pull_request_leaves_the_issue_to_say_whether_anything_is_left() {
    let task = ended(
        at(task("1", forge_flow(), Some("GitHub")), 6, 900),
        Outcome::Done,
        None,
    );
    let merged = pr(PrState::Merged, false);
    let next = next_of(
        &task,
        None,
        Around {
            pr: PrSeen::Read(Some(&merged)),
            ..open()
        },
    );
    assert_eq!(next.said, None);
    assert_eq!(acts(&next), (None, vec![Act::RunWorkflow, Act::Edit]));
}

#[test]
fn a_pull_request_closed_unmerged_is_reopened_not_the_issue() {
    let task = ended(
        at(task("1", forge_flow(), Some("GitHub")), 6, 900),
        Outcome::Done,
        None,
    );
    let shut = pr(PrState::Closed, false);
    let next = next_of(
        &task,
        None,
        Around {
            pr: PrSeen::Read(Some(&shut)),
            ..open()
        },
    );
    assert!(next
        .said
        .clone()
        .unwrap()
        .contains("reopen the pull request, not the issue"));
    assert_eq!(
        acts(&next),
        (Some(Act::OpenPullRequest), vec![Act::ShowTask, Act::Edit])
    );
}

#[test]
fn a_workflow_with_no_pull_request_step_ends_on_its_branch() {
    let task = ended(
        at(task("1", branch_flow(), Some("GitHub")), 3, 900),
        Outcome::Done,
        None,
    );
    let work = Work::of(&task, None, None);
    assert_eq!(work.pull_request, PrStep::None);
    // Never "could not be read": no pull request is the expected end.
    let next = next_action(
        Some(&work),
        Around {
            pr: PrSeen::Failed("offline"),
            ..open()
        },
    );
    assert_eq!(next.said.as_deref(), Some("The branch is the result"));
    assert_eq!(
        acts(&next),
        (Some(Act::OpenBranch), vec![Act::ShowTask, Act::Edit])
    );
}

#[test]
fn a_done_run_where_no_forge_serves_is_looked_at_on_its_branch() {
    let task = ended(
        at(task("1", forge_flow(), None), 6, 900),
        Outcome::Done,
        None,
    );
    let next = next_of(&task, None, open());
    assert!(next.said.clone().unwrap().contains("Close issue"));
    assert_eq!(acts(&next), (None, vec![Act::ShowTask, Act::Edit]));
}

#[test]
fn a_pull_request_that_could_not_be_read_is_said_and_refreshed() {
    let task = ended(
        at(task("1", forge_flow(), Some("GitHub")), 6, 900),
        Outcome::Done,
        None,
    );
    let next = next_of(
        &task,
        None,
        Around {
            pr: PrSeen::Failed("gh: no network"),
            ..open()
        },
    );
    assert_eq!(
        next.said.as_deref(),
        Some("The pull request could not be read: gh: no network")
    );
    assert_eq!(
        acts(&next),
        (Some(Act::Refresh), vec![Act::ShowTask, Act::Edit])
    );
}

#[test]
fn a_run_ended_by_a_person_is_no_fault_to_fix() {
    let stopped = ended(
        at(task("1", forge_flow(), None), 2, 900),
        Outcome::Stopped(Stop::ByPerson),
        None,
    );
    let taken = ended(
        at(task("1", forge_flow(), None), 2, 900),
        Outcome::Stopped(Stop::TakenOver),
        None,
    );
    let mut dismissed = ended(
        at(task("1", forge_flow(), None), 2, 900),
        Outcome::Failed("boom".into()),
        None,
    );
    dismissed.dismissed = true;
    for task in [stopped, taken, dismissed] {
        let next = next_of(&task, None, open());
        assert_eq!(next.said, None);
        assert_eq!(acts(&next), (None, vec![Act::RunWorkflow, Act::Edit]));
    }
}

#[test]
fn an_ended_task_on_a_closed_issue_mutes() {
    let task = ended(
        at(task("1", forge_flow(), None), 3, 900),
        Outcome::Exhausted {
            step: "verify".into(),
        },
        None,
    );
    let next = next_of(&task, None, closed());
    assert!(next.muted && !next.still_active);
    assert_eq!(
        acts(&next),
        (None, vec![Act::ReopenIssue, Act::ShowTask, Act::Edit])
    );
}

#[test]
fn an_active_task_on_a_closed_issue_keeps_its_controls() {
    let task = at(task("1", forge_flow(), None), 2, 900);
    let next = next_of(&task, Some(Working::Waiting), closed());
    assert!(next.still_active && !next.muted);
    assert_eq!(
        acts(&next),
        (Some(Act::AnswerInSession), vec![Act::Stop, Act::Edit])
    );
}

#[test]
fn a_run_sent_back_reports_the_earlier_step() {
    let task = at(at(task("1", forge_flow(), None), 3, 900), 2, 950);
    let work = Work::of(&task, Some(Working::Running), None);
    let step = work.step.unwrap();
    assert_eq!((step.label.as_str(), step.at, step.of), ("Implement", 3, 7));
}

#[test]
fn the_pull_request_step_counts_as_reached_once_visited() {
    let task = at(task("1", forge_flow(), Some("GitHub")), 4, 900);
    assert_eq!(
        Work::of(&task, Some(Working::Running), None).pull_request,
        PrStep::NotYet
    );
    let task = at(task, 5, 950);
    assert_eq!(
        Work::of(&task, Some(Working::Running), None).pull_request,
        PrStep::Reached
    );
}

#[test]
fn the_newest_task_is_the_issues_work_and_an_older_one_needing_attention_is_counted() {
    let older = ended(
        at(task("1", forge_flow(), None), 3, 100),
        Outcome::Exhausted {
            step: "verify".into(),
        },
        None,
    );
    let newer = at(task("2", forge_flow(), None), 2, 500);
    let key = IssueKey {
        file: PathBuf::from("/s/issues/repo.json"),
        number: 12,
    };
    let work = issue_work(
        key.clone(),
        [(&older, None, None), (&newer, Some(Working::Running), None)],
    )
    .unwrap();
    assert_eq!(work.key, key);
    assert_eq!(work.work.task, "2");
    assert_eq!(work.earlier.len(), 1);
    assert!(work.earlier[0].attention);
    assert_eq!(work.earlier[0].task, "1");
    assert_eq!(work.earlier_runs, 0);
    assert!(issue_work(key, []).is_none());
}

#[test]
fn a_stale_generation_is_dropped() {
    let mut reading: Reading<&str, u32> = Reading::default();
    let first = reading.ask("issue 1");
    let second = reading.ask("issue 2");
    assert!(!reading.land(first, Ok(1), 10), "another issue's answer");
    assert_eq!(reading.value, None);
    assert!(reading.land(second, Ok(2), 11));
    assert_eq!(reading.value, Some((2, 11)));
}

#[test]
fn of_two_answers_out_of_order_the_later_request_wins() {
    let mut reading: Reading<&str, u32> = Reading::default();
    let first = reading.ask("issue 1");
    let second = reading.ask("issue 1");
    assert!(reading.land(second, Ok(2), 20));
    assert!(
        !reading.land(first, Ok(1), 21),
        "the earlier one landing last"
    );
    assert_eq!(reading.value, Some((2, 20)));
}

#[test]
fn a_new_run_makes_older_reads_stale_and_a_failure_keeps_the_last_value() {
    let mut reading: Reading<(&str, &str), u32> = Reading::default();
    let asked = reading.ask(("task", "run 1"));
    assert!(reading.land(asked, Ok(1), 10));
    let asked = reading.ask(("task", "run 1"));
    assert!(reading.land(asked, Err("offline".into()), 20));
    assert_eq!(reading.value, Some((1, 10)), "kept, marked by the failure");
    assert_eq!(reading.failed.as_deref(), Some("offline"));
    // A retry started a new run: what was read of the old one goes.
    let old = reading.ask(("task", "run 1"));
    let asked = reading.ask(("task", "run 2"));
    assert_eq!(reading.value, None);
    assert!(!reading.land(old, Ok(9), 30));
    assert!(reading.land(asked, Ok(2), 31));
}
