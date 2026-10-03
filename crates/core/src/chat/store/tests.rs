use super::*;

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
    first.apply(AcpEvent::AgentChunk("it does this".into()));
    save(&mut first);
    assert_eq!(load(&dir).unwrap().items.len(), 2);

    // Reopened, and the adapter replays exactly what is already there.
    let mut next = chat_in(&store, "s1");
    next.resume_from(load(&dir).unwrap());
    next.apply(AcpEvent::UserChunk("what does this do".into()));
    next.apply(AcpEvent::AgentChunk("it does this".into()));
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
