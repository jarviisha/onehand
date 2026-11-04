use super::{add_refusal, busy_body};
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;

#[test]
fn add_refuses_until_both_fields_name_a_folder_from_the_root() {
    assert_eq!(add_refusal("", "/srv/app"), Some("Name the host"));
    assert_eq!(add_refusal("  ", "/srv/app"), Some("Name the host"));
    assert_eq!(add_refusal("box", ""), Some("Name the folder"));
    assert_eq!(add_refusal("box", " \t"), Some("Name the folder"));
    // The host resolves the path as written, so `~` and relative paths are
    // not folders it can find.
    assert_eq!(add_refusal("box", "~/app"), Some("The folder starts at /"));
    assert_eq!(add_refusal("box", "app"), Some("The folder starts at /"));
    assert_eq!(add_refusal("box", "/srv/app"), None);
    assert_eq!(add_refusal(" me@box ", " /srv/app "), None);
}

/// The question names the sessions it would stop by when they started, as
/// the host page lists them, five of them and then how many more.
#[test]
fn the_busy_question_names_five_sessions_by_when_they_started() {
    let host = HostName("box".into());
    let now = 10_000;
    // Started 3 minutes ago, then a second apart.
    let ids = |n: usize| -> Vec<HostSession> {
        (1..=n)
            .map(|i| HostSession(format!("{}-{i}", now - 180 + i as u64)))
            .collect()
    };
    let started = "a session started 2m ago";
    assert_eq!(
        busy_body(&host, &ids(2), now),
        format!(
            "box runs another build of the host service, and these sessions are mid-turn: \
             {started}, {started}. Restarting stops them."
        )
    );
    assert_eq!(
        busy_body(&host, &ids(8), now),
        format!(
            "box runs another build of the host service, and these sessions are mid-turn: \
             {started}, {started}, {started}, {started}, {started} and 3 more. \
             Restarting stops them."
        )
    );
    let fresh = [HostSession(format!("{now}-1"))];
    assert_eq!(
        busy_body(&host, &fresh, now),
        "box runs another build of the host service, and these sessions are mid-turn: \
         a session started just now. Restarting stops them."
    );
}
