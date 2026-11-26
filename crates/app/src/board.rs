//! The board's carrier: a project's To do started in order as its team has
//! room, and the issue a board task finished closed. Which issues to start,
//! drop or close is `onehand_core::board`'s to say; this reads the issues
//! off the UI thread and carries out the answer.

use crate::state::Shared;
use crate::task::Placed;
use gpui::{App, BorrowAppContext as _};
use gpui_component::WindowExt as _;
use onehand_core::board;
use onehand_core::connector::Connector;
use onehand_core::issues::Issues;
use onehand_core::issues::drive::Change;
use onehand_core::project::ProjectId;
use onehand_core::task::{Grant, Origin, Starting};
use std::path::{Path, PathBuf};

/// Look at every board's To do, once the tasks and the workflows are read.
pub(crate) fn pull_all(cx: &mut App) {
    let boards: Vec<ProjectId> = Shared::global(cx)
        .projects
        .iter()
        .filter(|project| project.board().is_some())
        .map(|project| project.id.clone())
        .collect();
    for project in &boards {
        pull_now(project, cx);
    }
}

/// Start as many of `project`'s To do as its team has room for, first
/// first, and drop from To do what is closed or gone.
pub(crate) fn pull_now(project: &ProjectId, cx: &mut App) {
    // Extends waiting for a place are first in line, and need neither the
    // issues read nor a window.
    give_places(project, cx);
    let Some((root, file)) = local(project, cx) else {
        return;
    };
    let project = project.clone();
    cx.spawn(async move |cx| {
        let at = root.clone();
        let read = cx
            .background_executor()
            .spawn(async move { read_blocking(&file, &at) })
            .await;
        // Unread, To do stays as it is until the next look.
        if let Ok((kept, forge)) = read {
            cx.update(|cx| pull_read(&project, &root, &kept, forge, cx));
        }
    })
    .detach();
}

/// Give the places `project`'s team has to the extends waiting for one, first
/// asked first.
pub(crate) fn give_places(project: &ProjectId, cx: &mut App) {
    let running = crate::task::board_running(project, cx);
    let waiting = crate::task::board_extends_waiting(project, cx);
    let given = Shared::global(cx)
        .projects
        .extends_given(project, running, &waiting);
    for task in given {
        crate::task::give_place(&task, cx);
    }
}

/// A board task of `project` on `issue` ended, `done` or not: a finished
/// one closes an issue the project's own tracker keeps, and, when core says
/// to `pull`, the team's room goes to the next.
pub(crate) fn ended(project: &ProjectId, issue: u64, done: bool, pull: bool, cx: &mut App) {
    if done && let Some((root, file)) = local(project, cx) {
        cx.spawn(async move |cx| {
            let (at, read) = (root.clone(), file.clone());
            let close = cx
                .background_executor()
                .spawn(async move {
                    let (kept, forge) = read_blocking(&read, &at)?;
                    let synced = forge.is_some_and(|forge| kept.in_step_with(forge.name()));
                    let issue = kept.get(issue).ok_or("no such issue")?;
                    Ok::<_, String>(board::after_end(issue, synced, done))
                })
                .await;
            if close == Ok(true) {
                cx.update(|cx| close_issue(&root, file, issue, cx));
            }
        })
        .detach();
    }
    if pull {
        pull_now(project, cx);
    }
}

fn close_issue(root: &Path, file: PathBuf, number: u64, cx: &mut App) {
    let change = Change::SetOpen {
        number,
        open: false,
    };
    let at = root.to_path_buf();
    let answer = move |written: Result<_, String>, cx: &mut App| {
        if let Err(why) = written {
            let note = crate::toast::failed(&format!("Closing #{number} on the board"), why);
            if let Some((window, _)) = crate::windows::holding_root(&at, cx).into_iter().next() {
                let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
            }
        }
    };
    onehand_issues::write(
        root,
        file,
        change,
        crate::features::connectors(),
        answer,
        cx,
    );
}

