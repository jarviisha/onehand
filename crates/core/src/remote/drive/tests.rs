use super::*;
use crate::chat::Away;
use crate::remote::{Announcement, Origin, TokenSource};

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
        who: String::new(),
        text: text.into(),
    }
}

/// What the effects send, as text.
fn sent(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|effect| match effect {
            Effect::Send(out) => Some(out.text.clone()),
            Effect::Ack { .. }
            | Effect::ShutDown
            | Effect::Start
            | Effect::Connect(_)
            | Effect::Away(_)
            | Effect::Prompt { .. }
            | Effect::Stop { .. }
            | Effect::Options { .. }
            | Effect::Press { .. }
            | Effect::Scan { .. }
            | Effect::Open { .. } => None,
        })
        .collect()
}

/// Each effect by its name, with the fields the tests read.
fn shown(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .map(|effect| match effect {
            Effect::Send(_) => "Send".into(),
            Effect::Ack { text, .. } => format!("Ack {text}"),
            Effect::ShutDown => "ShutDown".into(),
            Effect::Start => "Start".into(),
            Effect::Connect(token) => format!("Connect {token}"),
            Effect::Away(on) => format!("Away {on}"),
            Effect::Prompt { uid, text, .. } => format!("Prompt {uid} {text}"),
            Effect::Stop { uid, .. } => format!("Stop {uid}"),
            Effect::Options { uid, .. } => format!("Options {uid}"),
            Effect::Press { .. } => "Press".into(),
            Effect::Scan { .. } => "Scan".into(),
            Effect::Open { pick, .. } => format!("Open {}", pick.dir.display()),
        })
        .collect()
}

/// A bridge for chat `7` whose channel is up.
fn bridge() -> RemoteDrive {
    let mut drive = RemoteDrive::new(vec!["7".into()], true);
    drive.start();
    drive.handle(looked(Some("t")), &[]);
    drive.handle(
        Input::Connected {
            name: "@bot".into(),
        },
        &[],
    );
    drive
}

fn looked(token: Option<&str>) -> Input {
    Input::Looked {
        source: match token {
            Some(_) => TokenSource::File("/tg".into()),
            None => TokenSource::Missing,
        },
        token: token.map(str::to_string),
    }
}

