use super::*;
use crate::acp::{
    AcpEvent, ElicitField, ElicitKind, Elicitation, PermissionRequest, SubagentEnd, ToolCall,
    ToolCallUpdate, ToolKind, ToolStatus,
};
use crate::chat::model::UserMsg;
use std::path::PathBuf;

fn chat() -> Chat {
    Chat::new(1, PathBuf::from("/r"), "Claude".into(), None)
}

fn spawn(id: &str, name: &str, task: &str) -> AcpEvent {
    AcpEvent::SubagentSpawned {
        id: id.into(),
        name: name.into(),
        task: task.into(),
    }
}

fn ended(id: &str, end: SubagentEnd) -> AcpEvent {
    AcpEvent::SubagentEnded { id: id.into(), end }
}

fn inside(child: &str, event: AcpEvent) -> AcpEvent {
    AcpEvent::InChild {
        child: child.into(),
        event: Box::new(event),
    }
}

fn call(id: &str) -> AcpEvent {
    AcpEvent::ToolCall(ToolCall {
        id: id.into(),
        title: "Read".into(),
        description: None,
        kind: ToolKind::Read,
        status: ToolStatus::InProgress,
        content: vec![],
    })
}

fn done(id: &str) -> AcpEvent {
    AcpEvent::ToolUpdate(ToolCallUpdate {
        id: id.into(),
        status: Some(ToolStatus::Completed),
        title: None,
        description: None,
        content: None,
    })
}

fn card<'a>(items: &'a [ChatItem], id: &str) -> &'a SubagentItem {
    items
        .iter()
        .find_map(|item| match item {
            ChatItem::Subagent(s) if s.id == id => Some(s),
            _ => None,
        })
        .expect("a card with that id")
}

fn agent_text(item: &ChatItem) -> Option<&str> {
    match item {
        ChatItem::Agent(md) => Some(&md.source),
        _ => None,
    }
}

#[test]
fn a_spawn_adds_a_working_card() {
    let mut chat = chat();
    let outcome = chat.apply(spawn(
        "c1",
        "Read the auth tests",
        "Read tests/auth and report\nthen more",
    ));
    assert!(outcome.transcript_changed);
    assert!(chat.last_activity.is_some());
    let Some(ChatItem::Subagent(s)) = chat.items.last() else {
        panic!("the last item is a card");
    };
    assert_eq!(s.id, "c1");
    assert_eq!(s.name, "Read the auth tests");
    assert_eq!(s.state, SubagentState::Working);
    assert_eq!(s.task.as_deref(), Some("Read tests/auth and report"));
    assert_eq!(chat.subagents_working(), 1);
    assert!(chat.running());
}

#[test]
fn child_steps_land_in_their_card() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Reader", "Read"));
    chat.apply(inside("c1", AcpEvent::AgentChunk("a".into())));
    chat.apply(inside("c1", AcpEvent::AgentChunk("b".into())));
    chat.apply(inside("c1", call("t1")));
    chat.apply(inside("c1", done("t1")));
    assert_eq!(
        chat.items.len(),
        1,
        "the parent gained nothing but the card"
    );
    let items = &card(&chat.items, "c1").items;
    assert_eq!(items.len(), 2);
    assert_eq!(agent_text(&items[0]), Some("ab"));
    assert!(matches!(&items[1], ChatItem::Tool(t)
        if t.call.id == "t1" && t.call.status == ToolStatus::Completed));
}

#[test]
fn a_nested_spawn_nests() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Outer", "Outer task"));
    chat.apply(inside("c1", spawn("c2", "Inner", "Inner task")));
    chat.apply(inside("c2", AcpEvent::AgentChunk("x".into())));
    let inner = card(&card(&chat.items, "c1").items, "c2");
    assert_eq!(inner.items.len(), 1);
    assert_eq!(agent_text(&inner.items[0]), Some("x"));
    assert_eq!(chat.subagents_working(), 2);
}

