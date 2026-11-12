//! The Issues view's carrying of its drive's reads against its contract: a
//! read asked for runs off the UI thread and its answer reaches the drive
//! once, kept for the work it was about: what the work left, a pull
//! request, the page's lists of them and a branch looked up on its own.

use super::super::IssuesView;
use gpui::TestAppContext;
use onehand_core::connector::fake::Fake;
use onehand_core::connector::{Connector, PrState};
use onehand_core::issues::IssueKey;
use onehand_core::task::work::drive::LeftSeen;
use onehand_core::task::work::{IssueWork, PrSeen, issue_work};
use onehand_core::task::{Task, Working};
use onehand_core::workflow::{Brief, Setup, StepKind, StepSpec, Template};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

#[gpui::test]
fn what_the_work_left_is_read_off_the_ui_thread_and_kept_for_its_work(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = std::env::temp_dir().join(format!("onehand-issues-reads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (storage, project) = (dir.join("storage"), dir.join("project"));
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let task = Task::new(
        "t".into(),
        Template::blank("Work"),
        Brief {
            title: "t".into(),
            body: String::new(),
            instructions: None,
        },
        Setup {
            repo: project.clone(),
            dir: project.clone(),
            branch: None,
            agent: None,
            check: None,
            mode: None,
            forge: None,
        },
    );
    let key = IssueKey {
        file: onehand_core::issues::file_for(&storage, &project),
        number: 1,
    };
    let work = issue_work(key, [(&task, None, None)]).unwrap();

    let view = cx
        .update(|cx| IssuesView::new(&[], Rc::new(|_, _, _| {}), Rc::new(|_, _, _| {}), true, cx));
    view.update(cx, |view, cx| {
        view.set_storage(Some(storage.clone()), cx);
        view.set_root(&project, cx);
        view.state_for(&project, cx).unwrap().selected = Some(1);
        view.set_works(std::slice::from_ref(&work), &[PathBuf::new()], cx);
        view.refresh_left(cx);
        // Asked, not yet read: the read is off the UI thread.
        assert!(view.work.reading());
    });
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    view.read_with(cx, |view, _| {
        assert!(!view.work.reading());
        assert!(matches!(
            view.work.left_seen(&work.work),
            LeftSeen::Read(..)
        ));
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// The forge the pull request reads go to: it has one pull request, on the
/// branch the work is on, and its list of them was cut before it.
static FORGE: Fake = Fake {
    pulls: &[(BRANCH, PrState::Open, "https://forge.test/pr/1")],
    listed: Some((&[], true)),
    ..Fake::SERVING
};
static CONNECTORS: [&dyn Connector; 1] = [&FORGE];
const BRANCH: &str = "onehand/local-1";

/// A folder of the test's own, holding a workspace's storage and a project.
fn place(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("onehand-issues-prs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (storage, project) = (dir.join("storage"), dir.join("project"));
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    (dir, storage, project)
}

fn step(id: &str, kind: StepKind) -> StepSpec {
    StepSpec {
        id: id.into(),
        label: id.into(),
        kind,
    }
}

/// Issue `number`'s work on `project`: a run on the forge, past its pull
/// request and waiting on its checks.
fn waiting_on_checks(storage: &Path, project: &Path, number: u64) -> IssueWork {
    let mut template = Template::blank("Issue");
    template.steps = vec![
        step("push", StepKind::Push),
        step("pr", StepKind::PullRequest),
        step(
            "checks",
            StepKind::StatusChecks {
                on_fail: "push".into(),
                wait: "1h".into(),
            },
        ),
    ];
    let mut task = Task::new(
        format!("t{number}"),
        template,
        Brief {
            title: "t".into(),
            body: String::new(),
            instructions: None,
        },
        Setup {
            repo: project.to_path_buf(),
            dir: project.to_path_buf(),
            branch: Some(BRANCH.into()),
            agent: None,
            check: None,
            mode: None,
            forge: Some(FORGE.name.into()),
        },
    );
    let run = task.runs.last_mut().unwrap();
    run.step = 2;
    let key = IssueKey {
        file: onehand_core::issues::file_for(storage, project),
        number,
    };
    issue_work(key, [(&task, Some(Working::Running), None)]).unwrap()
}

/// Let what is read off the UI thread land.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn view(page: bool, cx: &mut TestAppContext) -> gpui::Entity<IssuesView> {
    cx.update(|cx| {
        IssuesView::new(
            &CONNECTORS,
            Rc::new(|_, _, _| {}),
            Rc::new(|_, _, _| {}),
            page,
            cx,
        )
    })
}

/// A pull request read asked for runs off the UI thread and lands once, kept
/// for the work it was about; one asked about another issue meanwhile is
/// never shown for it.
#[gpui::test]
fn a_pull_request_read_lands_once_for_the_work_it_was_about(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (dir, storage, project) = place("one");
    let (a, b) = (
        waiting_on_checks(&storage, &project, 1),
        waiting_on_checks(&storage, &project, 2),
    );
    let view = view(false, cx);
    view.update(cx, |view, cx| {
        view.set_storage(Some(storage.clone()), cx);
        view.set_root(&project, cx);
        view.set_works(&[a.clone(), b.clone()], &[PathBuf::new()], cx);
        view.state_for(&project, cx).unwrap().selected = Some(1);
        view.refresh(cx);
        assert!(matches!(view.work.pr_seen(Some(&a)), PrSeen::Unread));
        // Another issue opened before the first read lands.
        view.state_for(&project, cx).unwrap().selected = Some(2);
        view.refresh(cx);
    });
    settle(cx);
    view.read_with(cx, |view, _| {
        assert!(view.work.pr_value(Some(&a)).is_none());
        let read = view.work.pr_value(Some(&b)).and_then(|(pr, _)| pr.as_ref());
        assert_eq!(
            read.map(|pr| pr.url.as_str()),
            Some("https://forge.test/pr/1")
        );
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// The page reads each project's pull requests once, off the UI thread, and
/// looks up one by one what a cut list missed.
#[gpui::test]
fn the_page_reads_every_projects_pull_requests_and_looks_up_what_a_cut_missed(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (dir, storage, project) = place("page");
    let work = waiting_on_checks(&storage, &project, 1);
    let view = view(true, cx);
    view.update(cx, |view, cx| {
        view.set_storage(Some(storage.clone()), cx);
        view.set_projects(vec![(project.clone(), "project".into())], cx);
        view.set_root(&project, cx);
        let file = view.state_for(&project, cx).unwrap().file.clone();
        file.update(cx, |file, cx| file.load_if_stale(cx));
    });
    // The forge serving the project is found as its file is read.
    settle(cx);
    view.update(cx, |view, cx| {
        view.set_works(std::slice::from_ref(&work), &[PathBuf::new()], cx);
        view.read_prs(cx);
        assert!(view.work.prs_reading());
    });
    settle(cx);
    view.read_with(cx, |view, _| {
        assert!(!view.work.prs_reading());
        let reads = &view.work.prs().unwrap()[&project];
        assert!(reads.capped && reads.at.is_some());
        assert!(reads.by_branch.is_empty());
        let looked = reads.looked_up.get(BRANCH).and_then(Option::as_ref);
        assert_eq!(
            looked.map(|pr| pr.url.as_str()),
            Some("https://forge.test/pr/1")
        );
    });
    let _ = std::fs::remove_dir_all(&dir);
}
