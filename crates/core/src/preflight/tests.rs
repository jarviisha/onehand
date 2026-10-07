use super::*;
use crate::task::{Task, Working};
use crate::workflow::{builtin, Brief, Outcome, Place, Setup, Template};

fn brief() -> Brief {
    Brief {
        title: "Fix it".into(),
        body: String::new(),
        instructions: None,
    }
}

fn setup() -> Setup {
    Setup {
        repo: "/repo".into(),
        dir: "/repo".into(),
        branch: None,
        agent: Some("Claude".into()),
        check: Some("make test".into()),
        mode: None,
        forge: None,
    }
}

/// A shipped workflow that works on a worktree.
fn worktree() -> Template {
    builtin::all()
        .into_iter()
        .find(|t| t.place == Place::Worktree)
        .unwrap()
}

/// What a new issue run on a healthy project knows: everything fine, a forge
/// signed in, no other task on the issue, a slot free.
fn healthy() -> Facts {
    Facts {
        workflow: Ok(worktree()),
        agent: Some("Claude".into()),
        agent_configured: true,
        mode: None,
        offered: None,
        has_check: true,
        in_git: true,
        checked_out: Some("main".into()),
        forge: Some(Forge {
            name: "GitHub".into(),
            account: Some(Ok("me".into())),
        }),
        issue: Some(IssueFacts {
            named: "#3".into(),
            tasks: Vec::new(),
        }),
        slots: Some(Slots {
            holders: Vec::new(),
            starting: 0,
            at_once: 2,
            waiting: 0,
            waiting_cap: None,
        }),
        queued_behind: None,
        review: None,
        shared_checkout: None,
    }
}

fn found(kind: Kind, facts: &Facts, check: Check) -> Option<Finding> {
    preflight(kind, facts)
        .into_iter()
        .find(|f| f.check == check)
}

fn blocks(kind: Kind, facts: &Facts) -> Vec<Check> {
    preflight(kind, facts)
        .into_iter()
        .filter(|f| f.blocks)
        .map(|f| f.check)
        .collect()
}

#[test]
fn a_healthy_issue_run_is_not_blocked_and_says_its_base() {
    let facts = healthy();
    assert_eq!(blocks(Kind::NewIssueRun, &facts), Vec::<Check>::new());
    let base = found(Kind::NewIssueRun, &facts, Check::Base).unwrap();
    assert!(base.text.contains("origin"), "{}", base.text);
    assert!(!base.blocks);
}

#[test]
fn an_issue_run_without_a_forge_is_cut_off_the_branch_checked_out() {
    let facts = Facts {
        forge: None,
        ..healthy()
    };
    let base = found(Kind::NewIssueRun, &facts, Check::Base).unwrap();
    assert!(base.text.contains("`main`"), "{}", base.text);
}

#[test]
fn an_unreadable_or_checkout_workflow_blocks_an_issue_run() {
    let missing = Facts {
        workflow: Err("there is no workflow `gone`".into()),
        ..healthy()
    };
    let finding = found(Kind::NewIssueRun, &missing, Check::Workflow).unwrap();
    assert!(finding.blocks && finding.text.contains("gone"));
    let checkout = Facts {
        workflow: Ok(builtin::all().remove(0)),
        ..healthy()
    };
    assert_eq!(checkout.workflow.as_ref().unwrap().place, Place::Checkout);
    let finding = found(Kind::NewIssueRun, &checkout, Check::Workflow).unwrap();
    assert!(
        finding.blocks && finding.text.contains("checkout"),
        "{}",
        finding.text
    );
}

#[test]
fn validation_problems_are_listed_first_and_each_once() {
    let mut broken = worktree();
    broken.steps.clear();
    let facts = Facts {
        workflow: Ok(broken.clone()),
        agent: None,
        agent_configured: false,
        ..healthy()
    };
    let all = preflight(Kind::NewIssueRun, &facts);
    let problems = crate::workflow::validate(&broken);
    assert!(!problems.is_empty());
    let workflow: Vec<_> = all.iter().filter(|f| f.check == Check::Workflow).collect();
    assert_eq!(workflow.len(), problems.len());
    assert_eq!(all[0].check, Check::Workflow);
    // Blocks before anything that only informs.
    let first_inform = all.iter().position(|f| !f.blocks).unwrap_or(all.len());
    assert!(all[first_inform..].iter().all(|f| !f.blocks));
}

