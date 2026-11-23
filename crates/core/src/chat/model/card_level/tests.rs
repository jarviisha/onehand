use super::*;
use crate::acp::{AcpEvent, ElicitField, ElicitKind, Elicitation, PermissionOption};
use tokio::sync::mpsc::UnboundedReceiver;

fn up() -> (Chat, UnboundedReceiver<AcpRequest>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.tx = Some(tx);
    chat.root = work();
    (chat, rx)
}

/// A real folder standing for a session's, so its links can be resolved.
fn work() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-cards-work-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir.canonicalize().unwrap()
}

fn asks(kind: ToolKind) -> AcpEvent {
    asks_at(kind, &["/work/app/src/main.rs"])
}

fn asks_at(kind: ToolKind, paths: &[&str]) -> AcpEvent {
    AcpEvent::Permission(PermissionRequest {
        rpc_id: serde_json::Value::from(7),
        tool_call_id: None,
        kind,
        child: None,
        // Written against `/work/app`, the session's folder in these tests.
        locations: paths
            .iter()
            .map(|path| match path.strip_prefix("/work/app") {
                Some(rest) => std::path::PathBuf::from(format!("{}{rest}", work().display())),
                None => std::path::PathBuf::from(path),
            })
            .collect(),
        title: "do it".into(),
        options: vec![
            PermissionOption {
                id: "allow-1".into(),
                name: "Allow once".into(),
                kind: "allow_once".into(),
            },
            PermissionOption {
                id: "reject-1".into(),
                name: "Deny".into(),
                kind: "reject_once".into(),
            },
        ],
    })
}

fn granted(chat: &mut Chat) -> Option<String> {
    chat.take_requests().into_iter().find_map(|r| match r {
        AcpRequest::PermissionResponse { option_id, .. } => option_id,
        _ => None,
    })
}

#[test]
fn allow_edits_allows_an_edit_and_leaves_a_command() {
    let (mut chat, _rx) = up();
    chat.set_cards(Cards::AllowEdits);
    let outcome = chat.apply(asks(ToolKind::Edit));
    assert_eq!(granted(&mut chat).as_deref(), Some("allow-1"));
    assert!(
        outcome.asked_user.is_none(),
        "nobody is told of a card answered"
    );
    let outcome = chat.apply(asks(ToolKind::Execute));
    assert_eq!(granted(&mut chat), None);
    assert!(outcome.asked_user.is_some());
}

#[test]
fn manual_answers_nothing() {
    let (mut chat, _rx) = up();
    chat.apply(asks(ToolKind::Edit));
    assert_eq!(granted(&mut chat), None);
}

#[test]
fn bypass_answers_a_question() {
    let (mut chat, _rx) = up();
    chat.set_cards(Cards::Bypass);
    chat.apply(AcpEvent::Elicitation(Elicitation {
        rpc_id: serde_json::json!(1),
        tool_call_id: None,
        child: None,
        message: "which?".into(),
        fields: vec![ElicitField {
            key: "q".into(),
            title: None,
            description: None,
            kind: ElicitKind::Text,
            custom_key: None,
        }],
    }));
    let answered = chat
        .take_requests()
        .into_iter()
        .any(|r| matches!(r, AcpRequest::ElicitationResponse { .. }));
    assert!(answered);
}

#[test]
fn raising_the_level_answers_what_waits() {
    let (mut chat, _rx) = up();
    chat.apply(asks(ToolKind::Execute));
    assert_eq!(granted(&mut chat), None);
    chat.set_cards(Cards::Bypass);
    assert_eq!(granted(&mut chat).as_deref(), Some("allow-1"));
}

#[test]
fn the_level_is_kept_with_the_conversation() {
    let (mut chat, _rx) = up();
    chat.set_cards(Cards::AllowEdits);
    let prefs = chat.prefs();
    assert_eq!(prefs.cards, Cards::AllowEdits);
    let mut back = Chat::default();
    back.take_prefs_cards(&prefs);
    assert_eq!(back.cards(), Cards::AllowEdits);
}

/// Allow edits allows only work inside the session's folder that it can
/// name, and never what would let the agent give itself more: git's own
/// folder, an agent's settings, or anything outside.
#[test]
fn allow_edits_stays_inside_the_folder() {
    for paths in [
        &[][..],
        &["/home/me/.bashrc"][..],
        &["/work/app/.git/hooks/pre-commit"][..],
        &["/work/app/.claude/settings.local.json"][..],
        &["/work/app/../other/x"][..],
        &["src/relative.rs"][..],
        &["/work/app/src/a.rs", "/etc/passwd"][..],
    ] {
        let (mut chat, _rx) = up();
        chat.set_cards(Cards::AllowEdits);
        chat.apply(asks_at(ToolKind::Edit, paths));
        assert_eq!(granted(&mut chat), None, "{paths:?}");
    }
    let (mut chat, _rx) = up();
    chat.set_cards(Cards::AllowEdits);
    chat.apply(asks_at(
        ToolKind::Read,
        &["/work/app/README.md", "/work/app/src"],
    ));
    assert_eq!(granted(&mut chat).as_deref(), Some("allow-1"));
}

/// A link inside the folder that leads out of it is out of it: what counts
/// is where the work lands, not how its path is spelled.
#[test]
fn allow_edits_follows_links_out_of_the_folder() {
    let base = std::env::temp_dir().join(format!("onehand-cards-links-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (root, elsewhere) = (base.join("work"), base.join("elsewhere"));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join("out")).unwrap();
    std::os::unix::fs::symlink(root.join(".git-real"), root.join("hooks-link")).ok();
    let ask = |path: std::path::PathBuf| {
        let (mut chat, _rx) = up();
        chat.root = root.clone();
        chat.set_cards(Cards::AllowEdits);
        chat.apply(asks_at(ToolKind::Edit, &[path.to_str().unwrap()]));
        granted(&mut chat)
    };
    assert_eq!(ask(root.join("out/x.rs")), None, "a link out of the folder");
    assert_eq!(
        ask(root.join("src/new.rs")).as_deref(),
        Some("allow-1"),
        "a new file in it"
    );
    assert_eq!(
        ask(root.join("a/b/c.rs")).as_deref(),
        Some("allow-1"),
        "folders yet to be made"
    );
    let _ = std::fs::remove_dir_all(base);
}

/// A link to something not made yet is a link all the same: writing through
/// it lands wherever it points, so it is never allowed.
#[test]
fn allow_edits_refuses_a_link_to_nothing_yet() {
    let base = std::env::temp_dir().join(format!("onehand-cards-dangling-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("work");
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(base.join("not-yet/new.sh"), root.join("out")).unwrap();
    let (mut chat, _rx) = up();
    chat.root = root.clone();
    chat.set_cards(Cards::AllowEdits);
    chat.apply(asks_at(
        ToolKind::Edit,
        &[root.join("out").to_str().unwrap()],
    ));
    assert_eq!(granted(&mut chat), None);
    let _ = std::fs::remove_dir_all(base);
}
