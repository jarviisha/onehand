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
    }
}

#[test]
fn a_turn_is_judged_by_the_branch_not_by_what_the_agent_said() {
    // Only analysis: nothing committed, so the session carries on.
    assert_eq!(
        after_turn(&facts(0, false, None), true, 2),
        Step::CarryOn(Missing::NoCommits)
    );
    assert_eq!(
        after_turn(&facts(2, true, None), true, 2),
        Step::CarryOn(Missing::Uncommitted)
    );
    // An agent claiming a pull request it never opened is still missing one.
    assert_eq!(
        after_turn(&facts(2, false, None), true, 2),
        Step::CarryOn(Missing::NoPullRequest)
    );
    assert_eq!(
        after_turn(&facts(2, false, Some(pr("h0", &[]))), true, 2),
        Step::CarryOn(Missing::Unpushed)
    );
    assert_eq!(
        after_turn(&facts(2, false, Some(pr("h1", &[]))), true, 2),
        Step::AwaitChecks
    );
}

#[test]
fn a_turn_with_no_turns_left_exhausts_and_a_closed_pull_request_settles() {
    assert_eq!(after_turn(&facts(0, false, None), true, 0), Step::Exhausted);
    let mut closed = pr("h0", &[]);
    closed.state = PrState::Closed;
    assert_eq!(
        after_turn(&facts(0, false, Some(closed)), true, 2),
        Step::Settle
    );
}

#[test]
fn a_project_with_no_forge_settles_on_its_commits() {
    assert_eq!(after_turn(&facts(1, false, None), false, 2), Step::Settle);
    assert_eq!(
        after_turn(&facts(0, false, None), false, 2),
        Step::CarryOn(Missing::NoCommits)
    );
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
        ),
    };
    let file = dir.join("1-4.json");
    save_record_blocking(&file, &record).unwrap();
    std::fs::write(dir.join("2-5.json"), "{").unwrap();
    let found = load_records_blocking(&dir);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0], (file, Ok(record)));
    assert!(
        found[1].1.is_err(),
        "an unreadable file is said, not skipped"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
