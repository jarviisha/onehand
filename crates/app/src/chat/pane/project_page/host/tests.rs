use super::host_rows;
use onehand_core::host::wire::{HostSession, SessionInfo};
use std::path::Path;

fn info(id: &str, cwd: &str) -> SessionInfo {
    SessionInfo {
        session: HostSession(id.into()),
        cwd: cwd.into(),
        agent: "claude".into(),
        busy: false,
        ended: false,
    }
}

#[test]
fn host_sessions_are_listed_for_their_folder_newest_first() {
    let list = [
        info("100-1", "/srv/app"),
        info("300-2", "/srv/other"),
        info("200-3", "/srv/app"),
        info("200-4", "/srv/app"),
        info("50-5", "/srv/app"),
    ];
    let ids = |rows: &[&SessionInfo]| -> Vec<String> {
        rows.iter().map(|info| info.session.0.clone()).collect()
    };

    let (rows, left_out) = host_rows(&list, Path::new("/srv/app"), 50);
    assert_eq!(ids(&rows), ["200-4", "200-3", "100-1", "50-5"]);
    assert_eq!(left_out, 0);

    // The cap keeps the newest and counts the rest of this folder only.
    let (rows, left_out) = host_rows(&list, Path::new("/srv/app"), 2);
    assert_eq!(ids(&rows), ["200-4", "200-3"]);
    assert_eq!(left_out, 2);

    let (rows, left_out) = host_rows(&list, Path::new("/srv/none"), 2);
    assert!(rows.is_empty());
    assert_eq!(left_out, 0);
}
