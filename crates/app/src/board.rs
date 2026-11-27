//! The boards' carrier: what `onehand_core::board::drive::BoardDrive`
//! decides, carried out. It writes the board files, times the team's track,
//! reads a board's issues off the UI thread with what the tasks and the
//! library say beside them, starts the tasks the drive picked, and closes the
//! issues it names; which and when are the drive's.

use crate::state::Shared;
use crate::task::Placed;
use gpui::{App, BorrowAppContext as _};
use gpui_component::WindowExt as _;
use onehand_core::board;
use onehand_core::board::drive::{BoardWrite, Effect, Facts, Input, Signal};
use onehand_core::connector::Connector;
use onehand_core::issues::Issues;
use onehand_core::issues::drive::Change;
use onehand_core::project::ProjectId;
use onehand_core::task::{Grant, Origin, Starting};
use onehand_core::workflow::Template;
use std::path::{Path, PathBuf};

/// Hand `input` to the boards' drive and carry out what it answers.
fn handle(input: Input, cx: &mut App) {
    let now = onehand_core::Now::read();
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.boards.handle(input, now));
    carry(fx, cx);
}

/// Carry out the boards' effects, in order, each once.
pub(crate) fn carry(fx: Vec<Effect>, cx: &mut App) {
    for effect in fx {
        match effect {
            Effect::Write(write) => {
                let BoardWrite {
                    id,
                    pending,
                    board,
                    dir,
                    seq,
                } = *write;
                let written = Shared::global(cx).writer.write(move || {
                    board::file::save_blocking(&dir, board.as_ref()).map_err(|why| why.to_string())
                });
                cx.spawn(async move |cx| {
                    let result = written.await;
                    cx.update(|cx| {
                        handle(
                            Input::Written {
                                id,
                                pending,
                                seq,
                                result,
                            },
                            cx,
                        )
                    });
                })
                .detach();
            }
            Effect::Arm { token, after } => {
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(|cx| handle(Input::Elapsed { token }, cx));
                })
                .detach();
            }
            Effect::Changed(id) => {
                crate::projects::for_each_holding(&id, cx, |_, window, _| window.refresh());
            }
            Effect::Signal(Signal::SaveFailed { id, why }) => {
                let name = (Shared::global(cx).projects.get(&id))
                    .map_or_else(String::new, |project| project.name.clone());
                let what = format!("Saving the board of {name}");
                crate::projects::for_each_holding(&id, cx, move |_, window, cx| {
                    window.push_notification(crate::toast::failed(&what, why.clone()), cx);
                });
            }
            Effect::Signal(Signal::NotStarted { id, issue, why }) => {
                let note = crate::toast::warning(format!(
                    "Could not start #{issue} from the board: {why}"
                ));
                if let Some((window, _)) = holding(&id, cx) {
                    let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
                }
            }
            Effect::ReadIssues { id } => read_issues(id, cx),
            Effect::GivePlace { task } => crate::task::give_place(&task, cx),
            Effect::Start {
                id,
                issue,
                task,
                template,
                column,
            } => start(id, issue, task, template, column, cx),
            Effect::NextRun {
                id,
                issue,
                task,
                run,
                template,
                column,
            } => next_run(id, issue, task, run, template, column, cx),
            Effect::Held { tasks } => {
                crate::task::with_drive(cx, |d| {
                    ((), {
                        d.set_held(tasks);
                        Vec::new()
                    })
                });
            }
            Effect::CloseIssue { id, issue } => {
                if let Some((root, file)) = local(&id, cx) {
                    close_issue(&root, file, issue, cx);
                }
            }
        }
    }
}

/// Look at `project`'s board: what it starts and gives is the drive's.
pub(crate) fn look(project: &ProjectId, cx: &mut App) {
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.boards.look(project));
    carry(fx, cx);
}

/// Look at every board, once the tasks and the workflows are read.
pub(crate) fn pull_all(cx: &mut App) {
    let boards: Vec<ProjectId> = Shared::global(cx).boards.ids().cloned().collect();
    for project in &boards {
        look(project, cx);
    }
}

/// Write every board change still waiting on its burst, now.
pub(crate) fn flush(cx: &mut App) {
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.boards.flush());
    carry(fx, cx);
}

