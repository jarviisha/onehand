use super::permission_title;
use onehand_core::acp::ToolKind;

#[test]
fn a_permission_asks_its_own_question() {
    assert_eq!(
        permission_title(ToolKind::Execute, "cargo test -p retry"),
        "Run a command?",
        "a command is shown in the well, not asked back as the title"
    );
    assert_eq!(
        permission_title(ToolKind::Edit, "Edit `src/backoff.rs`"),
        "Edit src/backoff.rs?",
        "a title that already reads as the question is asked back"
    );
    assert_eq!(
        permission_title(ToolKind::Edit, "/abs/src/backoff.rs"),
        "Edit a file?",
        "a bare path is no question"
    );
    assert_eq!(
        permission_title(ToolKind::Other, "mcp__x__y"),
        "Allow this?"
    );
}
