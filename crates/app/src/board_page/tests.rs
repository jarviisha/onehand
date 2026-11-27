use super::{task_line, task_offers};
use crate::task::Row;
use onehand_core::task::Group;

fn row(group: Group, at: &str, waits: Option<&str>) -> Row {
    Row {
        id: "t".into(),
        title: "Fix it".into(),
        name: "from the board · Build".into(),
        at: at.into(),
        waits: waits.map(str::to_string),
        waits_for_room: false,
        project: "/p".into(),
        group,
        session: None,
        resumable: false,
        stoppable: false,
        stopping: false,
        extendable: false,
    }
}

#[test]
fn a_task_says_what_it_waits_on() {
    let approval = row(Group::Waiting, "Build", Some("Approving starts Review"));
    assert_eq!(
        task_line(&approval, "Build"),
        "Needs attention · Approving starts Review"
    );
    let card = row(
        Group::Waiting,
        "Build",
        Some("The agent asks something in its session"),
    );
    assert_eq!(
        task_line(&card, "Build"),
        "Needs attention · The agent asks something in its session"
    );
    let mut room = row(Group::Running, "Waiting for room at Review", None);
    room.waits_for_room = true;
    assert_eq!(task_line(&room, "Review"), "Waiting for room at Review");
    assert_eq!(
        task_line(&row(Group::Running, "Build", None), "Build"),
        "Running"
    );
    assert_eq!(
        task_line(&row(Group::Queued, "Build", None), "Gone"),
        "Queued · Build"
    );
    let failed = row(
        Group::Ended,
        "failed",
        Some("The agent of Build did not start"),
    );
    assert_eq!(
        task_line(&failed, "Build"),
        "Needs attention · The agent of Build did not start"
    );
}

/// A board's task offers what the Tasks page offers it, but Dismiss: the
/// board holds it, and it leaves by *Remove from board*.
#[test]
fn a_task_offers_the_tasks_pages_acts_but_dismiss() {
    let labels = |row: &Row| -> Vec<&'static str> {
        task_offers(row)
            .into_iter()
            .map(|act| act.map_or("Show task", |act| act.label()))
            .collect()
    };
    let mut ended = row(Group::Ended, "failed", Some("It failed"));
    ended.resumable = true;
    assert_eq!(labels(&ended), ["Show task", "Resume", "Retry…"]);
    let mut running = row(Group::Running, "Build", None);
    running.stoppable = true;
    assert_eq!(labels(&running), ["Show task", "Stop"]);
    assert_eq!(labels(&row(Group::Finished, "done", None)), ["Show task"]);
    let mut out = row(Group::Waiting, "Build", Some("Out of time at Build"));
    out.stoppable = true;
    out.extendable = true;
    assert_eq!(labels(&out), ["Show task", "Extend", "Stop"]);
}

mod strain {
    use super::super::column::COLUMN_W;
    use crate::shell::rig::strain::{
        NARROWEST, TO_150, WIDE, assert_inside, bounds, same, selector, unbroken, width,
    };
    use crate::shell::rig::{board_page, folder, keep_projects, nowhere, shell_on, zoom_pane};
    use gpui::{Entity, Pixels, TestAppContext, VisualTestContext};
    use onehand_core::board::Board;
    use onehand_core::config::PanelLayout;
    use onehand_core::project::{Project, ProjectId};
    use onehand_core::workflow::Template;
    use onehand_core::workspace::Workspace;
    use std::time::Duration;

    /// The shipped board workflow, which has more steps than a narrow pane
    /// holds side by side.
    fn team() -> Template {
        onehand_core::workflow::builtin::all()
            .into_iter()
            .find(|t| t.id == "builtin:board")
            .expect("the shipped board workflow")
    }