/// Task `task` of `project`'s board, on `issue`, ended its run `run` as
/// `outcome`.
pub(crate) fn ended(
    project: ProjectId,
    issue: u64,
    task: String,
    run: Option<String>,
    outcome: Option<onehand_core::workflow::Outcome>,
    cx: &mut App,
) {
    let ended = Input::Ended {
        id: project,
        issue,
        task,
        run,
        outcome,
    };
    handle(ended, cx);
}

/// Tell the boards the tasks as they now stand: each board takes its tasks
/// up the first time, and lets go of what is gone.
pub(crate) fn tasks_read(cx: &mut App) {
    let tasks = crate::task::board_seen(cx);
    handle(Input::TasksRead { tasks }, cx);
}

/// Take `issue` off `project`'s board, back to the Backlog, unless its task
/// works; what refuses it is said in `window`.
pub(crate) fn remove(
    project: &ProjectId,
    issue: u64,
    task: Option<&str>,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let working = task.is_some_and(|id| crate::task::is_working(id, cx));
    let removed =
        cx.update_global::<Shared, _>(|shared, _| shared.boards.remove(project, issue, working));
    match removed {
        Ok(fx) => carry(fx, cx),
        Err(why) => window.push_notification(
            crate::toast::failed(&format!("Removing #{issue} from the board"), why),
            cx,
        ),
    }
}

/// Read `id`'s issues off the UI thread, then what the tasks and the library
/// say of its board as the read lands, and report both.
fn read_issues(id: ProjectId, cx: &mut App) {
    let Some((root, file)) = local(&id, cx) else {
        let facts = Box::new(facts(&id, None, cx));
        let read = Err("the project is not on this machine".to_string());
        return handle(Input::IssuesRead { id, read, facts }, cx);
    };
    cx.spawn(async move |cx| {
        let at = root.clone();
        let read = cx
            .background_executor()
            .spawn(async move { read_blocking(&file, &at) })
            .await;
        cx.update(|cx| {
            let facts = Box::new(facts(&id, Some(&root), cx));
            handle(Input::IssuesRead { id, read, facts }, cx);
        });
    })
    .detach();
}

/// What the tasks and the library say of `id`'s board, at `root`.
fn facts(id: &ProjectId, root: Option<&Path>, cx: &App) -> Facts {
    let board = Shared::global(cx).boards.board(id);
    let workflows: Vec<String> = (board.into_iter())
        .flat_map(|board| board.columns.iter())
        .filter_map(|column| match &column.work {
            board::Work::Workflow { id } => Some(id.clone()),
            board::Work::Person => None,
        })
        .collect();
    let templates: std::collections::HashMap<String, Result<Template, String>> = (workflows.iter())
        .map(|w| (w.clone(), crate::control::template(w, cx)))
        .collect();
    let first = (templates.iter())
        .map(|(w, template)| {
            let room = match (root, template) {
                (Some(root), Ok(template)) => crate::task::room_at(root, template, 0, cx),
                (None, _) | (_, Err(_)) => None,
            };
            (w.clone(), room)
        })
        .collect();
    // A fresh id for each start a look may decide: as many as the team, and
    // one for each issue waiting in a column that starts on entry.
    let waiting = (board.into_iter())
        .flat_map(|board| board.columns.iter())
        .map(|column| column.entries.len())
        .sum::<usize>();
    let window = root.is_some_and(|root| !crate::windows::holding_root(root, cx).is_empty());
    Facts {
        templates,
        running: crate::task::board_running(id, cx),
        extends_waiting: crate::task::board_extends_waiting(id, cx),
        first,
        window,
        ids: (0..usize::from(board::model::MOST_TEAM) + waiting)
            .map(|_| onehand_core::task::new_id())
            .collect(),
    }
}

