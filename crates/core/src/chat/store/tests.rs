use super::*;
use crate::acp::{ToolCall, ToolContent, ToolKind, ToolStatus};
use crate::chat::model::{Chat, Md, SubagentItem, SubagentState, ToolItem};
use std::sync::Arc;

/// One conversation's metadata, read back off disk.
///
/// Only the tests ask this: nothing in the app reads a single conversation
/// by directory — the pickers list a whole store through
/// [`list_conversations`], which reads the same file for its own reasons.
fn read_meta(dir: &Path) -> Option<ConvMeta> {
    meta_at(dir).map(|meta| describe(dir, meta))
}

/// A temp directory of this test's own, cleaned up on the way out.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn chat_in(store: &Path, sid: &str) -> Chat {
    let mut chat = Chat::new(
        1,
        PathBuf::from("/r"),
        "Claude".into(),
        Some(store.to_path_buf()),
    );
    chat.session_id = Some(sid.to_string());
    chat
}

fn save(chat: &mut Chat) {
    if let Some(write) = chat.flush() {
        commit(&write).unwrap();
    }
}

#[test]
fn sanitize_keeps_uuid_and_replaces_unsafe() {
    assert_eq!(sanitize("3ff1f752-4189-8601"), "3ff1f752-4189-8601");
    assert!(sanitize("a/b c").starts_with("a_b_c-"));
    assert_ne!(sanitize("a/b"), sanitize("a.b"));
    assert_ne!(sanitize("a/b"), "a_b");
}

/// The reason the format changed: a turn writes its own turn, not the
/// conversation it is part of.
#[test]
fn a_turn_appends_only_what_it_added() {
    let store = scratch("append");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("first".into(), Vec::new());
    save(&mut chat);

    let items = conv_dir(&store, "s1").join("items.jsonl");
    let after_one = std::fs::read(&items).unwrap();

    chat.push_user("second".into(), Vec::new());
    save(&mut chat);
    let after_two = std::fs::read(&items).unwrap();

    assert!(
        after_two.starts_with(&after_one),
        "the first turn's bytes were rewritten rather than kept"
    );
    assert_eq!(after_two.iter().filter(|b| **b == b'\n').count(), 2);
    let _ = std::fs::remove_dir_all(&store);
}

/// …and a save with nothing new does not move the conversation's date
/// either, which is what stops the picker reordering itself around
/// conversations that were merely opened.
#[test]
fn a_second_save_with_nothing_new_writes_no_line() {
    let store = scratch("nothing-new");
    let dir = conv_dir(&store, "s1");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("only".into(), Vec::new());
    save(&mut chat);
    let items = dir.join("items.jsonl");
    let once = std::fs::read(&items).unwrap();
    let dated = read_meta(&dir).unwrap().updated;

    save(&mut chat);
    assert_eq!(std::fs::read(&items).unwrap(), once);
    assert_eq!(read_meta(&dir).unwrap().updated, dated);
    let _ = std::fs::remove_dir_all(&store);
}

/// The question the whole format turns on: a `session/load` re-delivers the
/// conversation as ordinary content, so what stops it being written down a
/// second time?
///
/// Through the real file rather than in memory, because the answer is a
/// claim about the file: the mark says how much of the transcript is
/// already in it, the replay is not added while it is arriving, and what
/// settles it decides whether the file is left alone or becomes what was
/// replayed.
#[test]
fn a_replay_is_not_written_a_second_time() {
    use crate::acp::AcpEvent;

    let store = scratch("replay");
    let dir = conv_dir(&store, "s1");
    let mut first = chat_in(&store, "s1");
    first.push_user("what does this do".into(), Vec::new());
    first.apply(AcpEvent::AgentChunk {
        text: "it does this".into(),
        message: None,
    });
    save(&mut first);
    assert_eq!(load(&dir).unwrap().items.len(), 2);

    // Reopened, and the adapter replays exactly what is already there.
    let mut next = chat_in(&store, "s1");
    next.resume_from(load(&dir).unwrap());
    next.apply(AcpEvent::UserChunk("what does this do".into()));
    next.apply(AcpEvent::AgentChunk {
        text: "it does this".into(),
        message: None,
    });
    // Nothing is written while the replay is arriving.
    assert!(next.flush().is_none());

    next.push_user("and now this".into(), Vec::new());
    save(&mut next);

    let back = load(&dir).unwrap();
    assert_eq!(
        back.items.len(),
        3,
        "the conversation once, and the new prompt after it"
    );
    let _ = std::fs::remove_dir_all(&store);
}

