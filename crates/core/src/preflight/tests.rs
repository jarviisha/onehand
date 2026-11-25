use super::*;
use crate::workflow::{builtin, Offer, Place, StepSpec, Template};
use std::collections::BTreeMap;

/// A shipped workflow that works on a worktree.
fn worktree() -> Template {
    builtin::all()
        .into_iter()
        .find(|t| t.place == Place::Worktree)
        .unwrap()
}

/// What a new run on a healthy project knows: everything fine, a forge
/// serving it.
fn healthy() -> Facts {
    Facts {
        workflow: Ok(worktree()),
        agent: Some("Claude".into()),
        agent_configured: true,
        mode: None,
        offered: None,
        step_agents: Vec::new(),
        has_check: true,
        in_git: true,
        forge: Some(Forge {
            name: "GitHub".into(),
            account: Some(Ok("me".into())),
        }),
        queued_behind: None,
        shared_checkout: None,
        forge_read: true,
    }
}

/// What an agent offers when only its modes are known, no option.
fn modes(values: &[&str]) -> Offer {
    Offer {
        modes: values.iter().map(|v| v.to_string()).collect(),
        options: Vec::new(),
    }
}

fn found(kind: Kind, facts: &Facts, about: About) -> Option<Finding> {
    preflight(kind, facts)
        .into_iter()
        .find(|f| f.about == about)
}

fn blocks(kind: Kind, facts: &Facts) -> Vec<About> {
    preflight(kind, facts)
        .into_iter()
        .filter(|f| f.blocks)
        .map(|f| f.about)
        .collect()
}

#[test]
fn an_unreadable_workflow_blocks() {
    let missing = Facts {
        workflow: Err("there is no workflow `gone`".into()),
        ..healthy()
    };
    let finding = found(Kind::NewRun, &missing, About::Workflow).unwrap();
    assert!(finding.blocks && finding.text.contains("gone"));
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
    let all = preflight(Kind::NewRun, &facts);
    let problems = crate::workflow::validate(&broken);
    assert!(!problems.is_empty());
    let workflow: Vec<_> = all.iter().filter(|f| f.about == About::Workflow).collect();
    assert_eq!(workflow.len(), problems.len());
    assert_eq!(all[0].about, About::Workflow);
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
    let finding = found(Kind::NewRun, &gone, About::Agent).unwrap();
    assert!(finding.blocks && finding.text.contains("Claude"));
    let none = Facts {
        agent: None,
        agent_configured: false,
        ..healthy()
    };
    assert!(found(Kind::NewRun, &none, About::Agent).unwrap().blocks);
}

#[test]
fn a_mode_known_and_not_offered_blocks_and_one_not_known_informs() {
    let known = Facts {
        mode: Some("plan".into()),
        offered: Some(modes(&["default", "acceptEdits"])),
        ..healthy()
    };
    let finding = found(Kind::NewRun, &known, About::Mode).unwrap();
    assert!(finding.blocks);
    assert!(finding.text.contains("`plan`") && finding.text.contains("acceptEdits"));
    let unknown = Facts {
        offered: None,
        ..known.clone()
    };
    let finding = found(Kind::NewRun, &unknown, About::Mode).unwrap();
    assert!(!finding.blocks && finding.text.contains("not known yet"));
    // Checking the agent now might resolve it: pressing it can only ever
    // learn whether this mode is one it offers.
    assert!(finding.checkable);
    let offered = Facts {
        offered: Some(modes(&["plan"])),
        ..known
    };
    assert_eq!(found(Kind::NewRun, &offered, About::Mode), None);
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
    let finding = found(Kind::NewRun, &missing, About::CheckCommand).unwrap();
    assert!(finding.blocks);
    let why = needs.lacks_check(false).unwrap();
    assert!(
        finding
            .text
            .starts_with(&format!("{}.", super::capital(&why))),
        "{}",
        finding.text
    );
    needs
        .steps
        .retain(|s| !matches!(s.kind, crate::workflow::StepKind::Command { .. }));
    let none = Facts {
        workflow: Ok(needs),
        has_check: false,
        ..healthy()
    };
    let finding = found(Kind::NewRun, &none, About::CheckCommand).unwrap();
    assert!(!finding.blocks && finding.text.contains("nothing verifies"));
}

