use super::*;
use crate::acp::{AcpEvent, ToolCall, ToolContent, ToolKind, ToolStatus};
use crate::chat::model::{Chat, ChatItem, Md, NoticeLevel, ToolItem, TranscriptItemId, UserMsg};
use crate::chat::store::{commit, conv_dir, load};
use std::path::PathBuf;
use std::sync::Arc;

fn prompt(text: &str) -> ChatItem {
    ChatItem::User(UserMsg::text(text))
}

fn answer(text: &str, message: Option<&str>) -> ChatItem {
    let mut md = Md::parse(text);
    md.message = message.map(String::from);
    ChatItem::Agent(md)
}

fn tool(id: &str) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: id.into(),
        title: "Read a.rs".into(),
        description: None,
        kind: ToolKind::parse("read"),
        status: ToolStatus::parse("completed"),
        content: Vec::new(),
    }))
}

/// A chat whose agent forks, on session `orig`, holding `items` live.
fn chat_of(items: Vec<ChatItem>) -> Chat {
    let mut chat = Chat::default();
    chat.forkable = true;
    chat.session_id = Some("orig".into());
    chat.items = items;
    chat
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-fork-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_point_is_the_answer_just_before() {
    let chat = chat_of(vec![
        prompt("one"),
        answer("done", Some("m1")),
        prompt("two"),
    ]);
    let point = chat.fork_point(TranscriptItemId::Live(2)).unwrap();
    assert_eq!(point.from, "orig");
    assert_eq!(point.message, "m1");
    assert_eq!(point.text, "two");
    assert_eq!(
        point.title,
        format!("Fork: {}", chat.conversation_title().unwrap())
    );
}

#[test]
fn the_first_prompt_has_no_point() {
    let chat = chat_of(vec![prompt("one"), answer("done", Some("m1"))]);
    assert!(chat.fork_point(TranscriptItemId::Live(0)).is_none());
    let chat = chat_of(vec![
        ChatItem::notice("Connected"),
        answer("hello", Some("m0")),
        prompt("one"),
    ]);
    assert!(chat.fork_point(TranscriptItemId::Live(2)).is_none());
}

#[test]
fn no_point_after_a_turn_not_ending_in_an_answer() {
    for last in [
        tool("t1"),
        ChatItem::notice("Interrupted"),
        ChatItem::failure("The turn failed: boom", NoticeLevel::Failed, None),
    ] {
        let chat = chat_of(vec![
            prompt("one"),
            answer("working", Some("m1")),
            last,
            prompt("two"),
        ]);
        assert!(chat.fork_point(TranscriptItemId::Live(3)).is_none());
    }
}

#[test]
fn no_point_without_a_stored_id() {
    let chat = chat_of(vec![prompt("one"), answer("done", None), prompt("two")]);
    assert!(chat.fork_point(TranscriptItemId::Live(2)).is_none());
}

#[test]
fn no_point_when_the_agent_cannot_fork() {
    let mut chat = chat_of(vec![
        prompt("one"),
        answer("done", Some("m1")),
        prompt("two"),
    ]);
    chat.forkable = false;
    assert!(chat.fork_point(TranscriptItemId::Live(2)).is_none());
}

#[test]
fn no_point_without_a_session_id() {
    let mut chat = chat_of(vec![
        prompt("one"),
        answer("done", Some("m1")),
        prompt("two"),
    ]);
    chat.session_id = None;
    assert!(chat.fork_point(TranscriptItemId::Live(2)).is_none());
}

#[test]
fn a_point_survives_a_lost_link() {
    let mut chat = chat_of(vec![
        prompt("one"),
        answer("done", Some("m1")),
        prompt("two"),
    ]);
    chat.apply(AcpEvent::Disconnected("gone".into()));
    assert!(chat.fork_point(TranscriptItemId::Live(2)).is_some());
}

#[test]
fn the_point_spans_history_and_live() {
    let mut chat = chat_of(vec![prompt("two")]);
    chat.history = vec![prompt("one"), answer("done", Some("m1"))];
    let point = chat.fork_point(TranscriptItemId::Live(0)).unwrap();
    assert_eq!(point.message, "m1");
}

#[test]
fn only_a_prompt_has_a_point() {
    let chat = chat_of(vec![
        prompt("one"),
        answer("done", Some("m1")),
        prompt("two"),
    ]);
    assert!(chat.fork_point(TranscriptItemId::Live(1)).is_none());
}

/// The original conversation, saved on session `orig` under `store`.
fn saved(store: &std::path::Path, items: Vec<ChatItem>) -> Chat {
    let mut chat = Chat::new(1, PathBuf::from("/r"), "Claude".into(), Some(store.into()));
    chat.forkable = true;
    chat.session_id = Some("orig".into());
    chat.items = items;
    commit(&chat.flush().unwrap()).unwrap();
    chat
}

#[test]
fn the_seed_ends_on_the_answer() {
    let store = scratch("seed");
    let chat = saved(
        &store,
        vec![
            prompt("one"),
            answer("a", Some("m1")),
            prompt("two"),
            answer("b", Some("m2")),
            prompt("three"),
        ],
    );
    let point = chat.fork_point(TranscriptItemId::Live(2)).unwrap();
    let seed = fork_seed(load(&conv_dir(&store, "orig")).unwrap(), &point).unwrap();
    assert!(matches!(
        seed.items.as_slice(),
        [ChatItem::User(u), ChatItem::Agent(md)] if u.text == "one" && md.source == "a"
    ));

    let mut unknown = point;
    unknown.message = "nope".into();
    assert!(fork_seed(load(&conv_dir(&store, "orig")).unwrap(), &unknown).is_none());
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn an_adopted_fork_writes_itself_whole() {
    let store = scratch("adopt");
    let chat = saved(
        &store,
        vec![prompt("one"), answer("a", Some("m1")), prompt("two")],
    );
    let point = chat.fork_point(TranscriptItemId::Live(2)).unwrap();
    let seed = fork_seed(load(&conv_dir(&store, "orig")).unwrap(), &point).unwrap();

    let mut fork = Chat::new(2, PathBuf::from("/r"), "Claude".into(), Some(store.clone()));
    fork.adopt_fork(seed);
    assert!(
        fork.flush().is_none(),
        "nothing is written before the fork has an id"
    );
    fork.apply(AcpEvent::SessionId("f1".into()));
    fork.apply(AcpEvent::AgentChunk {
        text: "new".into(),
        message: Some("m9".into()),
    });
    commit(&fork.flush().unwrap()).unwrap();

    let back = load(&conv_dir(&store, "f1")).unwrap();
    assert!(matches!(
        back.items.as_slice(),
        [ChatItem::User(_), ChatItem::Agent(a), ChatItem::Agent(b)]
            if a.source == "a" && b.source == "new"
    ));
    assert_eq!(back.title, Some(point.title));
    let original = load(&conv_dir(&store, "orig")).unwrap();
    assert_eq!(original.items.len(), 3, "the original is untouched");
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn fork_seed_survives_the_original_being_deleted() {
    let store = scratch("deleted");
    let mut shot = tool("t1");
    if let ChatItem::Tool(t) = &mut shot {
        t.call.content = vec![ToolContent::Image(Arc::new(b"pretend-png".to_vec()))];
    }
    let chat = saved(
        &store,
        vec![prompt("one"), shot, answer("a", Some("m1")), prompt("two")],
    );
    let point = chat.fork_point(TranscriptItemId::Live(3)).unwrap();
    let seed = fork_seed(load(&conv_dir(&store, "orig")).unwrap(), &point).unwrap();
    let mut fork = Chat::new(2, PathBuf::from("/r"), "Claude".into(), Some(store.clone()));
    fork.adopt_fork(seed);
    fork.apply(AcpEvent::SessionId("f1".into()));
    commit(&fork.flush().unwrap()).unwrap();
    std::fs::remove_dir_all(conv_dir(&store, "orig")).unwrap();

    let back = load(&conv_dir(&store, "f1")).unwrap();
    let ChatItem::Tool(t) = &back.items[1] else {
        panic!("the tool card comes back");
    };
    assert!(matches!(&t.call.content[0], ToolContent::Image(b) if b.as_slice() == b"pretend-png"));
    let _ = std::fs::remove_dir_all(&store);
}

fn adopted_fork(name: &str) -> Chat {
    let store = scratch(name);
    let chat = saved(
        &store,
        vec![prompt("one"), answer("a", Some("m1")), prompt("two")],
    );
    let point = chat.fork_point(TranscriptItemId::Live(2)).unwrap();
    let seed = fork_seed(load(&conv_dir(&store, "orig")).unwrap(), &point).unwrap();
    let _ = std::fs::remove_dir_all(&store);
    let mut fork = Chat::default();
    fork.adopt_fork(seed);
    fork
}

#[test]
fn a_fork_that_never_opened_says_why() {
    let mut fork = adopted_fork("never-opened");
    assert_eq!(fork.fork_failure(), None);
    fork.apply(AcpEvent::Disconnected(
        "Could not fork the conversation: no such session".into(),
    ));
    assert_eq!(
        fork.fork_failure().as_deref(),
        Some("Could not fork the conversation: no such session")
    );

    let mut fork = adopted_fork("timed-out");
    fork.apply(AcpEvent::Disconnected("handshake timed out".into()));
    assert_eq!(
        fork.fork_failure().as_deref(),
        Some("Could not fork the conversation: handshake timed out")
    );
}

#[test]
fn an_opened_fork_that_drops_is_no_failed_fork() {
    let mut fork = adopted_fork("opened");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    fork.apply(AcpEvent::Connected {
        tx,
        resumed: true,
        forkable: true,
    });
    fork.apply(AcpEvent::Disconnected("adapter closed stdout".into()));
    assert_eq!(fork.fork_failure(), None);
    let mut chat = chat_of(vec![prompt("one")]);
    chat.apply(AcpEvent::Disconnected("gone".into()));
    assert_eq!(chat.fork_failure(), None);
}
