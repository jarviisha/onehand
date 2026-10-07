use super::*;
use crate::workflow::{Brief, Setup, StepSpec, Template, Transition, Visit};
use std::path::PathBuf;

pub(super) fn step(id: &str, kind: StepKind) -> StepSpec {
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

pub(super) fn agent() -> StepKind {
    StepKind::Agent {
        prompt: "do it".into(),
        gates: Vec::new(),
        keep_answer: true,
    }
}

/// Plan, approve, implement, verify, push, a pull request, its checks.
pub(super) fn forge_flow() -> Template {
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
pub(super) fn branch_flow() -> Template {
    let mut template = forge_flow();
    template.steps.truncate(4);
    template
}

/// `task` as an issue's task, issue #3 of a local file.
pub(super) fn on_issue(mut task: Task) -> Task {
    task.source = super::super::Source::Issue(crate::unattended::IssueSource {
        tracker: crate::unattended::TrackerRef::Local {
            file: PathBuf::from("/repo/issues.toml"),
        },
        number: 3,
        forge_ref: None,
        forge: task.setup.forge.clone(),
        base: "origin/main".into(),
        picked: false,
        unsent: Vec::new(),
        notes: Vec::new(),
    });
    task
}

pub(super) fn task(id: &str, template: Template, forge: Option<&str>) -> Task {
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
pub(super) fn at(mut task: Task, at: usize, since: u64) -> Task {
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
        command: None,
    });
    run.history.push(Transition {
        at: since,
        from: String::new(),
        to: id,
        why: String::new(),
    });
    task
}

pub(super) fn ended(mut task: Task, outcome: Outcome, why: Option<&str>) -> Task {
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
        session: false,
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

pub(super) fn pr(state: PrState, draft: bool) -> PullRequest {
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
    assert_eq!(next.said, None, "the progress line says it");
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
    // A session it started still works it: that is offered, not a second.
    let next = next_action(
        None,
        Around {
            session: true,
            ..open()
        },
    );
    assert_eq!(
        acts(&next),
        (
            Some(Act::RunWorkflow),
            vec![Act::OpenWorkingSession, Act::Edit]
        )
    );
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
    let mut task = ended(
        at(task("1", forge_flow(), None), 3, 900),
        Outcome::Exhausted {
            step: "verify".into(),
        },
        Some("Workflow stopped: too many misses at the Verify step"),
    );
    // What the visit kept, not why it ended, which the outcome says already.
    task.runs
        .last_mut()
        .unwrap()
        .visits
        .last_mut()
        .unwrap()
        .output = Some("running 3 tests\nmake test failed\n".into());
    let next = next_of(&task, None, open());
    assert_eq!(
        next.said.as_deref(),
        Some("Too many misses at Verify: make test failed")
    );
    assert_eq!(
        acts(&next),
        (Some(Act::Retry), vec![Act::ShowTask, Act::Edit])
    );
    let mut task = ended(
        at(self::task("1", forge_flow(), None), 2, 900),
        Outcome::Stopped(Stop::TimedOut),
        None,
    );
    task.runs[0].spent_secs = 45 * 60;
    let timed_out = next_of(&task, None, open());
    assert_eq!(
        timed_out.said.as_deref(),
        Some("Timed out at Implement after 45m, against its timeout of 45m")
    );
    assert_eq!(
        acts(&timed_out),
        (Some(Act::Retry), vec![Act::ShowTask, Act::Edit]),
        "Retry keeps the timeout"
    );
    // With the timeout on offer changed since, the second way is offered.
    let mut work = Work::of(&task, None, None);
    work.timeout_moved = true;
    assert_eq!(
        acts(&next_action(Some(&work), open())),
        (
            Some(Act::Retry),
            vec![Act::RetryCurrent, Act::ShowTask, Act::Edit]
        )
    );
}

/// A failed run is offered the way out its failure fits: a configuration
/// failure is changed where the configuration is, the rest retried.
#[test]
fn a_failed_run_says_why_and_offers_the_way_out_that_fits() {
    let failed = |kind: Option<crate::workflow::Failure>| {
        let mut task = ended(
            at(task("1", forge_flow(), None), 0, 900),
            Outcome::Failed("the agent `x` is no longer configured".into()),
            None,
        );
        task.runs[0].failure = kind;
        next_of(&task, None, open())
    };
    let next = failed(Some(crate::workflow::Failure::Configuration));
    assert_eq!(
        next.said.as_deref(),
        Some("the agent `x` is no longer configured")
    );
    assert_eq!(
        acts(&next),
        (
            Some(Act::RetryCurrent),
            vec![Act::Retry, Act::ShowTask, Act::Edit]
        )
    );
    for kind in [
        Some(crate::workflow::Failure::Forge),
        Some(crate::workflow::Failure::Other),
        None,
    ] {
        assert_eq!(
            acts(&failed(kind)),
            (Some(Act::Retry), vec![Act::ShowTask, Act::Edit]),
            "{kind:?}"
        );
    }
}

#[test]
fn a_done_run_with_its_pull_request_open_is_reviewed_on_the_forge() {
    let task = ended(
        at(on_issue(task("1", forge_flow(), Some("GitHub"))), 6, 900),
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
    // A review left there is answered from here as well as by putting the
    // label back; whether it can be is the preflight's to say when pressed.
    assert_eq!(
        acts(&next),
        (
            Some(Act::OpenPullRequest),
            vec![Act::AnswerReview, Act::Edit]
        )
    );
    assert_eq!(pr_named(&open_pr), "#7 · open, draft");
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
    assert_eq!(work.pull_request, PrStep::Absent);
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
    assert!(next
        .said
        .clone()
        .unwrap()
        .contains("close the issue when satisfied"));
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
    // Done where no forge serves: the issue is closed already, so closing it
    // is no longer asked.
    let done = ended(
        at(self::task("2", forge_flow(), None), 6, 900),
        Outcome::Done,
        None,
    );
    let next = next_of(&done, None, closed());
    assert_eq!(next.said.as_deref(), Some("Look at the branch"));
}

#[test]
fn an_active_task_on_a_closed_issue_keeps_its_controls() {
    let task = at(task("1", forge_flow(), None), 2, 900);
    let next = next_of(&task, Some(Working::Waiting), closed());
    assert!(next.still_active && !next.muted);
    assert_eq!(
        acts(&next),
        (
            Some(Act::AnswerInSession),
            vec![Act::Stop, Act::ReopenIssue, Act::Edit]
        )
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
    assert!(work.earlier[0].attention && !work.earlier[0].active);
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

#[test]
fn a_done_run_with_no_branch_is_never_left_reading_a_pull_request() {
    let mut task = ended(
        at(task("1", forge_flow(), Some("GitHub")), 6, 900),
        Outcome::Done,
        None,
    );
    task.runs.last_mut().unwrap().setup.branch = None;
    let mut work = Work::of(&task, None, None);
    work.branch = None;
    assert!(!work.has_pull_request());
    let next = next_action(Some(&work), open());
    assert_eq!(next.said.as_deref(), Some("The branch is the result"));
}

#[test]
fn the_steps_to_come_are_the_ones_after_the_step_it_is_at() {
    let task = at(task("1", forge_flow(), Some("GitHub")), 4, 900);
    let work = Work::of(&task, Some(Working::Running), None);
    assert_eq!(work.rest, ["Pr", "Checks"]);
    // Sent back, the steps to come are counted from where it is again.
    let task = at(task, 2, 950);
    let work = Work::of(&task, Some(Working::Running), None);
    assert_eq!(work.rest, ["Verify", "Push", "Pr", "Checks"]);
    let done = ended(task, Outcome::Done, None);
    assert!(Work::of(&done, None, None).rest.is_empty());
}

#[test]
fn the_run_and_the_branch_are_measured_from_their_first_marks() {
    let mut task = at(task("1", forge_flow(), Some("GitHub")), 0, 900);
    assert_eq!(Work::of(&task, Some(Working::Running), None).span, None);
    {
        let visit = &mut task.runs[0].visits[0];
        visit.start = Some("s1".into());
        visit.end = Some("e1".into());
        visit.ended_at = Some(901);
    }
    let mut task = at(task, 2, 902);
    task.runs[0].visits[1].start = Some("s2".into());
    let work = Work::of(&task, Some(Working::Running), None);
    assert_eq!(work.span, Some(("s1".to_string(), "s2".to_string())));
    assert_eq!(work.base.as_deref(), Some("s1"));
}

#[test]
fn a_check_vouches_only_for_the_work_it_passed_on() {
    use super::left::{check_stands, CheckStands, Vouched, WorkNow};
    let now = |head: &str, dirty: bool, digest: &str, behind: u64| WorkNow {
        head: head.into(),
        dirty,
        digest: digest.into(),
        behind,
    };
    let ran = |digest: Option<&str>| Vouched {
        commit: "abc".into(),
        digest: digest.map(str::to_string),
        tail: None,
    };
    assert_eq!(
        check_stands(None, &now("abc", false, "d0", 0)),
        CheckStands::NotRecorded
    );
    // The commit and the uncommitted work it ran on, as they are now.
    assert_eq!(
        check_stands(Some(&ran(Some("d1"))), &now("abc", true, "d1", 0)),
        CheckStands::OnThis("abc".into())
    );
    // A tracked file edited since, without a commit; or only an untracked
    // file added: either way the fingerprint differs.
    assert_eq!(
        check_stands(Some(&ran(Some("d1"))), &now("abc", true, "d2", 0)),
        CheckStands::Changed("abc".into())
    );
    // Another commit, counted.
    assert_eq!(
        check_stands(Some(&ran(Some("d1"))), &now("def", false, "d0", 2)),
        CheckStands::Behind {
            at: "abc".into(),
            commits: 2
        }
    );
    // From before the fingerprint was kept: the commit alone vouches for a
    // clean worktree, and cannot tell for a dirty one.
    assert_eq!(
        check_stands(Some(&ran(None)), &now("abc", false, "d0", 0)),
        CheckStands::OnThis("abc".into())
    );
    assert_eq!(
        check_stands(Some(&ran(None)), &now("abc", true, "d2", 0)),
        CheckStands::CannotTell("abc".into())
    );
    assert_eq!(
        CheckStands::Behind {
            at: "0123456789abcdef".into(),
            commits: 2
        }
        .said(),
        "passed on 0123456789, 2 commits before the work now"
    );
    assert_eq!(
        CheckStands::Changed("abc".into()).said(),
        "passed on abc; the work changed since the check passed"
    );
    assert_eq!(
        CheckStands::CannotTell("abc".into()).said(),
        "passed on abc; cannot tell whether the check covers the work now"
    );
}

/// The check a run's work shows is its last passed command, with what that
/// command printed; a run from before part B has the commit alone.
#[test]
fn the_check_shown_is_the_last_passed_command() {
    let verify = |command: Option<crate::workflow::CommandResult>| Visit {
        id: 9,
        step: "verify".into(),
        started_at: 1,
        ended_at: Some(2),
        start: None,
        end: None,
        output: None,
        why: None,
        command,
    };
    // A retry carries the commit the last run's check passed on, for its
    // push; no command has run in it, so its check is not recorded.
    let mut task = task("1", branch_flow(), None);
    task.runs[0].marks.verified_at = Some("abc".into());
    assert_eq!(Work::of(&task, None, None).vouched, None);

    // A run from before results were kept ran its command and kept the
    // commit alone.
    task.runs[0].visits.push(verify(None));
    let vouched = Work::of(&task, None, None).vouched.unwrap();
    assert_eq!((vouched.commit.as_str(), vouched.digest), ("abc", None));

    task.runs[0]
        .visits
        .push(verify(Some(crate::workflow::CommandResult {
            passed: true,
            exit: Some(0),
            tail: "all 12 passed".into(),
            commit: Some("abc".into()),
            digest: Some("d1".into()),
        })));
    let vouched = Work::of(&task, None, None).vouched.unwrap();
    assert_eq!(vouched.digest.as_deref(), Some("d1"));
    assert_eq!(vouched.tail.as_deref(), Some("all 12 passed"));
}

/// A task whose run, driven by the engine, answered its plan and waits for
/// approval, with the plan's visit pinned from `m1` to `m2`.
fn plan_waiting() -> Task {
    use crate::workflow::{Facts, Mark};
    let mut task = task("1", forge_flow(), None);
    let run = task.runs.last_mut().unwrap();
    run.resume();
    run.pinned("m1", 0);
    let mark = Mark {
        head: "a".into(),
        digests: vec!["d0".into()],
    };
    run.measured(mark);
    let facts = Facts {
        head: "a".into(),
        dirty: false,
        commits: 0,
        digest: "d0".into(),
    };
    run.turn_ended(&facts, "The plan.");
    run.pinned("m2", 1);
    task
}

/// What waits for approval is read from the run, with the visit a press
/// carries, what each answer starts, and the plan's own span of the work.
#[test]
fn an_approval_says_what_is_reviewed_and_what_each_answer_starts() {
    let task = plan_waiting();
    let work = Work::of(&task, Some(Working::Waiting), None);
    let review = work.review.clone().expect("it waits for approval");
    assert_eq!(review.at, task.runs[0].approval_at().unwrap());
    assert_eq!(review.of, "Plan");
    assert_eq!(review.answer, "The plan.");
    assert_eq!(
        review.continue_said(),
        "Continue starts Implement: the agent works"
    );
    assert_eq!(review.revise_said(), "Plan runs again with your note");
    assert_eq!(review.span, Some(("m1".to_string(), "m2".to_string())));
    assert_eq!(
        review.check,
        Checked::NotRun,
        "no command ran since the plan started"
    );

    // A command that ran since the step under review began is what its
    // check says, failed as well as passed.
    let mut checked = task.clone();
    let failed = crate::workflow::CommandResult {
        passed: false,
        exit: Some(2),
        tail: "1 test failed".into(),
        commit: Some("a".into()),
        digest: None,
    };
    let at = checked.runs[0].visits.len() - 1;
    checked.runs[0].visits.insert(
        at,
        Visit {
            id: 99,
            step: "verify".into(),
            started_at: 1,
            ended_at: Some(2),
            start: None,
            end: None,
            output: None,
            why: None,
            command: Some(failed.clone()),
        },
    );
    let review = Work::of(&checked, Some(Working::Waiting), None)
        .review
        .unwrap();
    assert_eq!(review.check, Checked::Ran(failed));
    // One from before results were kept is said as not kept, for the work's
    // own check to judge.
    checked.runs[0].visits[at].command = None;
    let review = Work::of(&checked, Some(Working::Waiting), None)
        .review
        .unwrap();
    assert_eq!(review.check, Checked::NotKept);

    // Nothing is under review once the run moved on, or for a run that is
    // not waiting.
    assert!(Work::of(&task, Some(Working::Running), None)
        .review
        .is_none());
}

/// What a step does, in the words beside *Continue*.
#[test]
fn a_step_says_what_it_does() {
    let changes = StepKind::Agent {
        prompt: String::new(),
        gates: vec![crate::workflow::GateKind::CodeChanged],
        keep_answer: false,
    };
    assert_eq!(changes.does(), "the agent changes the code");
    let answers = StepKind::Agent {
        prompt: String::new(),
        gates: vec![crate::workflow::GateKind::Answered],
        keep_answer: true,
    };
    assert_eq!(answers.does(), "the agent answers");
    assert_eq!(
        StepKind::Command {
            command: None,
            on_fail: "x".into()
        }
        .does(),
        "onehand runs the check command"
    );
    assert_eq!(
        StepKind::Command {
            command: Some("make package".into()),
            on_fail: "x".into()
        }
        .does(),
        "onehand runs make package"
    );
    assert_eq!(
        StepKind::Push.does(),
        "onehand pushes the commit the check passed on"
    );
    assert_eq!(
        StepKind::PullRequest.does(),
        "onehand opens a draft pull request"
    );
}

/// An answer is drawn by its last lines, saying how many it left out.
#[test]
fn an_answer_is_cut_to_its_last_lines() {
    let long: String = (1..=70).map(|n| format!("line {n}\n")).collect();
    let (shown, left_out) = last_lines(&long, ANSWER_LINES);
    assert_eq!(left_out, 10);
    assert!(shown.starts_with("line 11\n"));
    assert_eq!(last_lines("short", ANSWER_LINES), ("short", 0));
}