#[test]
fn a_taken_place_informs_naming_the_task_ahead() {
    let facts = Facts {
        queued_behind: Some("Fix the parser".into()),
        ..healthy()
    };
    let finding = found(Kind::Resume, &facts, About::PlaceTaken).unwrap();
    assert!(!finding.blocks && finding.text.contains("Fix the parser"));
}

/// What a start on a project no forge serves knows.
fn plain() -> Facts {
    Facts {
        forge: None,
        ..healthy()
    }
}

#[test]
fn a_new_run_on_a_worktree_says_head_and_one_outside_git_blocks() {
    let facts = plain();
    let base = found(Kind::NewRun, &facts, About::Base).unwrap();
    assert!(base.text.contains("`HEAD`"));
    let outside = Facts {
        in_git: false,
        ..plain()
    };
    assert!(found(Kind::NewRun, &outside, About::Place).unwrap().blocks);
    assert_eq!(found(Kind::NewRun, &outside, About::Base), None);
    let checkout = Facts {
        workflow: Ok(builtin::all().remove(0)),
        in_git: false,
        ..plain()
    };
    assert_eq!(blocks(Kind::NewRun, &checkout), Vec::<About>::new());
    assert_eq!(found(Kind::NewRun, &checkout, About::Base), None);
}

#[test]
fn a_resume_says_nothing_of_a_base_and_judges_no_workflow_anew() {
    let mut broken = worktree();
    broken.steps.clear();
    let facts = Facts {
        workflow: Ok(broken),
        ..plain()
    };
    assert_eq!(found(Kind::Resume, &facts, About::Base), None);
    assert_eq!(found(Kind::Resume, &facts, About::Workflow), None);
    assert_eq!(found(Kind::Retry, &facts, About::Workflow), None);
    assert!(
        found(Kind::RetryCurrent, &facts, About::Workflow)
            .unwrap()
            .blocks
    );
}

#[test]
fn a_retry_whose_own_mode_is_not_offered_blocks_and_says_it_keeps_its_setup() {
    let facts = Facts {
        mode: Some("plan".into()),
        offered: Some(modes(&["default"])),
        ..plain()
    };
    for kind in [Kind::Retry, Kind::Resume] {
        let finding = found(kind, &facts, About::Mode).unwrap();
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
            Kind::NewRun | Kind::RetryCurrent => unreachable!(),
        };
        assert_eq!(finding.change, change, "{kind:?}");
    }
    let new = found(Kind::NewRun, &facts, About::Mode).unwrap();
    assert!(!new.text.contains("keeps its own setup"));
}

/// A retry with current settings is judged as a new start is: on the
/// configuration Settings give now, with the workflow judged anew.
#[test]
fn a_retry_with_current_settings_is_judged_on_what_settings_say() {
    let refused = Facts {
        mode: Some("plan".into()),
        offered: Some(modes(&["default"])),
        ..plain()
    };
    let mode = found(Kind::RetryCurrent, &refused, About::Mode).unwrap();
    assert!(mode.blocks);
    assert!(!mode.text.contains("keeps its own setup"));
    let gone = Facts {
        agent_configured: false,
        ..plain()
    };
    assert!(
        found(Kind::RetryCurrent, &gone, About::Agent)
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
        found(Kind::RetryCurrent, &broken, About::Workflow)
            .unwrap()
            .blocks
    );
    let missing = Facts {
        workflow: Err("there is no workflow `fix`".into()),
        ..plain()
    };
    assert!(
        found(Kind::RetryCurrent, &missing, About::Workflow)
            .unwrap()
            .blocks
    );
    assert_eq!(found(Kind::RetryCurrent, &plain(), About::Base), None);
}

