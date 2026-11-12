use super::*;
use crate::chat::Away;
use crate::remote::{Announcement, Origin};

fn session(uid: u64, here: bool) -> RemoteSession {
    RemoteSession {
        uid,
        project: format!("project-{uid}"),
        conversation: None,
        agent: "agent".into(),
        state: None,
        here,
    }
}

fn line(text: &str) -> Input {
    Input::Message {
        chat: "7".into(),
        from: "7".into(),
        text: text.into(),
    }
}

/// What the effects send, as text.
fn sent(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|effect| match effect {
            Effect::Send(out) => Some(out.text.clone()),
            _ => None,
        })
        .collect()
}

fn bridge() -> RemoteDrive {
    RemoteDrive::new(vec!["7".into()])
}

/// A chat off the list hears nothing at all, whatever it sends.
#[test]
fn a_stranger_hears_nothing() {
    let mut drive = bridge();
    let open = [session(1, true)];
    for input in [
        Input::Message {
            chat: "9".into(),
            from: "9".into(),
            text: "/sessions".into(),
        },
        Input::Unreadable {
            chat: "9".into(),
            from: "9".into(),
        },
        Input::Pressed {
            chat: "9".into(),
            from: "9".into(),
            press_id: "p".into(),
            data: "x".into(),
        },
    ] {
        assert!(drive.handle(input, &open).is_empty());
    }
}

/// A prompt goes to the session the chat points at, and its answer is said
/// in the bridge's words; one pointed at nothing is told so.
#[test]
fn a_prompt_goes_where_the_chat_points() {
    let mut drive = bridge();
    let open = [session(1, true)];
    let fx = drive.handle(line("hello"), &open);
    assert!(sent(&fx)[0].starts_with("This chat isn't pointed at a session"));
    drive.handle(line("/use 1"), &open);
    let fx = drive.handle(line("hello"), &open);
    assert!(matches!(
        &fx[..],
        [Effect::Prompt { uid: 1, text, .. }] if text == "hello"
    ));
    for (handled, said) in [
        (Some(Handled::Sent), "Sent to 1."),
        (
            Some(Handled::Queued),
            "1 is mid-turn — this goes in the moment that one ends.",
        ),
        (
            Some(Handled::Refused("busy".into())),
            "1 didn't take it: busy",
        ),
    ] {
        let fx = drive.handle(
            Input::Prompted {
                chat: "7".into(),
                uid: 1,
                handled,
            },
            &open,
        );
        assert_eq!(sent(&fx), [said]);
    }
    let fx = drive.handle(
        Input::Prompted {
            chat: "7".into(),
            uid: 1,
            handled: None,
        },
        &open,
    );
    assert!(sent(&fx)[0].starts_with("Session 1 is gone."));
}

/// #308: a session whose project runs on a host, or is offloaded to one, is
/// not listed, cannot be pointed at or pressed, and nobody hears of it.
#[test]
fn a_session_not_running_here_is_out_of_reach() {
    let mut drive = bridge();
    let open = [session(1, true), session(2, false)];
    let listing = sent(&drive.handle(line("/sessions"), &open)).remove(0);
    assert!(listing.contains("1 · project-1"), "{listing}");
    assert!(!listing.contains("project-2"), "{listing}");
    let fx = drive.handle(line("/use 2"), &open);
    assert!(sent(&fx)[0].starts_with("There's no session 2."));
    let press = Press::Skip { uid: 2, item: 0 };
    let fx = drive.handle(
        Input::Pressed {
            chat: "7".into(),
            from: "7".into(),
            press_id: "p".into(),
            data: press.encode(),
        },
        &open,
    );
    assert!(!fx
        .iter()
        .any(|effect| matches!(effect, Effect::Press { .. })));
    assert_eq!(sent(&fx), ["Session 2 is gone."]);
    // Nor can a chat follow it, so its news reaches nobody.
    let fx = drive.handle(line("/follow 2"), &open);
    assert!(sent(&fx)[0].starts_with("There's no session 2."));
    let elsewhere = Origin {
        uid: 2,
        agent: "agent".into(),
        project: "p".into(),
        conversation: None,
    };
    assert!(drive
        .announce(&elsewhere, &Announcement::plain(Away::TurnEnded))
        .is_empty());
}