fn stranger(chat: &str, from: &str, who: &str) -> Input {
    Input::Message {
        chat: chat.into(),
        from: from.into(),
        who: who.into(),
        text: "hi".into(),
    }
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
            who: String::new(),
            text: "/sessions".into(),
        },
        Input::Unreadable {
            chat: "9".into(),
            from: "9".into(),
            who: String::new(),
        },
        Input::Pressed {
            chat: "9".into(),
            from: "9".into(),
            who: String::new(),
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
    assert_eq!(shown(&fx), ["Prompt 1 hello"]);
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
            who: String::new(),
            press_id: "p".into(),
            data: press.encode(),
        },
        &open,
    );
    assert!(!shown(&fx).contains(&"Press".to_string()));
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
    assert_eq!(shown(&drive.handle(line("/stop"), &open)), ["Stop 1"]);
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
            who: String::new(),
            press_id: "p".into(),
            data: press.encode(),
        },
        &open,
    );
    assert_eq!(shown(&fx), ["Press"]);
    let fx = drive.handle(
        Input::Answered {
            chat: "7".into(),
            press_id: "p".into(),
            uid: 1,
            said: Some("Skipped.".into()),
        },
        &open,
    );
    assert_eq!(shown(&fx)[0], "Ack Skipped.");
    assert_eq!(sent(&fx), ["Skipped."]);
    let fx = drive.handle(
        Input::Pressed {
            chat: "7".into(),
            from: "7".into(),
            who: String::new(),
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
    assert_eq!(shown(&fx)[0], "Away true");
    assert!(drive.away());
    let origin = Origin {
        uid: 1,
        agent: "agent".into(),
        project: "p".into(),
        conversation: None,
    };
    let news = Announcement::plain(Away::TurnEnded);
    assert_eq!(drive.announce(&origin, &news).len(), 1);
    let fx = drive.handle(Input::Disconnected("token refused".into()), &open);
    assert_eq!(shown(&fx), ["ShutDown"]);
    assert!(!drive.away());
    assert!(drive.announce(&origin, &news).is_empty());
}

/// `/archive` scans off the UI thread and `/open` reopens what it listed,
/// pointing the chat at it.
#[test]
fn an_archive_is_scanned_then_opened() {
    let mut drive = bridge();
    assert!(sent(&drive.handle(line("/open 1"), &[]))[0].starts_with("Ask for /archive first"));
    assert_eq!(shown(&drive.handle(line("/archive"), &[])), ["Scan"]);
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
    assert_eq!(shown(&fx), [format!("Open {}", row.dir.display())]);
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

#[test]
fn a_bridge_switched_off_starts_nothing() {
    let mut drive = RemoteDrive::new(vec![], false);
    assert!(drive.start().is_empty());
    assert_eq!(drive.state(), &BridgeState::Off);
}

#[test]
fn a_bridge_on_looks_for_its_token() {
    let mut drive = RemoteDrive::new(vec![], true);
    assert_eq!(shown(&drive.start()), ["Start"]);
    assert_eq!(drive.state(), &BridgeState::Connecting { retrying: None });
}

#[test]
fn no_token_is_said_and_nothing_starts() {
    let mut drive = RemoteDrive::new(vec!["7".into()], true);
    drive.start();
    assert!(drive.handle(looked(None), &[]).is_empty());
    assert_eq!(drive.state(), &BridgeState::NoToken);
    assert_eq!(drive.source(), Some(&TokenSource::Missing));
}

/// **An empty list still connects**: it answers nobody, and the chats it
/// ignores are how somebody setting it up finds their own.
#[test]
fn an_empty_list_still_connects() {
    let mut drive = RemoteDrive::new(vec![], true);
    drive.start();
    assert_eq!(shown(&drive.handle(looked(Some("t")), &[])), ["Connect t"]);
    assert_eq!(drive.source(), Some(&TokenSource::File("/tg".into())));
}

/// Switched off while the token was being read: what the read found starts
/// nothing.
#[test]
fn a_look_that_lands_after_off_starts_nothing() {
    let mut drive = RemoteDrive::new(vec![], false);
    drive.start();
    assert!(drive.handle(looked(Some("t")), &[]).is_empty());
    assert_eq!(drive.state(), &BridgeState::Off);
}

#[test]
fn connected_again_clears_the_retry() {
    let mut drive = bridge();
    assert_eq!(
        drive.state(),
        &BridgeState::Connected {
            name: "@bot".into()
        }
    );
    drive.handle(Input::Retrying("no network".into()), &[]);
    assert_eq!(
        drive.state(),
        &BridgeState::Connecting {
            retrying: Some("no network".into())
        }
    );
    drive.handle(
        Input::Connected {
            name: "@bot".into(),
        },
        &[],
    );
    assert_eq!(
        drive.state(),
        &BridgeState::Connected {
            name: "@bot".into()
        }
    );
}

#[test]
fn a_refusal_stops_with_its_reason() {
    let mut drive = bridge();
    let fx = drive.handle(Input::Disconnected("token refused".into()), &[]);
    assert_eq!(shown(&fx), ["ShutDown"]);
    assert_eq!(
        drive.state(),
        &BridgeState::Stopped {
            why: "token refused".into()
        }
    );
}

#[test]
fn a_stranger_is_remembered_and_not_answered() {
    let mut drive = bridge();
    assert!(drive.handle(stranger("9", "9", "Minh"), &[]).is_empty());
    let pressed = Input::Pressed {
        chat: "-5".into(),
        from: "6".into(),
        who: "An in Ops".into(),
        press_id: "p".into(),
        data: "d".into(),
    };
    assert!(drive.handle(pressed, &[]).is_empty());
    assert_eq!(
        drive.ignored(),
        [
            Ignored {
                chat: "-5".into(),
                from: "6".into(),
                who: "An in Ops".into()
            },
            Ignored {
                chat: "9".into(),
                from: "9".into(),
                who: "Minh".into()
            },
        ]
    );
}

#[test]
fn one_stranger_is_one_row() {
    let mut drive = bridge();
    drive.handle(stranger("9", "9", "Minh"), &[]);
    drive.handle(stranger("8", "8", "An"), &[]);
    for _ in 0..10 {
        drive.handle(stranger("9", "9", "Minh"), &[]);
    }
    let chats: Vec<_> = drive.ignored().iter().map(|i| i.chat.as_str()).collect();
    assert_eq!(chats, ["9", "8"]);
}

#[test]
fn the_ignored_list_keeps_the_newest_five() {
    let mut drive = bridge();
    for id in 1..=7 {
        drive.handle(stranger(&format!("{id}0"), &format!("{id}0"), ""), &[]);
    }
    let chats: Vec<_> = drive.ignored().iter().map(|i| i.chat.as_str()).collect();
    assert_eq!(chats, ["70", "60", "50", "40", "30"]);
    assert_eq!(IGNORED_CAP, 5);
}

#[test]
fn a_restart_forgets_who_was_ignored() {
    let mut drive = bridge();
    drive.handle(stranger("9", "9", "Minh"), &[]);
    drive.start();
    assert!(drive.ignored().is_empty());
}
