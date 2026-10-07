use super::*;
use std::path::PathBuf;

fn brief() -> Brief {
    Brief {
        title: "Fix the thing".into(),
        body: "It is broken.".into(),
        instructions: None,
    }
}

fn setup(check: Option<&str>) -> Setup {
    Setup {
        repo: PathBuf::from("/repo"),
        dir: PathBuf::from("/repo"),
        branch: None,
        agent: None,
        check: check.map(str::to_string),
        mode: None,
        forge: None,
    }
}

/// The shipped checkout template: plan, approve, implement, verify.
fn checkout() -> Template {
    builtin::all().remove(0)
}

fn mark(head: &str, digest: &str) -> Mark {
    Mark {
        head: head.into(),
        digests: vec![digest.into()],
    }
}

fn facts(head: &str, dirty: bool, commits: u64, digest: &str) -> Facts {
    Facts {
        head: head.into(),
        dirty,
        commits,
        digest: digest.into(),
    }
}

fn prompt_of(action: Action) -> String {
    match action {
        Action::Prompt(text) => text,
        other => panic!("expected a prompt, got {other:?}"),
    }
}

/// A run of `template`, started: what `Run::new` and `Run::resume` do for a
/// task that found its place free.
fn begin(id: String, template: Template, brief: Brief, setup: Setup) -> (Run, Action) {
    let mut run = Run::new(id, template, brief, setup);
    let first = run.resume();
    (run, first)
}

/// A checkout run past its plan and at its approval.
fn at_approval() -> Run {
    let (mut run, first) = begin("1".into(), checkout(), brief(), setup(Some("make check")));
    assert_eq!(first, Action::Measure);
    prompt_of(run.measured(mark("a", "d0")));
    assert_eq!(
        run.turn_ended(&facts("a", false, 0, "d0"), "The plan."),
        Action::AwaitApproval
    );
    run
}

/// Approve what `run` waits on, as a press drawn from it now does.
fn approve(run: &mut Run) -> Action {
    match run.approval_at() {
        Some(at) => run.approved(&at),
        None => Action::Idle,
    }
}

/// Send what `run` waits on back with `note`, as a press drawn from it now
/// does.
fn revise(run: &mut Run, note: &str) -> Action {
    match run.approval_at() {
        Some(at) => run.revised(&at, note.to_string()),
        None => Action::Idle,
    }
}

/// Window A reads the plan, B revises it, the plan runs again and waits at a
/// new approval visit: A's *Continue* names a visit that is no longer open,
/// and approves nothing it did not read.
#[test]
fn an_approval_drawn_from_a_closed_visit_is_refused() {
    let mut run = at_approval();
    let read_by_a = run.approval_at().unwrap();
    assert_eq!(read_by_a.run, "1");
    assert_eq!(
        run.revised(&read_by_a.clone(), "Smaller, please.".into()),
        Action::Measure
    );
    prompt_of(run.measured(mark("a", "d0")));
    assert_eq!(
        run.turn_ended(&facts("a", false, 0, "d0"), "A smaller plan."),
        Action::AwaitApproval
    );
    let now = run.approval_at().unwrap();
    assert_ne!(now.visit, read_by_a.visit, "the plan waits at a new visit");
    assert_eq!(run.approved(&read_by_a), Action::Idle);
    assert_eq!(
        run.revised(&read_by_a, "Again.".into()),
        Action::Idle,
        "a revision is refused the same way"
    );
    assert!(run.awaiting_approval(), "the run has not moved");
    assert_eq!(run.approved(&now), Action::Measure);
}

/// A double press, or a second press held while the next step's mark was
/// pinned, names the visit the first one closed.
#[test]
fn a_second_press_of_continue_approves_nothing() {
    let mut run = at_approval();
    let at = run.approval_at().unwrap();
    assert_eq!(run.approved(&at), Action::Measure);
    assert_eq!(run.approved(&at), Action::Idle);
    assert_eq!(run.revised(&at, "Late.".into()), Action::Idle);
    assert_eq!(run.current().unwrap().id, "implement");
    assert!(run.approval_at().is_none(), "nothing waits on a person");
}

/// A run cut off while it waited has nothing to approve; it is resumed.
#[test]
fn an_approval_of_a_run_cut_off_is_refused() {
    let mut run = at_approval();
    let at = run.approval_at().unwrap();
    assert!(matches!(run.stopped(Stop::LinkLost), Action::Finish(o) if o.resumable()));
    assert_eq!(run.approved(&at), Action::Idle);
    assert!(run.approval_at().is_none());
}

/// Visit ids count from 1 in every run: a press drawn from an earlier run of
/// the same task, at a visit with the same number, is refused by its run.
#[test]
fn an_approval_drawn_from_an_earlier_run_is_refused() {
    let mut first = at_approval();
    let old = first.approval_at().unwrap();
    first.stopped(Stop::TimedOut);
    let mut next = Run::retry_of(&first, "2".into(), first.template.clone(), Some("plan"));
    assert_eq!(next.resume(), Action::Measure);
    prompt_of(next.measured(mark("a", "d0")));
    assert_eq!(
        next.turn_ended(&facts("a", false, 0, "d0"), "The plan."),
        Action::AwaitApproval
    );
    let now = next.approval_at().unwrap();
    assert_eq!(now.visit, old.visit, "the same number in another run");
    assert_eq!(next.approved(&old), Action::Idle);
    assert_eq!(next.revised(&old, "No.".into()), Action::Idle);
    assert_eq!(next.approved(&now), Action::Measure);
}

#[test]
fn every_shipped_template_parses_and_validates() {
    let all = builtin::all();
    assert_eq!(all.len(), 3);
    for template in &all {
        assert_eq!(validate(template), [], "{}", template.name);
    }
    assert_eq!(all[0].place, Place::Checkout);
    assert_eq!(all[1].place, Place::Worktree);
    assert_eq!(all[2].place, Place::Worktree);
    assert_eq!(all[2].id, "builtin:issue");
}

#[test]
fn a_template_survives_a_round_trip_through_toml() {
    let template = checkout();
    let text = toml::to_string_pretty(&template).unwrap();
    assert_eq!(store::parse(&text).unwrap(), template);
}

