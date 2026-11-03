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

#[test]
fn the_busy_question_names_five_sessions_then_counts_the_rest() {
    let host = HostName("box".into());
    let ids =
        |n: usize| -> Vec<HostSession> { (1..=n).map(|i| HostSession(format!("s{i}"))).collect() };
    assert_eq!(
        busy_body(&host, &ids(2)),
        "box runs another build of the host service, and these sessions are mid-turn: \
         s1, s2. Restarting stops them."
    );
    assert_eq!(
        busy_body(&host, &ids(5)),
        "box runs another build of the host service, and these sessions are mid-turn: \
         s1, s2, s3, s4, s5. Restarting stops them."
    );
    assert_eq!(
        busy_body(&host, &ids(8)),
        "box runs another build of the host service, and these sessions are mid-turn: \
         s1, s2, s3, s4, s5 and 3 more. Restarting stops them."
    );
}
