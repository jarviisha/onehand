//! The clock schedules run by, as the app carries it: the timer, the file
//! read and written, and each start, done here and decided in
//! `onehand_core::schedule::drive`.
//!
//! Each look is given the file as it is, the first window holding each
//! schedule's project, which of the clock's last tasks still work, the
//! workflows on offer and the time; a start goes through what every task off
//! screen goes through (`control::start_task`). Every write of the file goes
//! through the one writer, so the clock's and a switch's land in the order
//! they were made.

use crate::control::Started;
use crate::state::Shared;
use gpui::{AnyWindowHandle, App, BorrowAppContext as _, Global};
use gpui_component::WindowExt as _;
use onehand_core::schedule::drive::{Effect, Input, ScheduleDrive, Signal};
use onehand_core::schedule::{self, Schedule};
use std::collections::HashSet;
use std::path::PathBuf;

/// The clock: its drive, the file it reads, and its timer.
struct Clock {
    drive: ScheduleDrive<AnyWindowHandle>,
    path: PathBuf,
    _tick: Option<gpui::Task<()>>,
}

impl Global for Clock {}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Start the clock on the schedules kept at their usual place.
pub(crate) fn boot(cx: &mut App) {
    boot_at(schedule::path(), cx);
}

/// Start the clock on the schedules kept at `path`.
fn boot_at(path: PathBuf, cx: &mut App) {
    let (drive, fx) = ScheduleDrive::new(now());
    cx.set_global(Clock {
        drive,
        path,
        _tick: None,
    });
    perform(fx, cx);
}

/// Give the clock `command`, and carry out what it says.
fn with_drive(
    cx: &mut App,
    command: impl FnOnce(&mut ScheduleDrive<AnyWindowHandle>) -> Vec<Effect<AnyWindowHandle>>,
) {
    if !cx.has_global::<Clock>() {
        return;
    }
    let fx = cx.update_global::<Clock, _>(|c, _| command(&mut c.drive));
    perform(fx, cx);
}

/// Tell the clock what the world found, now.
fn feed(input: Input<AnyWindowHandle>, cx: &mut App) {
    with_drive(cx, |d| d.handle(input, now(), &chrono::Local));
}

/// Do what the clock said, in order.
fn perform(effects: Vec<Effect<AnyWindowHandle>>, cx: &mut App) {
    for effect in effects {
        match effect {
            Effect::Arm(after) => {
                let tick = cx.spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(|cx| feed(Input::Elapsed, cx));
                });
                cx.update_global::<Clock, _>(|c, _| c._tick = Some(tick));
            }
            Effect::Read => read_file(cx),
            Effect::Start { starting, window } => start(*starting, window, cx),
            Effect::Write { looked } => {
                let path = cx.global::<Clock>().path.clone();
                let write = Shared::global(cx).writer.write(move || {
                    let mut kept = schedule::load_blocking(&path)?;
                    schedule::merge_looked(&mut kept, &looked);
                    schedule::save_blocking(&path, &kept).map(|()| kept)
                });
                cx.spawn(async move |cx| {
                    let written = write.await;
                    cx.update(|cx| feed(Input::Written(written), cx));
                })
                .detach();
            }
            Effect::Switch {
                id,
                on,
                since,
                window,
            } => {
                let path = cx.global::<Clock>().path.clone();
                let switch = {
                    let id = id.clone();
                    Shared::global(cx).writer.write(move || {
                        let mut all = schedule::load_blocking(&path)?;
                        let written = schedule::switch(&mut all, &id, on, since)
                            .and_then(|()| schedule::save_blocking(&path, &all));
                        // What the file holds, read back after a failure, in
                        // the same order, so the switch shows what is kept.
                        Ok(written
                            .err()
                            .map(|why| (why, schedule::load_blocking(&path).ok())))
                    })
                };
                cx.spawn(async move |cx| {
                    let (failed, kept) = match switch.await {
                        Ok(None) => (None, None),
                        Ok(Some((why, kept))) => (Some(why), kept),
                        Err(why) => (Some(why), None),
                    };
                    let input = Input::Switched {
                        id,
                        on,
                        window,
                        failed,
                        kept,
                    };
                    cx.update(|cx| feed(input, cx));
                })
                .detach();
            }
            Effect::Signal(Signal::NotStarted { name, why }) => {
                crate::chat::session::notify_scheduled(name, format!("did not start: {why}"))
            }
            Effect::Signal(Signal::SwitchFailed { window, on, why }) => {
                let doing = match on {
                    true => "Switching the schedule on",
                    false => "Switching the schedule off",
                };
                let _ = window.update(cx, |_, window, cx| {
                    window.push_notification(crate::toast::failed(doing, why), cx);
                });
            }
            Effect::Signal(Signal::Log(said)) => eprintln!("onehand: {said}"),
            Effect::Refresh => cx.refresh_windows(),
        }
    }
}