#[test]
fn validate_names_each_kind_of_problem() {
    let said =
        |t: &Template| -> Vec<String> { validate(t).iter().map(|p| p.to_string()).collect() };
    let has = |t: &Template, part: &str| {
        let all = said(t);
        assert!(
            all.iter().any(|s| s.contains(part)),
            "{part:?} not in {all:?}"
        );
    };

    let mut t = Template::blank("");
    t.timeout = "soon".into();
    t.schema_version = SCHEMA_VERSION + 1;
    has(&t, "no name");
    has(&t, "no steps");
    has(&t, "timeout");
    has(&t, "schema");

    let mut t = checkout();
    t.steps[2].id = "plan".into();
    has(&t, "used by an earlier step");
    t.steps[2].id = "Bad Id".into();
    has(&t, "must be lowercase");

    let mut t = checkout();
    if let StepKind::Agent { prompt, .. } = &mut t.steps[0].kind {
        prompt.push_str(" {nonsense} {output.implement} and code {} { x }");
    }
    has(&t, "`{nonsense}`, which is no variable");
    has(&t, "`{output.implement}`, which is no earlier step");
    assert_eq!(said(&t).len(), 2, "braces that are not names are text");

    let mut t = checkout();
    if let StepKind::Agent { keep_answer, .. } = &mut t.steps[0].kind {
        *keep_answer = false;
    }
    has(&t, "keeps no answer");

    let mut t = checkout();
    t.steps[3].kind = StepKind::Command {
        command: Some(" ".into()),
        on_fail: "approve".into(),
    };
    has(&t, "command is blank");
    has(&t, "no earlier agent step");

    let mut t = checkout();
    if let StepKind::Agent { gates, .. } = &mut t.steps[2].kind {
        gates.push(GateKind::Committed);
        gates.push(GateKind::CodeUnchanged);
    }
    has(&t, "cannot hold in a checkout");
    has(&t, "both changed and unchanged");

    let mut t = checkout();
    if let StepKind::Agent { prompt, .. } = &mut t.steps[0].kind {
        prompt.push_str(" {check_output}");
    }
    if let StepKind::Agent { prompt, .. } = &mut t.steps[2].kind {
        prompt.push_str(" {check_output} {revise}");
    }
    let all = said(&t);
    assert_eq!(all.len(), 2, "{all:?}");
    has(
        &t,
        "Step 1: its prompt names `{check_output}`, but no later command or status checks step",
    );
    has(
        &t,
        "Step 3: its prompt names `{revise}`, but no later approval",
    );

    let mut t = checkout();
    if let StepKind::Agent { prompt, .. } = &mut t.steps[0].kind {
        prompt.push_str(" {revise}");
    }
    assert_eq!(said(&t), Vec::<String>::new(), "the approval sends it back");

    let mut t = builtin::all().remove(1);
    if let StepKind::Agent { gates, .. } = &mut t.steps[1].kind {
        gates.retain(|gate| *gate != GateKind::Committed);
    }
    has(&t, "no agent step has the gate Committed");

    let mut t = checkout();
    t.steps[2].label = " Plan ".into();
    has(&t, "Step 3: its label `Plan` is used by an earlier step");
    let mut t = checkout();
    t.steps.push(StepSpec {
        id: "push".into(),
        label: "Push".into(),
        kind: StepKind::Push,
    });
    has(&t, "only a workflow on a worktree");

    let mut t = builtin::all().remove(2);
    t.steps
        .retain(|step| step.id != "verify" && step.id != "push");
    if let Some(StepSpec {
        kind: StepKind::StatusChecks { on_fail, wait },
        ..
    }) = t.steps.last_mut()
    {
        *on_fail = "plan_x".into();
        *wait = "later".into();
    }
    has(&t, "no earlier step pushes");
    has(&t, "on failure it goes back to `plan_x`");
    has(&t, "its wait `later`");

    let mut t = builtin::all().remove(2);
    t.steps
        .retain(|step| step.id != "verify" && step.id != "pull_request");
    has(&t, "no earlier step is a command");
    has(&t, "no earlier step opens one");
}

#[test]
fn a_prompt_is_filled_in_and_told_where_it_works_and_what_is_checked() {
    let (mut run, _) = begin("1".into(), checkout(), brief(), setup(None));
    let text = prompt_of(run.measured(mark("a", "d0")));
    assert!(text.contains("Title: Fix the thing\n\nIt is broken."));
    assert!(!text.contains("{brief}"));
    assert!(text.contains("You are in this checkout"));
    assert!(text.contains("Do not edit any file or commit"));
    assert!(!text.contains("Instructions from the person"));

    let mut with = brief();
    with.instructions = Some("Keep it small.".into());
    let (mut run, _) = begin("1".into(), checkout(), with, setup(None));
    let text = prompt_of(run.measured(mark("a", "d0")));
    assert!(
        text.contains("> Keep it small."),
        "instructions are added when not placed"
    );
}

#[test]
fn an_agent_step_passes_on_its_gates_and_keeps_its_answer() {
    let mut run = at_approval();
    assert!(run.awaiting_approval());
    assert_eq!(run.outputs["plan"], "The plan.");
    assert_eq!(approve(&mut run), Action::Measure);
    let text = prompt_of(run.measured(mark("a", "d0")));
    assert!(
        text.contains("The plan."),
        "the plan is carried into the change"
    );
    assert_eq!(
        run.turn_ended(&facts("a", true, 0, "d1"), ""),
        Action::RunCommand("make check".into())
    );
    assert_eq!(
        run.command_finished(Ok(Some("a".into()))),
        Action::Finish(Outcome::Done)
    );
    assert_eq!(run.marks.verified_at.as_deref(), Some("a"));
    assert!(run.over());
    let done = Outcome::Done.said();
    assert_eq!(
        run.visits.last().unwrap().why.as_deref(),
        Some(done.as_str())
    );
}

#[test]
fn a_missed_gate_carries_on_and_too_many_exhaust_the_step() {
    let (mut run, _) = begin("1".into(), checkout(), brief(), setup(None));
    prompt_of(run.measured(mark("a", "d0")));
    for missed in 1..=3 {
        let text = prompt_of(run.turn_ended(&facts("a", false, 0, "d0"), "  "));
        assert!(text.contains("gave no answer"), "{text}");
        assert_eq!(run.misses, missed);
    }
    assert_eq!(
        run.turn_ended(&facts("a", false, 0, "d0"), ""),
        Action::Finish(Outcome::Exhausted {
            step: "Plan".into()
        })
    );
}

#[test]
fn a_change_in_a_checkout_plan_is_measured_again_so_a_persons_edit_may_stay() {
    let (mut run, _) = begin("1".into(), checkout(), brief(), setup(None));
    prompt_of(run.measured(mark("a", "d0")));
    // The plan turn left the checkout changed: perhaps the person's edit.
    assert_eq!(
        run.turn_ended(&facts("a", true, 0, "d1"), "The plan."),
        Action::Measure
    );
    let text = prompt_of(run.measured(mark("a", "d1")));
    assert!(text.contains("leave every change that is not yours"));
    // As the step found it or as that turn left it both pass now.
    assert_eq!(
        run.turn_ended(&facts("a", true, 0, "d1"), "The plan."),
        Action::AwaitApproval
    );
}

