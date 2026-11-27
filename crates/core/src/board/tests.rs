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

/// A board whose work column names `work` and whose Done names `done`.
fn holding(work: &[(&str, u64)], done: &[(&str, u64)]) -> Board {
    let entry = |&(task, issue): &(&str, u64)| Entry {
        issue,
        task: Some(task.into()),
        run: Some(task.into()),
        since: 0,
        done: false,
    };
    board(2, &[]).upgraded(
        work.iter().map(entry).collect(),
        done.iter().map(entry).collect(),
    )
}

#[test]
fn running_counts_only_entries_tasks() {
    let tasks = [
        task("1", "p", 0, None),
        task("2", "p", 0, None),
        task("3", "p", 0, None),
    ];
    let on = holding(&[("1", 1), ("2", 2)], &[]);
    let holds = |id: &str| id == "1" || id == "3";
    assert_eq!(running(tasks.iter(), &on, &holds), 1);
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

/// The waiting extends of a board, by when each asked.
#[test]
fn extends_waiting_are_listed_by_when_they_asked() {
    let tasks = [
        task("1", "p", 0, None),
        task("2", "p", 0, None),
        task("3", "p", 0, None),
        task("4", "p", 0, None),
    ];
    let asked = |id: &str| match id {
        "1" => Some(20),
        "2" => Some(10),
        "3" => Some(5),
        _ => None,
    };
    let on = holding(&[("1", 1), ("2", 2), ("4", 4)], &[]);
    assert_eq!(extends_waiting(tasks.iter(), &on, &asked), ["2", "1"]);
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
fn taken_holds_the_issues_entries_name_and_done_ones_off_it() {
    let with = |id: &str, project: &str, issue: u64, outcome: Option<Outcome>| {
        let mut t = task(id, project, 0, outcome);
        t.origin = Some(Origin::Board {
            project: serde_json::from_str(&format!("\"{project}\"")).unwrap(),
            issue,
        });
        t
    };
    let mut dismissed = with("6", "p", 6, Some(Outcome::Done));
    dismissed.dismissed = true;
    let tasks = [
        with("1", "p", 1, None),
        with("2", "p", 2, Some(Outcome::Done)),
        with("4", "p", 4, Some(Outcome::Stopped(Stop::ByPerson))),
        with("5", "q", 5, Some(Outcome::Done)),
        with("7", "p", 7, Some(Outcome::Failed("x".into()))),
        dismissed,
    ];
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    let on = holding(&[("1", 1), ("4", 4)], &[("8", 8)]);
    let got = taken(&on, tasks.iter(), &project);
    assert_eq!(
        got,
        HashSet::from([1, 4, 8, 2]),
        "a done issue fallen off Done stays out of the Backlog; one removed does not"
    );
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
/// its place, or off its board, goes on.
#[test]
fn an_extend_waits_for_a_place_only_on_a_board_and_at_rest() {
    let on_board = task("1", "p", 0, None);
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    assert_eq!(
        extend_way(&on_board, true, false),
        ExtendWay::WaitForPlace(project)
    );
    assert_eq!(extend_way(&on_board, true, true), ExtendWay::Now);
    assert_eq!(
        extend_way(&on_board, false, false),
        ExtendWay::Now,
        "off the board"
    );
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

/// Only the work column holds its tasks: one in Done is the person's.
#[test]
fn a_board_holds_only_its_work_columns_tasks() {
    let on = holding(&[("1", 1)], &[("2", 2)]);
    assert!(on.holds("1"));
    assert!(!on.holds("2"));
    assert!(!on.holds("3"));
}

/// An issue a task works in the first column is the board's, not the
/// Backlog's.
#[test]
fn taken_counts_every_column() {
    let mut on = holding(&[], &[]);
    on.columns[0].entries.push(Entry {
        issue: 6,
        task: Some("t".into()),
        ..Entry::default()
    });
    let project: ProjectId = serde_json::from_str("\"p\"").unwrap();
    let none: [Task; 0] = [];
    assert!(taken(&on, none.iter(), &project).contains(&6));
}
