//! The tasks' drive as the app carries it out: every start asked for, the
//! place each task waits for, and what an ended run still holds, written,
//! looked up, watched and said here, and decided in
//! `onehand_core::task::tasks`.

use super::{Tasks, with_drive};
use crate::chat::session::{ChatEvent, ChatSession};
use futures::channel::oneshot;
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Entity, Subscription, Task as GpuiTask};
use gpui_component::WindowExt as _;
use onehand_core::task::tasks::{Effect, Input, Placed, Signal};
use onehand_core::task::{Launch, Task, marks, queue};
use onehand_core::unattended::review::ReviewAsk;
use onehand_core::workflow::Failure;
use onehand_core::worktree::removal;

/// What watches a run that ended while its session's turn still goes: the
/// session's events, its release, and the drain limit's clock.
pub(super) struct Drain {
    _watch: Vec<Subscription>,
    _limit: GpuiTask<()>,
}

/// Keep `task`, new, and ask for its place from `window`: the one way in for
/// a task that did not exist before, whatever started it.
pub(crate) fn submit(task: Task, window: AnyWindowHandle, cx: &mut App) {
    let id = task.id.clone();
    super::add(task, cx);
    request(id, window, cx);
}

/// [`submit`], answering where the task landed once that is known: for a
/// caller that has to say so somewhere other than the window's toasts.
#[cfg(test)]
pub(crate) fn submit_told(
    task: Task,
    window: AnyWindowHandle,
    cx: &mut App,
) -> oneshot::Receiver<Placed> {
    let id = task.id.clone();
    let told = told(&id, cx);
    super::add(task, cx);
    request(id, window, cx);
    told
}

/// Start `launch` off screen as task `id`, from `window`, answering where it
/// landed once that is known.
pub(crate) fn start_off_screen(
    id: String,
    launch: Result<Launch, String>,
    window: AnyWindowHandle,
    cx: &mut App,
) -> oneshot::Receiver<Placed> {
    let told = told(&id, cx);
    with_drive(cx, |d| ((), d.start_off_screen(id, launch, window)));
    told
}

/// Where task `id` lands, once it is told.
fn told(id: &str, cx: &mut App) -> oneshot::Receiver<Placed> {
    let (tell, told) = oneshot::channel();
    cx.default_global::<Tasks>()
        .told
        .insert(id.to_string(), tell);
    told
}

/// Start task `id` once its place is free, saying in `window` what happens.
/// It starts in `window`, or, when it waited, in the window it was asked
/// from, if that is still open.
fn request(id: String, window: AnyWindowHandle, cx: &mut App) {
    super::with_joined(cx, |j| ((), j.request(&id, window)));
}

/// Task `id`, holding its place, could not start for `why`, of `kind`: the
/// drive says whether its run ends failed or its place only passes on.
pub(crate) fn not_started(id: String, why: String, kind: Failure, cx: &mut App) {
    with_drive(cx, |d| ((), d.not_started(&id, why, kind)));
}

/// Tell the tasks' drive what the world found, and carry out what it says.
fn feed(input: Input<AnyWindowHandle>, cx: &mut App) {
    with_drive(cx, |d| ((), d.handle(input)));
}