/// A stop from outside asks the session, and says what it did.
#[test]
fn a_stop_says_what_it_took_back() {
    let mut drive = bridge();
    let open = [session(1, true)];
    drive.handle(line("/use 1"), &open);
    assert!(matches!(
        &drive.handle(line("/stop"), &open)[..],
        [Effect::Stop { uid: 1, .. }]
    ));
    let fx = drive.handle(
        Input::Stopped {
            chat: "7".into(),
            uid: 1,
            stopped: Some(Stopped::Cancelled {
                taken_back: Some("next".into()),
            }),
        },
        &open,
    );
    assert_eq!(
        sent(&fx),
        ["Stopped 1. The prompt waiting behind it was taken back, not sent:\n\nnext"]
    );
    let fx = drive.handle(
        Input::Stopped {
            chat: "7".into(),
            uid: 1,
            stopped: Some(Stopped::Idle),
        },
        &open,
    );
    assert_eq!(sent(&fx), ["Nothing is running on 1."]);
}

/// A press is acknowledged and said; one from an older build says so.
#[test]
fn a_press_is_acknowledged_and_said() {
    let mut drive = bridge();
    let open = [session(1, true)];
    let press = Press::Skip { uid: 1, item: 3 };
    let fx = drive.handle(
        Input::Pressed {
            chat: "7".into(),
            from: "7".into(),
            press_id: "p".into(),
            data: press.encode(),
        },
        &open,
    );
    assert!(matches!(&fx[..], [Effect::Press { .. }]));
    let fx = drive.handle(
        Input::Answered {
            chat: "7".into(),
            press_id: "p".into(),
            uid: 1,
            said: Some("Skipped.".into()),
        },
        &open,
    );
    assert!(matches!(&fx[0], Effect::Ack { text, .. } if text == "Skipped."));
    assert_eq!(sent(&fx), ["Skipped."]);
    let fx = drive.handle(
        Input::Pressed {
            chat: "7".into(),
            from: "7".into(),
            press_id: "q".into(),
            data: "garbage".into(),
        },
        &open,
    );
    assert_eq!(sent(&fx), ["That button is from an older message."]);
}

/// Away is said by a chat and goes with the channel; a bridge whose channel
/// stopped tells nobody anything.
#[test]
fn away_goes_with_the_channel() {
    let mut drive = bridge();
    let open = [session(1, true)];
    drive.handle(line("/use 1"), &open);
    let fx = drive.handle(line("/away"), &open);
    assert!(matches!(fx[0], Effect::Away(true)));
    assert!(drive.away());
    let origin = Origin {
        uid: 1,
        agent: "agent".into(),
        project: "p".into(),
        conversation: None,
    };
    let news = Announcement::plain(Away::TurnEnded);
    assert_eq!(drive.announce(&origin, &news).len(), 1);
    let fx = drive.handle(Input::Disconnected, &open);
    assert!(matches!(&fx[..], [Effect::ShutDown]));
    assert!(!drive.away());
    assert!(drive.announce(&origin, &news).is_empty());
}

/// `/archive` scans off the UI thread and `/open` reopens what it listed,
/// pointing the chat at it.
#[test]
fn an_archive_is_scanned_then_opened() {
    let mut drive = bridge();
    assert!(sent(&drive.handle(line("/open 1"), &[]))[0].starts_with("Ask for /archive first"));
    assert!(matches!(
        &drive.handle(line("/archive"), &[])[..],
        [Effect::Scan { .. }]
    ));
    let row = ArchiveRow {
        root: "/repo".into(),
        project: "repo".into(),
        dir: "/store/c1".into(),
        agent: "agent".into(),
        title: "fix it".into(),
        updated: 0,
    };
    drive.handle(
        Input::Scanned {
            chat: "7".into(),
            found: vec![row.clone()],
            total: 1,
            now: crate::Now::read(),
        },
        &[],
    );
    let fx = drive.handle(line("/open 1"), &[]);
    let [Effect::Open { pick, .. }] = &fx[..] else {
        panic!("an open");
    };
    assert_eq!(pick.dir, row.dir);
    let open = [session(4, true)];
    let fx = drive.handle(
        Input::Opened {
            chat: "7".into(),
            pick: row,
            uid: Some(4),
        },
        &open,
    );
    assert_eq!(
        sent(&fx),
        ["Opened repo — fix it as session 4, and pointed this chat at it."]
    );
    assert_eq!(drive.bound("7"), Some(4));
}