/// Start task `task` of `template` on `issue` of `project` in a window
/// holding the project, and report where it landed.
fn start(
    project: ProjectId,
    issue: u64,
    task: String,
    template: Template,
    column: String,
    cx: &mut App,
) {
    let Some((window, shell)) = holding(&project, cx) else {
        let landed = Input::Landed {
            id: project,
            issue,
            task,
            kept: false,
            result: Err("no window holds the project".to_string()),
        };
        return handle(landed, cx);
    };
    let root = (Shared::global(cx).projects.get(&project)).map(|p| p.path.clone());
    cx.spawn(async move |cx| {
        let id = task.clone();
        let started = async {
            let root = root.ok_or("the project is gone")?;
            let file = Some(onehand_core::project::file::issues_file(&project));
            let at = root.clone();
            let brief = cx
                .background_executor()
                .spawn(async move {
                    let forge = serving(&at);
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
                column: Some(column),
            };
            let (_, placed) =
                crate::control::start_task_as(id.clone(), window, shell, starting, cx).await?;
            match placed {
                Placed::Started | Placed::Queued(_) => Ok(()),
                Placed::Refused(why) => Err(why),
            }
        }
        .await;
        cx.update(|cx| {
            let kept = crate::task::task(&id, cx).is_some();
            let landed = Input::Landed {
                id: project,
                issue,
                task: id,
                kept,
                result: started,
            };
            handle(landed, cx);
        });
    })
    .detach();
}

/// Start run `run` of `template` on task `task`, which works `issue` of
/// `project` and entered column `column`, and report where it landed: on
/// the work as its last run left it, with what a new task of the board takes
/// now. Refused, said by its landing, when another branch is checked out.
#[allow(clippy::too_many_arguments)]
fn next_run(
    project: ProjectId,
    issue: u64,
    task: String,
    run: String,
    template: Template,
    column: String,
    cx: &mut App,
) {
    let refused = |why: String, cx: &mut App| {
        let landed = Input::Landed {
            id: project.clone(),
            issue,
            task: task.clone(),
            kept: true,
            result: Err(why),
        };
        handle(landed, cx);
    };
    let Some((window, shell)) = holding(&project, cx) else {
        return refused("no window holds the project".to_string(), cx);
    };
    let Some(last) = crate::task::task(&task, cx).and_then(|t| t.runs().last().cloned()) else {
        return refused("its task is gone".to_string(), cx);
    };
    let (dir, end) = (last.setup.dir.clone(), last.last_mark().map(str::to_string));
    let check = shell.read(cx).check_of(&last.setup.repo);
    cx.spawn(async move |cx| {
        let against = match end {
            Some(end) => cx
                .background_executor()
                .spawn(async move { onehand_core::task::marks::against_blocking(&dir, &end) })
                .await
                .ok(),
            None => None,
        };
        cx.update(|cx| {
            if let Some(why) = against.as_ref().and_then(|a| a.refuses_retry()) {
                let landed = Input::Landed {
                    id: project,
                    issue,
                    task,
                    kept: true,
                    result: Err(why),
                };
                return handle(landed, cx);
            }
            let current = crate::task::facts::now_for(check, cx);
            let made = crate::task::with_drive(cx, |d| {
                match d.next_run(&task, run, template, current, column, window) {
                    Ok(fx) => (Ok(()), fx),
                    Err(why) => (Err(why), Vec::new()),
                }
            })
            .unwrap_or_else(|| Err("no tasks are kept yet".to_string()));
            let landed = Input::Landed {
                id: project,
                issue,
                task,
                kept: true,
                result: made,
            };
            handle(landed, cx);
        });
    })
    .detach();
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

/// The first window holding `project`, and its shell.
fn holding(
    project: &ProjectId,
    cx: &App,
) -> Option<(gpui::AnyWindowHandle, gpui::Entity<crate::shell::Shell>)> {
    let (root, _) = local(project, cx)?;
    crate::windows::holding_root(&root, cx).into_iter().next()
}

/// Where project `project`'s board works and where it keeps its issues.
fn local(project: &ProjectId, cx: &App) -> Option<(PathBuf, PathBuf)> {
    board::local(Shared::global(cx).projects.get(project)?).ok()
}

/// The forge serving `root`, if any.
fn serving(root: &Path) -> Option<&'static dyn Connector> {
    let all = crate::features::connectors();
    onehand_core::connector::serving(all, root)
        .ok()
        .map(|at| all[at])
}

/// The issues in `file`, and the forge serving `root` if any. Blocking.
fn read_blocking(
    file: &Path,
    root: &Path,
) -> Result<(Issues, Option<&'static dyn Connector>), String> {
    let kept = onehand_core::issues::load_blocking(file)?;
    Ok((kept, serving(root)))
}

#[cfg(test)]
mod tests;
