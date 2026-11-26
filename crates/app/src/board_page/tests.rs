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

#[test]
fn a_task_offers_the_tasks_pages_acts() {
    let labels = |row: &Row| -> Vec<&'static str> {
        task_offers(row)
            .into_iter()
            .map(|act| act.map_or("Show task", |act| act.label()))
            .collect()
    };
    let mut ended = row(Group::Ended, "failed", Some("It failed"));
    ended.resumable = true;
    assert_eq!(labels(&ended), ["Show task", "Resume", "Retry…", "Dismiss"]);
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
    use onehand_core::config::PanelLayout;
    use onehand_core::project::{Board, Project, ProjectId};
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

    /// The selectors of the board's columns, left to right.
    fn columns(template: &Template) -> Vec<&'static str> {
        [
            "board-column-backlog".to_string(),
            "board-column-todo".to_string(),
        ]
        .into_iter()
        .chain(
            template
                .steps
                .iter()
                .map(|s| format!("board-column-step-{}", s.id)),
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
        let mut project = Project::new(ProjectId::mint(seed), None, home);
        project.board = Some(Board {
            workflow: team().id,
            team: 2,
            todo: Vec::new(),
        });
        let file = onehand_core::project::file::issues_file(&project.id);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let issues = serde_json::json!({
            "next": 2,
            "issues": [{"number": 1, "title": title, "body": "What it is for."}],
        });
        std::fs::write(&file, issues.to_string()).unwrap();
        keep_projects(handle, &shell, vec![project.clone()], cx);
        let id = project.id.clone();
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
                    bounds(cx, "board-issue-1"),
                    bounds(cx, "board-column-backlog-box"),
                    &format!("a long issue title at {w:?}, zoomed {steps}"),
                );
            }
        }
    }
}