#[test]
fn an_end_stops_the_count() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Outer", "Outer task"));
    chat.apply(inside("c1", spawn("c2", "Inner", "Inner task")));
    chat.apply(inside("c1", ended("c2", SubagentEnd::Failed)));
    assert_eq!(chat.subagents_working(), 1);
    assert_eq!(
        card(&card(&chat.items, "c1").items, "c2").state,
        SubagentState::Failed
    );
    chat.apply(ended("c1", SubagentEnd::Completed));
    assert_eq!(card(&chat.items, "c1").state, SubagentState::Completed);
    assert_eq!(chat.subagents_working(), 0);
    assert!(!chat.running());

    for (end, state) in [
        (SubagentEnd::Completed, SubagentState::Completed),
        (SubagentEnd::Failed, SubagentState::Failed),
        (SubagentEnd::Cancelled, SubagentState::Cancelled),
        (SubagentEnd::Disconnected, SubagentState::Interrupted),
    ] {
        let mut chat = Chat::new(1, PathBuf::from("/r"), "C".into(), None);
        chat.apply(spawn("c", "N", "T"));
        chat.apply(ended("c", end));
        assert_eq!(card(&chat.items, "c").state, state);
    }
}

#[test]
fn unknown_or_ended_children_are_dropped() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Reader", "Read"));
    let before = chat.revision();
    let outcome = chat.apply(inside("nope", AcpEvent::AgentChunk("lost".into())));
    assert!(!outcome.transcript_changed);
    assert_eq!(chat.revision(), before);
    chat.apply(ended("c1", SubagentEnd::Completed));
    let before = chat.revision();
    chat.apply(inside("c1", AcpEvent::AgentChunk("late".into())));
    chat.apply(ended("c1", SubagentEnd::Failed));
    assert_eq!(chat.revision(), before);
    let c1 = card(&chat.items, "c1");
    assert!(c1.items.is_empty());
    assert_eq!(c1.state, SubagentState::Completed);
    assert_eq!(chat.items.len(), 1);
}

#[test]
fn a_second_generation_is_a_new_card() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Reader", "Read"));
    chat.apply(ended("c1", SubagentEnd::Completed));
    chat.apply(spawn("c1:generation:2", "Reader", "Read"));
    assert_eq!(chat.items.len(), 2);
    assert_eq!(
        card(&chat.items, "c1:generation:2").state,
        SubagentState::Working
    );
}

#[test]
fn the_task_line_is_left_out_when_it_says_nothing() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Reader", "Reader"));
    chat.apply(spawn("c2", "Agent", "Delegated task for Agent 1a2b"));
    assert_eq!(card(&chat.items, "c1").task, None);
    assert_eq!(card(&chat.items, "c2").task, None);
}

#[test]
fn a_lost_adapter_interrupts_working_cards() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Reader", "Read"));
    chat.apply(inside("c1", spawn("c2", "Inner", "Inner")));
    chat.apply(inside("c1", call("t1")));
    chat.apply(AcpEvent::Disconnected("gone".into()));
    let c1 = card(&chat.items, "c1");
    assert_eq!(c1.state, SubagentState::Interrupted);
    assert_eq!(card(&c1.items, "c2").state, SubagentState::Interrupted);
    assert!(
        matches!(&c1.items[1], ChatItem::Tool(t) if t.call.status == ToolStatus::Failed),
        "a step left running inside settles with the card"
    );
    assert_eq!(chat.subagents_working(), 0);
    assert!(!chat.running());

    let mut chat = Chat::new(1, PathBuf::from("/r"), "C".into(), None);
    chat.apply(spawn("c1", "Reader", "Read"));
    chat.stream_ended();
    assert_eq!(card(&chat.items, "c1").state, SubagentState::Interrupted);
    assert_eq!(chat.subagents_working(), 0);
    assert!(!chat.running());
}

