use super::*;
use onehand_core::project::worktrees::WorktreeRow;
use onehand_core::project::{MadeBy, Project, ProjectId, Worktree};
use onehand_core::worktree::Listed;
use std::collections::HashMap;
use std::path::PathBuf;

/// `/r`, recording `/r-x` on `x` and `/r-gone` on `g`.
fn project() -> Project {
    let mut p = Project::new(ProjectId::mint(1), None, "/r".into());
    for (path, branch) in [("/r-x", "x"), ("/r-gone", "g")] {
        p.worktrees.push(Worktree {
            path: path.into(),
            branch: branch.into(),
            from: None,
            made_by: MadeBy::Split,
            offload: None,
        });
    }
    p
}

fn kinds(lines: &[Line]) -> Vec<(&str, Kind)> {
    lines.iter().map(|l| (l.branch.as_str(), l.kind)).collect()
}

#[test]
fn a_git_list_that_fails_keeps_the_recorded_rows() {
    let p = project();
    let failed = Err("not a git repository".to_string());
    let (lines, note) = lines(&p, Some(&failed), &HashMap::new());
    assert_eq!(
        kinds(&lines),
        [
            ("x", Kind::Recorded { sessions: 0 }),
            ("g", Kind::Recorded { sessions: 0 })
        ]
    );
    assert_eq!(
        note.as_deref(),
        Some("git's worktrees could not be read: not a git repository")
    );
    let (reading, note) = super::lines(&p, None, &HashMap::new());
    assert_eq!(reading.len(), 2, "drawn while git is read");
    assert_eq!(note.as_deref(), Some("Reading git's worktrees…"));
}

#[test]
fn the_worktrees_box_lists_recorded_missing_and_offered() {
    let p = project();
    let read = Ok(vec![
        WorktreeRow::Recorded {
            at: 0,
            on_disk: true,
        },
        WorktreeRow::Recorded {
            at: 1,
            on_disk: false,
        },
        WorktreeRow::Offered(Listed {
            path: "/r-y".into(),
            branch: Some("y".into()),
        }),
    ]);
    let sessions = HashMap::from([(PathBuf::from("/r-x"), 2)]);
    let (lines, note) = lines(&p, Some(&read), &sessions);
    assert_eq!(note, None);
    assert_eq!(
        kinds(&lines),
        [
            ("x", Kind::Recorded { sessions: 2 }),
            ("g", Kind::Missing),
            ("y", Kind::Offered)
        ]
    );
    assert_eq!(lines[2].folder, PathBuf::from("/r-y"));
}

/// The page reads git's worktrees of the project on screen: a checkout git
/// lists that the project does not record is offered.
#[gpui::test]
fn the_page_reads_git_for_its_worktrees(cx: &mut gpui::TestAppContext) {
    use crate::shell::rig;
    let dir = rig::folder("page-worktrees");
    let (handle, shell, cx) = rig::shell(cx, dir.join("tasks"), Vec::new());
    let (main, wt) = rig::repo_with_worktree(&dir);
    let project = Project::new(ProjectId::mint(913_001), None, main.clone());
    rig::keep_projects(handle, &shell, vec![project], cx);
    let at = main.clone();
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.open_root(at, window, cx)));
    let mut read = None;
    for _ in 0..300 {
        cx.run_until_parked();
        read = cx.update(|_, cx| {
            let chat = shell.read(cx).chat_pane().read(cx);
            chat.empty
                .as_ref()
                .filter(|page| page.path == main)
                .and_then(|page| page.worktrees.clone())
        });
        if read.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        read,
        Some(Ok(vec![WorktreeRow::Offered(Listed {
            path: wt,
            branch: Some("w".into())
        })]))
    );
    let _ = std::fs::remove_dir_all(dir);
}
