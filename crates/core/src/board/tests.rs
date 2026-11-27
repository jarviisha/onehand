use super::*;
use crate::task::Origin;
use crate::workflow::{Brief, Setup, Template};
use std::path::PathBuf;

fn board(team: u8, todo: &[u64]) -> Board {
    Board::made("builtin:branch".into(), team, todo, 0)
}

fn numbers(issues: Vec<&LocalIssue>) -> Vec<u64> {
    issues.iter().map(|i| i.number()).collect()
}

fn none() -> HashSet<u64> {
    HashSet::new()
}

#[test]
fn pull_fills_up_to_team() {
    let got = pull(&board(2, &[4, 3, 2]), 0, None, &five(), None, &none());
    assert_eq!(got.start, [4, 3]);
    assert!(got.gone.is_empty());
}

#[test]
fn pull_none_when_full() {
    let got = pull(&board(2, &[4, 3]), 2, None, &five(), None, &none());
    assert!(got.start.is_empty());
}

#[test]
fn pull_after_team_lowered() {
    let got = pull(&board(1, &[4, 3]), 3, None, &five(), None, &none());
    assert!(got.start.is_empty());
    assert!(got.gone.is_empty());
}

/// Only an issue that is closed or no longer there leaves To do; one that is
/// open but cannot be worked now, as a forge-synced one while the forge is
/// not found, waits in To do for the next look.
#[test]
fn pull_drops_only_closed_or_missing_issues() {
    let kept = issues(
        r#"{"next":5,"issues":[
        {"number":1,"title":"mine"},
        {"number":2,"title":"theirs","imported_from":"github"},
        {"number":3,"title":"closed","open":false}]}"#,
    );
    let got = pull(&board(4, &[3, 2, 9, 1]), 0, None, &kept, None, &none());
    assert_eq!(got.gone, [3, 9]);
    assert_eq!(got.start, [1]);
}

/// An issue a task of the board still holds is not started twice: it stays
/// in To do, skipped, until the task gives it up.
#[test]
fn pull_skips_an_issue_a_task_holds() {
    let got = pull(
        &board(2, &[4, 3, 2]),
        0,
        None,
        &five(),
        None,
        &HashSet::from([4]),
    );
    assert_eq!(got.start, [3, 2]);
    assert!(got.gone.is_empty(), "kept in To do");
}

/// The board's first step is limited and full: nothing is pulled, whatever
/// room the team has, until a task leaves it.
#[test]
fn pull_waits_for_room_at_the_first_step() {
    let full = pull(&board(4, &[4, 3]), 1, Some((1, 1)), &five(), None, &none());
    assert!(full.start.is_empty());
    let one = pull(&board(4, &[4, 3]), 1, Some((2, 1)), &five(), None, &none());
    assert_eq!(one.start, [4], "only as many as the step has room for");
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
    assert_eq!(
        why,
        "A board needs a workflow that works on a worktree: Implement on a branch works in the checkout"
    );
}

