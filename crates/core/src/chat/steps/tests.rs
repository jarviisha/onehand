use super::*;
use crate::acp::{ToolCall, ToolKind, ToolStatus};
use crate::chat::model::ToolItem;

fn run(title: &str, status: ToolStatus) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: title.into(),
        title: title.into(),
        description: None,
        kind: ToolKind::Execute,
        status,
        content: Vec::new(),
    }))
}

/// A write of `new` over `old` at `path`.
fn wrote(id: &str, diffs: &[(&str, &str, &str)]) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: id.into(),
        title: id.into(),
        description: None,
        kind: ToolKind::Edit,
        status: ToolStatus::Completed,
        content: diffs
            .iter()
            .map(|(path, old, new)| crate::acp::ToolContent::Diff {
                path: (*path).to_string(),
                old: Some((*old).to_string()),
                new: (*new).to_string(),
            })
            .collect(),
    }))
}

/// A file written more than once in a turn is one row, not one per write.
///
/// The question the summary answers is what is different now, and three
/// rows naming one file answer how the agent got there instead -- which
/// the transcript above it is already the full account of.
#[test]
fn a_file_written_twice_is_summed_into_one_row() {
    let items = [
        wrote("first", &[("src/lib.rs", "a\nb\n", "1\n2\n3\n")]),
        run("cargo test", ToolStatus::Failed),
        wrote(
            "again",
            &[
                ("src/lib.rs", "1\n2\n3\n", "1\n9\n3\n"),
                ("src/main.rs", "x\n", "x\ny\n"),
            ],
        ),
    ];
    let items: Vec<&ChatItem> = items.iter().collect();
    let changes = turn_changes(&items).expect("something was written");

    let named: Vec<(&str, usize, usize, usize)> = changes
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.added, f.removed, f.total))
        .collect();
    assert_eq!(
        named,
        // In the order the turn first touched them, which is the order the
        // reader watched it happen in, and the counts of both writes added
        // together.
        vec![("src/lib.rs", 4, 3, 3), ("src/main.rs", 1, 0, 2)]
    );
    assert_eq!((changes.added, changes.removed), (5, 3));
    // Both existed before the turn and both survive it.
    assert!(changes
        .files
        .iter()
        .all(|f| f.verdict == FileVerdict::Modified));

    // The turn's diff for a file written twice runs from before the first
    // write to after the last, not from the last write alone.
    assert!(!turn_file_diff(&items, "src/lib.rs").is_empty());
}

/// A failed edit is not part of what a turn wrote.
///
/// The diff on a failed call is what the agent proposed, not what is on
/// disk -- counting it puts a file in a list headed "what this turn wrote"
/// that the turn did not write.
#[test]
fn a_failed_edit_is_not_counted_as_written() {
    let mut failed = wrote("nope", &[("src/gone.rs", "a\n", "b\n")]);
    if let ChatItem::Tool(tool) = &mut failed {
        tool.call.status = ToolStatus::Failed;
    }
    let items = [failed, wrote("ok", &[("src/lib.rs", "x\n", "x\ny\n")])];
    let items: Vec<&ChatItem> = items.iter().collect();

    let changes = turn_changes(&items).expect("one file was written");
    assert_eq!(changes.files.len(), 1);
    assert_eq!(changes.files[0].path, "src/lib.rs");
    assert!(turn_file_diff(&items, "src/gone.rs").is_empty());
}

/// A turn that only read files has nothing to summarise.
#[test]
fn a_turn_that_wrote_nothing_has_no_summary() {
    let items = [run("rg todo", ToolStatus::Completed)];
    let items: Vec<&ChatItem> = items.iter().collect();
    assert_eq!(turn_changes(&items), None);
}