#[test]
fn an_agent_no_longer_configured_blocks() {
    let gone = Facts {
        agent_configured: false,
        ..healthy()
    };
    let finding = found(Kind::NewIssueRun, &gone, Check::Agent).unwrap();
    assert!(finding.blocks && finding.text.contains("Claude"));
    let none = Facts {
        agent: None,
        agent_configured: false,
        ..healthy()
    };
    assert!(
        found(Kind::NewIssueRun, &none, Check::Agent)
            .unwrap()
            .blocks
    );
}

#[test]
fn a_mode_known_and_not_offered_blocks_and_one_not_known_informs() {
    let known = Facts {
        mode: Some("plan".into()),
        offered: Some(vec!["default".into(), "acceptEdits".into()]),
        ..healthy()
    };
    let finding = found(Kind::NewIssueRun, &known, Check::Mode).unwrap();
    assert!(finding.blocks);
    assert!(finding.text.contains("`plan`") && finding.text.contains("acceptEdits"));
    let unknown = Facts {
        offered: None,
        ..known.clone()
    };
    let finding = found(Kind::NewIssueRun, &unknown, Check::Mode).unwrap();
    assert!(!finding.blocks && finding.text.contains("not known yet"));
    let offered = Facts {
        offered: Some(vec!["plan".into()]),
        ..known
    };
    assert_eq!(found(Kind::NewIssueRun, &offered, Check::Mode), None);
}

#[test]
fn a_check_command_missing_blocks_and_no_command_step_informs() {
    let mut needs = worktree();
    assert!(
        needs.needs_check(),
        "the shipped worktree workflow runs the check"
    );
    let missing = Facts {
        workflow: Ok(needs.clone()),
        has_check: false,
        ..healthy()
    };
    assert!(
        found(Kind::NewIssueRun, &missing, Check::CheckCommand)
            .unwrap()
            .blocks
    );
    needs
        .steps
        .retain(|s| !matches!(s.kind, crate::workflow::StepKind::Command { .. }));
    let none = Facts {
        workflow: Ok(needs),
        has_check: false,
        ..healthy()
    };
    let finding = found(Kind::NewIssueRun, &none, Check::CheckCommand).unwrap();
    assert!(!finding.blocks && finding.text.contains("nothing verifies"));
}

#[test]
fn a_detached_head_with_no_forge_blocks_an_issue_run() {
    let facts = Facts {
        forge: None,
        checked_out: Some(DETACHED.into()),
        ..healthy()
    };
    assert!(
        found(Kind::NewIssueRun, &facts, Check::Place)
            .unwrap()
            .blocks
    );
    let with_forge = Facts {
        checked_out: Some(DETACHED.into()),
        ..healthy()
    };
    assert_eq!(found(Kind::NewIssueRun, &with_forge, Check::Place), None);
}

#[test]
fn a_forge_signed_out_as_last_seen_blocks_and_one_not_seen_yet_does_not() {
    let out = Facts {
        forge: Some(Forge {
            name: "GitHub".into(),
            account: Some(Err("gh is not signed in".into())),
        }),
        ..healthy()
    };
    let finding = found(Kind::NewIssueRun, &out, Check::Forge).unwrap();
    assert!(finding.blocks && finding.text.contains("not signed in"));
    let unseen = Facts {
        forge: Some(Forge {
            name: "GitHub".into(),
            account: None,
        }),
        ..healthy()
    };
    assert_eq!(found(Kind::NewIssueRun, &unseen, Check::Forge), None);
}

fn task_on_issue(outcome: Option<Outcome>) -> Task {
    let mut task = Task::new("1".into(), worktree(), brief(), setup());
    task.runs[0].outcome = outcome;
    task
}