/// The property that keeps a session nobody used from overwriting one
/// somebody did.
#[test]
fn an_empty_chat_creates_no_directory() {
    let store = scratch("empty");
    let mut chat = chat_in(&store, "s1");
    assert!(chat.flush().is_none());

    // …and neither does one that has never been given a session id.
    let mut nameless = Chat::new(1, PathBuf::from("/r"), "Claude".into(), Some(store.clone()));
    nameless.push_user("typed but never sent anywhere".into(), Vec::new());
    assert!(nameless.flush().is_none());

    assert!(!conv_dir(&store, "s1").exists());
    let _ = std::fs::remove_dir_all(&store);
}

/// Listing is the operation that used to parse every transcript in full,
/// images included, to read a title off the front.
#[test]
fn listing_never_opens_the_transcript() {
    let store = scratch("listing");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("Fix the login flow".into(), Vec::new());
    save(&mut chat);

    std::fs::write(conv_dir(&store, "s1").join("items.jsonl"), "not json").unwrap();
    let found = list_conversations(&store, Path::new("/r"), Some("Claude"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Fix the login flow");
    assert_eq!(found[0].session_id, "s1");
    // Another project's conversations are not this project's.
    assert!(list_conversations(&store, Path::new("/other"), None).is_empty());
    // Nor another agent's.
    assert!(list_conversations(&store, Path::new("/r"), Some("Other")).is_empty());
    // Found by its id alone, with the project it ran in, whatever is open.
    let (root, conv) = find_conversation(&store, "s1").unwrap();
    assert_eq!(
        (root, conv.title.as_str()),
        (PathBuf::from("/r"), "Fix the login flow")
    );
    assert!(find_conversation(&store, "gone").is_none());
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn listing_across_projects_names_each_ones_project() {
    let store = scratch("across");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("Fix the login flow".into(), Vec::new());
    save(&mut chat);

    let projects = [PathBuf::from("/other"), PathBuf::from("/r")];
    let found = list_across(&store, &projects);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, PathBuf::from("/r"));
    assert_eq!(found[0].1.session_id, "s1");
    assert!(list_across(&store, &projects[..1]).is_empty());
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn an_unknown_format_is_neither_listed_nor_loaded() {
    let store = scratch("format");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("hello".into(), Vec::new());
    save(&mut chat);

    let dir = conv_dir(&store, "s1");
    let text = std::fs::read_to_string(dir.join("meta.json")).unwrap();
    let bumped = text.replace(
        &format!("\"format\": {FORMAT}"),
        &format!("\"format\": {}", FORMAT + 1),
    );
    assert_ne!(bumped, text, "the format field must be what was replaced");
    std::fs::write(dir.join("meta.json"), bumped).unwrap();

    assert!(read_meta(&dir).is_none());
    assert!(load(&dir).is_none());
    assert!(list_conversations(&store, Path::new("/r"), None).is_empty());
    assert!(dir.exists(), "and it is refused, not removed");
    let _ = std::fs::remove_dir_all(&store);
}

/// What append-only buys at the tail.
#[test]
fn a_torn_last_line_costs_one_item_not_the_conversation() {
    let store = scratch("torn");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("one".into(), Vec::new());
    chat.push_user("two".into(), Vec::new());
    save(&mut chat);

    let items = conv_dir(&store, "s1").join("items.jsonl");
    let mut text = std::fs::read_to_string(&items).unwrap();
    text.push_str("{\"t\":\"user\",\"tex");
    std::fs::write(&items, text).unwrap();

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert_eq!(back.items.len(), 2);
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn an_image_is_a_blob_and_the_transcript_stays_small() {
    let store = scratch("blob");
    let mut chat = chat_in(&store, "s1");
    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "t1".into(),
        title: "Read shot.png".into(),
        description: None,
        kind: ToolKind::parse("read"),
        status: ToolStatus::parse("completed"),
        content: vec![ToolContent::Image(Arc::new(b"pretend-png".to_vec()))],
    })));
    save(&mut chat);

    let dir = conv_dir(&store, "s1");
    let text = std::fs::read_to_string(dir.join("items.jsonl")).unwrap();
    assert!(
        !text.contains("pretend-png"),
        "the bytes are not in the line"
    );
    let blobs: Vec<_> = std::fs::read_dir(dir.join("blobs"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(blobs.len(), 1);
    assert_eq!(std::fs::read(blobs[0].path()).unwrap(), b"pretend-png");

    let back = load(&dir).unwrap();
    let ChatItem::Tool(t) = &back.items[0] else {
        panic!("expected a tool card")
    };
    let ToolContent::Image(bytes) = &t.call.content[0] else {
        panic!("expected an image")
    };
    assert_eq!(bytes.as_slice(), b"pretend-png");

    // And a blob that has gone missing degrades rather than disappearing.
    std::fs::remove_file(blobs[0].path()).unwrap();
    let back = load(&dir).unwrap();
    let ChatItem::Tool(t) = &back.items[0] else {
        panic!()
    };
    assert!(matches!(&t.call.content[0], ToolContent::Text(s) if s == "(image)"));
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn an_oversized_image_is_recorded_but_not_stored() {
    let store = scratch("big-blob");
    let mut chat = chat_in(&store, "s1");
    let huge = Arc::new(vec![0u8; (MAX_BLOB_BYTES + 1) as usize]);
    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "t1".into(),
        title: "Read huge.png".into(),
        description: None,
        kind: ToolKind::parse("read"),
        status: ToolStatus::parse("completed"),
        content: vec![ToolContent::Image(huge)],
    })));
    save(&mut chat);

    let dir = conv_dir(&store, "s1");
    assert!(!dir.join("blobs").exists(), "nothing that large is kept");
    let back = load(&dir).unwrap();
    let ChatItem::Tool(t) = &back.items[0] else {
        panic!()
    };
    // It says what was there, rather than leaving a card that looks wrong
    // about having held a picture.
    assert!(matches!(&t.call.content[0], ToolContent::Text(s) if s.contains("not archived")));
    let _ = std::fs::remove_dir_all(&store);
}

