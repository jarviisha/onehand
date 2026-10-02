use super::*;
use crate::connector::Check;

fn pr(head: &str, checks: &[(&str, CheckState)]) -> PullRequest {
    PullRequest {
        url: "https://forge/pr/1".into(),
        number: 1,
        state: PrState::Open,
        draft: true,
        head: head.into(),
        conflicting: false,
        checks: checks
            .iter()
            .map(|(name, state)| Check {
                name: name.to_string(),
                state: *state,
                link: None,
            })
            .collect(),
    }
}

fn facts(commits: u64, dirty: bool, pr: Option<PullRequest>) -> Facts {
    Facts {
        commits,
        dirty,
        head: "h1".into(),
        pr,
        changed: false,
    }
}

fn gate(forge: bool, turns_left: u32) -> Gate<'static> {
    Gate {
        committed: true,
        forge,
        approve_plans: false,
        check: false,
        turns_left,
        answer: "",
    }
}

fn open_pr(facts: &Facts) -> Next {
    next(Step::OpenPr, facts, &gate(true, 2))
}

#[test]
fn a_turn_is_judged_by_the_branch_not_by_what_the_agent_said() {
    // Only analysis: nothing committed, so the session carries on.
    assert_eq!(
        open_pr(&facts(0, false, None)),
        Next::CarryOn(Missing::NoCommits)
    );
    assert_eq!(
        open_pr(&facts(2, true, None)),
        Next::CarryOn(Missing::Uncommitted)
    );
    // An agent claiming a pull request it never opened is still missing one.
    assert_eq!(
        open_pr(&facts(2, false, None)),
        Next::CarryOn(Missing::NoPullRequest)
    );
    assert_eq!(
        open_pr(&facts(2, false, Some(pr("h0", &[])))),
        Next::CarryOn(Missing::Unpushed)
    );
    assert_eq!(
        open_pr(&facts(2, false, Some(pr("h1", &[])))),
        Next::AwaitChecks
    );
}

#[test]
fn a_turn_with_no_turns_left_exhausts_and_a_closed_pull_request_settles() {
    assert_eq!(
        next(Step::OpenPr, &facts(0, false, None), &gate(true, 0)),
        Next::Exhausted
    );
    let mut closed = pr("h0", &[]);
    closed.state = PrState::Closed;
    assert_eq!(open_pr(&facts(0, false, Some(closed))), Next::Settle);
}

#[test]
fn a_project_with_no_forge_settles_on_its_commits() {
    let implement = |f: &Facts| next(Step::Implement, f, &gate(false, 2));
    assert_eq!(implement(&facts(1, false, None)), Next::Settle);
    assert_eq!(
        implement(&facts(0, false, None)),
        Next::CarryOn(Missing::NoCommits)
    );
}

#[test]
fn a_plan_is_an_answer_that_changed_nothing() {
    let plan = |f: &Facts, answer: &'static str, approve_plans: bool| {
        next(
            Step::Plan,
            f,
            &Gate {
                answer,
                approve_plans,
                ..gate(true, 2)
            },
        )
    };
    let clean = facts(0, false, None);
    assert_eq!(plan(&clean, "  ", false), Next::CarryOn(Missing::NoPlan));
    assert_eq!(
        plan(&facts(1, false, None), "the plan", false),
        Next::CarryOn(Missing::PlanTouchedCode)
    );
    assert_eq!(
        plan(&facts(0, true, None), "the plan", false),
        Next::CarryOn(Missing::PlanTouchedCode)
    );
    assert_eq!(
        plan(&clean, "the plan", false),
        Next::Advance(Step::Implement)
    );
    assert_eq!(plan(&clean, "the plan", true), Next::AwaitApproval);
}

#[test]
fn a_committed_change_goes_to_the_check_then_the_pull_request() {
    let done = facts(1, false, None);
    let with_check = Gate {
        check: true,
        ..gate(true, 2)
    };
    assert_eq!(next(Step::Implement, &done, &with_check), Next::RunCheck);
    assert_eq!(next(Step::Verify, &done, &with_check), Next::RunCheck);
    assert_eq!(
        next(Step::Implement, &done, &gate(true, 2)),
        Next::Advance(Step::OpenPr),
        "no check command skips the check"
    );
    assert_eq!(
        next(Step::Verify, &facts(1, true, None), &with_check),
        Next::CarryOn(Missing::Uncommitted)
    );
}

#[test]
fn a_check_passing_moves_on_and_one_failing_is_handed_back() {
    assert_eq!(after_check(Ok(()), 2, true), Next::Advance(Step::OpenPr));
    assert_eq!(after_check(Ok(()), 2, false), Next::Settle);
    assert_eq!(
        after_check(Err("boom".into()), 1, true),
        Next::CarryOn(Missing::CheckFailed("boom".into()))
    );
    assert_eq!(after_check(Err("boom".into()), 0, true), Next::Exhausted);
    let said = carry_on(
        &Missing::CheckFailed("error: boom".into()),
        Step::Verify,
        None,
        Place::Branch,
    );
    assert!(said.contains("```\nerror: boom\n```"), "{said}");
}