#[test]
fn another_run_on_the_issue_blocks() {
    let mut facts = healthy();
    facts.issue.as_mut().unwrap().tasks = vec![(task_on_issue(None), Some(Working::Running))];
    let finding = found(Kind::NewIssueRun, &facts, Check::Issue).unwrap();
    assert!(finding.blocks && finding.text.contains("#3"));
}

#[test]
fn an_earlier_task_needing_attention_informs_and_offers_it() {
    let mut facts = healthy();
    let ended = task_on_issue(Some(Outcome::Exhausted {
        step: "implement".into(),
    }));
    facts.issue.as_mut().unwrap().tasks = vec![(ended, None)];
    let finding = found(Kind::NewIssueRun, &facts, Check::EarlierTask).unwrap();
    assert!(!finding.blocks);
    assert_eq!(finding.task.as_deref(), Some("1"));
    assert!(finding.text.contains("second task"), "{}", finding.text);
    let note = earlier_note(&facts.issue.as_ref().unwrap().tasks).unwrap();
    assert!(note.contains("worktree is kept"), "{note}");
    // One done is history, not attention.
    facts.issue.as_mut().unwrap().tasks = vec![(task_on_issue(Some(Outcome::Done)), None)];
    assert_eq!(found(Kind::NewIssueRun, &facts, Check::EarlierTask), None);
}

#[test]
fn a_full_slot_blocks_naming_who_holds_it() {
    let facts = Facts {
        slots: Some(Slots {
            holders: vec![crate::unattended::Holder {
                task: "t5".into(),
                shown: "#5".into(),
            }],
            starting: 0,
            at_once: 1,
            waiting: 0,
            waiting_cap: None,
        }),
        ..healthy()
    };
    let finding = found(Kind::NewIssueRun, &facts, Check::Slot).unwrap();
    assert!(
        finding.blocks && finding.text.contains("#5"),
        "{}",
        finding.text
    );
}

#[test]
fn a_taken_place_informs_naming_the_task_ahead() {
    let facts = Facts {
        queued_behind: Some("Fix the parser".into()),
        ..healthy()
    };
    let finding = found(Kind::Resume, &facts, Check::PlaceTaken).unwrap();
    assert!(!finding.blocks && finding.text.contains("Fix the parser"));
}

/// What a start that is not an issue's knows: no issue, no slots.
fn plain() -> Facts {
    Facts {
        issue: None,
        slots: None,
        forge: None,
        ..healthy()
    }
}

#[test]
fn a_new_run_on_a_worktree_says_head_and_one_outside_git_blocks() {
    let facts = plain();
    let base = found(Kind::NewRun, &facts, Check::Base).unwrap();
    assert!(base.text.contains("`HEAD`"));
    let outside = Facts {
        in_git: false,
        checked_out: None,
        ..plain()
    };
    assert!(found(Kind::NewRun, &outside, Check::Place).unwrap().blocks);
    assert_eq!(found(Kind::NewRun, &outside, Check::Base), None);
    let checkout = Facts {
        workflow: Ok(builtin::all().remove(0)),
        in_git: false,
        ..plain()
    };
    assert_eq!(blocks(Kind::NewRun, &checkout), Vec::<Check>::new());
    assert_eq!(found(Kind::NewRun, &checkout, Check::Base), None);
}

#[test]
fn a_resume_says_nothing_of_a_base_and_judges_no_workflow_anew() {
    let mut broken = worktree();
    broken.steps.clear();
    let facts = Facts {
        workflow: Ok(broken),
        ..plain()
    };
    assert_eq!(found(Kind::Resume, &facts, Check::Base), None);
    assert_eq!(found(Kind::Resume, &facts, Check::Workflow), None);
    assert_eq!(found(Kind::Retry, &facts, Check::Workflow), None);
    assert!(
        found(Kind::RetryCurrent, &facts, Check::Workflow)
            .unwrap()
            .blocks
    );
}

