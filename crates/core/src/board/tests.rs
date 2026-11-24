use super::*;
use crate::task::Origin;
use crate::workflow::{Brief, Setup, Template};
use std::path::PathBuf;

fn board(team: u8, todo: &[u64]) -> Board {
    Board {
        workflow: "builtin:branch".into(),
        team,
        todo: todo.to_vec(),
    }
}

fn numbers(issues: Vec<&LocalIssue>) -> Vec<u64> {
    issues.iter().map(|i| i.number()).collect()
}

#[test]
fn pull_fills_up_to_team() {
    let got = pull(&board(2, &[4, 9, 2]), 0, &|_| true);
    assert_eq!(got.start, [4, 9]);
    assert!(got.gone.is_empty());
}

#[test]
fn pull_none_when_full() {
    assert!(pull(&board(2, &[4, 9]), 2, &|_| true).start.is_empty());
}

#[test]
fn pull_after_team_lowered() {
    let got = pull(&board(1, &[4, 9]), 3, &|_| true);
    assert!(got.start.is_empty());
    assert!(got.gone.is_empty());
}

#[test]
fn pull_skips_closed_issue() {
    let got = pull(&board(2, &[4, 9]), 0, &|n| n != 4);
    assert_eq!(got.gone, [4]);
    assert_eq!(got.start, [9]);
}

fn issues(json: &str) -> Issues {
    serde_json::from_str(json).unwrap()
}

fn five() -> Issues {
    issues(
        r#"{"next":6,"issues":[
        {"number":1,"title":"a"},{"number":2,"title":"b"},{"number":3,"title":"c"},
        {"number":4,"title":"d"},{"number":5,"title":"e"}]}"#,
    )
}

#[test]
fn backlog_leaves_out_todo_and_taken() {
    let kept = five();
    let got = backlog(&kept, None, &board(2, &[2]), &HashSet::from([3]));
    assert_eq!(numbers(got), [5, 4, 1]);
}

#[test]
fn backlog_includes_issue_taken_from_todo_without_task() {
    let kept = five();
    let got = backlog(&kept, None, &board(2, &[]), &HashSet::new());
    assert_eq!(numbers(got), [5, 4, 3, 2, 1]);
}

#[test]
fn backlog_leaves_out_unworkable() {
    let kept = issues(
        r#"{"next":4,"issues":[
        {"number":1,"title":"mine"},
        {"number":2,"title":"theirs","imported_from":"github"},
        {"number":3,"title":"closed","open":false}]}"#,
    );
    let got = backlog(&kept, None, &board(2, &[]), &HashSet::new());
    assert_eq!(numbers(got), [1]);
}

fn template(place: Place) -> Template {
    let mut template = crate::workflow::builtin::all()
        .into_iter()
        .find(|t| t.id == "builtin:branch")
        .unwrap();
    template.place = place;
    template
}

#[test]
fn usable_refuses_checkout_and_missing() {
    assert!(usable(Some(&template(Place::Worktree))).is_ok());
    assert!(usable(None).unwrap_err().contains("gone"));
    let why = usable(Some(&template(Place::Checkout))).unwrap_err();
    assert!(why.starts_with("A board needs a workflow that works on a worktree"));
}

fn task(id: &str, project: &str, step: usize, outcome: Option<Outcome>) -> Task {
    let brief = Brief {
        title: id.into(),
        body: String::new(),
        instructions: None,
    };
    let setup = Setup {
        repo: PathBuf::from("/a"),
        dir: PathBuf::from("/a"),
        branch: None,
        agent: None,
        check: None,
        mode: None,
        forge: None,
    };
    let mut task = Task::new(id.into(), template(Place::Worktree), brief, setup);
    task.origin = Some(Origin::Board {
        project: serde_json::from_str(&format!("\"{project}\"")).unwrap(),
        issue: 1,
    });
    let run = task.runs.last_mut().unwrap();
    run.step = step;
    run.outcome = outcome;
    task
}

fn ids(tasks: &[&Task]) -> Vec<String> {
    tasks.iter().map(|t| t.id().to_string()).collect()
}

#[test]
fn columns_place_by_current_step() {
    let template = template(Place::Worktree);
    let all = [
        task("1", "p", 0, None),
        task("2", "p", 1, None),
        task("3", "p", 1, Some(Outcome::Done)),
        task("4", "p", 0, Some(Outcome::Stopped(Stop::ByPerson))),
        task("5", "p", 99, None),
    ];
    // Task 5's run is at a step the template does not have: build it by
    // pointing the run at a template with other steps.
    let mut all = all;
    let mut lost = template.clone();
    lost.steps[0].id = "gone".into();
    all[4].runs[0].template = lost;
    all[4].runs[0].step = 0;
    let working = |id: &str| (id == "1" || id == "2" || id == "5").then_some(Working::Running);
    let got = columns(&template, all.iter(), &working);
    assert_eq!(got.steps.len(), template.steps.len() + 1);
    assert_eq!(ids(&got.steps[0].tasks), ["1"]);
    assert_eq!(ids(&got.steps[1].tasks), ["2"]);
    assert_eq!(ids(&got.done), ["3"]);
    let last = got.steps.last().unwrap();
    assert_eq!((last.step.as_str(), last.label.as_str()), ("gone", "gone"));
    assert_eq!(ids(&last.tasks), ["5"]);
}

#[test]
fn cut_off_task_not_counted() {
    let tasks = [
        task("1", "p", 0, None),
        task("2", "p", 0, None),
        task("3", "q", 0, None),
    ];
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    let working = |id: &str| (id == "1" || id == "3").then_some(Working::Queued);
    assert_eq!(running(tasks.iter(), &project, &working), 1);
}