#[test]
fn only_the_pull_request_step_is_told_to_push() {
    let forge = &crate::connector::fake::Fake::SERVING;
    let implement = carry_on(
        &Missing::Uncommitted,
        Step::Implement,
        Some(forge),
        Place::Branch,
    );
    assert!(!implement.contains("push"), "{implement}");
    let open = carry_on(
        &Missing::Uncommitted,
        Step::OpenPr,
        Some(forge),
        Place::Branch,
    );
    assert!(open.contains("push"), "{open}");
}

const HOUR: Duration = Duration::from_secs(3600);
const MINUTE: Duration = Duration::from_secs(60);

#[test]
fn only_every_check_passing_on_the_head_is_ready() {
    use CheckState::*;
    let checks =
        |c: &[(&str, CheckState)]| after_checks(Some(&pr("h", c)), MINUTE, HOUR, 2, 0, &[]);
    assert_eq!(
        checks(&[("a", Passed), ("b", Passed)]),
        Checked::Ready { ran: true }
    );
    assert_eq!(checks(&[("a", Passed), ("b", Pending)]), Checked::Wait);
    assert_eq!(
        checks(&[("a", Failed), ("b", Pending)]),
        Checked::Repair {
            failing: vec!["a".into()],
            conflicting: false
        },
        "a failure is repaired without waiting on the rest"
    );
}

#[test]
fn no_checks_at_all_waits_for_them_to_appear_before_it_counts_as_none() {
    let none = pr("h", &[]);
    assert_eq!(
        after_checks(Some(&none), MINUTE, HOUR, 2, 0, &[]),
        Checked::Wait
    );
    assert_eq!(
        after_checks(Some(&none), CHECKS_GRACE, HOUR, 2, 0, &[]),
        Checked::Ready { ran: false }
    );
}

#[test]
fn checks_that_never_finish_or_keep_failing_exhaust_the_run() {
    use CheckState::*;
    let running = pr("h", &[("a", Pending)]);
    assert_eq!(
        after_checks(Some(&running), HOUR, HOUR, 2, 0, &[]),
        Checked::Exhausted(Spent::ChecksPending(HOUR))
    );
    let failing = pr("h", &[("a", Failed), ("b", Failed)]);
    assert_eq!(
        after_checks(Some(&failing), MINUTE, HOUR, 1, 1, &["b".into()]),
        Checked::Exhausted(Spent::FailedAgain(vec!["b".into()]))
    );
    assert_eq!(
        after_checks(Some(&failing), MINUTE, HOUR, 0, 2, &[]),
        Checked::Exhausted(Spent::Repairs(2))
    );
}

#[test]
fn a_conflict_is_repaired_and_a_closed_pull_request_is_gone() {
    let mut conflicting = pr("h", &[]);
    conflicting.conflicting = true;
    assert_eq!(
        after_checks(Some(&conflicting), MINUTE, HOUR, 1, 0, &[]),
        Checked::Repair {
            failing: Vec::new(),
            conflicting: true
        }
    );
    let mut merged = pr("h", &[]);
    merged.state = PrState::Merged;
    assert_eq!(
        after_checks(Some(&merged), MINUTE, HOUR, 1, 0, &[]),
        Checked::Gone
    );
    assert_eq!(after_checks(None, MINUTE, HOUR, 1, 0, &[]), Checked::Gone);
}

#[test]
fn a_repair_hands_over_the_failing_logs_and_never_asks_for_a_second_pull_request() {
    let start = Start::Repair {
        pr: "https://forge/pr/1".into(),
        conflicting: false,
        failing: vec![Failure {
            check: "Clippy".into(),
            log: "error: unused variable".into(),
        }],
    };
    let said = start.said(None).unwrap();
    assert!(said.contains("https://forge/pr/1"));
    assert!(said.contains("Clippy:\n\n```\nerror: unused variable\n```"));
    assert!(start.has_pull_request());
    assert!(!Start::Earlier.has_pull_request());
}

#[test]
fn a_run_file_reads_back_as_it_was_written() {
    let dir = std::env::temp_dir().join(format!("onehand-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let record = Record {
        repo: "/r".into(),
        kept: Kept::Synced {
            file: "/r/issues.json".into(),
            forge: "Forge".into(),
        },
        forge: Some("Forge".into()),
        issue: Issue::new(4, "t".into(), "b".into()),
        branch: "onehand/issue-4-t".into(),
        base: "origin/main".into(),
        dir: "/r-wt".into(),
        by_hand: false,
        phase: Phase::AwaitingChecks { since: 7 },
        progress: Progress::new(
            Start::Review {
                pr: "u".into(),
                number: 3,
            },
            Some("abc".into()),
            Some("keep it small".into()),
        ),
    };
    let mut approval = record.clone();
    approval.phase = Phase::AwaitingApproval { since: 9 };
    approval.progress.plan = Some("the plan".into());
    approval.progress.revise = Some("smaller".into());
    let file = dir.join("1-4.json");
    save_record_blocking(&file, &record).unwrap();
    std::fs::write(dir.join("2-5.json"), "{").unwrap();
    let found = load_records_blocking(&dir);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0], (file.clone(), Ok(record)));
    assert!(
        found[1].1.is_err(),
        "an unreadable file is said, not skipped"
    );
    save_record_blocking(&file, &approval).unwrap();
    assert_eq!(load_records_blocking(&dir)[0].1, Ok(approval));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_record_from_before_steps_reads_as_the_change() {
    let old = r#"{"start":"Fresh","repairs":0,"failed_before":[],"spent_secs":5,"since":null}"#;
    let progress: Progress = serde_json::from_str(old).unwrap();
    assert_eq!(progress.step, Step::Implement);
    assert_eq!(progress.plan, None);
    assert_eq!(Progress::new(Start::Fresh, None, None).step, Step::Plan);
    let repair = Start::Repair {
        pr: "u".into(),
        conflicting: true,
        failing: Vec::new(),
    };
    assert_eq!(Progress::new(repair, None, None).step, Step::Implement);
}