/// Every block of the configuration that will run, found once the run ran,
/// is a configuration failure: the agent, its mode, the check command and
/// the workflow. Where the work goes is not.
#[test]
fn a_configuration_block_found_late_is_a_configuration_failure() {
    use crate::workflow::Failure;
    for about in [
        About::Workflow,
        About::Agent,
        About::Mode,
        About::Settings,
        About::CheckCommand,
    ] {
        assert_eq!(found_late(about), Failure::Configuration, "{about:?}");
    }
    assert_eq!(found_late(About::Forge), Failure::Forge);
    for check in [About::Place, About::Base, About::PlaceTaken] {
        assert_eq!(found_late(check), Failure::Other, "{check:?}");
    }
}

const ALL: [Kind; 4] = [Kind::NewRun, Kind::Resume, Kind::Retry, Kind::RetryCurrent];

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

fn informs(kind: Kind, facts: &Facts, about: About) -> Option<Finding> {
    preflight(kind, facts)
        .into_iter()
        .find(|f| f.about == about && !f.blocks)
}

#[test]
fn a_checkout_workflow_beside_a_persons_session_is_told_on_a_new_run() {
    let shared = Facts {
        workflow: Ok(checkout()),
        shared_checkout: Some("Fix the parser".into()),
        ..healthy()
    };
    let finding = informs(Kind::NewRun, &shared, About::Place).unwrap();
    assert!(finding.text.contains("Fix the parser"), "{}", finding.text);
    assert!(blocks(Kind::NewRun, &shared).is_empty());
    // Nobody else in the checkout: nothing said.
    let alone = Facts {
        shared_checkout: None,
        ..shared.clone()
    };
    assert!(informs(Kind::NewRun, &alone, About::Place).is_none());
    // A worktree workflow works apart from the session, whatever it does.
    let apart = Facts {
        workflow: Ok(worktree()),
        ..shared.clone()
    };
    assert!(informs(Kind::NewRun, &apart, About::Place).is_none());
    // A workflow that does not read is not known to work in the checkout.
    let unread = Facts {
        workflow: Err("gone".into()),
        ..shared.clone()
    };
    assert!(informs(Kind::NewRun, &unread, About::Place).is_none());
    // Every other kind runs where it ran, or on a worktree of its own.
    for kind in ALL.into_iter().filter(|k| *k != Kind::NewRun) {
        assert!(informs(kind, &shared, About::Place).is_none(), "{kind:?}");
    }
}

#[test]
fn forge_steps_on_a_project_no_forge_serves_are_told() {
    let none = Facts {
        workflow: Ok(with_forge_steps()),
        forge: None,
        ..healthy()
    };
    for kind in ALL {
        let finding = informs(kind, &none, About::Forge)
            .unwrap_or_else(|| panic!("{kind:?} says nothing of the forge"));
        assert!(
            finding.text.contains("pass at once") && finding.text.contains("branch"),
            "{}",
            finding.text
        );
    }
    // Not read yet, no forge is not said.
    let unread = Facts {
        forge_read: false,
        ..none.clone()
    };
    assert!(informs(Kind::NewRun, &unread, About::Forge).is_none());
    // A forge serving, or no forge steps: nothing said.
    let served = Facts {
        workflow: Ok(with_forge_steps()),
        ..healthy()
    };
    assert!(informs(Kind::NewRun, &served, About::Forge).is_none());
    let no_steps = Facts {
        workflow: Ok(checkout()),
        forge: None,
        ..healthy()
    };
    assert!(informs(Kind::NewRun, &no_steps, About::Forge).is_none());
}

/// A forge whose account cannot be used blocks a run with forge steps, and
/// says where it is seen to; one not asked yet, or a run with no forge
/// step, is not held up by it.
#[test]
fn a_forge_that_cannot_be_used_blocks_only_its_forge_steps() {
    let signed_out = |workflow, account| Facts {
        workflow: Ok(workflow),
        forge: Some(Forge {
            name: "GitHub".into(),
            account,
        }),
        ..healthy()
    };
    let out = Some(Err("`gh` is not signed in".to_string()));
    for kind in ALL {
        let found = preflight(kind, &signed_out(with_forge_steps(), out.clone()));
        let block = found
            .iter()
            .find(|f| f.about == About::Forge && f.blocks)
            .unwrap_or_else(|| panic!("{kind:?} is not blocked"));
        assert!(block.text.contains("not signed in"), "{}", block.text);
        assert_eq!(block.change, Some(Change::At("Settings ▸ Connections")));
    }
    let unasked = preflight(Kind::NewRun, &signed_out(with_forge_steps(), None));
    assert!(!unasked.iter().any(|f| f.about == About::Forge && f.blocks));
    let no_steps = preflight(Kind::NewRun, &signed_out(worktree(), out));
    assert!(!no_steps.iter().any(|f| f.about == About::Forge && f.blocks));
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
        let finding = informs(kind, &facts, About::Limits)
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
    assert!(informs(Kind::Retry, &unread, About::Limits).is_none());
}

