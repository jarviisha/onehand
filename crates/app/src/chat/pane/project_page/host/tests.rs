use super::{host_rows, place, watching_uid};
use crate::chat::conversation::OnHost;
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

#[test]
fn a_host_session_already_watched_in_its_folder_is_found() {
    let host = HostName("box".into());
    let id = HostSession("100-1".into());
    let on = |host: &HostName, id: Option<&str>| {
        Some(OnHost {
            host: host.clone(),
            session: id.map(|id| HostSession(id.into())),
        })
    };
    let (a, b, unknown) = (
        on(&host, Some("100-1")),
        on(&host, Some("200-2")),
        on(&host, None),
    );
    let other_host = on(&HostName("else".into()), Some("100-1"));
    let (app, srv) = (Path::new("/srv/app"), Path::new("/srv/other"));
    let convs = || {
        [
            (1, srv, a.as_ref()),
            (2, app, other_host.as_ref()),
            (3, app, unknown.as_ref()),
            (4, app, None),
            (5, app, b.as_ref()),
            (6, app, a.as_ref()),
        ]
    };
    assert_eq!(watching_uid(convs(), app, &host, &id), Some(6));
    assert_eq!(watching_uid(convs()[..5].to_vec(), app, &host, &id), None);
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