fn task(id: &str, project: &str, step: usize, outcome: Option<Outcome>) -> Task {
    let brief = Brief {
        title: id.into(),
        body: String::new(),
        instructions: None,
        closes: None,
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
    let mut lost = template.clone();
    lost.steps[0].id = "gone".into();
    let mut at_lost = task("5", "p", 0, None);
    at_lost.runs[0].template = lost;
    let all = [
        task("1", "p", 0, None),
        task("2", "p", 1, None),
        task("3", "p", 1, Some(Outcome::Done)),
        task("4", "p", 0, Some(Outcome::Stopped(Stop::ByPerson))),
        at_lost,
    ];
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
fn columns_order_done_newest_first() {
    let template = template(Place::Worktree);
    let all = [
        task("1000000000000", "p", 0, Some(Outcome::Done)),
        task("2000000000000", "p", 0, Some(Outcome::Done)),
    ];
    let got = columns(&template, all.iter(), &|_| None);
    assert_eq!(ids(&got.done), ["2000000000000", "1000000000000"]);
}

#[test]
fn columns_leave_out_dismissed_and_taken_over() {
    let template = template(Place::Worktree);
    let mut dismissed = task("1", "p", 0, Some(Outcome::Failed("x".into())));
    dismissed.dismissed = true;
    let all = [
        dismissed,
        task("2", "p", 0, Some(Outcome::Stopped(Stop::TakenOver))),
    ];
    let got = columns(&template, all.iter(), &|_| None);
    assert!(got.steps.iter().all(|c| c.tasks.is_empty()));
    assert!(got.done.is_empty());
}

#[test]
fn columns_keep_needs_attention_in_their_step() {
    let template = template(Place::Worktree);
    let all = [
        task("1", "p", 1, Some(Outcome::Failed("x".into()))),
        task("2", "p", 0, None),
    ];
    let got = columns(&template, all.iter(), &|_| None);
    assert_eq!(ids(&got.steps[0].tasks), ["2"]);
    assert_eq!(ids(&got.steps[1].tasks), ["1"]);
}

#[test]
fn cut_off_task_not_counted() {
    let tasks = [
        task("1", "p", 0, None),
        task("2", "p", 0, None),
        task("3", "q", 0, None),
    ];
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    let holds = |id: &str| id == "1" || id == "3";
    assert_eq!(running(tasks.iter(), &project, &holds), 1);
}

/// A board task out of time at rest, or waiting for a place, holds none.
#[test]
fn a_board_task_that_gives_its_place_is_not_running() {
    let tasks = [task("1", "p", 0, None), task("2", "p", 0, None)];
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    assert_eq!(running(tasks.iter(), &project, &|id| id == "1"), 1);
}

/// Places go to extends waiting, the one that asked first first, never past
/// the team.
#[test]
fn extends_waiting_take_places_oldest_first() {
    let waiting = ["a".to_string(), "b".to_string()];
    assert_eq!(extends_given(&board(2, &[]), 1, &waiting), ["a"]);
    assert_eq!(extends_given(&board(3, &[]), 0, &waiting), ["a", "b"]);
    assert!(extends_given(&board(2, &[]), 2, &waiting).is_empty());
}

/// The waiting extends of a project's board, by when each asked.
#[test]
fn extends_waiting_are_listed_by_when_they_asked() {
    let tasks = [
        task("1", "p", 0, None),
        task("2", "p", 0, None),
        task("3", "q", 0, None),
        task("4", "p", 0, None),
    ];
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    let asked = |id: &str| match id {
        "1" => Some(20),
        "2" => Some(10),
        "3" => Some(5),
        _ => None,
    };
    assert_eq!(extends_waiting(tasks.iter(), &project, &asked), ["2", "1"]);
}

#[test]
fn done_closes_only_local_issue() {
    let kept = issues(
        r#"{"next":3,"issues":[
        {"number":1,"title":"open"},{"number":2,"title":"closed","open":false}]}"#,
    );
    let open = kept.get(1).unwrap();
    assert!(after_end(open, false, true));
    assert!(
        !after_end(open, true, true),
        "the forge closes a synced issue"
    );
    assert!(
        !after_end(open, false, false),
        "a task that did not finish leaves it open"
    );
    assert!(!after_end(kept.get(2).unwrap(), false, true));
}

#[test]
fn taken_holds_the_issues_of_tasks_on_the_board() {
    let with = |id: &str, project: &str, issue: u64, outcome: Option<Outcome>| {
        let mut t = task(id, project, 0, outcome);
        t.origin = Some(Origin::Board {
            project: serde_json::from_str(&format!("\"{project}\"")).unwrap(),
            issue,
        });
        t
    };
    let mut dismissed = with("6", "p", 6, Some(Outcome::Failed("x".into())));
    dismissed.dismissed = true;
    let tasks = [
        with("1", "p", 1, None),
        with("2", "p", 2, Some(Outcome::Done)),
        with("3", "p", 3, Some(Outcome::Failed("x".into()))),
        with("4", "p", 4, Some(Outcome::Stopped(Stop::ByPerson))),
        with("5", "q", 5, None),
        dismissed,
    ];
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    let got = taken(tasks.iter(), &project, &|_| None);
    assert_eq!(got, HashSet::from([1, 2, 3]));
}

#[test]
fn a_board_works_only_on_a_project_here() {
    let mut project = Project::new(ProjectId::mint(1), None, "/code/a".into());
    let (root, file) = local(&project).unwrap();
    assert_eq!(root, PathBuf::from("/code/a"));
    assert_eq!(file, crate::project::file::issues_file(&project.id));
    project.host = Some(crate::workspace::HostName("vps".into()));
    assert_eq!(
        local(&project).unwrap_err(),
        "A board works on a project on this machine"
    );
}

/// A board's task out of time at rest waits for a place; one still holding
/// its place, or of no board, or whose project has no board now, goes on.
#[test]
fn an_extend_waits_for_a_place_only_on_a_board_and_at_rest() {
    let on_board = task("1", "p", 0, None);
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    assert_eq!(
        extend_way(&on_board, &|_| true, false),
        ExtendWay::WaitForPlace(project)
    );
    assert_eq!(extend_way(&on_board, &|_| true, true), ExtendWay::Now);
    assert_eq!(extend_way(&on_board, &|_| false, false), ExtendWay::Now);
}

/// Only an end that left the board pulls the next, done or stopped by a
/// person; one that failed would most likely fail the next the same way.
#[test]
fn pulls_next_only_after_an_end_that_left_the_board() {
    for (outcome, pull) in [
        (Outcome::Done, true),
        (Outcome::Failed("no".into()), false),
        (Outcome::Stopped(Stop::ByPerson), true),
        (Outcome::Stopped(Stop::TakenOver), true),
        (Outcome::Stopped(Stop::TimedOut), false),
        (Outcome::Stopped(Stop::LinkLost), false),
        (Outcome::Stopped(Stop::Closed), false),
        (
            Outcome::Exhausted {
                step: "Verify".into(),
            },
            false,
        ),
    ] {
        assert_eq!(pulls_next(Some(&outcome)), pull, "{outcome:?}");
    }
    assert!(!pulls_next(None));
}