/// Do what the tasks' drive said, in order. `session` is the session of the
/// run that just ended, if that is what this answers.
pub(super) fn perform(
    effects: Vec<Effect<AnyWindowHandle>>,
    session: Option<&Entity<ChatSession>>,
    cx: &mut App,
) {
    for effect in effects {
        match effect {
            Effect::Save(task) => {
                let written = cx.global::<Tasks>().writer.save_said(*task);
                cx.spawn(async move |cx| {
                    let written = written.await;
                    cx.update(|cx| feed(Input::Saved(written), cx));
                })
                .detach();
            }
            Effect::SaveDismissed(task) => {
                let id = task.id.clone();
                let written = cx.global::<Tasks>().writer.save_said(*task);
                cx.spawn(async move |cx| {
                    let written = written.await;
                    cx.update(|cx| feed(Input::Dismissed { task: id, written }, cx));
                })
                .detach();
            }
            Effect::Remove(id) => cx.global::<Tasks>().writer.remove(id),
            Effect::DropMarks { dir, repo, id } => cx
                .background_executor()
                .spawn(async move {
                    let at = if dir.is_dir() { dir } else { repo };
                    if let Err(why) = marks::drop_blocking(&at, &id) {
                        eprintln!("onehand: the marks of an old task were not dropped: {why}");
                    }
                })
                .detach(),
            Effect::FindPlace { task, dir, window } => {
                cx.spawn(async move |cx| {
                    let place = cx
                        .background_executor()
                        .spawn(async move { queue::place_blocking(&dir) })
                        .await;
                    cx.update(|cx| {
                        let window_open =
                            crate::state::with_shell(window, cx, |_, _, _| ()).is_some();
                        feed(
                            Input::PlaceFound {
                                task,
                                place,
                                window,
                                window_open,
                            },
                            cx,
                        );
                    });
                })
                .detach();
            }
            Effect::Cut {
                task,
                launch,
                window,
            } => {
                let (repo, title) = (launch.repo().to_path_buf(), launch.title().to_string());
                cx.spawn(async move |cx| {
                    let made = cx
                        .background_executor()
                        .spawn(async move {
                            onehand_core::task::cut_blocking(
                                &repo,
                                &title,
                                crate::features::connectors(),
                            )
                        })
                        .await;
                    cx.update(|cx| {
                        feed(
                            Input::Cut {
                                task,
                                launch,
                                window,
                                made,
                            },
                            cx,
                        )
                    });
                })
                .detach();
            }
            Effect::Keep {
                task,
                launch,
                window,
            } => {
                let dir = launch.dir().to_path_buf();
                let open = crate::state::with_shell(window, cx, |shell, window, cx| {
                    shell.keep_project_quietly(dir, window, cx)
                })
                .is_some();
                feed(
                    Input::Kept {
                        task,
                        launch,
                        window,
                        open,
                    },
                    cx,
                );
            }
            Effect::ReadReview { task, window } => read_review(task, window, cx),
            Effect::ConfirmAnswer { task, window } => cx.defer(move |cx| {
                let Some(read) = super::take_review(&task, cx) else {
                    return;
                };
                crate::state::with_shell(window, cx, |shell, window, cx| {
                    shell.confirm_answer(task, read, window, cx)
                });
            }),
            Effect::JudgeRemoval {
                task,
                dir,
                repo,
                branch,
                forge,
                confirmed,
                window,
            } => judge_removal(task, dir, repo, branch, forge, confirmed, window, cx),
            Effect::ConfirmRemoval {
                task,
                facts,
                judged,
                window,
            } => cx.defer(move |cx| {
                crate::state::with_shell(window, cx, |shell, window, cx| {
                    shell.confirm_removal(task, *facts, judged, window, cx)
                });
            }),
            Effect::RemoveWorktree {
                task,
                repo,
                folder,
                branch,
                window,
            } => {
                // Not tied to the window: a removal that went is kept with
                // its task even when the window closed meanwhile.
                cx.spawn(async move |cx| {
                    let removed =
                        cx.background_executor()
                            .spawn({
                                let (folder, branch) = (folder.clone(), branch.clone());
                                async move {
                                    removal::remove_blocking(&repo, &folder, branch.as_deref())
                                }
                            })
                            .await;
                    let input = Input::WorktreeRemoved {
                        task,
                        window,
                        folder,
                        branch,
                        removed,
                    };
                    cx.update(|cx| feed(input, cx));
                })
                .detach();
            }
            Effect::Drive { task, window } => drive(task, window, cx),
            Effect::WatchDrain { task, limit } => watch_drain(task, limit, session, cx),
            Effect::Pin {
                task,
                dir,
                refs,
                from,
            } => {
                cx.spawn(async move |cx| {
                    let pinned = cx
                        .background_executor()
                        .spawn(async move { marks::pin_blocking(&dir, &refs) })
                        .await;
                    cx.update(|cx| feed(Input::Pinned { task, from, pinned }, cx));
                })
                .detach();
            }
            Effect::Defer(input) => cx.defer(move |cx| feed(input, cx)),
            Effect::Told { task, placed } => tell(&task, placed, cx),
            // Unattended runs' to act on: the link hands them over, and
            // never on to here.
            Effect::IssueEnded(_)
            | Effect::Deliver(_)
            | Effect::DeliverAll
            | Effect::Asked(_)
            | Effect::Ask { .. } => {}
            Effect::CloseSession { uid, window } => cx.defer(move |cx| {
                crate::state::with_shell(window, cx, |shell, window, cx| {
                    shell.close_finished(uid, window, cx)
                });
            }),
            Effect::Signal(signal) => say(signal, cx),
            Effect::Refresh => cx.refresh_windows(),
        }
    }
}

