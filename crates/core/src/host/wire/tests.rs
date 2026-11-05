use super::*;

fn event(e: AcpEvent) -> Entry {
    Entry::Event(e)
}

#[test]
fn a_prompt_starts_a_turn_and_the_turn_ending_or_the_agent_dying_stops_it() {
    let prompt = Entry::Request(AcpRequest::Prompt {
        text: "hi".into(),
        attachments: vec![],
    });
    assert!(busy_after(false, &prompt));
    let ended = event(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(!busy_after(true, &ended));
    assert!(!busy_after(
        true,
        &event(AcpEvent::Disconnected("x".into()))
    ));
}

#[test]
fn other_entries_keep_the_busy_state_they_were_given() {
    let chunk = event(AcpEvent::AgentChunk {
        text: "a".into(),
        message: None,
    });
    assert!(busy_after(true, &chunk));
    assert!(!busy_after(false, &chunk));
}

#[test]
fn build_id_is_fnv1a_64() {
    assert_eq!(build_id(b"").0, "cbf29ce484222325");
    assert_eq!(build_id(b"a").0, "af63dc4c8601ec8c");
}

#[test]
fn an_open_ask_and_an_entry_line_round_trip() {
    let ask = Question {
        id: 7,
        call: Call::Open {
            cwd: "/srv/app".into(),
            agent: AgentSpec::default(),
            opening: Opening::Resume("s1".into()),
        },
    };
    let json = serde_json::to_string(&ask).unwrap();
    let back: Question = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), json);

    let line = FromHost::Entry {
        session: HostSession("s1".into()),
        seq: 3,
        at: 1_700_000_000,
        entry: Entry::Event(AcpEvent::AgentChunk {
            text: "hello".into(),
            message: Some("m".into()),
        }),
    };
    let json = serde_json::to_string(&line).unwrap();
    let back: FromHost = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), json);
}

/// `value` to JSON and back, as the same JSON.
fn round_trips<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    let json = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), json);
    back
}

#[test]
fn file_calls_round_trip() {
    let mtime = Some(std::time::UNIX_EPOCH + std::time::Duration::from_nanos(1_700_000_000_123));
    for call in [
        Call::ReadDir {
            path: "/srv/app".into(),
        },
        Call::ReadFile {
            path: "/srv/app/a.rs".into(),
        },
        Call::SaveFile {
            path: "/srv/app/a.rs".into(),
            text: "fn main() {}\n".into(),
            expected: mtime,
        },
        Call::GitStatus {
            root: "/srv/app".into(),
        },
    ] {
        round_trips(&Question { id: 1, call });
    }

    let entry = crate::tree::TreeEntry {
        path: "/srv/app/src".into(),
        name: "src".into(),
        is_dir: true,
    };
    let back = round_trips(&Reply::Listing {
        entries: vec![entry.clone()],
        cut: 3,
    });
    assert!(matches!(back, Reply::Listing { entries, cut: 3 } if entries == vec![entry]));
    let back = round_trips(&Reply::File {
        text: "hi".into(),
        mtime,
    });
    assert!(matches!(back, Reply::File { text, mtime: m } if text == "hi" && m == mtime));

    let saved = Reply::Saved(crate::editor::SaveOutcome::Saved {
        mtime,
        text: std::sync::Arc::new("kept".into()),
    });
    let back = round_trips(&saved);
    assert!(
        matches!(&back, Reply::Saved(crate::editor::SaveOutcome::Saved { text, mtime: m })
            if text.as_str() == "kept" && *m == mtime),
        "{back:?}"
    );

    let status = crate::gitstat::GitStatus {
        branch: "main".into(),
        changed: 1,
        ahead: 0,
        behind: 2,
        entries: [("src/a.rs".into(), crate::gitstat::FileChange::Modified)].into(),
    };
    let back = round_trips(&Reply::Git(Some(status.clone())));
    assert!(matches!(back, Reply::Git(Some(s)) if s == status));
}

#[test]
fn bytes_travel_as_base64() {
    let bytes = Bytes(vec![0, 255, b'\n']);
    let text = serde_json::to_string(&bytes).unwrap();
    assert_eq!(text, "\"AP8K\"");
    assert_eq!(serde_json::from_str::<Bytes>(&text).unwrap(), bytes);
}

#[test]
fn a_clone_is_named_by_its_urls_last_segment() {
    assert_eq!(
        clone_name("https://github.com/o/repo.git").as_deref(),
        Some("repo")
    );
    assert_eq!(
        clone_name("git@github.com:o/repo.git").as_deref(),
        Some("repo")
    );
    assert_eq!(
        clone_name("git@github.com:repo.git").as_deref(),
        Some("repo")
    );
    assert_eq!(clone_name("/srv/git/repo/").as_deref(), Some("repo"));
    assert_eq!(clone_name("https://x/"), None);
    assert_eq!(clone_name(".."), None);
    assert_eq!(clone_name("https://x/.git"), None);
    assert_eq!(clone_name("ssh://git@x:22/o/repo").as_deref(), Some("repo"));
}

fn allow() -> AcpRequest {
    AcpRequest::PermissionResponse {
        rpc_id: serde_json::json!(7),
        option_id: Some("o".into()),
    }
}

#[test]
fn an_answer_does_not_change_busy() {
    let answered = Entry::Answered {
        card: None,
        answer: allow(),
    };
    assert!(busy_after(true, &answered));
    assert!(!busy_after(false, &answered));
}

#[test]
fn the_switch_calls_round_trip() {
    let call = round_trips(&Call::Bypass {
        cwd: "/srv/app".into(),
        on: None,
    });
    assert!(
        matches!(call, Call::Bypass { cwd, on: None } if cwd == std::path::Path::new("/srv/app"))
    );
    let call = round_trips(&Call::Bypass {
        cwd: "/srv/app".into(),
        on: Some(false),
    });
    assert!(matches!(
        call,
        Call::Bypass {
            on: Some(false),
            ..
        }
    ));
    assert!(matches!(
        round_trips(&Reply::Bypass { on: true }),
        Reply::Bypass { on: true }
    ));
    let line = round_trips(&Entry::Answered {
        card: None,
        answer: allow(),
    });
    assert!(matches!(line, Entry::Answered { card: None, .. }));
}

/// A laptop or a host from before branch mirrors still reads, and is read,
/// as mirror mode.
#[test]
fn an_older_mirror_call_still_reads() {
    let call: Call = serde_json::from_str(r#"{"mirror":{"name":"m"}}"#).unwrap();
    assert!(matches!(call, Call::Mirror { branch: None, .. }));
    let state: MirrorState =
        serde_json::from_str(r#"{"path":"/m","key":"host","generation":3,"working":false}"#)
            .unwrap();
    assert_eq!(state.head, None);
    let update = Call::MirrorUpdate {
        path: "/m".into(),
        commit: "abc".into(),
    };
    assert!(matches!(
        round_trips(&update),
        Call::MirrorUpdate { commit, .. } if commit == "abc"
    ));
}
