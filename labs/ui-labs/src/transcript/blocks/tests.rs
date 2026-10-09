use super::*;

fn tool(kind: Kind, state: State) -> Tool {
    Tool {
        id: "t",
        kind,
        verb: "",
        object: "cargo test",
        state,
        detail: Detail::None,
    }
}

#[test]
fn backticks_mark_code_and_leave_the_text() {
    let (text, spans) = code_spans("call `wait` then `now()`.");
    assert_eq!(text, "call wait then now().");
    assert_eq!(spans, [5..9, 15..20]);
    assert!(code_spans("plain").1.is_empty());
}

#[test]
fn the_summary_counts_each_kind_once_running_first() {
    let tools = [
        tool(Kind::Read, State::Done),
        tool(Kind::Run, State::Failed("exit 1")),
        tool(Kind::Read, State::Done),
        tool(Kind::Search, State::Done),
    ];
    assert_eq!(
        summary(&tools),
        "Read 2 files, ran 1 command, searched 1 time"
    );
    let running = [
        tool(Kind::Read, State::Done),
        tool(Kind::Run, State::Running),
    ];
    assert_eq!(summary(&running), "Running cargo test, read 1 file");
    let both = [
        tool(Kind::Run, State::Running),
        tool(Kind::Read, State::Running),
    ];
    assert_eq!(summary(&both), "Running cargo test, running cargo test");
}

#[test]
fn a_failure_is_told_from_a_count_of_none() {
    assert!(failing("test backoff_caps ... FAILED"));
    assert!(failing(
        "thread 'backoff_caps' panicked at 'elapsed 5.2s > 5s'"
    ));
    assert!(failing("test result: FAILED. 11 passed; 1 failed"));
    assert!(failing("error[E0308]: mismatched types"));
    assert!(!failing("test result: ok. 12 passed; 0 failed"));
    assert!(!failing("test backoff_caps ... ok"));
}