#[test]
fn a_secret_never_reaches_a_line() {
    let connection = "sqlcmd -S 10.0.0.5 -U sa -P Hunter2 \
             -Q \"SELECT 1\"";
    let masked = redact(connection);
    assert!(!masked.contains("Hunter2"), "{masked}");
    // The user and the host are what make the line worth drawing at all.
    assert!(masked.contains("sa"), "{masked}");
    assert!(masked.contains("10.0.0.5"), "{masked}");

    let pairs = "Server=db;User Id=sa;Password=p@ss w0rd;Encrypt=false";
    let masked = redact(pairs);
    assert!(!masked.contains("p@ss"), "{masked}");
    assert!(masked.contains("Encrypt=false"), "{masked}");

    // URL-encoded is the same value in another spelling, so masking the
    // whole of it covers both without knowing which it was.
    let url = "curl 'https://api.example.com/v1?api_key=abc%2Fdef&page=2'";
    let masked = redact(url);
    assert!(!masked.contains("abc%2Fdef"), "{masked}");
    assert!(masked.contains("page=2"), "{masked}");

    let authority = "psql postgres://sa:s3cret@10.0.0.5:5432/app";
    let masked = redact(authority);
    assert!(!masked.contains("s3cret"), "{masked}");
    assert!(masked.contains("sa:"), "{masked}");
    assert!(masked.contains("10.0.0.5"), "{masked}");

    // **A password may hold an `@` of its own**, which is legal and common.
    // Split at the first one, everything after it fell outside the masked
    // span and was printed.
    let awkward = "psql postgres://sa:p@ssw0rd@10.0.0.5:5432/app";
    let masked = redact(awkward);
    assert!(!masked.contains("ssw0rd"), "{masked}");
    assert!(masked.contains("10.0.0.5:5432/app"), "{masked}");

    // A word that merely ends in the letters of a secret is not one.
    assert_eq!(redact("--bypass=true"), "--bypass=true");
    assert_eq!(redact("--key=value"), "--key=value");

    // **Everything around the secret survives byte for byte.** This runs
    // over the command a permission card shows, which claims to be what the
    // agent actually ran -- so a script that came back reflowed would be a
    // grant given on one text and spent on another.
    let script =
        "#!/usr/bin/env bash\nset -euo pipefail\n\tmysql -p hunter2 <<'EOF'\n  SELECT 1;\nEOF";
    let masked = redact(script);
    assert!(!masked.contains("hunter2"), "{masked}");
    assert!(
        masked.starts_with("#!/usr/bin/env bash\nset -euo pipefail\n\tmysql"),
        "{masked}"
    );
    assert!(masked.contains("<<'EOF'\n  SELECT 1;\nEOF"), "{masked}");
    // A command with no secret in it comes back exactly as it went in.
    let plain = "git commit -m 'two  spaces'\n\tdocs/plan.md";
    assert_eq!(redact(plain), plain);
}

fn read(title: &str) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: title.into(),
        title: title.into(),
        description: None,
        kind: ToolKind::Read,
        status: ToolStatus::Completed,
        content: Vec::new(),
    }))
}

/// **The one line a cluster says about itself.**
///
/// Kinds of work in the order they happened, each with a count — not one
/// phrase per step, which costs as much to scan folded as unfolded, and not
/// a bare total, which says how much without saying what.
#[test]
fn a_cluster_says_what_it_did_and_in_what_order() {
    let members = [
        read("a.rs"),
        read("b.rs"),
        run("cargo build", ToolStatus::Completed),
        read("c.rs"),
        run("cargo test", ToolStatus::Failed),
    ];
    let summary = cluster_summary(&members.iter().collect::<Vec<_>>());
    // Three reads are one phrase, wherever the third one happened; the
    // order is where each *kind* first appeared.
    assert_eq!(summary.plain(), "Read 3 files, ran 2 commands");
    assert_eq!(summary.errors, 1);
    assert!(summary.running.is_none());
    // Only the opening word is capitalised: a second capital mid-line
    // reads as two sentences run together.
    assert_eq!(summary.done[1].verb, "ran");
}

/// **A duration is stamped once, when a step settles.**
///
/// The other two readings are both wrong: measured from the row that draws
/// it, the number changes every frame and never comes to rest even after
/// the step has; measured again on each later update — and several arrive
/// after the status does, since content follows it — the clock restarts and
/// the step reports the time since it finished.
#[test]
fn a_step_is_timed_once_and_the_cluster_adds_them_up() {
    use crate::acp::{AcpEvent, ToolCallUpdate};

    let mut chat = crate::chat::model::Chat::default();
    chat.apply(AcpEvent::ToolCall(ToolCall {
        id: "one".into(),
        title: "cargo build".into(),
        description: None,
        kind: ToolKind::Execute,
        status: ToolStatus::InProgress,
        content: Vec::new(),
    }));
    let settle = |chat: &mut crate::chat::model::Chat| {
        chat.apply(AcpEvent::ToolUpdate(ToolCallUpdate {
            id: "one".into(),
            status: Some(ToolStatus::Completed),
            title: None,
            description: None,
            content: None,
        }));
    };
    settle(&mut chat);
    let stamped = match &chat.items[0] {
        ChatItem::Tool(t) => t.elapsed_secs,
        _ => unreachable!(),
    };
    assert!(stamped.is_some(), "settling is when the duration exists");

    // Content arrives after the status on a real adapter, and each of those
    // is another update to the same card.
    settle(&mut chat);
    let again = match &chat.items[0] {
        ChatItem::Tool(t) => t.elapsed_secs,
        _ => unreachable!(),
    };
    assert_eq!(stamped, again, "a later update must not restart the clock");

    // A step that arrived already settled was timed by whoever ran it.
    let done = run("cargo test", ToolStatus::Completed);
    assert!(matches!(&done, ChatItem::Tool(t) if t.elapsed_secs.is_none()));
}