#[test]
fn a_failed_command_goes_back_with_its_output_and_keeps_counting() {
    let mut run = at_approval();
    approve(&mut run);
    prompt_of(run.measured(mark("a", "d0")));
    run.turn_ended(&facts("a", true, 0, "d1"), "");
    assert_eq!(
        run.command_finished(Err("test failed: x".into())),
        Action::Measure
    );
    assert_eq!(run.current().unwrap().id, "implement");
    let text = prompt_of(run.measured(mark("a", "d1")));
    assert!(text.contains("What onehand checked failed:\n\n```\ntest failed: x\n```"));
    // Back and forth between the change and the check is bounded too.
    for n in 2..4 {
        let digest = format!("d{n}");
        run.turn_ended(&facts("a", true, 0, &digest), "");
        run.command_finished(Err("again".into()));
        prompt_of(run.measured(mark("a", &digest)));
    }
    run.turn_ended(&facts("a", true, 0, "d9"), "");
    assert!(matches!(
        run.command_finished(Err("again".into())),
        Action::Finish(Outcome::Exhausted { .. })
    ));
}

#[test]
fn a_revision_sends_the_plan_back_with_the_note_and_the_old_plan() {
    let mut run = at_approval();
    assert_eq!(revise(&mut run, "Smaller, please."), Action::Measure);
    assert_eq!(run.current().unwrap().id, "plan");
    let text = prompt_of(run.measured(mark("a", "d0")));
    assert!(text.contains("> Smaller, please."));
    assert!(text.contains("> The plan."));
    assert_eq!(
        run.turn_ended(&facts("a", false, 0, "d0"), "A smaller plan."),
        Action::AwaitApproval
    );
    assert_eq!(run.revise, None);
    assert_eq!(run.outputs["plan"], "A smaller plan.");
    assert_eq!(run.misses, 0, "a revision is not a miss");
}

/// Finding #1 of the closed pull request: a Stop ends the run whatever it was
/// waiting for, and the turn it cut short is never judged.
#[test]
fn every_stop_ends_the_run_without_judging_the_turn() {
    for stop in [
        Stop::ByPerson,
        Stop::TakenOver,
        Stop::TimedOut,
        Stop::LinkLost,
        Stop::Closed,
    ] {
        let (mut run, _) = begin("1".into(), checkout(), brief(), setup(None));
        prompt_of(run.measured(mark("a", "d0")));
        assert_eq!(run.stopped(stop), Action::Finish(Outcome::Stopped(stop)));
        assert!(run.outputs.is_empty(), "a cut-short plan is not kept");
        // The turn ending after the Stop changes nothing.
        assert_eq!(
            run.turn_ended(&facts("a", false, 0, "d0"), "Half a plan"),
            Action::Idle
        );
        assert_eq!(run.stopped(stop), Action::Idle);
        assert_eq!(run.current().unwrap().id, "plan");
    }
    let mut run = at_approval();
    assert!(matches!(run.stopped(Stop::ByPerson), Action::Finish(_)));
    assert_eq!(approve(&mut run), Action::Idle);
}

/// Finding #3: a resumed run keeps the mark its step was measured from, so
/// work done before the restart still counts.
#[test]
fn resume_keeps_where_the_step_started() {
    let mut run = at_approval();
    approve(&mut run);
    prompt_of(run.measured(mark("a", "d0")));
    let text = serde_json::to_string(&run).unwrap();
    let mut back: Run = serde_json::from_str(&text).unwrap();
    assert!(!back.awaiting_turn(), "what it waits for is not kept");
    let prompt = prompt_of(back.resume());
    assert!(prompt.contains("This step is the change"));
    assert_eq!(back.marks.step_from, Some(mark("a", "d0")));
    // The change made before the restart passes the gate.
    assert_eq!(
        back.turn_ended(&facts("a", true, 0, "d1"), ""),
        Action::RunCommand("make check".into())
    );

    // A run waiting for its approval waits again; one restarted before its
    // step was measured measures.
    let mut waiting: Run =
        serde_json::from_str(&serde_json::to_string(&at_approval()).unwrap()).unwrap();
    assert_eq!(waiting.resume(), Action::AwaitApproval);
    // What it waits on comes back with it, for the new session to show.
    let (step, answer) = waiting.under_review().unwrap();
    assert_eq!((step.id.as_str(), answer), ("plan", "The plan."));
    approve(&mut waiting);
    assert!(waiting.under_review().is_none());
    let (fresh, _) = begin("1".into(), checkout(), brief(), setup(None));
    let mut fresh: Run = serde_json::from_str(&serde_json::to_string(&fresh).unwrap()).unwrap();
    assert_eq!(fresh.resume(), Action::Measure);
}

/// A run parked in this process, its agent gone or its session closed,
/// resumes without a trip through its file.
#[test]
fn a_run_parked_in_this_process_resumes() {
    for stop in [Stop::LinkLost, Stop::Closed] {
        let mut run = at_approval();
        approve(&mut run);
        prompt_of(run.measured(mark("a", "d0")));
        assert!(matches!(run.stopped(stop), Action::Finish(outcome) if outcome.resumable()));
        let mut parked = run.clone();
        assert!(prompt_of(parked.resume()).contains("This step is the change"));
    }
    // A run that ended on its own outcome does not come back.
    let mut run = at_approval();
    assert!(!matches!(run.stopped(Stop::ByPerson), Action::Finish(o) if o.resumable()));
    assert_eq!(run.resume(), Action::Idle);
}

#[test]
fn a_template_needs_a_check_command_only_for_a_command_step_naming_none() {
    assert!(checkout().needs_check());
    let mut named = checkout();
    named.steps[3].kind = StepKind::Command {
        command: Some("make check".into()),
        on_fail: "implement".into(),
    };
    assert!(!named.needs_check());
    named.steps.pop();
    assert!(!named.needs_check());
}

#[test]
fn a_run_keeps_the_template_it_began_with() {
    let mut template = checkout();
    let (run, _) = begin("1".into(), template.clone(), brief(), setup(None));
    template.steps.remove(1);
    template.name = "Edited".into();
    assert_eq!(run.template, checkout());
}

#[test]
fn a_command_step_with_no_command_and_no_check_fails() {
    let mut run = at_approval();
    run.setup.check = None;
    approve(&mut run);
    prompt_of(run.measured(mark("a", "d0")));
    assert!(matches!(
        run.turn_ended(&facts("a", true, 0, "d1"), ""),
        Action::Finish(Outcome::Failed(_))
    ));
}

#[test]
fn every_move_is_in_the_history() {
    let mut run = at_approval();
    approve(&mut run);
    let moves: Vec<(&str, &str)> = run
        .history
        .iter()
        .filter(|t| t.from != t.to)
        .map(|t| (t.from.as_str(), t.to.as_str()))
        .collect();
    assert_eq!(
        moves,
        [
            ("start", "plan"),
            ("plan", "approve"),
            ("approve", "implement")
        ]
    );
}