#[test]
fn only_a_failed_gate_spends_a_turn() {
    // A budget of one turn: the first miss is already the last.
    let max = 1;
    assert_eq!(turns_left(max, 0), 0);
    // Yet a run that never misses goes through every step on it.
    let last = |step, f: &Facts| {
        next(
            step,
            f,
            &Gate {
                answer: "the plan",
                check: true,
                ..gate(true, turns_left(max, 0))
            },
        )
    };
    assert_eq!(
        last(Step::Plan, &facts(0, false, None)),
        Next::Advance(Step::Implement)
    );
    assert_eq!(
        last(Step::Implement, &facts(1, false, None)),
        Next::RunCheck
    );
    assert_eq!(
        after_check(Ok(()), turns_left(max, 0), true),
        Next::Advance(Step::OpenPr)
    );
    assert_eq!(
        last(Step::OpenPr, &facts(1, false, Some(pr("h1", &[])))),
        Next::AwaitChecks
    );
    // With three, two misses carry on and the third exhausts.
    assert_eq!(turns_left(3, 1), 1);
    assert_eq!(turns_left(3, 2), 0);
    assert_eq!(turns_left(3, 5), 0);
}

#[test]
fn steps_are_listed_in_the_order_a_run_takes_them() {
    // What is behind a run's step is read by comparing steps, so the order
    // they compare in has to be the order they are taken in.
    assert!(Step::ALL.is_sorted());
    assert!(Step::Plan < Step::OpenPr);
}

/// A checkout's facts: `commits` past the step's start, and whether its
/// uncommitted work changed.
fn checkout(commits: u64, changed: bool) -> Facts {
    Facts {
        changed,
        ..facts(commits, false, None)
    }
}

/// A checkout's gate, with or without a check command.
fn in_checkout(check: bool) -> Gate<'static> {
    Gate {
        committed: false,
        check,
        answer: "the plan",
        ..gate(false, 2)
    }
}

#[test]
fn a_checkout_is_judged_by_its_uncommitted_change() {
    let gate = in_checkout(true);
    assert_eq!(
        next(Step::Implement, &checkout(0, true), &gate),
        Next::RunCheck
    );
    assert_eq!(
        next(Step::Implement, &checkout(0, false), &gate),
        Next::CarryOn(Missing::Unchanged)
    );
    assert_eq!(
        next(Step::Implement, &checkout(1, true), &gate),
        Next::CarryOn(Missing::Committed)
    );
    // With no check command, and once the check passes, the change is left
    // where it is: there is no forge to take it to.
    assert_eq!(
        next(Step::Verify, &checkout(0, true), &in_checkout(false)),
        Next::Settle
    );
    assert_eq!(after_check(Ok(()), 2, false), Next::Settle);
    // A plan in a checkout that was already dirty is fine; one that changed it
    // is not.
    assert_eq!(
        next(Step::Plan, &checkout(0, false), &gate),
        Next::Advance(Step::Implement)
    );
    assert_eq!(
        next(Step::Plan, &checkout(0, true), &gate),
        Next::CarryOn(Missing::PlanTouchedCode)
    );
}

#[test]
fn a_checkout_is_never_told_to_commit() {
    for missing in [
        Missing::Committed,
        Missing::Unchanged,
        Missing::PlanTouchedCode,
        Missing::CheckFailed("boom".into()),
    ] {
        let said = carry_on(&missing, Step::Implement, None, Place::Checkout);
        assert!(!said.contains("then commit"), "{said}");
        assert!(!said.contains("branch"), "{said}");
    }
}

#[test]
fn a_checkout_never_tells_the_agent_to_undo_what_a_person_did() {
    // The person may be working in the checkout too, so a change or a
    // commit the gate saw may be theirs: only the agent's own is undone.
    for missing in [Missing::Committed, Missing::PlanTouchedCode] {
        let said = carry_on(&missing, Step::Plan, None, Place::Checkout);
        assert!(!said.contains("back as they were"), "{said}");
        assert!(said.contains("leave"), "{said}");
        assert!(said.contains("not yours"), "{said}");
        assert!(said.contains("commit"), "{said}");
    }
}