/// A rename is not new work, and the list must not reorder for it.
#[test]
fn a_rename_rewrites_only_the_metadata() {
    let store = scratch("rename");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("Fix the login flow".into(), Vec::new());
    save(&mut chat);

    let dir = conv_dir(&store, "s1");
    let items = std::fs::read(dir.join("items.jsonl")).unwrap();
    let was = read_meta(&dir).unwrap().updated;

    assert!(chat.rename("Authentication cleanup"));
    commit(&chat.flush_meta().unwrap()).unwrap();

    assert_eq!(std::fs::read(dir.join("items.jsonl")).unwrap(), items);
    let meta = read_meta(&dir).unwrap();
    assert_eq!(meta.title, "Authentication cleanup");
    assert_eq!(meta.updated, was, "a rename is not a message");
    let _ = std::fs::remove_dir_all(&store);
}

/// A rename's metadata landing after a turn's save that was read later
/// leaves the turn's count alone; the turn's lines still land.
#[test]
fn a_rename_landing_late_does_not_write_back_an_older_count() {
    let store = scratch("rename-late");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("first".into(), Vec::new());
    save(&mut chat);
    let dir = conv_dir(&store, "s1");

    assert!(chat.rename("Named"));
    let rename = chat.flush_meta().unwrap();
    chat.push_user("second".into(), Vec::new());
    let turn = chat.flush().unwrap();
    commit(&turn).unwrap();
    commit(&rename).unwrap();

    let meta = read_meta(&dir).unwrap();
    assert_eq!(meta.item_count, 2, "the turn's count stands");
    assert_eq!(
        meta.title, "Named",
        "read after the rename, the turn carries it"
    );
    let _ = std::fs::remove_dir_all(&store);
}

