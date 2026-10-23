//! A task's place: asking for it, waiting for it, and letting it go, so one
//! task at a time works in a checkout and the next one waiting starts there.

use super::{Tasks, enforce_cap, resumable_run};
use crate::shell::Shell;
use crate::state::Shared;
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Context, Window};
use gpui_component::WindowExt as _;
use onehand_core::task::Task;
use onehand_core::task::{marks, queue};
use onehand_core::workflow::{Failure, Stop};

/// Keep `task`, new, and ask for its place from `window`: the one way in for
/// a task that did not exist before, whatever started it.
pub(crate) fn submit(task: Task, window: AnyWindowHandle, cx: &mut App) {
    let id = task.id.clone();
    super::add(task, cx);
    request(id, window, cx);
}

/// Run `f` on the shell of `window`, inside it: `None` when the window or its
/// shell has gone.
fn in_window<R>(
    window: AnyWindowHandle,
    cx: &mut App,
    f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) -> R,
) -> Option<R> {
    let shell = Shared::global(cx)
        .windows
        .iter()
        .find(|w| w.handle == window)?
        .shell
        .upgrade()?;
    window
        .update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| f(shell, window, cx))
        })
        .ok()
}

/// Start task `id` once its place is free, saying in `window` what happens:
/// at once when nobody works there, or after the task that does, saying so.
/// A task already running or waiting is left as it is. It starts in
/// `window`, or, when it waited, in the window it was asked from, if that is
/// still open.
pub(crate) fn request(id: String, window: AnyWindowHandle, cx: &mut App) {
    if let Some(why) = crate::unattended::over_cap(&id, cx) {
        // A retry's new run never began: dropped, the task is as it was.
        cx.update_global::<Tasks, _>(|t, _| {
            let Some(task) = t.task_mut(&id) else {
                return;
            };
            if task.drop_unbegun_retry() {
                t.save(&id);
            }
        });
        // Deferred: a request is often made from inside that window's shell.
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                window.push_notification(crate::toast::warning(why), cx)
            });
        });
        cx.refresh_windows();
        return;
    }
    let Some(dir) = cx
        .try_global::<Tasks>()
        .and_then(|t| t.task(&id))
        .map(|task| task.setup.dir.clone())
    else {
        crate::unattended::placed(&id, cx);
        return;
    };
    cx.spawn(async move |cx| {
        let place = cx
            .background_executor()
            .spawn(async move { queue::place_blocking(&dir) })
            .await;
        cx.update(|cx| {
            let asked = id.clone();
            let landed = in_window(window, cx, |shell, window, cx| {
                let free = cx.update_global::<Tasks, _>(|t, _| {
                    if t.busy(&id) {
                        return None;
                    }
                    let free = t.queue.ask(place.clone(), id.clone());
                    if !free {
                        t.asked_in.insert(id.clone(), window.window_handle());
                    }
                    Some(free)
                });
                // Asked for, or held already: an issue's run counts against
                // the cap from here as queued or running.
                crate::unattended::placed(&id, cx);
                match free {
                    Some(true) => shell.drive_task(id, window, cx),
                    Some(false) => {
                        let t = cx.global::<Tasks>();
                        let title = t
                            .task(&id)
                            .map_or_else(String::new, |task| task.brief.title.clone());
                        let at = place.file_name().map_or_else(
                            || place.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        let holder = t.queue.holder_of(&id).and_then(|holder| t.task(holder));
                        let said = onehand_core::task::queue::queued_said(
                            &title,
                            holder.map(|holder| holder.brief.title.as_str()),
                            &at,
                        );
                        window.push_notification(crate::toast::info(said), cx);
                        cx.refresh_windows();
                    }
                    None => {}
                }
            });
            // The window went while the place was looked up, so nothing asks
            // for it: the cap lets go here, and the task reads cut off, as
            // after a restart, rather than holding a slot until one.
            if landed.is_none() {
                crate::unattended::placed(&asked, cx);
            }
        })
    })
    .detach();
}

