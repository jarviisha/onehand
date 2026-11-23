use super::*;

#[test]
fn the_lead_in_follows_the_words() {
    let text = lead_in(
        "fix the parser",
        Path::new("/p/.onehand/continued/a.md"),
        "Parser",
        false,
    );
    assert!(text.starts_with("fix the parser"));
    assert!(text.contains("\"Parser\""));
    assert!(text.contains("/p/.onehand/continued/a.md"));
    assert!(!text.contains("summary"));
    let summarised = lead_in("go", Path::new("/a.md"), "T", true);
    assert!(summarised.contains("summary"));
}

#[test]
fn a_hand_off_is_written_where_git_ignores_it() {
    let root = std::env::temp_dir().join(format!("onehand-handoff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (store, folder) = (root.join("store"), root.join("work"));
    std::fs::create_dir_all(&folder).unwrap();
    let mut chat = crate::chat::Chat::new(1, folder.clone(), "agent".into(), Some(store), None);
    chat.session_id = Some("sess-1".into());
    chat.items
        .push(crate::chat::ChatItem::notice("hello there"));
    let write = chat.flush().expect("a write");
    crate::chat::store::commit(&write).unwrap();
    let kept = chat.conversation_dir().unwrap();
    let file = write_blocking(&kept, &folder).unwrap();
    assert!(file.starts_with(folder.join(".onehand/continued")));
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("onehand conversation"));
    assert_eq!(
        std::fs::read_to_string(folder.join(".onehand/.gitignore")).unwrap(),
        "*\n"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_continued_conversation_says_what_it_continued() {
    let root = std::env::temp_dir().join(format!("onehand-continued-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = root.join("store");
    let mut chat = crate::chat::Chat::new(
        1,
        root.join("work"),
        "agent".into(),
        Some(store.clone()),
        None,
    );
    chat.session_id = Some("sess-2".into());
    chat.continues("Parser work".into());
    chat.items.push(crate::chat::ChatItem::notice("hi"));
    let write = chat.flush().expect("a write");
    crate::chat::store::commit(&write).unwrap();
    let (_, meta) = crate::chat::store::find_conversation(&store, "sess-2").expect("listed");
    assert_eq!(meta.continued.as_deref(), Some("Parser work"));
    let _ = std::fs::remove_dir_all(root);
}