#[test]
fn a_run_not_started_has_no_visits_and_starts_on_resume() {
    let mut run = Run::new("1".into(), checkout(), brief(), setup(None));
    assert!(run.visits.is_empty() && run.history.is_empty() && !run.over());
    assert_eq!(run.resume(), Action::Measure);
    assert_eq!(run.visits.len(), 1);
    assert_eq!(run.visits[0].step, "plan");
    assert_eq!(run.history[0].from, "start");
}

#[test]
fn going_back_is_a_new_visit_and_each_keeps_how_it_came_out() {
    let mut run = at_approval();
    approve(&mut run);
    prompt_of(run.measured(mark("a", "d0")));
    run.turn_ended(&facts("a", true, 0, "d1"), "");
    run.command_finished(Err("test failed".into()));
    let steps: Vec<&str> = run.visits.iter().map(|v| v.step.as_str()).collect();
    assert_eq!(
        steps,
        ["plan", "approve", "implement", "verify", "implement"]
    );
    let ids: Vec<u32> = run.visits.iter().map(|v| v.id).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5]);
    assert_eq!(run.visits[0].output.as_deref(), Some("The plan."));
    assert_eq!(run.visits[0].why.as_deref(), Some("its gates held"));
    assert_eq!(run.visits[3].output.as_deref(), Some("test failed"));
    assert_eq!(run.visits[3].why.as_deref(), Some("the command failed"));
    assert!(run.visits[4].ended_at.is_none(), "the last is the open one");

    run.stopped(Stop::ByPerson);
    let said = Outcome::Stopped(Stop::ByPerson).said();
    assert_eq!(run.visits[4].why.as_deref(), Some(said.as_str()));
}

#[test]
fn resume_closes_the_cut_off_visit_and_opens_one_of_the_same_step() {
    let mut run = at_approval();
    // As a restart finds it: nothing ended the visit.
    let mut back: Run = serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
    assert_eq!(back.resume(), Action::AwaitApproval);
    assert_eq!(back.visits.len(), 3);
    assert_eq!(back.visits[1].why.as_deref(), Some("interrupted"));
    assert_eq!(back.visits[2].step, "approve");

    // Stopped by its agent going: the visit was ended then, and resume opens
    // the next one.
    run.stopped(Stop::LinkLost);
    assert_eq!(run.resume(), Action::AwaitApproval);
    assert_eq!(run.visits.len(), 3);
    assert_ne!(run.visits[1].why.as_deref(), Some("interrupted"));
    assert_eq!(run.outcome, None, "a resumed run has not ended");
}

#[test]
fn one_commit_pins_a_visits_end_and_the_next_ones_start() {
    let (mut run, _) = begin("1".into(), checkout(), brief(), setup(None));
    assert_eq!(run.boundaries(), [(1, false)]);
    run.pinned("c1", 0);
    prompt_of(run.measured(mark("a", "d0")));
    run.turn_ended(&facts("a", false, 0, "d0"), "The plan.");
    assert_eq!(run.boundaries(), [(1, false), (1, true), (2, false)]);
    run.pinned("c2", 1);
    assert_eq!(run.visits[0].start.as_deref(), Some("c1"));
    assert_eq!(run.visits[0].end.as_deref(), Some("c2"));
    assert_eq!(run.visits[1].start.as_deref(), Some("c2"));
    assert_eq!(run.visits[1].end, None);
}

/// A mark lost when the app quit before it landed is pinned again on resume,
/// never counted as made.
#[test]
fn a_boundary_counts_as_pinned_only_up_to_the_last_one_that_landed() {
    let (mut run, _) = begin("1".into(), checkout(), brief(), setup(None));
    assert_eq!(run.pinned_count(), 0);
    run.pinned("c1", 0);
    assert_eq!(run.pinned_count(), 1);
    prompt_of(run.measured(mark("a", "d0")));
    run.stopped(Stop::Closed);
    assert_eq!(run.boundaries().len(), 2);
    assert_eq!(run.pinned_count(), 1, "the end mark never landed");
    run.pinned("c2", 1);
    assert_eq!(run.pinned_count(), 2);
}

