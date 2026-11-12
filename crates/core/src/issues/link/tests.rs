use super::*;
use crate::acp::AcpEvent;
use crate::chat::SessionDrive;
use crate::Now;

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

/// A project gone, or a workspace keeping no issues, is said; otherwise the
/// file the note goes in.
#[test]
fn an_issue_is_worked_here_only_on_a_project_kept_with_its_issues() {
    let file = PathBuf::from("/store/p.json");
    assert_eq!(
        work_here::<u8>(None, Some(file.clone())),
        Err(NotHere::ProjectGone)
    );
    assert_eq!(work_here::<u8>(None, None), Err(NotHere::ProjectGone));
    assert_eq!(work_here(Some(2), None), Err(NotHere::NoIssuesFile));
    assert_eq!(work_here(Some(2), Some(file.clone())), Ok((2, file)));
}