/// Read the file off the UI thread, then tell the clock what it holds with
/// what a look decides by, as they are when the read lands.
fn read_file(cx: &mut App) {
    let path = cx.global::<Clock>().path.clone();
    cx.spawn(async move |cx| {
        let all = cx
            .background_executor()
            .spawn(async move { schedule::load_blocking(&path) })
            .await;
        cx.update(|cx| {
            let holders = match &all {
                Ok(all) => holders(all, cx),
                Err(_) => Vec::new(),
            };
            let working: HashSet<String> = cx
                .global::<Clock>()
                .drive
                .last_tasks()
                .filter(|task| crate::task::is_working(task, cx))
                .map(str::to_string)
                .collect();
            let library = crate::workflow::templates(cx)
                .into_iter()
                .filter_map(|entry| entry.template.ok())
                .collect();
            let input = Input::Read {
                all,
                holders,
                working,
                library,
            };
            feed(input, cx);
        });
    })
    .detach();
}

/// The first window holding each schedule's project.
fn holders(all: &[Schedule], cx: &App) -> Vec<(PathBuf, AnyWindowHandle)> {
    all.iter()
        .filter_map(|s| {
            let window = Shared::global(cx).windows.iter().find(|w| {
                w.shell
                    .upgrade()
                    .is_some_and(|shell| shell.read(cx).holds_root(&s.project))
            })?;
            Some((s.project.clone(), window.handle))
        })
        .collect()
}

/// Start `starting` in `window`, as every task off screen starts, and tell
/// the clock where it landed.
fn start(starting: onehand_core::schedule::drive::Starting, window: AnyWindowHandle, cx: &mut App) {
    let shell = Shared::global(cx)
        .windows
        .iter()
        .find(|w| w.handle == window)
        .and_then(|w| w.shell.upgrade());
    let Some(shell) = shell else {
        return feed(Input::Started(Err("the window closed".to_string())), cx);
    };
    let started = Started {
        origin: starting.origin,
        grant: starting.grant,
        agent: starting.agent,
        mode: starting.mode,
    };
    cx.spawn(async move |cx| {
        let placed = crate::control::start_task(
            window,
            shell,
            starting.project,
            starting.template,
            starting.brief,
            started,
            cx,
        )
        .await;
        cx.update(|cx| feed(Input::Started(placed), cx));
    })
    .detach();
}

/// The schedules as the clock last found them.
pub(crate) fn listed(cx: &App) -> &[Schedule] {
    cx.try_global::<Clock>().map_or(&[], |c| c.drive.listed())
}

/// What the last read of the schedules came to: `None` before the first,
/// else why it failed.
pub(crate) fn read(cx: &App) -> Option<Result<(), String>> {
    cx.try_global::<Clock>()
        .and_then(|c| c.drive.read().cloned())
}

/// Switch schedule `id` on or off: at once on screen, then in the file as it
/// is, through the one writer, in the order the switches were made. A write
/// that fails puts back what the file holds, and says so in `window`.
pub(crate) fn set_on(id: &str, on: bool, window: &mut gpui::Window, cx: &mut App) {
    let handle = window.window_handle();
    with_drive(cx, |d| d.set_on(id, on, now(), handle));
}

/// What a save or a delete left in the file: the list the pages show, until
/// the next look reads it again.
pub(crate) fn put(all: Vec<Schedule>, cx: &mut App) {
    with_drive(cx, |d| d.put(all));
}

#[cfg(test)]
mod tests;