#[test]
fn a_replay_adds_no_cards() {
    let mut chat = chat();
    chat.resume_from(crate::chat::store::ConversationSnapshot {
        session_id: "sid-1".into(),
        title: None,
        updated: 1,
        prefs: crate::chat::store::Prefs::default(),
        items: vec![ChatItem::User(UserMsg::text("message 0"))],
        written: 1,
        complete: true,
    });
    // Armed: nothing replayed yet.
    chat.apply(spawn("c1", "Reader", "Read"));
    chat.apply(inside("c1", AcpEvent::AgentChunk("a".into())));
    chat.apply(ended("c1", SubagentEnd::Completed));
    assert!(chat.items.is_empty());
    // Partial: the replay has begun.
    chat.apply(AcpEvent::UserChunk("message 0".into()));
    let before = chat.revision();
    chat.apply(spawn("c2", "Reader", "Read"));
    chat.apply(inside("c2", AcpEvent::AgentChunk("a".into())));
    chat.apply(ended("c2", SubagentEnd::Completed));
    assert_eq!(chat.revision(), before);
    assert_eq!(chat.items.len(), 1);
    assert_eq!(chat.subagents_working(), 0);
}

fn permission(child: Option<&str>) -> PermissionRequest {
    PermissionRequest {
        rpc_id: serde_json::Value::from(7),
        tool_call_id: None,
        kind: ToolKind::Execute,
        title: "Run command".into(),
        options: vec![],
        child: child.map(str::to_string),
    }
}

fn question(child: Option<&str>) -> Elicitation {
    Elicitation {
        rpc_id: serde_json::Value::from(8),
        tool_call_id: None,
        message: "pick".into(),
        fields: vec![ElicitField {
            key: "q".into(),
            title: None,
            description: None,
            kind: ElicitKind::Text,
            custom_key: None,
        }],
        child: child.map(str::to_string),
    }
}

#[test]
fn a_childs_permission_carries_the_askers_name() {
    let mut chat = chat();
    chat.apply(spawn("c1", "Read the auth tests", "Read"));
    chat.apply(AcpEvent::Permission(permission(Some("c1"))));
    chat.apply(AcpEvent::Permission(permission(Some("nope"))));
    chat.apply(AcpEvent::Elicitation(question(Some("c1"))));
    chat.apply(AcpEvent::Elicitation(question(None)));
    let askers: Vec<Option<&str>> = chat
        .items
        .iter()
        .filter_map(|item| match item {
            ChatItem::Permission(p) => Some(p.asker.as_deref()),
            ChatItem::Ask(a) => Some(a.asker.as_deref()),
            _ => None,
        })
        .collect();
    assert_eq!(
        askers,
        vec![
            Some("Read the auth tests"),
            None,
            Some("Read the auth tests"),
            None
        ]
    );
}

fn terminal_run(chat: &mut Chat) {
    chat.apply(spawn("c1", "Builder", "Build"));
    chat.apply(inside(
        "c1",
        AcpEvent::ToolCall(ToolCall {
            id: "t1".into(),
            title: "cargo build".into(),
            description: None,
            kind: ToolKind::Execute,
            status: ToolStatus::InProgress,
            content: vec![crate::acp::ToolContent::Terminal("term1".into())],
        }),
    ));
    chat.apply(AcpEvent::TerminalOutput {
        terminal_id: "term1".into(),
        chunk: "hello".into(),
    });
}

fn card_tool_output(chat: &Chat) -> (String, Option<i32>) {
    match &card(&chat.items, "c1").items[0] {
        ChatItem::Tool(t) => match &t.call.content[0] {
            crate::acp::ToolContent::Text(text) => (text.clone(), t.exit_code),
            _ => panic!("the terminal was not flattened into the card's step"),
        },
        _ => panic!("the card holds the step"),
    }
}

#[test]
fn a_terminal_inside_a_card_keeps_its_output() {
    let mut chat = chat();
    terminal_run(&mut chat);
    chat.apply(AcpEvent::TerminalExit {
        terminal_id: "term1".into(),
        exit_code: Some(0),
    });
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    let (text, code) = card_tool_output(&chat);
    assert!(text.starts_with("hello"), "{text}");
    assert_eq!(code, Some(0));

    let mut chat = Chat::new(1, PathBuf::from("/r"), "C".into(), None);
    terminal_run(&mut chat);
    chat.apply(AcpEvent::Disconnected("gone".into()));
    let (text, _) = card_tool_output(&chat);
    assert!(text.starts_with("hello"), "{text}");
}
