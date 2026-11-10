//! What a tool call is refused for before anything is done, given what the
//! window holds: a project on a host, a file outside the project on screen, a
//! task that is the caller's own or has nothing to stop.

use super::pick_project;
use crate::task::Group;
use std::path::{Component, Path, PathBuf};

/// Refuse `named` by name when it is no project here but one on a host or
/// offloaded to one, rather than say it does not exist.
pub fn hosted(
    here: &[(String, PathBuf)],
    hosted: &[(String, PathBuf)],
    named: &str,
) -> Result<(), String> {
    if pick_project(here, named).is_err() && pick_project(hosted, named).is_ok() {
        return Err(format!(
            "{named} runs its sessions on a host; onehand's tools do not reach it yet"
        ));
    }
    Ok(())
}

/// The file `open_file` names, relative to the caller's project when it is a
/// session's (`project`), inside the project on screen (`on_screen`, by its
/// name and path), and a file (`is_file`).
pub fn file_to_open(
    path: PathBuf,
    project: Option<&Path>,
    on_screen: Option<(&str, &Path)>,
    is_file: impl Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    // Relative to the calling session's project, which is where its agent
    // works; an outside program has no project to start from.
    let path = match (project, path.is_absolute()) {
        (_, true) => path,
        (Some(project), false) => project.join(path),
        (None, false) => return Err("give the file's absolute path".to_string()),
    };
    // Refused rather than resolved, so a path can never climb out of the
    // project it names by its first components.
    if path.components().any(|c| c == Component::ParentDir) {
        return Err("name the file without `..`".to_string());
    }
    let (name, root) = on_screen.ok_or("no project on this machine is on screen")?;
    // Another project's file is refused rather than switching projects under
    // the person.
    if !path.starts_with(root) {
        return Err(format!(
            "{} is not in {name}, the project on screen",
            path.display()
        ));
    }
    if !is_file(&path) {
        return Err(format!("{} is not a file", path.display()));
    }
    Ok(path)
}

/// Whether a tool may stop the task `title`: never the calling session's own
/// (`own`), an agent ending the run that watches it, and only one with
/// something to stop.
pub fn may_stop(title: &str, group: Group, own: bool, stoppable: bool) -> Result<(), String> {
    if own {
        return Err("that is this session's own task".to_string());
    }
    if !stoppable {
        return Err(format!(
            "{title} has nothing to stop: it is {}",
            group_said(group)
        ));
    }
    Ok(())
}

/// A task's group, as a caller reads it.
pub fn group_said(group: Group) -> &'static str {
    match group {
        Group::Waiting => "waiting on a person",
        Group::Ended => "ended",
        Group::Running => "running",
        Group::Queued => "queued",
        Group::Finished => "finished",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here() -> Vec<(String, PathBuf)> {
        vec![("app".into(), PathBuf::from("/repo/app"))]
    }

    #[test]
    fn a_hosted_project_is_refused_by_name_not_as_missing() {
        let on_host = vec![("far".into(), PathBuf::from("/srv/far"))];
        let why = hosted(&here(), &on_host, "far").unwrap_err();
        assert!(why.contains("runs its sessions on a host"), "{why}");
        assert!(hosted(&here(), &on_host, "app").is_ok());
        assert!(
            hosted(&here(), &on_host, "nothing").is_ok(),
            "said missing later"
        );
    }

    #[test]
    fn a_relative_file_is_the_callers_and_must_stay_on_screen() {
        let screen = Some(("app", Path::new("/repo/app")));
        let yes = |_: &Path| true;
        assert_eq!(
            file_to_open("src/a.rs".into(), Some(Path::new("/repo/app")), screen, yes),
            Ok(PathBuf::from("/repo/app/src/a.rs"))
        );
        assert!(file_to_open("src/a.rs".into(), None, screen, yes).is_err());
        assert_eq!(
            file_to_open("/repo/app/../x".into(), None, screen, yes),
            Err("name the file without `..`".into())
        );
        assert!(file_to_open("/elsewhere/a.rs".into(), None, screen, yes)
            .unwrap_err()
            .contains("the project on screen"));
        assert!(
            file_to_open("/repo/app/dir".into(), None, screen, |_: &Path| false)
                .unwrap_err()
                .ends_with("is not a file")
        );
    }

    #[test]
    fn a_session_never_stops_its_own_task() {
        assert!(may_stop("t", Group::Running, true, true).is_err());
        assert_eq!(
            may_stop("t", Group::Finished, false, false),
            Err("t has nothing to stop: it is finished".into())
        );
        assert!(may_stop("t", Group::Running, false, true).is_ok());
    }
}