#[test]
fn a_mode_learned_by_checking_the_agent_turns_not_known_into_a_block_or_nothing() {
    let unknown = Facts {
        mode: Some("plan".into()),
        offered: None,
        ..healthy()
    };
    let finding = found(Kind::NewRun, &unknown, About::Mode).unwrap();
    assert!(!finding.blocks && finding.text.contains("not known yet"));
    let refused = Facts {
        offered: Some(modes(&["default"])),
        ..unknown.clone()
    };
    assert!(found(Kind::NewRun, &refused, About::Mode).unwrap().blocks);
    let offered = Facts {
        offered: Some(modes(&["plan"])),
        ..unknown
    };
    assert!(found(Kind::NewRun, &offered, About::Mode).is_none());
}

#[test]
fn a_finding_names_where_things_are_changed_now() {
    let mut broken = worktree();
    broken.steps.clear();
    let invalid = Facts {
        workflow: Ok(broken),
        ..healthy()
    };
    let finding = found(Kind::NewRun, &invalid, About::Workflow).unwrap();
    assert_eq!(finding.change, Some(Change::At("the Workflows page")));
    let no_check = Facts {
        has_check: false,
        ..healthy()
    };
    if let Some(finding) = found(Kind::NewRun, &no_check, About::CheckCommand).filter(|f| f.blocks)
    {
        assert_eq!(finding.change, Some(Change::At("the project's page")));
    }
    for kind in ALL {
        for facts in [&invalid, &no_check] {
            for finding in preflight(kind, facts) {
                let said = finding.change.and_then(Change::said).unwrap_or_default();
                assert!(!said.contains("Settings ▸ Workflows"), "{said}");
            }
        }
    }
}

#[test]
fn only_a_checkout_start_waits_behind_a_holder() {
    let holder = Some("Fix the build".to_string());
    assert_eq!(queued_behind_for(Place::Checkout, holder.clone()), holder);
    assert_eq!(queued_behind_for(Place::Worktree, holder), None);
}

#[test]
fn a_step_mode_not_offered_blocks() {
    let mut t = worktree();
    let label = t.steps[0].label.clone();
    if let StepKind::Agent { mode, .. } = &mut t.steps[0].kind {
        *mode = Some("plan".into());
    } else {
        panic!("the worktree workflow's first step is not an agent step");
    }
    let facts = Facts {
        workflow: Ok(t),
        offered: Some(modes(&["default"])),
        ..healthy()
    };
    let finding = found(Kind::NewRun, &facts, About::Settings).unwrap();
    assert!(finding.blocks);
    assert!(
        finding.text.starts_with(&format!(
            "Step 1 ({label}): the agent offers no mode `plan`"
        )),
        "{}",
        finding.text
    );
}

#[test]
fn a_step_model_not_offered_blocks() {
    let mut t = worktree();
    let label = t.steps[0].label.clone();
    if let StepKind::Agent { options, .. } = &mut t.steps[0].kind {
        options.insert("model".into(), "opus".into());
    } else {
        panic!("the worktree workflow's first step is not an agent step");
    }
    let facts = Facts {
        workflow: Ok(t),
        offered: Some(modes(&["default"])),
        ..healthy()
    };
    let finding = found(Kind::NewRun, &facts, About::Settings).unwrap();
    assert!(finding.blocks);
    assert!(
        finding.text.starts_with(&format!(
            "Step 1 ({label}): the agent offers no option `model`"
        )),
        "{}",
        finding.text
    );
}