#[test]
fn a_retry_whose_own_mode_is_not_offered_blocks_and_says_it_keeps_its_setup() {
    let facts = Facts {
        mode: Some("plan".into()),
        offered: Some(vec!["default".into()]),
        ..plain()
    };
    for kind in [Kind::Retry, Kind::Resume] {
        let finding = found(kind, &facts, Check::Mode).unwrap();
        assert!(finding.blocks);
        assert!(
            finding.text.contains("keeps its own setup"),
            "{}",
            finding.text
        );
        // A Retry blocked by its own setup is changed by retrying with what
        // Settings say now; a Resume carries the same run, and cannot be.
        let change = match kind {
            Kind::Resume => None,
            Kind::Retry => Some(Change::RetryCurrent),
            Kind::NewRun | Kind::NewIssueRun | Kind::RetryCurrent | Kind::AnswerReview => {
                unreachable!()
            }
        };
        assert_eq!(finding.change, change, "{kind:?}");
    }
    let new = found(Kind::NewIssueRun, &facts, Check::Mode).unwrap();
    assert!(!new.text.contains("keeps its own setup"));
}

/// A retry with current settings is judged as a new start is: on the
/// configuration Settings give now, with the workflow judged anew.
#[test]
fn a_retry_with_current_settings_is_judged_on_what_settings_say() {
    let refused = Facts {
        mode: Some("plan".into()),
        offered: Some(vec!["default".into()]),
        ..plain()
    };
    let mode = found(Kind::RetryCurrent, &refused, Check::Mode).unwrap();
    assert!(mode.blocks);
    assert!(!mode.text.contains("keeps its own setup"));
    let gone = Facts {
        agent_configured: false,
        ..plain()
    };
    assert!(
        found(Kind::RetryCurrent, &gone, Check::Agent)
            .unwrap()
            .blocks
    );
    let mut broken = worktree();
    broken.steps.clear();
    let broken = Facts {
        workflow: Ok(broken),
        ..plain()
    };
    assert!(
        found(Kind::RetryCurrent, &broken, Check::Workflow)
            .unwrap()
            .blocks
    );
    let missing = Facts {
        workflow: Err("there is no workflow `fix`".into()),
        ..plain()
    };
    assert!(
        found(Kind::RetryCurrent, &missing, Check::Workflow)
            .unwrap()
            .blocks
    );
    assert_eq!(found(Kind::RetryCurrent, &plain(), Check::Base), None);
}

#[test]
fn a_run_with_forge_steps_needs_its_forge_and_one_without_does_not() {
    let out = Some(Forge {
        name: "GitHub".into(),
        account: Some(Err("signed out".into())),
    });
    let with_steps = Facts {
        forge: out.clone(),
        ..plain()
    };
    let has_steps = worktree()
        .steps
        .iter()
        .any(|s| matches!(s.kind, crate::workflow::StepKind::PullRequest));
    assert_eq!(
        found(Kind::Retry, &with_steps, Check::Forge).is_some(),
        has_steps
    );
    let mut bare = worktree();
    bare.steps.retain(|s| {
        !matches!(
            s.kind,
            crate::workflow::StepKind::Push
                | crate::workflow::StepKind::PullRequest
                | crate::workflow::StepKind::StatusChecks { .. }
        )
    });
    let without = Facts {
        workflow: Ok(bare),
        forge: out,
        ..plain()
    };
    assert_eq!(found(Kind::Retry, &without, Check::Forge), None);
}

#[test]
fn an_issue_check_applies_to_an_issue_run_only() {
    let mut facts = healthy();
    facts.issue.as_mut().unwrap().tasks = vec![(task_on_issue(None), Some(Working::Running))];
    assert_eq!(found(Kind::Resume, &facts, Check::Issue), None);
    assert_eq!(found(Kind::Retry, &facts, Check::EarlierTask), None);
}

