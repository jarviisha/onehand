use super::*;
use std::path::PathBuf;
use crate::board::model::Board;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-boardfile-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn made() -> Board {
    Board::made("builtin:board".into(), 3, &[4, 9], 100)
}

#[test]
fn a_board_round_trips() {
    let dir = scratch("round");
    save_blocking(&dir, Some(&made())).unwrap();
    match load_from(&dir) {
        BoardLoad::Found(board) => assert_eq!(board, made()),
        BoardLoad::Missing | BoardLoad::Unreadable(_) => panic!("not read back"),
    }
}

#[test]
fn none_removes_the_file() {
    let dir = scratch("none");
    save_blocking(&dir, Some(&made())).unwrap();
    save_blocking(&dir, None).unwrap();
    assert!(!dir.join(FILE).exists());
    save_blocking(&dir, None).unwrap();
    match load_from(&dir) {
        BoardLoad::Missing => {}
        BoardLoad::Found(_) | BoardLoad::Unreadable(_) => panic!("still there"),
    }
}

#[test]
fn a_newer_version_is_unreadable() {
    let dir = scratch("newer");
    let mut board = made();
    board.schema_version = 7;
    save_blocking(&dir, Some(&board)).unwrap();
    match load_from(&dir) {
        BoardLoad::Unreadable(why) => assert!(why.contains('7'), "{why}"),
        BoardLoad::Found(_) | BoardLoad::Missing => panic!("read a newer board"),
    }
}

#[test]
fn load_all_lists_boards_and_problems() {
    let root = scratch("all");
    let id = crate::project::ProjectId::mint;
    let (a, b, c) = (
        root.join(id(1).as_str()),
        root.join(id(2).as_str()),
        root.join(id(3).as_str()),
    );
    for dir in [&a, &b, &c] {
        std::fs::create_dir_all(dir).unwrap();
    }
    save_blocking(&a, Some(&made())).unwrap();
    std::fs::write(b.join(FILE), "not = [toml").unwrap();
    let (boards, problems) = load_all_blocking(&root);
    assert_eq!(boards.get(&id(1)), Some(&made()));
    assert!(boards.get(&id(3)).is_none());
    assert!(problems.contains_key(&id(2)));
    assert_eq!(problems.len(), 1);
}