#[test]
fn an_effort_missing_from_what_was_last_seen_does_not_block() {
    // What was learned may have been offered under a model a person picked
    // in the composer, not the one the run starts on.
    let mut t = worktree();
    if let StepKind::Agent { options, .. } = &mut t.steps[0].kind {
        options.insert("effort".into(), "high".into());
    } else {
        panic!("the worktree workflow's first step is not an agent step");
    }
    let offer = Offer {
        modes: vec!["default".into()],
        options: vec![
            ("model".into(), vec!["strong".into(), "cheap".into()]),
            ("effort".into(), vec!["low".into()]),
        ],
    };
    let facts = Facts {
        workflow: Ok(t),
        offered: Some(offer),
        ..healthy()
    };
    let findings: Vec<_> = preflight(Kind::NewRun, &facts)
        .into_iter()
        .filter(|f| f.about == About::Settings)
        .collect();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(!findings[0].blocks);
    assert!(
        findings[0]
            .text
            .contains("is not known yet under the model the step runs on"),
        "{}",
        findings[0].text
    );
}

#[test]
fn an_effort_under_another_model_is_not_known_yet() {
    let mut t = worktree();
    let label = t.steps[0].label.clone();
    if let StepKind::Agent { options, .. } = &mut t.steps[0].kind {
        options.insert("model".into(), "opus".into());
        options.insert("effort".into(), "high".into());
    } else {
        panic!("the worktree workflow's first step is not an agent step");
    }
    let offer = Offer {
        modes: vec!["default".into()],
        options: vec![
            ("model".into(), vec!["opus".into()]),
            ("effort".into(), vec!["low".into()]),
        ],
    };
    let facts = Facts {
        workflow: Ok(t),
        offered: Some(offer),
        ..healthy()
    };
    let findings: Vec<_> = preflight(Kind::NewRun, &facts)
        .into_iter()
        .filter(|f| f.about == About::Settings)
        .collect();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let effort = &findings[0];
    assert!(!effort.blocks);
    assert!(
        effort.text.starts_with(&format!(
            "Step 1 ({label}): `high` for `effort` is not known yet under model `opus`"
        )),
        "{}",
        effort.text
    );
    // The agent is known to come up; a probe never selects a model, so
    // pressing *Check the agent* again could never settle this one.
    assert!(!effort.checkable);
    assert_eq!(effort.change, None);
}

#[test]
fn settings_with_no_offer_known_are_not_known_yet() {
    let mut t = worktree();
    if let StepKind::Agent { mode, .. } = &mut t.steps[0].kind {
        *mode = Some("plan".into());
    } else {
        panic!("the worktree workflow's first step is not an agent step");
    }
    let facts = Facts {
        workflow: Ok(t),
        offered: None,
        ..healthy()
    };
    let findings: Vec<_> = preflight(Kind::NewRun, &facts)
        .into_iter()
        .filter(|f| f.about == About::Settings)
        .collect();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(!findings[0].blocks);
    assert!(
        findings[0].text.contains("not known yet"),
        "{}",
        findings[0].text
    );
    assert!(findings[0].checkable);
}

#[test]
fn a_mode_not_known_yet_does_not_also_say_settings_are_unknown() {
    let mut t = worktree();
    if let StepKind::Agent { options, .. } = &mut t.steps[0].kind {
        options.insert("model".into(), "opus".into());
    } else {
        panic!("the worktree workflow's first step is not an agent step");
    }
    let facts = Facts {
        workflow: Ok(t),
        mode: Some("plan".into()),
        offered: None,
        ..healthy()
    };
    let findings = preflight(Kind::NewRun, &facts);
    let mode = findings
        .iter()
        .find(|f| f.about == About::Mode)
        .expect("the run's own mode is named, so it says it is not known yet");
    assert!(!mode.blocks && mode.text.contains("not known yet"));
    // One sentence, one button: the step's settings say nothing more of an
    // agent already said not to have come up.
    assert!(
        findings.iter().all(|f| f.about != About::Settings),
        "{findings:?}"
    );
}