/// Every block of the configuration that will run, found once the run ran,
/// is a configuration failure: the agent, its mode, the check command and
/// the workflow. Where the work goes and who holds the slot are not.
#[test]
fn a_configuration_block_found_late_is_a_configuration_failure() {
    use crate::workflow::Failure;
    for check in [
        Check::Workflow,
        Check::Agent,
        Check::Mode,
        Check::CheckCommand,
    ] {
        assert_eq!(found_late(check), Failure::Configuration, "{check:?}");
    }
    assert_eq!(found_late(Check::Forge), Failure::Forge);
    for check in [
        Check::Place,
        Check::Base,
        Check::Issue,
        Check::EarlierTask,
        Check::Slot,
        Check::PlaceTaken,
    ] {
        assert_eq!(found_late(check), Failure::Other, "{check:?}");
    }
}

/// What answering a pull request review is refused for, said before anything
/// is claimed, in the words the label path uses.
#[test]
fn answering_a_review_is_refused_before_the_claim() {
    use crate::connector::PrState;
    let url = "https://forge/pr/7".to_string();
    let review = |pr: Result<Option<(PrState, String)>, String>, answers, diverged| Facts {
        review: Some(ReviewFacts {
            pr,
            answers,
            diverged,
            issue_open: Ok(true),
        }),
        ..healthy()
    };
    let refused = |facts: &Facts| {
        found(Kind::AnswerReview, facts, Check::Issue)
            .filter(|f| f.blocks)
            .map(|f| f.text)
    };
    assert_eq!(
        refused(&review(Ok(Some((PrState::Open, url.clone()))), true, false)),
        None,
        "an open pull request on a workflow that repairs is answered"
    );
    // The label path's own words, the first letter raised.
    let raised = |said: String| said[..1].to_uppercase() + &said[1..];
    assert_eq!(
        refused(&review(
            Ok(Some((PrState::Open, url.clone()))),
            false,
            false
        )),
        Some(raised(crate::unattended::review_unanswerable(&url)))
    );
    assert_eq!(
        refused(&review(
            Ok(Some((PrState::Closed, url.clone()))),
            true,
            false
        )),
        Some(raised(crate::unattended::review_closed(&url)))
    );
    assert!(
        refused(&review(Ok(Some((PrState::Open, url.clone()))), true, true))
            .unwrap()
            .contains("went its own way")
    );
    assert!(refused(&review(
        Ok(Some((PrState::Merged, url.clone()))),
        true,
        false
    ))
    .unwrap()
    .contains("merged"));
    assert!(refused(&review(Ok(None), true, false)).is_some());
    assert!(refused(&review(Err("offline".into()), true, false))
        .unwrap()
        .contains("offline"));
    let closed_issue = Facts {
        review: Some(ReviewFacts {
            pr: Ok(Some((PrState::Open, url.clone()))),
            answers: true,
            diverged: false,
            issue_open: Ok(false),
        }),
        ..healthy()
    };
    assert!(refused(&closed_issue).unwrap().contains("issue is closed"));
    let no_forge = Facts {
        forge: None,
        ..review(Ok(None), true, false)
    };
    assert!(refused(&no_forge).unwrap().contains("No forge"));
    // Its own snapshot and setup, as a Retry: Settings changed do not judge it.
    assert_eq!(
        found(
            Kind::AnswerReview,
            &review(Ok(Some((PrState::Open, url))), true, false),
            Check::Base
        ),
        None
    );
}

const ALL: [Kind; 6] = [
    Kind::NewRun,
    Kind::NewIssueRun,
    Kind::Resume,
    Kind::Retry,
    Kind::RetryCurrent,
    Kind::AnswerReview,
];

/// A shipped workflow that works in the checkout.
fn checkout() -> Template {
    builtin::all()
        .into_iter()
        .find(|t| t.place == Place::Checkout)
        .unwrap()
}

/// A shipped workflow with forge steps.
fn with_forge_steps() -> Template {
    builtin::all()
        .into_iter()
        .find(|t| t.steps.iter().any(|s| matches!(s.kind, StepKind::Push)))
        .unwrap()
}