/// Read what answering task `task`'s review is judged on, its facts as the
/// app knows them now, off the UI thread; kept for the question, and what
/// it came to told to the drive.
fn read_review(task: String, window: AnyWindowHandle, cx: &mut App) {
    let ask = super::task(&task, cx)
        .ok_or_else(|| "The task is gone".to_string())
        .and_then(|kept| {
            let template = kept
                .runs
                .last()
                .map(|run| run.template.clone())
                .ok_or_else(|| "The task has never run".to_string())?;
            let facts = crate::unattended::task_facts(&kept, template, cx);
            ReviewAsk::of(&kept, facts, crate::features::connectors())
        });
    let ask = match ask {
        Ok(ask) => ask,
        Err(why) => {
            let read = Err(why);
            return feed(Input::ReviewRead { task, window, read }, cx);
        }
    };
    cx.spawn(async move |cx| {
        let read = cx
            .background_executor()
            .spawn(async move { ask.read_blocking() })
            .await;
        cx.update(|cx| {
            cx.default_global::<Tasks>()
                .reviews
                .insert(task.clone(), read);
            let read = Ok(());
            feed(Input::ReviewRead { task, window, read }, cx);
        });
    })
    .detach();
}

/// Judge removing task `task`'s worktree, asked in `window`: its top found
/// off the UI thread, what uses it read from every window, then the forge
/// and git asked off it; what it came to told to the drive, `None` when
/// `window` went meanwhile.
#[allow(clippy::too_many_arguments)]
fn judge_removal(
    task: String,
    dir: std::path::PathBuf,
    repo: std::path::PathBuf,
    branch: Option<String>,
    forge: Option<String>,
    confirmed: bool,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let folder = cx
            .background_executor()
            .spawn(async move { removal::top_blocking(&dir) })
            .await;
        let asked = cx.update(|cx| {
            let (users, processes) = crate::state::with_shell(window, cx, |shell, _, cx| {
                shell.folder_users(&folder, cx)
            })?;
            Some(removal::Asked {
                repo: repo.clone(),
                folder,
                branch,
                forge: forge.as_deref().and_then(|name| {
                    onehand_core::connector::named(crate::features::connectors(), name)
                }),
                users,
                processes,
            })
        });
        let judged = match asked {
            Some(asked) => {
                let (facts, judged) = cx
                    .background_executor()
                    .spawn(async move { asked.judge_blocking() })
                    .await;
                Some((Box::new(facts), judged))
            }
            None => None,
        };
        let input = Input::Judged {
            task,
            confirmed,
            window,
            repo,
            judged,
        };
        cx.update(|cx| feed(input, cx));
    })
    .detach();
}

/// Answer whoever asked where task `id` landed, if anybody did.
fn tell(id: &str, placed: Placed, cx: &mut App) {
    if let Some(tell) = cx.update_global::<Tasks, _>(|t, _| t.told.remove(id)) {
        let _ = tell.send(placed);
    }
}

/// Drive task `id`, which holds its place, in `window`. Its refusal is said
/// in the window, and comes back to the drive as a start that failed.
fn drive(id: String, window: AnyWindowHandle, cx: &mut App) {
    let task = id.clone();
    let driven = crate::state::with_shell(window, cx, |shell, window, cx| {
        shell.drive_task(task, window, cx)
    });
    match driven {
        Some(started) => feed(Input::Driven { task: id, started }, cx),
        None => feed(Input::NotDriven(id), cx),
    }
}