#[test]
fn a_workflow_without_settings_finds_nothing() {
    let t = worktree();
    for offered in [None, Some(modes(&["default"]))] {
        let facts = Facts {
            workflow: Ok(t.clone()),
            offered,
            ..healthy()
        };
        assert_eq!(found(Kind::NewRun, &facts, About::Settings), None);
    }
}

#[test]
fn step_settings_findings_are_capped() {
    let mut t = Template::blank("many");
    t.place = Place::Worktree;
    t.steps = (0..10)
        .map(|i| StepSpec {
            limit: None,
            id: format!("s{i}"),
            label: format!("Step {i}"),
            kind: StepKind::Agent {
                prompt: "go".into(),
                gates: Vec::new(),
                keep_answer: false,
                mode: Some("plan".into()),
                options: BTreeMap::new(),
                agent: None,
            },
        })
        .collect();
    // A Resume judges the run's own snapshot, so the workflow itself is
    // never validated here, only its steps' settings.
    let facts = Facts {
        workflow: Ok(t),
        offered: Some(modes(&["default"])),
        ..plain()
    };
    let findings: Vec<_> = preflight(Kind::Resume, &facts)
        .into_iter()
        .filter(|f| f.about == About::Settings)
        .collect();
    assert_eq!(findings.len(), 9, "{findings:?}");
    let last = findings.last().unwrap();
    assert!(last.blocks);
    assert!(last.text.contains("2 more"), "{}", last.text);
}

/// The worktree workflow with its first step naming `agent` and, when
/// given, a `model`.
fn naming(agent: &str, model: Option<&str>) -> Template {
    let mut t = worktree();
    if let StepKind::Agent {
        agent: named,
        options,
        ..
    } = &mut t.steps[0].kind
    {
        *named = Some(agent.into());
        if let Some(model) = model {
            options.insert("model".into(), model.into());
        }
    } else {
        panic!("the worktree workflow's first step is not an agent step");
    }
    t
}

fn model_offer(model: &str) -> Offer {
    Offer {
        modes: vec!["default".into()],
        options: vec![("model".into(), vec![model.into()])],
    }
}

#[test]
fn a_step_agent_not_in_the_list_blocks() {
    let t = naming("Codex", None);
    let label = t.steps[0].label.clone();
    let facts = Facts {
        workflow: Ok(t),
        step_agents: vec![StepAgent {
            step: 0,
            agent: "Codex".into(),
            configured: false,
            offered: None,
        }],
        ..healthy()
    };
    let finding = found(Kind::NewRun, &facts, About::Agent).unwrap();
    assert!(finding.blocks);
    assert_eq!(
        finding.text,
        format!("{label} names Codex, which is not in the agent list.")
    );
    assert_eq!(finding.change, Some(Change::At("Settings ▸ Agents")));
}

#[test]
fn a_step_agents_options_are_checked_against_its_own_offer() {
    let step_agent = |offered| StepAgent {
        step: 0,
        agent: "Codex".into(),
        configured: true,
        offered,
    };
    let facts = |model| Facts {
        workflow: Ok(naming("Codex", Some(model))),
        offered: Some(model_offer("a")),
        step_agents: vec![step_agent(Some(model_offer("b")))],
        ..healthy()
    };
    assert_eq!(found(Kind::NewRun, &facts("b"), About::Settings), None);
    let finding = found(Kind::NewRun, &facts("a"), About::Settings).unwrap();
    assert!(finding.blocks);
    // One whose offer is not known yet is told as the run's own is.
    let unknown = Facts {
        step_agents: vec![step_agent(None)],
        ..facts("a")
    };
    let finding = found(Kind::NewRun, &unknown, About::Settings).unwrap();
    assert!(!finding.blocks && finding.checkable, "{finding:?}");
}

#[test]
fn steps_naming_no_agent_check_as_before() {
    let mut t = worktree();
    if let StepKind::Agent { options, .. } = &mut t.steps[0].kind {
        options.insert("model".into(), "a".into());
    }
    let facts = Facts {
        workflow: Ok(t),
        offered: Some(model_offer("b")),
        ..healthy()
    };
    assert!(found(Kind::NewRun, &facts, About::Settings).unwrap().blocks);
}
