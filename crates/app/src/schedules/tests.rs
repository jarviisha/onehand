//! The clock's carrier against its contract: its first look comes after
//! the wait and each one after it, a file that cannot be read is said and
//! the clock goes on, a look that starts nothing leaves the file alone, and
//! a switch is written through the one writer and put back when it fails.

use super::{boot_at, listed, read, set_on};
use crate::state::Shared;
use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use gpui_component::Root;
use onehand_core::schedule::drive::{EVERY, FIRST_LOOK};
use onehand_core::schedule::{self, Missed, Overlap, Schedule, Starts};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Blank;

impl gpui::Render for Blank {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div()
    }
}

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-schedules-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("schedules.toml")
}

/// A schedule of every minute on a project no window holds.
fn every_minute(id: &str) -> Schedule {
    Schedule {
        id: id.into(),
        name: format!("Schedule {id}"),
        project: "/nowhere/onehand-test".into(),
        on: true,
        when: "* * * * *".into(),
        agent: None,
        mode: None,
        tools: Vec::new(),
        labels: Vec::new(),
        missed: Missed::RunOnce,
        overlap: Overlap::Queue,
        on_since: 1,
        last_fired: None,
        said: None,
        options: BTreeMap::new(),
        starts: Starts::Prompt {
            prompt: "look".into(),
        },
    }
}

/// The process's state and the clock on the schedules at `path`.
fn app(cx: &mut TestAppContext, path: &Path) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let path = path.to_path_buf();
    cx.update(|cx| {
        cx.set_global(Shared::for_test());
        boot_at(path, cx);
    });
}

/// Run what is under way, background reads and writes included.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    let writer = cx.update(|cx| Shared::global(cx).writer.clone());
    assert!(writer.flush(Duration::from_secs(5)));
    cx.run_until_parked();
}

/// The first look waits for the first window, and each later one comes a
/// minute after; a file that cannot be read is said, and the next look
/// reads it again.
#[gpui::test]
fn the_clock_looks_after_its_wait_and_says_a_file_it_cannot_read(cx: &mut TestAppContext) {
    let path = temp_file("read");
    std::fs::write(&path, "not [toml").unwrap();
    app(cx, &path);
    settle(cx);
    assert_eq!(cx.update(|cx| read(cx)), None, "nothing before the wait");
    cx.executor()
        .advance_clock(FIRST_LOOK + Duration::from_secs(1));
    settle(cx);
    assert!(matches!(cx.update(|cx| read(cx)), Some(Err(_))));
    schedule::save_blocking(&path, &[every_minute("a")]).unwrap();
    cx.executor().advance_clock(EVERY);
    settle(cx);
    assert_eq!(cx.update(|cx| read(cx)), Some(Ok(())));
    assert_eq!(cx.update(|cx| listed(cx).len()), 1);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A look that starts and skips nothing leaves the file as it was.
#[gpui::test]
fn a_look_that_moves_nothing_writes_nothing(cx: &mut TestAppContext) {
    let path = temp_file("still");
    schedule::save_blocking(&path, &[every_minute("a")]).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    app(cx, &path);
    cx.executor()
        .advance_clock(FIRST_LOOK + Duration::from_secs(1));
    settle(cx);
    assert_eq!(cx.update(|cx| read(cx)), Some(Ok(())));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A switch is shown at once and written to the file; one whose schedule
/// is gone from the file is said where it was pressed and put back.
#[gpui::test]
fn a_switch_is_written_and_put_back_when_it_fails(cx: &mut TestAppContext) {
    let path = temp_file("switch");
    schedule::save_blocking(&path, &[every_minute("a")]).unwrap();
    app(cx, &path);
    cx.executor()
        .advance_clock(FIRST_LOOK + Duration::from_secs(1));
    settle(cx);
    let window = cx.add_window(|window, cx| Root::new(cx.new(|_| Blank), window, cx));
    let cx = &mut VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| set_on("a", false, window, cx));
    assert!(!cx.update(|_, cx| listed(cx)[0].on), "shown at once");
    cx.run_until_parked();
    let writer = cx.update(|_, cx| Shared::global(cx).writer.clone());
    assert!(writer.flush(Duration::from_secs(5)));
    cx.run_until_parked();
    assert!(!schedule::load_blocking(&path).unwrap()[0].on, "written");
    // A write that cannot land: the switch goes back to what is kept.
    use std::os::unix::fs::PermissionsExt as _;
    let dir = path.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    cx.update(|window, cx| set_on("a", true, window, cx));
    assert!(cx.update(|_, cx| listed(cx)[0].on), "shown at once");
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(writer.flush(Duration::from_secs(5)));
    cx.run_until_parked();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        !cx.update(|_, cx| listed(cx)[0].on),
        "put back to what is kept"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
