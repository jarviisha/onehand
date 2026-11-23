use super::*;

fn p(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[test]
fn a_send_is_taken_from_the_page_on_screen_once() {
    let mut starts = PageStarts::default();
    assert!(
        !starts.take(&p("/a"), Some(&p("/b"))),
        "a page left is not taken"
    );
    assert!(!starts.take(&p("/a"), None));
    assert!(starts.take(&p("/a"), Some(&p("/a"))));
    assert!(!starts.take(&p("/a"), Some(&p("/a"))), "one at a time");
    starts.gave_up(&p("/a"), "no".into());
    assert!(
        starts.take(&p("/a"), Some(&p("/a"))),
        "taken again once it ended"
    );
}

#[test]
fn an_agent_up_shows_its_session_where_the_person_stands() {
    let mut starts = PageStarts::default();
    starts.take(&p("/a"), Some(&p("/a")));
    assert_eq!(
        starts.started(&p("/a"), 7, "Claude"),
        [Effect::Busy {
            root: p("/a"),
            said: Some("Starting Claude…".into())
        }]
    );
    assert!(starts.link(7, Link::Connecting, None, true).is_empty());
    assert_eq!(
        starts.link(7, Link::Connected, None, true),
        [
            Effect::Busy {
                root: p("/a"),
                said: None
            },
            Effect::Sent { root: p("/a") },
            Effect::Show { uid: 7 }
        ]
    );
    assert!(!starts.waits_on(7));
    assert!(
        starts.take(&p("/a"), Some(&p("/a"))),
        "the page sends again"
    );
}

#[test]
fn a_person_gone_elsewhere_is_not_pulled_to_the_session() {
    let mut starts = PageStarts::default();
    starts.take(&p("/a"), Some(&p("/a")));
    starts.started(&p("/a"), 7, "Claude");
    let fx = starts.link(7, Link::Connected, None, false);
    assert!(!fx.contains(&Effect::Show { uid: 7 }));
}

#[test]
fn an_agent_never_up_leaves_no_session_and_says_why() {
    let mut starts = PageStarts::default();
    starts.take(&p("/a"), Some(&p("/a")));
    starts.started(&p("/a"), 7, "Claude");
    assert_eq!(
        starts.link(7, Link::Lost, Some("not found".into()), true),
        [
            Effect::Close { uid: 7 },
            Effect::Failed {
                root: p("/a"),
                why: "not found".into()
            }
        ]
    );
    assert!(
        starts.link(7, Link::Lost, None, true).is_empty(),
        "said once"
    );
}

/// A session closed while it waits ends its page's send: the page takes
/// words and sends again.
#[test]
fn a_session_closed_while_it_waits_frees_its_page() {
    let mut starts = PageStarts::default();
    starts.take(&p("/a"), Some(&p("/a")));
    starts.started(&p("/a"), 7, "Claude");
    assert_eq!(
        starts.closed(7),
        [Effect::Busy {
            root: p("/a"),
            said: None
        }]
    );
    assert!(!starts.waits_on(7));
    assert!(starts.take(&p("/a"), Some(&p("/a"))));
    assert!(starts.closed(8).is_empty(), "a session no page waits on");
}
