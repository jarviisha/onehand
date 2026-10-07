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
            working: Vec::new(),
            starting: 0,
            at_once: 2,
        }),
        queued_behind: None,
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
            working: vec!["#5".into()],
            starting: 0,
            at_once: 1,
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
    assert_eq!(
        found(Kind::Retry { newer: false }, &facts, Check::Workflow),
        None
    );
    assert!(
        found(Kind::Retry { newer: true }, &facts, Check::Workflow)
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
    for kind in [
        Kind::Retry { newer: false },
        Kind::Retry { newer: true },
        Kind::Resume,
    ] {
        let finding = found(kind, &facts, Check::Mode).unwrap();
        assert!(finding.blocks);
        assert!(
            finding.text.contains("keeps its own setup"),
            "{}",
            finding.text
        );
        assert_eq!(finding.change, None);
    }
    let new = found(Kind::NewIssueRun, &facts, Check::Mode).unwrap();
    assert!(!new.text.contains("keeps its own setup"));
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
        found(Kind::Retry { newer: false }, &with_steps, Check::Forge).is_some(),
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
    assert_eq!(
        found(Kind::Retry { newer: false }, &without, Check::Forge),
        None
    );
}

#[test]
fn an_issue_check_applies_to_an_issue_run_only() {
    let mut facts = healthy();
    facts.issue.as_mut().unwrap().tasks = vec![(task_on_issue(None), Some(Working::Running))];
    assert_eq!(found(Kind::Resume, &facts, Check::Issue), None);
    assert_eq!(
        found(Kind::Retry { newer: false }, &facts, Check::EarlierTask),
        None
    );
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