/// A turn's save landing after a rename that was read later keeps the name,
/// and still moves the conversation's date.
#[test]
fn a_turn_landing_late_keeps_the_newer_name() {
    let store = scratch("turn-late");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("first".into(), Vec::new());
    save(&mut chat);
    let dir = conv_dir(&store, "s1");
    let lines = |dir: &Path| {
        std::fs::read_to_string(dir.join("items.jsonl"))
            .unwrap()
            .lines()
            .count()
    };

    chat.push_user("second".into(), Vec::new());
    let turn = chat.flush().unwrap();
    assert!(chat.rename("Named"));
    let rename = chat.flush_meta().unwrap();
    commit(&rename).unwrap();
    commit(&turn).unwrap();

    let meta = read_meta(&dir).unwrap();
    assert_eq!(meta.title, "Named", "the older read does not undo the name");
    assert_eq!(meta.item_count, 2);
    assert_eq!(lines(&dir), 2, "the turn's line still lands");
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn a_bounded_load_says_so_and_keeps_the_tail() {
    let store = scratch("bounded");
    let mut chat = chat_in(&store, "s1");
    for i in 0..MAX_RESTORED_ITEMS + 10 {
        chat.push_user(format!("message {i}"), Vec::new());
    }
    save(&mut chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert!(!back.complete);
    assert_eq!(
        back.items.len(),
        MAX_RESTORED_ITEMS + 1,
        "the tail plus a line saying so"
    );
    assert!(matches!(back.items[0], ChatItem::Notice { .. }));
    let ChatItem::User(last) = back.items.last().unwrap() else {
        panic!()
    };
    assert_eq!(last.text, format!("message {}", MAX_RESTORED_ITEMS + 9));
    let _ = std::fs::remove_dir_all(&store);
}

/// Deleting takes the whole directory, which is the point of there being
/// one: the images a conversation collected are inside it, so nothing is
/// left holding megabytes that no line refers to any more.
#[test]
fn deleting_a_conversation_takes_its_images_with_it() {
    let store = scratch("delete");
    let mut chat = chat_in(&store, "s1");
    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "t1".into(),
        title: "Read shot.png".into(),
        description: None,
        kind: ToolKind::parse("read"),
        status: ToolStatus::parse("completed"),
        content: vec![ToolContent::Image(Arc::new(b"pretend-png".to_vec()))],
    })));
    save(&mut chat);

    let dir = conv_dir(&store, "s1");
    assert!(dir.join("blobs").exists());
    delete(&dir).unwrap();

    assert!(!dir.exists());
    assert!(read_meta(&dir).is_none());
    assert!(list_conversations(&store, Path::new("/r"), None).is_empty());
    // Deleting what is already gone is the outcome asked for.
    assert!(delete(&dir).is_ok());
    // And it took only its own: the store is still there for the rest.
    assert!(store.exists());
    let _ = std::fs::remove_dir_all(&store);
}

/// Two writers on one conversation add whole turns, never halves of a line.
#[test]
fn two_writers_in_one_process_do_not_interleave() {
    let store = scratch("writers");
    let dir = conv_dir(&store, "s1");
    std::thread::scope(|scope| {
        for who in 0..2 {
            let store = store.clone();
            scope.spawn(move || {
                let mut chat = chat_in(&store, "s1");
                for i in 0..20 {
                    chat.push_user(format!("writer {who} message {i}"), Vec::new());
                    save(&mut chat);
                }
            });
        }
    });

    let text = std::fs::read_to_string(dir.join("items.jsonl")).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 40);
    for line in lines {
        serde_json::from_str::<Line>(line).expect("every line parses whole");
    }
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn fnv1a_gives_the_published_values_so_a_saved_digest_survives_any_rebuild() {
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
}