/// Where project `project`'s board works and where it keeps its issues.
fn local(project: &ProjectId, cx: &App) -> Option<(PathBuf, PathBuf)> {
    board::local(Shared::global(cx).projects.get(project)?).ok()
}

/// The issues in `file`, and the forge serving `root` if any. Blocking.
fn read_blocking(
    file: &Path,
    root: &Path,
) -> Result<(Issues, Option<&'static dyn Connector>), String> {
    let kept = onehand_core::issues::load_blocking(file)?;
    let all = crate::features::connectors();
    let forge = onehand_core::connector::serving(all, root)
        .ok()
        .map(|at| all[at]);
    Ok((kept, forge))
}

/// The pull of `project` at `root`, over the issues as they were just read.
fn pull_read(
    project: &ProjectId,
    root: &Path,
    kept: &Issues,
    forge: Option<&'static dyn Connector>,
    cx: &mut App,
) {
    let Some(board) = Shared::global(cx)
        .projects
        .get(project)
        .and_then(|p| p.board())
        .cloned()
    else {
        return;
    };
    // Places freed while the issues were read go to waiting extends first.
    give_places(project, cx);
    // ponytail: with no window holding the project nothing is pulled until
    // To do or the team changes, a board task ends or the app starts again;
    // pull when a window opens on the project if that proves too late.
    let Some((window, shell)) = crate::windows::holding_root(root, cx).into_iter().next() else {
        return;
    };
    let Ok(template) = crate::control::template(&board.workflow, cx) else {
        return;
    };
    let running = crate::task::board_running(project, cx);
    let extends = crate::task::board_extends_waiting(project, cx).len();
    let taken = crate::task::board_taken(project, cx);
    let first = crate::task::room_at(root, &template, 0, cx);
    let Some(pull) = Shared::global(cx).projects.pull(
        project, &template, running, extends, first, kept, forge, &taken,
    ) else {
        return;
    };
    let fx = cx.update_global::<Shared, _>(|shared, _| {
        let mut fx = shared.projects.take_todo(project, &pull.gone);
        fx.extend(shared.projects.start_todo(project, &pull.start));
        fx
    });
    crate::projects::carry(fx, cx);
    for issue in pull.start {
        let project = project.clone();
        let (root, template, shell) = (root.to_path_buf(), template.clone(), shell.clone());
        cx.spawn(async move |cx| {
            let mut made = None;
            let started = async {
                let file = Some(onehand_core::project::file::issues_file(&project));
                let brief = cx
                    .background_executor()
                    .spawn(async move {
                        onehand_core::issues::supervised_brief_blocking(file, forge, issue)
                    })
                    .await?;
                let starting = Starting {
                    project: root,
                    template,
                    brief,
                    origin: Origin::Board {
                        project: project.clone(),
                        issue,
                    },
                    grant: Grant::default(),
                    agent: None,
                    mode: None,
                };
                let (id, placed) = crate::control::start_task(window, shell, starting, cx).await?;
                made = Some(id);
                match placed {
                    Placed::Started | Placed::Queued(_) => Ok(()),
                    Placed::Refused(why) => Err(why),
                }
            }
            .await;
            cx.update(|cx| {
                cx.update_global::<Shared, _>(|shared, _| shared.projects.landed(&project, issue));
                match started {
                    // Its task now counts itself, and any place left over
                    // from counting it twice meanwhile is filled.
                    Ok(()) => pull_now(&project, cx),
                    // Refused once its task was kept: the task holds the
                    // issue, and its own start already said why.
                    Err(_) if made.is_some_and(|id| crate::task::task(&id, cx).is_some()) => {}
                    // Out of To do with no task on it, the issue is back in
                    // the Backlog. Nothing is pulled in its place, so a
                    // reason every issue shares is said once, not per issue.
                    Err(why) => {
                        let note = crate::toast::warning(format!(
                            "Could not start #{issue} from the board: {why}"
                        ));
                        let _ =
                            window.update(cx, |_, window, cx| window.push_notification(note, cx));
                    }
                }
            });
        })
        .detach();
    }
}