/// Tell the drive once task `id`'s session's turn is over, the session goes,
/// or `limit` runs out, whichever is first.
fn watch_drain(
    id: String,
    limit: std::time::Duration,
    session: Option<&Entity<ChatSession>>,
    cx: &mut App,
) {
    let drained = {
        let id = id.clone();
        move |cx: &mut App| {
            let id = id.clone();
            cx.defer(move |cx| {
                cx.update_global::<Tasks, _>(|t, _| t.draining.remove(&id));
                feed(Input::Drained(id), cx);
            });
        }
    };
    let Some(session) = session else {
        return drained(cx);
    };
    let late = drained.clone();
    let limit = cx.spawn(async move |cx| {
        cx.background_executor().timer(limit).await;
        cx.update(|cx| late(cx));
    });
    let on_event = drained.clone();
    let watch = cx.subscribe(session, move |_, event: &ChatEvent, cx| {
        use onehand_core::chat::drive::Signal as S;
        match event {
            ChatEvent::Session(S::TurnEnded | S::Disconnected(_)) => on_event(cx),
            ChatEvent::Session(
                S::Appended
                | S::AwaitingUser(_)
                | S::PickRefused(_)
                | S::CycleEnded
                | S::StopRefused(_)
                | S::SaveFailed(_)
                | S::CodeFailed(_)
                | S::RenameFailed(_),
            )
            | ChatEvent::OpenFile(_)
            | ChatEvent::Fork(_)
            | ChatEvent::HostSession(_)
            | ChatEvent::HostBusy
            | ChatEvent::RestartHost(_)
            | ChatEvent::Look(_) => {}
        }
    });
    let gone = cx.observe_release(session, move |_, cx| drained(cx));
    cx.update_global::<Tasks, _>(|t, _| {
        t.draining.insert(
            id,
            Drain {
                _watch: vec![watch, gone],
                _limit: limit,
            },
        );
    });
}

/// Say `signal` where it belongs, in this front end's words.
fn say(signal: Signal<AnyWindowHandle>, cx: &mut App) {
    let toast = |window: AnyWindowHandle, note, cx: &mut App| {
        let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
    };
    match signal {
        Signal::Queued { window, said } => toast(window, crate::toast::info(said), cx),
        // Deferred: a request is often made from inside that window's shell.
        Signal::Refused { window, why } => {
            cx.defer(move |cx| toast(window, crate::toast::warning(why), cx))
        }
        // Deferred, as a refusal is: a Resume is pressed inside that window.
        Signal::NotResumed { window, why } => cx.defer(move |cx| {
            toast(
                window,
                crate::toast::warning(format!("Not resumed: {why}")),
                cx,
            )
        }),
        Signal::NotRemoved { window, why } => {
            let why = format!("Not removed, as things stand now: {}", why.join(" "));
            toast(window, crate::toast::warning(why), cx)
        }
        Signal::Removed {
            window,
            folder,
            branch,
            kept,
        } => {
            let folder = folder.display();
            let note = match (branch, kept) {
                (Some(branch), Some(why)) => crate::toast::warning(format!(
                    "Removed {folder}; the branch {branch} was kept: {why}"
                )),
                (Some(branch), None) => {
                    crate::toast::success(format!("Removed {folder} and {branch}"))
                }
                (None, _) => crate::toast::success(format!("Removed {folder}")),
            };
            toast(window, note, cx)
        }
        Signal::ReviewNotRead { window, why } => {
            toast(window, crate::toast::failed("Reading the review", why), cx)
        }
        Signal::RemoveFailed { window, why } => toast(
            window,
            crate::toast::failed("Removing the worktree", why),
            cx,
        ),
        Signal::CheckEnded {
            window,
            project,
            passed,
        } => {
            let note = match passed {
                true => crate::toast::success(format!("Check passed in {project}")),
                false => crate::toast::warning(format!("Check failed in {project}")),
            };
            toast(window, note, cx);
        }
        // Ended with nobody necessarily watching: said on the desktop as well
        // as on the Tasks page.
        Signal::ScheduleEnded { name, said } => crate::chat::session::notify_scheduled(name, said),
        Signal::DismissFailed { window, why } => {
            toast(window, crate::toast::failed("Dismissing the task", why), cx)
        }
        Signal::SaveFailed(why) => eprintln!("onehand: could not write a task's file: {why}"),
        Signal::PinFailed(why) => eprintln!("onehand: a task's last mark was not pinned: {why}"),
    }
}
