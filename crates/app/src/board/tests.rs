//! The boards' carrier: a write landing once and reaching the drive, a
//! failed one put back, and the team's track written at quit.

use crate::shell::rig::{folder, shell};
use crate::state::Shared;
use gpui::{BorrowAppContext as _, TestAppContext, VisualTestContext};
use onehand_core::board::Board;
use onehand_core::board::drive::BoardDrive;
use onehand_core::project::ProjectId;
use std::path::Path;

/// Run the loop until `done`, letting what runs off the UI thread land.
fn until(cx: &mut VisualTestContext, mut done: impl FnMut(&mut VisualTestContext) -> bool) {
    for _ in 0..300 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never came to pass");
}

/// A boards' drive writing into `store`, with project `id`'s board holding
/// `todo` written there.
fn boards(id: &ProjectId, store: &Path, todo: &[u64], cx: &mut VisualTestContext) {
    let (id, store) = (id.clone(), store.to_path_buf());
    let board = Board::made("builtin:branch".into(), 2, todo, 0);
    cx.update(|_, cx| {
        let fx = cx.update_global::<Shared, _>(|shared, _| {
            shared.boards = BoardDrive::new(Default::default(), Default::default(), store);
            shared.boards.set_board(&id, Some(board))
        });
        crate::board::carry(fx, cx);
    });
}

fn file_of(store: &Path, id: &ProjectId) -> String {
    std::fs::read_to_string(
        store
            .join(id.as_str())
            .join(onehand_core::board::file::FILE),
    )
    .unwrap_or_default()
}

fn todo(id: &ProjectId, cx: &mut VisualTestContext) -> Option<Vec<u64>> {
    cx.update(|_, cx| Shared::global(cx).boards.board(id).map(Board::todo))
}

#[gpui::test]
fn a_write_lands_once_and_reaches_the_drive(cx: &mut TestAppContext) {
    let dir = folder("board-write");
    let (_, _, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let (id, store) = (ProjectId::mint(61), dir.join("projects"));
    boards(&id, &store, &[], cx);
    cx.update(|_, cx| {
        let now = onehand_core::Now::read();
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.boards.add_todo(&id, 4, now));
        crate::board::carry(fx, cx);
    });
    until(cx, |_| file_of(&store, &id).contains("issue = 4"));
    assert_eq!(todo(&id, cx), Some(vec![4]));
    let waiting = cx.update(|_, cx| cx.update_global::<Shared, _>(|s, _| s.boards.flush().len()));
    assert_eq!(waiting, 0, "nothing waits to be written");
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn a_failed_write_reaches_the_drive(cx: &mut TestAppContext) {
    let dir = folder("board-write-fails");
    let (_, _, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let (id, store) = (ProjectId::mint(62), dir.join("projects"));
    boards(&id, &store, &[4], cx);
    until(cx, |_| file_of(&store, &id).contains("issue = 4"));
    // The project's folder made a file: no board can be written in it.
    std::fs::remove_dir_all(store.join(id.as_str())).unwrap();
    std::fs::write(store.join(id.as_str()), "").unwrap();
    cx.update(|_, cx| {
        let now = onehand_core::Now::read();
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.boards.add_todo(&id, 9, now));
        crate::board::carry(fx, cx);
    });
    assert_eq!(todo(&id, cx), Some(vec![4, 9]), "shown at once");
    until(cx, |cx| todo(&id, cx) == Some(vec![4]));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn quit_flushes_a_waiting_team(cx: &mut TestAppContext) {
    let dir = folder("board-flush");
    let (_, _, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let (id, store) = (ProjectId::mint(63), dir.join("projects"));
    boards(&id, &store, &[], cx);
    until(cx, |_| file_of(&store, &id).contains("team = 2"));
    cx.update(|_, cx| {
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.boards.set_team(&id, 6));
        crate::board::carry(fx, cx);
        crate::board::flush(cx);
    });
    until(cx, |_| file_of(&store, &id).contains("team = 6"));
    let _ = std::fs::remove_dir_all(dir);
}

/// What the boards hold reaches the tasks once they are read: a task they no
/// longer hold can be dismissed again.
#[gpui::test]
fn a_held_set_reaches_the_tasks(cx: &mut TestAppContext) {
    let dir = folder("board-held");
    let (handle, _, cx) = shell(cx, dir.join("tasks"), Vec::new());
    let (id, store) = (ProjectId::mint(64), dir.join("projects"));
    boards(&id, &store, &[], cx);
    let refused = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            crate::task::with_drive(cx, |d| {
                let fx = d.dismiss("gone", handle);
                (!fx.is_empty(), Vec::new())
            }) == Some(true)
        })
    };
    cx.update(|_, cx| {
        crate::task::with_drive(cx, |d| {
            d.set_held(["gone".to_string()].into_iter().collect());
            ((), Vec::new())
        })
    });
    assert!(refused(cx), "held, so refused");
    cx.update(|_, cx| crate::board::tasks_read(cx));
    cx.run_until_parked();
    assert!(!refused(cx), "the boards hold nothing now");
    let _ = std::fs::remove_dir_all(dir);
}