/// A relaunch reads the session back from its file, so a failed turn's notice
/// has to come back failed, with the detail the agent attached, or the
/// session would read as idle beside a failure it still shows.
#[test]
fn a_failed_turn_comes_back_failed() {
    let store = scratch("failed");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("go".into(), Vec::new());
    chat.items.push(ChatItem::failure(
        "The turn failed: rate limited",
        crate::chat::NoticeLevel::Failed,
        Some("{\"reason\": \"quota\"}".into()),
    ));
    save(&mut chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert!(matches!(
        back.items.as_slice(),
        [
            ChatItem::User(_),
            ChatItem::Notice {
                level: crate::chat::NoticeLevel::Failed,
                detail: Some(_),
                ..
            }
        ]
    ));
    let _ = std::fs::remove_dir_all(&store);
}

/// A build before failed turns had a level of their own wrote one as an error
/// reading `Error: …`; read back, it is the failed turn it was, so a session
/// that ended on one still needs attention after the upgrade. The resume
/// fallback that build wrote the same way failed no turn, and stays an error.
#[test]
fn an_old_turn_error_reads_as_failed() {
    use crate::chat::NoticeLevel;
    let store = scratch("legacy-failed");
    let mut chat = chat_in(&store, "s1");
    for text in [
        "Error: {\"code\":-32000,\"message\":\"gave up\"}",
        "Error: session/load failed (x) — starting a fresh session",
    ] {
        chat.items
            .push(ChatItem::failure(text, NoticeLevel::Error, None));
    }
    save(&mut chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    let levels: Vec<_> = back
        .items
        .iter()
        .filter_map(|it| match it {
            ChatItem::Notice { level, .. } => Some(*level),
            _ => None,
        })
        .collect();
    assert_eq!(levels, [NoticeLevel::Failed, NoticeLevel::Error]);
    let _ = std::fs::remove_dir_all(&store);
}

/// **The save after a torn line starts on a line of its own.** Appended
/// straight onto the fragment, its first item became part of a line that does
/// not parse, and was lost with it.
#[test]
fn the_save_after_a_torn_line_keeps_its_first_item() {
    let store = scratch("torn-then-saved");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("one".into(), Vec::new());
    save(&mut chat);

    let items = conv_dir(&store, "s1").join("items.jsonl");
    let mut text = std::fs::read_to_string(&items).unwrap();
    text.push_str("{\"t\":\"user\",\"tex");
    std::fs::write(&items, text).unwrap();

    chat.push_user("two".into(), Vec::new());
    save(&mut chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert_eq!(back.items.len(), 2);
    let _ = std::fs::remove_dir_all(&store);
}

/// **A line this build cannot read is not a reason to throw it away.** A newer
/// build's item kind, or a torn line, is skipped on screen; a conversation read
/// with one skipped must not be taken as the whole file, or the one rewrite
/// there is would delete it from disk.
#[test]
fn a_load_that_skipped_a_line_is_not_whole() {
    let store = scratch("skipped");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("one".into(), Vec::new());
    save(&mut chat);

    let items = conv_dir(&store, "s1").join("items.jsonl");
    let mut text = std::fs::read_to_string(&items).unwrap();
    text.push_str("{\"t\":\"from-a-newer-build\"}\n");
    std::fs::write(&items, text).unwrap();

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert!(!back.complete);
    assert_eq!(back.items.len(), 1, "and no notice about older messages");
    let _ = std::fs::remove_dir_all(&store);
}

/// **A save that failed is tried again by the next one.** The mark moves when
/// a save is built, so without handing it back a turn that never reached the
/// disk was counted as written, and nothing would ever send it.
#[test]
fn a_failed_save_is_carried_by_the_next() {
    let store = scratch("failed-save");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("lost".into(), Vec::new());
    let failed = chat.flush().unwrap();
    chat.unflush(&failed);

    chat.push_user("next".into(), Vec::new());
    save(&mut chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert_eq!(back.items.len(), 2);
    let _ = std::fs::remove_dir_all(&store);
}

/// **A step saved while it ran comes back stopped, not running.** A session
/// closed mid-turn writes its cards as they stand; restored as running, each
/// started a clock of its own and read as live work forever after.
#[test]
fn a_step_saved_running_comes_back_stopped() {
    let store = scratch("running-step");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("build it".into(), Vec::new());
    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "t1".into(),
        title: "cargo build".into(),
        description: None,
        kind: ToolKind::Execute,
        status: ToolStatus::InProgress,
        content: Vec::new(),
    })));
    save(&mut chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    let ChatItem::Tool(tool) = &back.items[1] else {
        panic!("the step came back");
    };
    assert_eq!(tool.call.status, ToolStatus::Failed);
    assert!(!tool.is_open(), "and it is not drawn as running");
    let _ = std::fs::remove_dir_all(&store);
}

fn subagent(id: &str, state: SubagentState, items: Vec<ChatItem>) -> ChatItem {
    ChatItem::Subagent(SubagentItem {
        id: id.into(),
        name: format!("explore {id}"),
        task: Some("find the config loader".into()),
        state,
        items,
    })
}

fn done_tool(id: &str) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: id.into(),
        title: "Read".into(),
        description: None,
        kind: ToolKind::Read,
        status: ToolStatus::Completed,
        content: vec![ToolContent::Text("fn main() {}".into())],
    }))
}