/// How long a cancelled turn is waited for before its place is given up anyway.
const DRAIN_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Task `id` ended in `session`: its place is given up once the work has
/// stopped, which is at once unless the session's turn is still going.
/// Marks from the `from`th boundary on are still to be pinned.
pub(super) fn ended(
    id: String,
    from: usize,
    session: Option<&gpui::Entity<crate::chat::session::ChatSession>>,
    cx: &mut App,
) {
    let Some(session) = session.filter(|s| s.read(cx).chat.busy) else {
        cx.defer(move |cx| freed(id, from, cx));
        return;
    };
    let drained = {
        let id = id.clone();
        move |cx: &mut App| {
            let id = id.clone();
            cx.defer(move |cx| {
                let waited = cx.update_global::<Tasks, _>(|t, _| t.draining.remove(&id));
                if waited.is_some() {
                    freed(id, from, cx);
                }
            });
        }
    };
    let on_event = drained.clone();
    // The second deadline. A turn that was cancelled ends when the agent says
    // so, and one that never does held this place, and a slot, for good.
    let late = drained.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(DRAIN_LIMIT).await;
        cx.update(|cx| late(cx));
    })
    .detach();
    let watch = cx.subscribe(
        session,
        move |_, event: &crate::chat::session::ChatEvent, cx| {
            use crate::chat::session::ChatEvent as E;
            match event {
                E::TurnEnded | E::Disconnected => on_event(cx),
                E::Appended | E::AwaitingUser(_) | E::OpenFile(_) | E::PickRefused(_) => {}
            }
        },
    );
    let gone = cx.observe_release(session, move |_, cx| drained(cx));
    cx.update_global::<Tasks, _>(|t, _| {
        t.draining.insert(id, vec![watch, gone]);
    });
}

/// Task `id` has stopped working: pin where its last visit left the work,
/// then give its place to whoever waits for it. An issue it works is told
/// how the run ended.
fn freed(id: String, from: usize, cx: &mut App) {
    let unpinned = cx.try_global::<Tasks>().and_then(|t| {
        let task = t.task(&id)?;
        let run = task.runs.last()?;
        let refs = marks::refs_from(&task.id, run, from);
        (!refs.is_empty()).then(|| (run.setup.dir.clone(), refs))
    });
    let Some((dir, refs)) = unpinned else {
        return let_go(id, cx);
    };
    cx.spawn(async move |cx| {
        let pinned = cx
            .background_executor()
            .spawn(async move { marks::pin_blocking(&dir, &refs) })
            .await;
        cx.update(|cx| {
            match pinned {
                Ok(commit) => cx.update_global::<Tasks, _>(|t, _| {
                    if let Some(run) = t.task_mut(&id).and_then(|task| task.runs.last_mut()) {
                        run.pinned(&commit, from);
                    }
                    t.save(&id);
                }),
                Err(why) => eprintln!("onehand: a task's last mark was not pinned: {why}"),
            }
            let_go(id, cx);
        });
    })
    .detach();
}

/// Task `id`, holding its place, could not start for `why`, of `kind`: its
/// run ends failed, its issue is told, and its place passes on.
pub(crate) fn fail(id: String, why: String, kind: Failure, cx: &mut App) {
    let Some(mut run) = resumable_run(&id, cx) else {
        return release(id, cx);
    };
    run.failed(why, kind);
    crate::unattended::keep(&id, &run, None, cx);
    cx.update_global::<Tasks, _>(|t, _| t.store_run(&id, run));
    let_go(id, cx);
}

/// Task `id` is over and its last mark pinned: tell an issue it works how it
/// ended, then give its place up.
fn let_go(id: String, cx: &mut App) {
    crate::unattended::ended(&id, cx);
    release(id, cx);
}

/// Give task `id`'s place up, and start the next task waiting for it in the
/// window it was asked from. One that cannot start there stays interrupted,
/// and the place passes on.
pub(crate) fn release(id: String, cx: &mut App) {
    let next = cx.update_global::<Tasks, _>(|t, _| {
        let next = t.queue.release(&id)?;
        let asked = t.asked_in.remove(&next);
        Some((next, asked))
    });
    enforce_cap(cx);
    cx.refresh_windows();
    let Some((next, asked)) = next else {
        return;
    };
    let started = asked.and_then(|window| {
        let next = next.clone();
        in_window(window, cx, |shell, window, cx| {
            shell.drive_task(next, window, cx)
        })
    });
    if started.is_none() {
        release(next, cx);
    }
}

/// A person pressed Stop on queued task `id`: it no longer waits. One that
/// never ran a step is over, stopped by a person, with no empty run kept;
/// one resumed goes back to how it was.
pub(crate) fn stop_queued(id: &str, cx: &mut App) {
    let dropped = cx.update_global::<Tasks, _>(|t, _| {
        if !t.queue.call_off(id) {
            return None;
        }
        t.asked_in.remove(id);
        let task = t.task_mut(id)?;
        let dropped = task.take_unbegun();
        t.save(id);
        dropped
    });
    // An issue whose run never began is told it was stopped, or it would
    // stay claimed with nothing after the claim.
    if let Some(mut run) = dropped {
        // Through the engine, so the stop is in the run's history as any is.
        run.stopped(Stop::ByPerson);
        crate::unattended::keep(id, &run, None, cx);
        crate::unattended::deliver(id.to_string(), cx);
    }
    enforce_cap(cx);
    cx.refresh_windows();
}
