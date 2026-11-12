use super::*;
use crate::acp::AcpEvent;
use crate::chat::model::Now;
use crate::chat::SessionDrive;

fn now() -> Now {
    Now {
        secs: 1,
        at: std::time::Instant::now(),
    }
}

/// Nothing is owed before the agent names the conversation; the note once
/// when it does, about that conversation, and nothing after.
#[test]
fn a_sessions_note_is_owed_once_its_conversation_is_named() {
    let mut drive = SessionDrive::new(Chat::default());
    let mut owed = TakenUp::new(PathBuf::from("/store/p.json"), 3);
    assert!(owed.heard(&drive).is_none());
    let _ = drive.apply(AcpEvent::SessionId("s-1".into()), now());
    let (file, change) = owed.heard(&drive).expect("named");
    assert_eq!(file, PathBuf::from("/store/p.json"));
    assert_eq!(
        change,
        Change::TakenUp {
            number: 3,
            text: TAKEN_UP_HERE.into(),
            session: "s-1".into(),
        }
    );
    assert!(owed.heard(&drive).is_none());
    let _ = drive.apply(AcpEvent::SessionId("s-2".into()), now());
    assert!(owed.heard(&drive).is_none());
}