/// **A card still working holds the save back**, so the append-only file
/// never gets a half card: the card is written whole after it ends, once.
#[test]
fn a_working_card_holds_the_save_back() {
    use crate::acp::{AcpEvent, SubagentEnd};
    let store = scratch("working-card");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("look around".into(), Vec::new());
    chat.items.push(subagent(
        "c1",
        SubagentState::Working,
        vec![ChatItem::Agent(Md::parse("reading"))],
    ));
    chat.items.push(ChatItem::Agent(Md::parse("meanwhile")));

    let first = chat.flush().expect("the prompt is new");
    assert_eq!(first.lines.len(), 1, "only the prompt: {:?}", first.lines);
    assert!(first.lines[0].contains("\"t\":\"user\""));
    assert_eq!(first.meta.items, 1, "the count stops where the lines do");
    commit(&first).unwrap();
    assert!(chat.flush().is_none(), "nothing new while the card works");

    chat.apply(AcpEvent::SubagentEnded {
        id: "c1".into(),
        end: SubagentEnd::Completed,
    });
    let second = chat.flush().expect("the card has ended");
    assert_eq!(second.lines.len(), 2, "{:?}", second.lines);
    assert!(second.lines[0].contains("\"t\":\"subagent\""));
    assert!(second.lines[0].contains("\"state\":\"completed\""));
    assert!(second.lines[0].contains("reading"), "with its steps");
    assert!(second.lines[1].contains("meanwhile"));
    commit(&second).unwrap();
    assert!(chat.flush().is_none(), "and written once");

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert_eq!(back.items.len(), 3);
    let _ = std::fs::remove_dir_all(&store);
}

/// A finished card, nested card and all, reads back as it was written.
#[test]
fn a_subagent_card_round_trips() {
    let store = scratch("card-round-trip");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("look around".into(), Vec::new());
    chat.items.push(subagent(
        "c1",
        SubagentState::Completed,
        vec![
            ChatItem::Agent(Md::parse("found **it**")),
            done_tool("t1"),
            subagent("c2", SubagentState::Failed, vec![done_tool("t2")]),
        ],
    ));
    save(&mut chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    let ChatItem::Subagent(card) = &back.items[1] else {
        panic!("the card came back");
    };
    assert_eq!(card.id, "c1");
    assert_eq!(card.name, "explore c1");
    assert_eq!(card.task.as_deref(), Some("find the config loader"));
    assert_eq!(card.state, SubagentState::Completed);
    assert_eq!(card.items.len(), 3);
    let ChatItem::Subagent(nested) = &card.items[2] else {
        panic!("the nested card came back");
    };
    assert_eq!(nested.state, SubagentState::Failed);
    assert_eq!(nested.items.len(), 1);
    assert_eq!(
        line_of(&chat, &back.items[1]).unwrap().0,
        line_of(&chat, &chat.items[1]).unwrap().0,
        "the same line again"
    );
    let _ = std::fs::remove_dir_all(&store);
}

/// A card on disk as working (a nested one under a finished parent) has
/// nothing left to end it, so it comes back interrupted.
#[test]
fn a_card_saved_working_reads_back_interrupted() {
    let store = scratch("card-working");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("look around".into(), Vec::new());
    save(&mut chat);
    let dir = conv_dir(&store, "s1");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("items.jsonl"))
        .unwrap();
    use std::io::Write;
    writeln!(
        file,
        r#"{{"t":"subagent","id":"c1","name":"explore","state":"working","items":[{{"t":"agent","text":"half"}}]}}"#
    )
    .unwrap();

    let back = load(&dir).unwrap();
    let ChatItem::Subagent(card) = &back.items[1] else {
        panic!("the card came back");
    };
    assert_eq!(card.state, SubagentState::Interrupted);
    assert_eq!(card.task, None);
    assert_eq!(card.items.len(), 1);
    let _ = std::fs::remove_dir_all(&store);
}