    /// A board of a column per step of the shipped board workflow, so it
    /// has more columns than a narrow pane holds side by side.
    fn wide(template: &Template) -> Board {
        let mut board = Board::made(template.id.clone(), 2, &[], 0);
        for step in &template.steps {
            board.columns.push(onehand_core::board::Column {
                id: step.id.clone(),
                name: step.label.clone(),
                work: onehand_core::board::Work::Person,
                starts: onehand_core::board::Starts::OnEntry,
                moves_on: onehand_core::board::MovesOn::ByPerson,
                limit: None,
                entries: Vec::new(),
            });
        }
        board
    }

    /// The selectors of the board's columns, left to right.
    fn columns(template: &Template) -> Vec<&'static str> {
        std::iter::once("board-column-backlog".to_string())
            .chain(
                wide(template)
                    .columns
                    .iter()
                    .map(|c| format!("board-column-{}", c.id)),
            )
            .chain(["board-column-Done".to_string()])
            .map(|id| selector(format!("{id}-box")))
            .collect()
    }

    /// A board on a project whose one open issue is titled `title`, beside
    /// the narrowest rail, drawn once its issue is read.
    fn board<'a>(
        name: &str,
        seed: u64,
        title: &str,
        cx: &'a mut TestAppContext,
    ) -> (Entity<crate::shell::Shell>, &'a mut VisualTestContext) {
        let dir = folder(name);
        let home = dir.join("p");
        std::fs::create_dir_all(&home).unwrap();
        let home = std::fs::canonicalize(home).unwrap();
        let mut ws = Workspace::seeded("/tmp");
        ws.layout.rail_w = PanelLayout::RAIL_MIN;
        let (handle, shell, cx) = shell_on(cx, dir.join("tasks"), vec![nowhere()], ws);
        cx.update(|_, cx| crate::workflow::offer(vec![team()], cx));
        let project = Project::new(ProjectId::mint(seed), None, home);
        let file = onehand_core::project::file::issues_file(&project.id);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let issues = serde_json::json!({
            "next": 2,
            "issues": [{"number": 1, "title": title, "body": "What it is for."}],
        });
        std::fs::write(&file, issues.to_string()).unwrap();
        keep_projects(handle, &shell, vec![project.clone()], cx);
        let id = project.id.clone();
        cx.update(|_, cx| {
            use gpui::BorrowAppContext as _;
            let board = wide(&team());
            let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
                shared.boards.set_board(&id, Some(board))
            });
            crate::board::carry(fx, cx);
        });
        cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show_board(id, window, cx)));
        for _ in 0..300 {
            cx.run_until_parked();
            if cx.debug_bounds("board-issue-1").is_some() {
                return (shell, cx);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the board never drew its issue");
    }

    fn across(shell: &Entity<crate::shell::Shell>, cx: &mut VisualTestContext) -> Pixels {
        let page = board_page(shell, cx);
        cx.update(|_, cx| page.read(cx).across_max())
    }

    #[gpui::test]
    fn columns_past_the_width_scroll_sideways_and_keep_their_width(cx: &mut TestAppContext) {
        let template = team();
        let (shell, cx) = board("strain-board-across", 914_001, "Notes", cx);
        let rem = cx.update(|window, _| window.rem_size());
        width(cx, NARROWEST);
        for column in columns(&template) {
            let drawn = bounds(cx, column);
            assert!(
                same(drawn.size.width, COLUMN_W.to_pixels(rem)),
                "{column} gave way to the pane: {drawn:?}"
            );
        }
        assert!(
            across(&shell, cx) > Pixels::ZERO,
            "the columns past the pane's width cannot be scrolled to"
        );
        width(cx, WIDE);
        let drawn = bounds(cx, "board-columns");
        let last = bounds(cx, columns(&template).last().copied().unwrap());
        let fits = last.right() <= drawn.right();
        assert_eq!(
            across(&shell, cx) > Pixels::ZERO,
            !fits,
            "the board scrolls sideways exactly when its columns do not fit"
        );
    }

    #[gpui::test]
    fn a_long_task_title_stays_inside_its_column(cx: &mut TestAppContext) {
        let (shell, cx) = board("strain-board-title", 914_002, &unbroken(), cx);
        for steps in [0, TO_150] {
            zoom_pane(&shell, steps, cx);
            for w in [NARROWEST, WIDE] {
                width(cx, w);
                assert_inside(
                    bounds(cx, "board-issue-1-title"),
                    bounds(cx, "board-column-backlog-box"),
                    &format!("a long issue title at {w:?}, zoomed {steps}"),
                );
            }
        }
    }
}