/// **The exit status is kept on the way past, or it is lost.**
///
/// A finished terminal is folded into the card it belonged to at turn end,
/// and after that the code is a line inside a string. Recovering it would
/// mean parsing the footer back out — a number recovered by parsing is a
/// number that is wrong the first time the wording changes — so it is
/// lifted onto the step in the one moment both are in hand.
#[test]
fn an_exit_status_survives_the_terminal_it_came_from() {
    use crate::acp::AcpEvent;

    let mut chat = crate::chat::model::Chat::default();
    chat.apply(AcpEvent::ToolCall(ToolCall {
        id: "t".into(),
        title: "cargo test".into(),
        description: None,
        kind: ToolKind::Execute,
        status: ToolStatus::InProgress,
        content: vec![crate::acp::ToolContent::Terminal("term-1".into())],
    }));
    chat.apply(AcpEvent::TerminalExit {
        terminal_id: "term-1".into(),
        exit_code: Some(101),
    });
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });

    let ChatItem::Tool(step) = &chat.items[0] else {
        unreachable!()
    };
    assert_eq!(step.exit_code, Some(101));
    // And the terminal is gone: the map only ever holds live ones.
    assert!(chat.terminals.is_empty());
}

/// A cluster of one is a cluster, and says so the same way.
#[test]
fn one_step_still_gets_a_sentence() {
    let summary = cluster_summary(&[&read("src/lib.rs")]);
    assert_eq!(summary.plain(), "Read 1 file");
}

/// **What is still happening leads, in the present tense.** A line opening
/// with what is finished buries the one part of it still changing.
#[test]
fn a_running_cluster_leads_with_what_it_is_doing() {
    let members = [
        read("a.rs"),
        read("b.rs"),
        run("dotnet test ./src/Api.Tests", ToolStatus::InProgress),
    ];
    let summary = cluster_summary(&members.iter().collect::<Vec<_>>());
    let running = summary.running.as_ref().expect("something is running");
    assert_eq!(running.verb, "Running");
    assert!(running.rest.contains("dotnet"), "{running:?}");
    // The step in flight has not been done yet, so it is not counted among
    // the things that have.
    assert_eq!(summary.plain(), "Running dotnet test · read 2 files");
}

/// Internal steps go to the end wherever they happened, and take no verb.
#[test]
fn the_agents_own_housekeeping_goes_last() {
    let other = |title: &str| {
        ChatItem::Tool(ToolItem::new(ToolCall {
            id: title.into(),
            title: title.into(),
            description: None,
            kind: ToolKind::Other,
            status: ToolStatus::Completed,
            content: Vec::new(),
        }))
    };
    let members = [other("ToolSearch"), read("a.rs"), other("TaskStop")];
    let summary = cluster_summary(&members.iter().collect::<Vec<_>>());
    assert_eq!(summary.plain(), "Read 1 file, 2 other steps");
}

#[test]
fn a_run_is_read_by_how_it_ended() {
    let clean = [run("a", ToolStatus::Completed)];
    assert_eq!(
        run_outcome(&clean.iter().collect::<Vec<_>>()),
        RunOutcome {
            outcome: Outcome::Clean,
            errors: 0
        }
    );

    // Fixed, so not an alarm — but the count of what went wrong is kept.
    let recovered = [
        run("a", ToolStatus::Failed),
        run("b", ToolStatus::Failed),
        run("c", ToolStatus::Completed),
    ];
    assert_eq!(
        run_outcome(&recovered.iter().collect::<Vec<_>>()),
        RunOutcome {
            outcome: Outcome::Recovered,
            errors: 2
        }
    );

    let broken = [
        run("a", ToolStatus::Completed),
        run("b", ToolStatus::Failed),
    ];
    assert_eq!(
        run_outcome(&broken.iter().collect::<Vec<_>>()).outcome,
        Outcome::Failed
    );

    let live = [
        run("a", ToolStatus::Failed),
        run("b", ToolStatus::InProgress),
    ];
    assert_eq!(
        run_outcome(&live.iter().collect::<Vec<_>>()).outcome,
        Outcome::Running
    );
}
