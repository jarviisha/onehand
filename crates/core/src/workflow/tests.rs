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

#[test]
fn both_shipped_templates_parse_and_validate() {
    let all = builtin::all();
    assert_eq!(all.len(), 2);
    for template in &all {
        assert_eq!(validate(template), [], "{}", template.name);
    }
    assert_eq!(all[0].place, Place::Checkout);
    assert_eq!(all[1].place, Place::Worktree);
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
    assert_eq!(run.approved(), Action::Measure);
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
        run.command_finished(Ok("a".into())),
        Action::Finish(Outcome::Done)
    );
    assert_eq!(run.marks.verified_at.as_deref(), Some("a"));
    assert!(run.over());
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
    run.approved();
    prompt_of(run.measured(mark("a", "d0")));
    run.turn_ended(&facts("a", true, 0, "d1"), "");
    assert_eq!(
        run.command_finished(Err("test failed: x".into())),
        Action::Measure
    );
    assert_eq!(run.current().unwrap().id, "implement");
    let text = prompt_of(run.measured(mark("a", "d1")));
    assert!(text.contains("The check failed:\n\n```\ntest failed: x\n```"));
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
    assert_eq!(run.revised("Smaller, please.".into()), Action::Measure);
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
    assert_eq!(run.approved(), Action::Idle);
}

/// Finding #3: a resumed run keeps the mark its step was measured from, so
/// work done before the restart still counts.
#[test]
fn resume_keeps_where_the_step_started() {
    let mut run = at_approval();
    run.approved();
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
    waiting.approved();
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
        run.approved();
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
    run.approved();
    prompt_of(run.measured(mark("a", "d0")));
    assert!(matches!(
        run.turn_ended(&facts("a", true, 0, "d1"), ""),
        Action::Finish(Outcome::Failed(_))
    ));
}

#[test]
fn every_move_is_in_the_history() {
    let mut run = at_approval();
    run.approved();
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
    run.approved();
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
    assert_eq!(run.visits[0].result.as_deref(), Some("its gates held"));
    assert_eq!(run.visits[3].output.as_deref(), Some("test failed"));
    assert_eq!(run.visits[3].result.as_deref(), Some("the command failed"));
    assert!(run.visits[4].ended_at.is_none(), "the last is the open one");

    run.stopped(Stop::ByPerson);
    let said = Outcome::Stopped(Stop::ByPerson).said();
    assert_eq!(run.visits[4].result.as_deref(), Some(said.as_str()));
}

#[test]
fn resume_closes_the_cut_off_visit_and_opens_one_of_the_same_step() {
    let mut run = at_approval();
    // As a restart finds it: nothing ended the visit.
    let mut back: Run = serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
    assert_eq!(back.resume(), Action::AwaitApproval);
    assert_eq!(back.visits.len(), 3);
    assert_eq!(back.visits[1].result.as_deref(), Some("interrupted"));
    assert_eq!(back.visits[2].step, "approve");

    // Stopped by its agent going: the visit was ended then, and resume opens
    // the next one.
    run.stopped(Stop::LinkLost);
    assert_eq!(run.resume(), Action::AwaitApproval);
    assert_eq!(run.visits.len(), 3);
    assert_ne!(run.visits[1].result.as_deref(), Some("interrupted"));
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
    let first = store::save_blocking(&dir, None, &checkout()).unwrap();
    let second = store::save_blocking(&dir, None, &checkout()).unwrap();
    assert_eq!(first, dir.join("work-in-checkout.toml"));
    assert_eq!(second, dir.join("work-in-checkout-2.toml"));
    let mut renamed = checkout();
    renamed.name = "Renamed".into();
    assert_eq!(
        store::save_blocking(&dir, Some(&first), &renamed).unwrap(),
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