mod columns {
    use crate::board_page::column::column_count;
    use crate::shell::rig::{board_page, folder, keep_projects, nowhere, shell_on};
    use gpui::{BorrowAppContext as _, TestAppContext};
    use onehand_core::board::{Board, Column, Entry, MovesOn, Starts, Work};
    use onehand_core::project::{Project, ProjectId};
    use onehand_core::workspace::Workspace;
    use std::time::Duration;

    fn column(id: &str, name: &str, work: Work, starts: Starts) -> Column {
        Column {
            id: id.into(),
            name: name.into(),
            work,
            starts,
            moves_on: MovesOn::ByPerson,
            limit: None,
            entries: Vec::new(),
        }
    }

    /// To do, Plan (the branch workflow, started on a press, holding issue
    /// 4) and Review (a person's).
    fn board() -> Board {
        let mut board = Board::made("builtin:branch".into(), 2, &[], 0);
        board.columns[1] = column(
            "plan",
            "Plan",
            Work::Workflow {
                id: "builtin:branch".into(),
            },
            Starts::OnPress,
        );
        board.columns[1].entries.push(Entry {
            issue: 4,
            ..Entry::default()
        });
        board
            .columns
            .push(column("review", "Review", Work::Person, Starts::OnEntry));
        board
    }

    #[gpui::test]
    fn the_page_draws_the_boards_columns_in_order(cx: &mut TestAppContext) {
        let dir = folder("board-own-columns");
        let home = dir.join("p");
        std::fs::create_dir_all(&home).unwrap();
        let home = std::fs::canonicalize(home).unwrap();
        let (handle, shell, cx) = shell_on(
            cx,
            dir.join("tasks"),
            vec![nowhere()],
            Workspace::seeded("/tmp"),
        );
        let branch = onehand_core::workflow::builtin::all()
            .into_iter()
            .find(|t| t.id == "builtin:branch")
            .unwrap();
        cx.update(|_, cx| crate::workflow::offer(vec![branch], cx));
        let project = Project::new(ProjectId::mint(921), None, home);
        let file = onehand_core::project::file::issues_file(&project.id);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let issues = serde_json::json!({"next": 5, "issues": [{"number": 4, "title": "Four"}]});
        std::fs::write(&file, issues.to_string()).unwrap();
        keep_projects(handle, &shell, vec![project.clone()], cx);
        let id = project.id.clone();
        cx.update(|_, cx| {
            let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
                shared.boards.set_board(&id, Some(board()))
            });
            crate::board::carry(fx, cx);
        });
        cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show_board(id, window, cx)));
        for _ in 0..300 {
            cx.run_until_parked();
            if cx.debug_bounds("board-issue-4").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = board_page(&shell, cx);
        let x = |selector: &'static str, cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} drawn"))
                .origin
                .x
        };
        let order = [
            "board-column-backlog-box",
            "board-column-todo-box",
            "board-column-plan-box",
            "board-column-review-box",
            "board-column-Done-box",
        ];
        let xs: Vec<_> = order.iter().map(|s| x(s, cx)).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "left to right");
        assert!(
            cx.debug_bounds("board-issue-4-button").is_some(),
            "a row waiting for Start draws Start"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_column_with_a_limit_says_how_full_it_is() {
        let mut review = column("review", "Review", Work::Person, Starts::OnEntry);
        assert_eq!(column_count(&review), "0");
        review.limit = Some(2);
        review.entries.push(Entry::default());
        assert_eq!(column_count(&review), "1 of 2");
    }
}
