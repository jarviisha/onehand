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