fn informs(kind: Kind, facts: &Facts, check: Check) -> Option<Finding> {
    preflight(kind, facts)
        .into_iter()
        .find(|f| f.check == check && !f.blocks)
}

#[test]
fn a_checkout_workflow_beside_a_persons_session_is_told_on_a_new_run() {
    let shared = Facts {
        workflow: Ok(checkout()),
        shared_checkout: Some("Fix the parser".into()),
        ..healthy()
    };
    let finding = informs(Kind::NewRun, &shared, Check::Place).unwrap();
    assert!(finding.text.contains("Fix the parser"), "{}", finding.text);
    assert!(blocks(Kind::NewRun, &shared).is_empty());
    // Nobody else in the checkout: nothing said.
    let alone = Facts {
        shared_checkout: None,
        ..shared.clone()
    };
    assert!(informs(Kind::NewRun, &alone, Check::Place).is_none());
    // A worktree workflow works apart from the session, whatever it does.
    let apart = Facts {
        workflow: Ok(worktree()),
        ..shared.clone()
    };
    assert!(informs(Kind::NewRun, &apart, Check::Place).is_none());
    // Every other kind runs where it ran, or on a worktree of its own.
    for kind in ALL.into_iter().filter(|k| *k != Kind::NewRun) {
        assert!(informs(kind, &shared, Check::Place).is_none(), "{kind:?}");
    }
}

#[test]
fn forge_steps_on_a_project_no_forge_serves_are_told() {
    let none = Facts {
        workflow: Ok(with_forge_steps()),
        forge: None,
        ..healthy()
    };
    for kind in ALL.into_iter().filter(|k| *k != Kind::AnswerReview) {
        let finding = informs(kind, &none, Check::Forge)
            .unwrap_or_else(|| panic!("{kind:?} says nothing of the forge"));
        assert!(
            finding.text.contains("pass at once") && finding.text.contains("branch"),
            "{}",
            finding.text
        );
    }
    // Answering a review on no forge is refused already, never told twice.
    assert!(informs(Kind::AnswerReview, &none, Check::Forge).is_none());
    // A forge serving, or no forge steps: nothing said.
    let served = Facts {
        workflow: Ok(with_forge_steps()),
        ..healthy()
    };
    assert!(informs(Kind::NewRun, &served, Check::Forge).is_none());
    let no_steps = Facts {
        workflow: Ok(checkout()),
        forge: None,
        ..healthy()
    };
    assert!(informs(Kind::NewRun, &no_steps, Check::Forge).is_none());
}

#[test]
fn every_start_says_the_limits_of_the_workflow_that_will_run() {
    let mut template = worktree();
    template.timeout = "20m".into();
    template.misses = 4;
    let facts = Facts {
        workflow: Ok(template),
        ..healthy()
    };
    for kind in ALL {
        let finding = informs(kind, &facts, Check::Limits)
            .unwrap_or_else(|| panic!("{kind:?} says no limits"));
        assert!(
            finding.text.contains("20m") && finding.text.contains('4'),
            "{kind:?}: {}",
            finding.text
        );
    }
    let unread = Facts {
        workflow: Err("gone".into()),
        ..healthy()
    };
    assert!(informs(Kind::Retry, &unread, Check::Limits).is_none());
}

#[test]
fn a_mode_learned_by_checking_the_agent_turns_not_known_into_a_block_or_nothing() {
    let unknown = Facts {
        mode: Some("plan".into()),
        offered: None,
        ..healthy()
    };
    let finding = found(Kind::NewRun, &unknown, Check::Mode).unwrap();
    assert!(!finding.blocks && finding.text.contains("not known yet"));
    let refused = Facts {
        offered: Some(vec!["default".into()]),
        ..unknown.clone()
    };
    assert!(found(Kind::NewRun, &refused, Check::Mode).unwrap().blocks);
    let offered = Facts {
        offered: Some(vec!["plan".into()]),
        ..unknown
    };
    assert!(found(Kind::NewRun, &offered, Check::Mode).is_none());
}
