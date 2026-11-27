use super::*;
use crate::board::model::Board;
use std::path::PathBuf;

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
    assert!(!boards.contains_key(&id(3)));
    assert!(problems.contains_key(&id(2)));
    assert_eq!(problems.len(), 1);
}

mod migrate {
    use super::*;
    use crate::project::{Project, ProjectId};

    /// A project folder in `root` whose `project.toml` holds `board` when
    /// given one, the old way.
    fn kept(root: &Path, board: Option<crate::project::OldBoard>) -> (ProjectId, PathBuf) {
        let mut project = Project::new(ProjectId::mint(7), None, "/code/a".into());
        project.old_board = board;
        let dir = root.join(project.id.as_str());
        crate::project::file::save_to(&project, &dir).unwrap();
        (project.id, dir)
    }

    fn old(workflow: &str, team: u8, todo: &[u64]) -> Option<crate::project::OldBoard> {
        Some(crate::project::OldBoard {
            workflow: workflow.into(),
            team,
            todo: todo.to_vec(),
        })
    }

    fn project_text(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(crate::project::file::FILE)).unwrap()
    }

    #[test]
    fn the_key_becomes_a_file_then_goes() {
        let root = scratch("mig-key");
        let (_, dir) = kept(&root, old("w", 3, &[4]));
        assert!(migrate_blocking(&root).is_empty());
        match load_from(&dir) {
            BoardLoad::Found(board) => {
                assert_eq!(board.workflow(), "w");
                assert_eq!(board.team, 3);
                assert_eq!(board.todo(), [4]);
                assert!(board.adopts(), "the board's tasks are taken up once read");
            }
            BoardLoad::Missing | BoardLoad::Unreadable(_) => panic!("no board file"),
        }
        assert!(!project_text(&dir).contains("board"));
    }

    #[test]
    fn a_file_already_there_wins() {
        let root = scratch("mig-both");
        let (_, dir) = kept(&root, old("w", 3, &[4]));
        let file = Board::made("mine".into(), 5, &[9], 1);
        save_blocking(&dir, Some(&file)).unwrap();
        assert!(migrate_blocking(&root).is_empty());
        match load_from(&dir) {
            BoardLoad::Found(board) => assert_eq!(board, file),
            BoardLoad::Missing | BoardLoad::Unreadable(_) => panic!("file lost"),
        }
        assert!(!project_text(&dir).contains("board"));
    }

    #[test]
    fn a_failed_board_write_keeps_the_key() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("mig-fail");
        let (_, dir) = kept(&root, old("w", 3, &[4]));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Run as root, the folder takes writes anyway and there is nothing
        // to test.
        let probe = dir.join("probe");
        if std::fs::write(&probe, "").is_ok() {
            let _ = std::fs::remove_file(&probe);
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let problems = migrate_blocking(&root);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(project_text(&dir).contains("board"));
        assert!(!dir.join(FILE).exists());
    }

    #[test]
    fn a_later_project_write_keeps_an_unmoved_key() {
        let root = scratch("mig-later");
        let (_, dir) = kept(&root, old("w", 3, &[4]));
        let project = match crate::project::file::load_from(&dir) {
            crate::project::file::ProjectLoad::Found(project) => project,
            other => panic!("{other:?}"),
        };
        crate::project::file::save_to(&project, &dir).unwrap();
        assert!(project_text(&dir).contains("board"));
    }

    #[test]
    fn an_unreadable_board_file_is_left_and_the_key_kept() {
        let root = scratch("mig-unread");
        let (_, dir) = kept(&root, old("w", 3, &[4]));
        std::fs::write(dir.join(FILE), "schema_version = 99\n").unwrap();
        let problems = migrate_blocking(&root);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE)).unwrap(),
            "schema_version = 99\n"
        );
        assert!(project_text(&dir).contains("board"));
    }

    #[test]
    fn a_project_without_a_board_is_untouched() {
        let root = scratch("mig-none");
        let (_, dir) = kept(&root, None);
        let before = project_text(&dir);
        assert!(migrate_blocking(&root).is_empty());
        assert_eq!(project_text(&dir), before);
        assert!(!dir.join(FILE).exists());
    }
}

/// A board whose move out of its project failed is said to be unreadable,
/// not taken for none: a person binding one anew would start an empty To do
/// that the next start's move then puts over the old one.
#[test]
fn a_board_still_in_its_project_is_unreadable_not_missing() {
    let root = scratch("still-in-project");
    let mut project =
        crate::project::Project::new(crate::project::ProjectId::mint(8), None, "/code/a".into());
    project.old_board = Some(crate::project::OldBoard {
        workflow: "w".into(),
        team: 3,
        todo: vec![4],
    });
    let dir = root.join(project.id.as_str());
    crate::project::file::save_to(&project, &dir).unwrap();
    let (boards, unreadable) = load_all_blocking(&root);
    assert!(boards.is_empty());
    assert!(unreadable.contains_key(&project.id), "{unreadable:?}");
}

/// A version-two board is a board of this version in one shape: read, it
/// is of this version, so a column can be added to it and an issue moved
/// into its work column with no task yet.
#[test]
fn a_version_two_board_reads_as_this_version() {
    let dir = scratch("second");
    let mut old = made();
    old.schema_version = 2;
    save_blocking(&dir, Some(&old)).unwrap();
    let board = match load_from(&dir) {
        BoardLoad::Found(board) => board,
        BoardLoad::Missing | BoardLoad::Unreadable(_) => panic!("not read back"),
    };
    assert_eq!(board.schema_version, crate::board::model::SCHEMA_VERSION);
    let mut more = board.clone();
    more.columns.push(more.columns[0].clone());
    more.columns[2].id = "review".into();
    more.columns[2].name = "Review".into();
    more.columns[2].entries.clear();
    assert!(more.shape().is_ok(), "{:?}", more.shape());
}