/// A session closed while a card works still keeps it, and what came after
/// it: the card goes down as interrupted rather than held back for ever.
#[test]
fn a_card_working_at_close_is_kept_interrupted() {
    let store = scratch("card-at-close");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("look around".into(), Vec::new());
    chat.items
        .push(subagent("c1", SubagentState::Working, Vec::new()));
    chat.items.push(ChatItem::Agent(Md::parse("meanwhile")));
    drop(chat);

    let back = load(&conv_dir(&store, "s1")).unwrap();
    assert_eq!(back.items.len(), 3);
    let ChatItem::Subagent(card) = &back.items[1] else {
        panic!("the card came back");
    };
    assert_eq!(card.state, SubagentState::Interrupted);
    let _ = std::fs::remove_dir_all(&store);
}

/// A restart carries a working card into history; the adapter that would
/// end it is the one replaced, so it goes over interrupted and the next
/// save writes it, and what follows it.
#[test]
fn a_card_working_at_a_restart_is_written_interrupted() {
    let store = scratch("card-at-restart");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("look around".into(), Vec::new());
    save(&mut chat);
    chat.items
        .push(subagent("c1", SubagentState::Working, Vec::new()));
    chat.items.push(ChatItem::Agent(Md::parse("meanwhile")));
    let snapshot = chat.take_snapshot().unwrap();

    let mut next = chat_in(&store, "s1");
    next.load_history(snapshot.items, snapshot.session_id, snapshot.written);
    let write = next.flush().expect("the card and what follows it");
    assert_eq!(write.lines.len(), 2, "{:?}", write.lines);
    assert!(write.lines[0].contains("\"state\":\"interrupted\""));
    assert!(write.lines[1].contains("meanwhile"));
    let _ = std::fs::remove_dir_all(&store);
}

/// A card that ends ends whatever still works inside it: a nested card
/// that never reports its end would otherwise hold the parent, and all
/// after it, out of the file.
#[test]
fn an_ended_card_interrupts_its_nested_cards() {
    use crate::acp::{AcpEvent, SubagentEnd};
    let store = scratch("card-nested-end");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("look around".into(), Vec::new());
    chat.items.push(subagent(
        "c1",
        SubagentState::Working,
        vec![subagent("c2", SubagentState::Working, Vec::new())],
    ));
    assert_eq!(chat.subagents_working(), 2);
    chat.apply(AcpEvent::SubagentEnded {
        id: "c1".into(),
        end: SubagentEnd::Completed,
    });
    assert_eq!(chat.subagents_working(), 0);
    let ChatItem::Subagent(card) = &chat.items[1] else {
        panic!("the card");
    };
    let ChatItem::Subagent(nested) = &card.items[0] else {
        panic!("the nested card");
    };
    assert_eq!(card.state, SubagentState::Completed);
    assert_eq!(nested.state, SubagentState::Interrupted);
    let write = chat.flush().unwrap();
    assert_eq!(write.lines.len(), 2, "the parent is written");
    let _ = std::fs::remove_dir_all(&store);
}

#[test]
fn agent_message_id_round_trips() {
    let store = scratch("message-id");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("go".into(), Vec::new());
    let mut md = Md::parse("answer");
    md.message = Some("m1".into());
    chat.items.push(ChatItem::Agent(md));
    save(&mut chat);

    let dir = conv_dir(&store, "s1");
    let file = std::fs::read_to_string(dir.join("items.jsonl")).unwrap();
    assert!(file.contains(r#""message_id":"m1""#));
    let back = load(&dir).unwrap();
    let Some(ChatItem::Agent(md)) = back.items.get(1) else {
        panic!("the answer comes back");
    };
    assert_eq!(md.message.as_deref(), Some("m1"));
}

#[test]
fn an_old_agent_line_reads_without_an_id() {
    let store = scratch("old-agent-line");
    let mut chat = chat_in(&store, "s1");
    chat.push_user("go".into(), Vec::new());
    chat.items.push(ChatItem::Agent(Md::parse("answer")));
    save(&mut chat);

    let dir = conv_dir(&store, "s1");
    let file = std::fs::read_to_string(dir.join("items.jsonl")).unwrap();
    assert!(!file.contains("message_id"));
    let back = load(&dir).unwrap();
    let Some(ChatItem::Agent(md)) = back.items.get(1) else {
        panic!("the answer comes back");
    };
    assert_eq!(md.message, None);
}
