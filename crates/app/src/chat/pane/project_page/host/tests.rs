use super::{host_rows, place};
use onehand_core::host::wire::{HostSession, SessionInfo};
use onehand_core::workspace::HostName;
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

/// A folder on a host is named on its host, never shortened with this
/// machine's home; one here is shortened with it.
#[test]
fn the_place_line_names_a_host_folder_on_its_host() {
    let home = Path::new("/home/me");
    let vps = HostName("vps".into());
    assert_eq!(
        place(Some(&vps), Path::new("/home/me/app"), Some(home)),
        "vps:/home/me/app"
    );
    assert_eq!(place(None, Path::new("/home/me/app"), Some(home)), "~/app");
    assert_eq!(place(None, Path::new("/srv/app"), Some(home)), "/srv/app");
    assert_eq!(place(None, Path::new("/srv/app"), None), "/srv/app");
}