#[test]
fn the_outcome_is_kept_and_an_old_run_without_it_still_loads() {
    let mut run = at_approval();
    run.stopped(Stop::TimedOut);
    let mut back: Run = serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
    assert_eq!(back.outcome, Some(Outcome::Stopped(Stop::TimedOut)));
    assert!(back.over());
    assert_eq!(back.resume(), Action::Idle, "a timeout is not resumed");

    let mut old = serde_json::to_value(at_approval()).unwrap();
    let fields = old.as_object_mut().unwrap();
    fields.remove("visits");
    fields.remove("outcome");
    let mut old: Run = serde_json::from_value(old).unwrap();
    assert!(old.visits.is_empty() && !old.over());
    assert_eq!(old.resume(), Action::AwaitApproval);
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-workflow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_template_written_by_a_newer_onehand_is_refused_and_never_written_over() {
    let dir = temp_dir("newer");
    let file = dir.join("future.toml");
    let text = format!(
        "schema_version = {}\nname = \"Future\"\n",
        SCHEMA_VERSION + 1
    );
    std::fs::write(&file, &text).unwrap();
    let loaded = store::load_all_blocking(&dir);
    assert!(loaded[0].1.as_ref().unwrap_err().contains("newer onehand"));
    assert!(store::save_blocking(&dir, Some(&file), &checkout()).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn templates_are_saved_one_file_each_under_their_name() {
    let dir = temp_dir("save");
    let first = store::save_blocking(&dir, None, &checkout()).unwrap().0;
    let second = store::save_blocking(&dir, None, &checkout()).unwrap().0;
    assert_eq!(first, dir.join("work-in-checkout.toml"));
    assert_eq!(second, dir.join("work-in-checkout-2.toml"));
    let mut renamed = checkout();
    renamed.name = "Renamed".into();
    assert_eq!(
        store::save_blocking(&dir, Some(&first), &renamed)
            .unwrap()
            .0,
        first
    );
    let loaded = store::load_all_blocking(&dir);
    assert_eq!(loaded.len(), 2);
    let (_, kept) = loaded.iter().find(|(path, _)| *path == first).unwrap();
    assert_eq!(kept.as_ref().unwrap().name, "Renamed");
    store::delete_blocking(&second).unwrap();
    assert_eq!(store::load_all_blocking(&dir).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_sets_the_id_and_counts_versions_of_what_changed() {
    let dir = temp_dir("versions");
    let (file, first) = store::save_blocking(&dir, None, &checkout()).unwrap();
    assert_eq!((first.id.as_str(), first.version), ("work-in-checkout", 1));

    let (_, same) = store::save_blocking(&dir, Some(&file), &first).unwrap();
    assert_eq!(same.version, 1, "nothing changed");

    let mut renamed = same.clone();
    renamed.name = "Renamed".into();
    // Whatever the caller says, the file's own id and version count.
    renamed.id = "elsewhere".into();
    renamed.version = 9;
    let (_, saved) = store::save_blocking(&dir, Some(&file), &renamed).unwrap();
    assert_eq!((saved.id.as_str(), saved.version), ("work-in-checkout", 2));
    let loaded = store::load_all_blocking(&dir);
    assert_eq!(loaded[0].1.as_ref().unwrap(), &saved);

    // A duplicate is a new file, and so a new id.
    let (_, copy) = store::save_blocking(&dir, None, &saved).unwrap();
    assert_eq!((copy.id.as_str(), copy.version), ("renamed", 1));

    // A file from before ids takes its file's name.
    let old = dir.join("old.toml");
    let text = toml::to_string_pretty(&checkout()).unwrap();
    let text: String = text
        .lines()
        .filter(|line| *line != "id = \"builtin:checkout\"" && !line.starts_with("version ="))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&old, text).unwrap();
    let loaded = store::load_all_blocking(&dir);
    let (_, read) = loaded.iter().find(|(path, _)| *path == old).unwrap();
    let read = read.as_ref().unwrap();
    assert_eq!((read.id.as_str(), read.version), ("old", 1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_export_is_offered_under_the_name_as_a_file_name() {
    assert_eq!(store::export_name(&checkout()), "work-in-checkout.toml");
}

#[test]
fn a_key_onehand_does_not_read_is_refused() {
    let text = toml::to_string_pretty(&checkout()).unwrap();
    let top = format!("colour = \"red\"\n{text}");
    assert!(store::parse(&top)
        .unwrap_err()
        .contains("a key `colour` that onehand does not read"));
    let in_step = text.replacen("of = \"plan\"", "of = \"plan\"\ngate = \"x\"", 1);
    assert!(store::parse(&in_step)
        .unwrap_err()
        .contains("`steps[1].gate`"));
    assert!(store::parse(&text).is_ok());
}

#[test]
fn a_newer_template_is_one_saved_later_under_the_same_id() {
    let mut snapshot = checkout();
    let mut later = snapshot.clone();
    later.name = "Renamed".into();
    later.version = 2;
    assert!(later.newer_than(&snapshot), "found by id after a rename");
    assert!(
        !snapshot.newer_than(&later),
        "an older version is not newer"
    );
    later.id = "other".into();
    assert!(!later.newer_than(&snapshot));

    // A file edited by hand keeps its version, and still counts.
    let mut by_hand = snapshot.clone();
    assert!(!by_hand.newer_than(&snapshot), "the same, unchanged");
    by_hand.misses += 1;
    assert!(by_hand.newer_than(&snapshot));
    by_hand.version = 0;
    assert!(!by_hand.newer_than(&snapshot), "an older version never is");

    // A run kept before ids falls back to the name and what it says.
    snapshot.id.clear();
    let mut same_name = checkout();
    assert!(
        !same_name.newer_than(&snapshot),
        "the same, only with an id"
    );
    same_name.misses += 1;
    assert!(same_name.newer_than(&snapshot));
}

#[test]
fn the_first_prompt_is_filled_with_the_brief_and_the_gates() {
    let text = first_prompt(&checkout(), &brief()).unwrap();
    assert!(text.contains("Title: Fix the thing\n\nIt is broken."));
    assert!(text.contains("Do not edit any file or commit"));
    assert!(first_prompt(&Template::blank("x"), &brief()).is_none());
}

#[test]
fn a_step_says_what_it_does_in_a_line() {
    let t = checkout();
    let said: Vec<String> = t.steps.iter().map(StepSpec::summary).collect();
    assert_eq!(
        said,
        [
            "Agent · Answered, Code unchanged · keeps its answer",
            "Approval of plan",
            "Agent · Code changed, Uncommitted",
            "Command · the project's check · back to implement on failure",
        ]
    );
}

#[test]
fn gates_read_the_work_against_the_mark() {
    let from = mark("a", "d0");
    let holds = |gate, f: &Facts, answer| facts::holds(gate, f, &from, answer);
    let same = facts("a", false, 0, "d0");
    let edited = facts("a", true, 0, "d1");
    let committed = facts("b", false, 1, "d0");
    assert!(holds(GateKind::Answered, &same, "yes") && !holds(GateKind::Answered, &same, " "));
    assert!(
        holds(GateKind::CodeUnchanged, &same, "") && !holds(GateKind::CodeUnchanged, &edited, "")
    );
    assert!(
        holds(GateKind::CodeChanged, &committed, "") && !holds(GateKind::CodeChanged, &same, "")
    );
    assert!(holds(GateKind::Committed, &committed, "") && !holds(GateKind::Committed, &edited, ""));
    assert!(
        holds(GateKind::Uncommitted, &edited, "") && !holds(GateKind::Uncommitted, &committed, "")
    );
}

#[test]
fn templates_move_to_the_new_directory_and_the_old_one_goes() {
    let root = temp_dir("migrate");
    let (old, new) = (root.join("old"), root.join("new"));
    std::fs::create_dir_all(&old).unwrap();
    let text = toml::to_string_pretty(&checkout()).unwrap();
    std::fs::write(old.join("plain.toml"), &text).unwrap();
    assert!(store::migrate_blocking(&old, &new).is_empty());
    assert_eq!(
        std::fs::read_to_string(new.join("plain.toml")).unwrap(),
        text
    );
    assert!(!old.exists());
    // A missing `old` is nothing to do.
    assert!(store::migrate_blocking(&old, &new).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_crash_between_the_write_and_the_removal_ends_with_one_copy() {
    let root = temp_dir("migrate-twice");
    let (old, new) = (root.join("old"), root.join("new"));
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let text = toml::to_string_pretty(&checkout()).unwrap();
    // As a crash between the write and the removal leaves it.
    std::fs::write(old.join("same.toml"), &text).unwrap();
    std::fs::write(new.join("same.toml"), &text).unwrap();
    assert!(store::migrate_blocking(&old, &new).is_empty());
    assert_eq!(
        std::fs::read_to_string(new.join("same.toml")).unwrap(),
        text
    );
    assert!(!old.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_old_copy_that_differs_from_the_moved_one_is_kept_and_reported() {
    let root = temp_dir("migrate-differs");
    let (old, new) = (root.join("old"), root.join("new"));
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let moved = toml::to_string_pretty(&checkout()).unwrap();
    // An older build, finding nothing in `old`, saved an edit there.
    let mut edited = checkout();
    edited.name = "Edited".into();
    let edited = toml::to_string_pretty(&edited).unwrap();
    std::fs::write(new.join("same.toml"), &moved).unwrap();
    std::fs::write(old.join("same.toml"), &edited).unwrap();
    let problems = store::migrate_blocking(&old, &new);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        std::fs::read_to_string(old.join("same.toml")).unwrap(),
        edited
    );
    assert_eq!(
        std::fs::read_to_string(new.join("same.toml")).unwrap(),
        moved
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_template_this_build_cannot_read_stays_where_it_is() {
    let root = temp_dir("migrate-unreadable");
    let (old, new) = (root.join("old"), root.join("new"));
    std::fs::create_dir_all(&old).unwrap();
    let future = format!("schema_version = {}\n", SCHEMA_VERSION + 1);
    std::fs::write(old.join("future.toml"), &future).unwrap();
    std::fs::write(old.join("notes.txt"), "mine").unwrap();
    let problems = store::migrate_blocking(&old, &new);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        std::fs::read_to_string(old.join("future.toml")).unwrap(),
        future
    );
    assert!(old.join("notes.txt").exists());
    assert!(!new.join("future.toml").exists());
    // Nor is it removed when a file of the same name is already there.
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("future.toml"), "mine").unwrap();
    assert_eq!(store::migrate_blocking(&old, &new).len(), 1);
    assert_eq!(
        std::fs::read_to_string(old.join("future.toml")).unwrap(),
        future
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_old_directory_that_cannot_be_read_is_reported() {
    let root = temp_dir("migrate-unlistable");
    // Something is at `old`, but it cannot be listed.
    let (old, new) = (root.join("old"), root.join("new"));
    std::fs::write(&old, "not a directory").unwrap();
    let problems = store::migrate_blocking(&old, &new);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(old.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn only_an_ending_nobody_chose_needs_attention() {
    for (outcome, needs) in [
        (Outcome::Done, false),
        (Outcome::Stopped(Stop::ByPerson), false),
        (Outcome::Stopped(Stop::TakenOver), false),
        (Outcome::Stopped(Stop::TimedOut), true),
        (Outcome::Stopped(Stop::LinkLost), true),
        (Outcome::Stopped(Stop::Closed), true),
        (Outcome::Exhausted { step: "x".into() }, true),
        (Outcome::Failed("x".into()), true),
    ] {
        assert_eq!(outcome.needs_attention(), needs, "{outcome:?}");
    }
}

/// A checkout run that passed its plan and its approval, then ran out of
/// misses at the change.
fn exhausted_at_implement() -> Run {
    let mut run = at_approval();
    approve(&mut run);
    prompt_of(run.measured(mark("a", "d0")));
    for _ in 0..4 {
        run.turn_ended(&facts("a", false, 0, "d0"), "");
    }
    assert!(matches!(run.outcome, Some(Outcome::Exhausted { .. })));
    assert_eq!(run.current().unwrap().id, "implement");
    run
}

#[test]
fn a_retry_on_the_same_template_starts_where_the_last_run_stopped() {
    let prev = exhausted_at_implement();
    let mut next = Run::retry_of(&prev, "2".into(), prev.template.clone(), None);
    assert_eq!((next.step, next.misses), (2, 0));
    assert_eq!(
        next.outputs.get("plan").map(String::as_str),
        Some("The plan.")
    );
    assert_eq!(next.resume(), Action::Measure);
    assert_eq!(next.current().unwrap().id, "implement");
    let text = prompt_of(next.measured(mark("a", "d0")));
    assert!(
        text.contains("The plan."),
        "the plan it carried is filled in"
    );
}

#[test]
fn a_retry_from_an_earlier_step_drops_what_came_after() {
    let prev = exhausted_at_implement();
    let next = Run::retry_of(&prev, "2".into(), prev.template.clone(), Some("plan"));
    assert_eq!(next.step, 0);
    assert!(next.outputs.is_empty(), "the plan runs again");
}

#[test]
fn a_retry_ignores_a_step_it_lacks_or_one_past_its_start() {
    let prev = exhausted_at_implement();
    for from in ["nope", "verify"] {
        let next = Run::retry_of(&prev, "2".into(), prev.template.clone(), Some(from));
        assert_eq!(next.step, 2, "{from}");
        assert_eq!(next.outputs.len(), 1);
    }
}

#[test]
fn a_done_run_retries_from_past_its_last_step_and_offers_the_first() {
    let mut prev = at_approval();
    approve(&mut prev);
    prompt_of(prev.measured(mark("a", "d0")));
    prev.turn_ended(&facts("a", true, 0, "d1"), "");
    assert_eq!(
        prev.command_finished(Ok(Some("a".into()))),
        Action::Finish(Outcome::Done)
    );
    assert_eq!(
        Run::retry_start(&prev, &prev.template),
        prev.template.steps.len()
    );
    assert_eq!(Run::retry_offered(&prev, &prev.template), 0);
}

#[test]
fn work_changed_since_is_checked_again_before_anything_past_the_check() {
    let t = builtin::all().remove(2);
    let at = |id: &str| t.index_of(id).unwrap();
    // Past the check, the check runs again on the work as it is now.
    assert_eq!(Run::recheck(&t, at("push")), at("verify"));
    assert_eq!(Run::recheck(&t, at("status_checks")), at("verify"));
    assert_eq!(Run::recheck(&t, t.steps.len()), at("verify"));
    // At or before it, nothing moves.
    assert_eq!(Run::recheck(&t, at("verify")), at("verify"));
    assert_eq!(Run::recheck(&t, at("implement")), at("implement"));
}

#[test]
fn a_retry_plan_says_where_it_starts_and_what_it_carries() {
    let prev = exhausted_at_implement();
    let t = &prev.template;
    assert_eq!(Run::retry_offered(&prev, t), 2);
    assert_eq!(Run::retry_plan(&prev, t, None), (2, 1));
    assert_eq!(Run::retry_plan(&prev, t, Some("plan")), (0, 0));
    assert_eq!(Run::retry_plan(&prev, t, Some("nope")), (2, 1));
    let next = Run::retry_of(&prev, "2".into(), t.clone(), Some("plan"));
    assert_eq!(
        (next.step, next.outputs.len()),
        Run::retry_plan(&prev, t, Some("plan"))
    );
}

/// A checkout run that passed its plan, its approval and its change, then
/// ran out of misses at the check.
fn exhausted_at_verify() -> Run {
    let mut run = at_approval();
    approve(&mut run);
    prompt_of(run.measured(mark("a", "d0")));
    for n in 0..4 {
        let digest = format!("d{}", n + 1);
        run.turn_ended(&facts("a", true, 0, &digest), "");
        if let Action::Measure = run.command_finished(Err("again".into())) {
            prompt_of(run.measured(mark("a", &digest)));
        }
    }
    assert!(matches!(run.outcome, Some(Outcome::Exhausted { .. })));
    assert_eq!(run.current().unwrap().id, "verify");
    run
}

#[test]
fn a_retry_starts_at_the_first_step_whose_prompt_changed() {
    let prev = exhausted_at_verify();
    let mut changed = prev.template.clone();
    if let StepKind::Agent { prompt, .. } = &mut changed.steps[2].kind {
        prompt.push_str("\nAnd more.");
    }
    let next = Run::retry_of(&prev, "2".into(), changed, None);
    assert_eq!(
        next.step, 2,
        "the change runs again; plan and approval carry"
    );
    assert_eq!(next.outputs.len(), 1);
    // The plan's prompt changed: everything from it on runs again.
    let mut changed = prev.template.clone();
    if let StepKind::Agent { prompt, .. } = &mut changed.steps[0].kind {
        prompt.push_str("\nAnd more.");
    }
    let next = Run::retry_of(&prev, "2".into(), changed, None);
    assert_eq!(next.step, 0);
    assert!(next.outputs.is_empty());
}

/// A step carries over only if the last run passed it in its own template:
/// dropping a step earlier on must not move the failed one into the past.
#[test]
fn a_retry_never_skips_a_step_the_last_run_did_not_pass() {
    let prev = exhausted_at_verify();
    let mut changed = prev.template.clone();
    changed.steps.remove(1);
    let mut next = Run::retry_of(&prev, "2".into(), changed, None);
    assert_eq!(next.current().map(|s| s.id.as_str()), Some("verify"));
    assert_eq!(next.outputs.len(), 1, "the plan carries");
    assert!(matches!(next.resume(), Action::RunCommand(_)));
}

#[test]
fn a_retry_does_not_carry_an_approval_whose_of_changed() {
    let prev = exhausted_at_implement();
    let mut changed = prev.template.clone();
    changed.steps[1].kind = StepKind::Approval {
        of: "implement".into(),
    };
    let next = Run::retry_of(&prev, "2".into(), changed, None);
    assert_eq!(next.step, 1);
    assert_eq!(next.outputs.len(), 1, "the plan before it still carries");
}

/// A step the same in both templates still runs again when a step it reads
/// is not: here the plan the approval approves is gone.
#[test]
fn a_retry_does_not_carry_a_step_that_reads_one_that_changed() {
    let prev = exhausted_at_implement();
    let mut changed = prev.template.clone();
    changed.steps.remove(0);
    assert_eq!(
        changed.steps[0], prev.template.steps[1],
        "the approval is unchanged"
    );
    let next = Run::retry_of(&prev, "2".into(), changed, None);
    assert_eq!(next.step, 0);
    assert!(next.outputs.is_empty());
}

#[test]
fn a_retry_counts_its_misses_from_zero() {
    let prev = exhausted_at_implement();
    assert!(prev.misses > 0);
    let mut next = Run::retry_of(&prev, "2".into(), prev.template.clone(), None);
    next.resume();
    prompt_of(next.measured(mark("a", "d0")));
    // One miss is under the allowance again.
    prompt_of(next.turn_ended(&facts("a", false, 0, "d0"), ""));
    assert_eq!((next.misses, next.outcome.clone()), (1, None));
}

/// A command passes on its exit status: the commit it ran on is kept when
/// there is one, and a folder outside git, or a repository with no commit,
/// passes the same way.
#[test]
fn a_command_passes_on_its_exit_status_with_or_without_a_commit() {
    let mut t = crate::task::Task::check("1".into(), "true".into(), setup(None));
    let run = &mut t.runs[0];
    assert!(matches!(run.resume(), Action::RunCommand(_)));
    assert_eq!(
        run.command_finished(Ok(None)),
        Action::Finish(Outcome::Done)
    );
    assert_eq!(run.marks.verified_at, None);
}

/// The shipped issue workflow, on a worktree served by a forge when
/// `forge`, run until its check has passed on `verified`.
fn issue_past_check(forge: bool, verified: &str) -> (Run, Action) {
    let template = builtin::all().remove(2);
    let mut setup = setup(Some("make check"));
    setup.forge = forge.then(|| "Forge".to_string());
    let (mut run, _) = begin("1".into(), template, brief(), setup);
    prompt_of(run.measured(mark("a", "d0")));
    run.turn_ended(&facts("a", false, 0, "d0"), "The plan.");
    prompt_of(run.measured(mark("a", "d0")));
    run.turn_ended(&facts("b", false, 1, "d0"), "");
    let next = run.command_finished(Ok(Some(verified.into())));
    (run, next)
}

/// [`issue_past_check`], pushed and with its pull request open.
fn at_status_checks() -> Run {
    let (mut run, _) = issue_past_check(true, "b");
    run.forge_done(Ok(()));
    run.forge_done(Ok(()));
    run
}

fn pull_request(state: PrState, checks: &[(&str, CheckState)], conflicting: bool) -> PullRequest {
    PullRequest {
        url: "https://forge/pr/7".into(),
        number: 7,
        state,
        draft: true,
        head: "b".into(),
        conflicting,
        checks: checks
            .iter()
            .map(|(name, state)| Check {
                name: (*name).into(),
                state: *state,
                link: None,
            })
            .collect(),
    }
}

use crate::connector::{Check, CheckState, PrState, PullRequest};
use std::time::Duration;

#[test]
fn the_push_carries_the_commit_the_check_passed_on() {
    let (mut run, next) = issue_past_check(true, "b");
    assert_eq!(next, Action::Push("b".into()));
    assert_eq!(run.forge_done(Ok(())), Action::OpenPullRequest);
    // The status checks are watched on the commit that was pushed.
    assert_eq!(
        run.forge_done(Ok(())),
        Action::AwaitStatusChecks {
            wait: Duration::from_secs(3600),
            pushed: Some("b".into()),
        }
    );
    assert!(run.awaiting_status_checks());
    assert_eq!(run.status_checks_seen(Seen::Pending), Action::Idle);
    assert_eq!(
        run.status_checks_seen(Seen::Passed),
        Action::Finish(Outcome::Done)
    );
}

#[test]
fn with_no_forge_the_branch_is_the_result() {
    let (run, next) = issue_past_check(false, "b");
    assert_eq!(next, Action::Finish(Outcome::Done));
    let passed: Vec<_> = run.visits().iter().map(|v| v.step.as_str()).collect();
    assert_eq!(
        passed,
        [
            "plan",
            "implement",
            "verify",
            "push",
            "pull_request",
            "status_checks"
        ]
    );
}

#[test]
fn a_push_with_nothing_checked_fails_and_a_failed_push_ends_the_run() {
    let template = builtin::all().remove(2);
    let mut setup = setup(Some("make check"));
    setup.forge = Some("Forge".into());
    let mut run = Run::new("1".into(), template, brief(), setup);
    run.step = run.template.index_of("push").unwrap();
    assert!(matches!(run.resume(), Action::Finish(Outcome::Failed(_))));

    let (mut run, _) = issue_past_check(true, "b");
    assert_eq!(
        run.forge_done(Err("rejected".into())),
        Action::Finish(Outcome::Failed("rejected".into()))
    );
}

#[test]
fn failing_status_checks_go_back_to_the_change_with_what_failed_until_misses_run_out() {
    let mut run = at_status_checks();
    assert_eq!(
        run.status_checks_seen(Seen::Repair("Lint failed".into())),
        Action::Measure
    );
    assert_eq!(run.current().unwrap().id, "implement");
    let text = prompt_of(run.measured(mark("b", "d0")));
    assert!(text.contains("Lint failed"), "{text}");
    // Repair, check, push again: the push is of the newly checked commit.
    for n in 2..=3 {
        let head = format!("c{n}");
        run.turn_ended(&facts(&head, false, 1, "d0"), "");
        assert_eq!(
            run.command_finished(Ok(Some(head.clone()))),
            Action::Push(head)
        );
        run.forge_done(Ok(()));
        run.forge_done(Ok(()));
        run.status_checks_seen(Seen::Repair("Lint failed".into()));
        prompt_of(run.measured(mark("b", "d0")));
    }
    run.turn_ended(&facts("c4", false, 1, "d0"), "");
    run.command_finished(Ok(Some("c4".into())));
    run.forge_done(Ok(()));
    run.forge_done(Ok(()));
    assert!(matches!(
        run.status_checks_seen(Seen::Repair("Lint failed".into())),
        Action::Finish(Outcome::Exhausted { .. })
    ));
}

#[test]
fn a_merged_pull_request_is_done_and_a_failure_ends_the_run() {
    let mut run = at_status_checks();
    assert_eq!(
        run.status_checks_seen(Seen::Merged),
        Action::Finish(Outcome::Done)
    );

    let mut run = at_status_checks();
    assert_eq!(
        run.status_checks_seen(Seen::Fail("it was closed".into())),
        Action::Finish(Outcome::Failed("it was closed".into()))
    );
}

#[test]
fn a_stop_while_status_checks_run_ends_the_run_and_a_late_look_changes_nothing() {
    let mut run = at_status_checks();
    assert_eq!(
        run.stopped(Stop::ByPerson),
        Action::Finish(Outcome::Stopped(Stop::ByPerson))
    );
    assert_eq!(run.status_checks_seen(Seen::Passed), Action::Idle);
}

#[test]
fn a_retry_keeps_the_commit_that_was_checked() {
    let (mut run, _) = issue_past_check(true, "b");
    run.forge_done(Err("offline".into()));
    let template = run.template.clone();
    let mut again = Run::retry_of(&run, "2".into(), template, None);
    assert_eq!(again.resume(), Action::Push("b".into()));
}

#[test]
fn a_review_is_answered_at_the_step_status_checks_send_back_to() {
    assert_eq!(builtin::all().remove(2).repair_step(), Some("implement"));
    assert!(
        crate::unattended::parse_every(DEFAULT_WAIT).is_some(),
        "the default wait reads, or a step leaving it out waits on nothing"
    );
    assert_eq!(checkout().repair_step(), None);
}

#[test]
fn only_every_status_check_passing_on_the_pushed_commit_is_ready() {
    let hour = Duration::from_secs(3600);
    let minute = Duration::from_secs(60);
    let judged = |pr: &PullRequest, waited| judge(Ok(Some(pr)), Some("b"), waited, hour);
    let pr = |checks: &[(&str, CheckState)], conflicting| {
        pull_request(PrState::Open, checks, conflicting)
    };
    let passed = pr(&[("Build", CheckState::Passed)], false);
    assert_eq!(judged(&passed, minute), Seen::Passed);
    // Status checks on another head say nothing about what was pushed.
    let behind = PullRequest {
        head: "a".into(),
        ..passed.clone()
    };
    assert_eq!(judged(&behind, minute), Seen::Pending);
    assert!(matches!(judged(&behind, hour), Seen::Fail(_)));
    let running = pr(
        &[("Build", CheckState::Passed), ("Test", CheckState::Pending)],
        false,
    );
    assert_eq!(judged(&running, minute), Seen::Pending);
    assert!(matches!(judged(&running, hour), Seen::Fail(_)));
    // A failure wins over a check still running.
    let failing = pr(
        &[("Lint", CheckState::Failed), ("Test", CheckState::Pending)],
        false,
    );
    let Seen::Repair(said) = judged(&failing, minute) else {
        panic!("a failing check is to be repaired");
    };
    assert!(said.contains("Lint") && !said.contains("Test"), "{said}");
    let Seen::Repair(said) = judged(&pr(&[], true), minute) else {
        panic!("a conflict is to be repaired");
    };
    assert!(said.contains("conflicts"), "{said}");
    // No checks at all: none yet within the grace, none at all after it.
    let none = pr(&[], false);
    assert_eq!(judged(&none, minute), Seen::Pending);
    assert_eq!(
        judged(&none, status_checks::STATUS_CHECKS_GRACE),
        Seen::Passed
    );
    let merged = pull_request(PrState::Merged, &[], false);
    assert_eq!(judged(&merged, minute), Seen::Merged);
    let closed = pull_request(PrState::Closed, &[], false);
    assert!(matches!(judged(&closed, minute), Seen::Fail(why) if why.contains("closed")));
    let Seen::Fail(why) = judge(Ok(None), Some("b"), minute, hour) else {
        panic!("a pull request gone is the end");
    };
    assert!(why.contains("no pull request"), "{why}");
    // A forge that cannot be read is waited on, but never past the wait.
    assert_eq!(
        judge(Err("offline".into()), Some("b"), minute, hour),
        Seen::Pending
    );
    assert!(matches!(
        judge(Err("offline".into()), Some("b"), hour, hour),
        Seen::Fail(why) if why.contains("offline")
    ));
}

#[test]
fn a_wait_shorter_than_the_grace_still_lets_no_status_checks_pass() {
    let pr = pull_request(PrState::Open, &[], false);
    let wait = Duration::from_secs(300);
    assert_eq!(
        judge(Ok(Some(&pr)), Some("b"), Duration::from_secs(60), wait),
        Seen::Pending
    );
    assert_eq!(judge(Ok(Some(&pr)), Some("b"), wait, wait), Seen::Passed);
}

#[test]
fn a_log_holding_a_fence_never_closes_the_one_around_it() {
    let said = with_logs(
        "Lint failed.".into(),
        &[("Lint".into(), "before\n```\nafter\n````".into())],
    );
    assert!(
        said.starts_with("Lint failed.\n\nLint:\n\n`````\n"),
        "{said}"
    );
    assert!(said.ends_with("\n`````"), "{said}");
    assert_eq!(prompt::fenced("plain"), "```\nplain\n```");
}
