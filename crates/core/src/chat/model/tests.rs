use super::meta::TITLE_MAX_CHARS;
use super::*;
use crate::acp::ConfigChoice;
use crate::acp::{
    AcpEvent, AcpRequest, ConfigOption, ElicitKind, ElicitValue, Elicitation, Mode,
    PermissionRequest, PlanEntry, PlanStatus, ToolCall, ToolContent, ToolStatus,
};
use crate::attachment::StagedAttachment;
use crate::chat::store::{ConfigPick, ConversationSnapshot, Prefs};
use std::path::PathBuf;

#[test]
fn title_uses_first_nonblank_line_collapsed() {
    assert_eq!(
        summarize_title("\n\n  fix   the   bug \nsecond line").as_deref(),
        Some("Fix the bug"),
    );
}

#[test]
fn title_removes_request_filler_and_question_suffixes() {
    assert_eq!(
        summarize_title("Could you please fix the flaky login test?").as_deref(),
        Some("Fix the flaky login test"),
    );
    assert_eq!(
        summarize_title("Có thể giúp mình sửa luồng đăng nhập được không nhỉ?").as_deref(),
        Some("Sửa luồng đăng nhập"),
    );
}

#[test]
fn title_rewrites_make_better_as_a_task() {
    assert_eq!(
        summarize_title("có thể làm cho phần tên Conversation đẹp hơn được không nhỉ").as_deref(),
        Some("Cải thiện phần tên Conversation"),
    );
    assert_eq!(
        summarize_title("Can you make the conversation names better?").as_deref(),
        Some("Improve the conversation names"),
    );
}

#[test]
fn title_caps_at_a_word_boundary() {
    let t = summarize_title(
        "Implement OAuth callback handling for the desktop application and document every edge case discovered",
    )
    .unwrap();
    assert_eq!(
        t,
        "Implement OAuth callback handling for the desktop application and document every edge case…",
    );
    assert!(t.chars().count() <= TITLE_MAX_CHARS + 1);
}

#[test]
fn title_is_none_for_blank_prompt() {
    assert_eq!(summarize_title("   \n\t "), None);
}

#[test]
fn derived_title_prefers_first_user_prompt() {
    let mut chat = Chat::default();
    chat.items.push(ChatItem::Agent(Md::parse("hi")));
    chat.items
        .push(ChatItem::User(UserMsg::text("Add a title per session")));
    chat.items
        .push(ChatItem::User(UserMsg::text("second prompt")));
    assert_eq!(
        chat.derived_title().as_deref(),
        Some("Add a title per session")
    );
}

#[test]
fn derived_title_is_none_before_any_prompt() {
    let chat = Chat::default();
    assert_eq!(chat.derived_title(), None);
}

#[test]
fn custom_title_overrides_and_reset_restores_automatic_title() {
    let mut chat = Chat::default();
    chat.items
        .push(ChatItem::User(UserMsg::text("Fix the login flow")));

    assert!(chat.rename("  Auth   cleanup\n"));
    assert_eq!(chat.conversation_title().as_deref(), Some("Auth cleanup"));

    chat.reset_title();
    assert_eq!(
        chat.conversation_title().as_deref(),
        Some("Fix the login flow")
    );
}

#[test]
fn blank_custom_title_is_ignored() {
    let mut chat = Chat::default();
    assert!(!chat.rename("  \n\t "));
    assert!(chat.custom_title.is_none());
}

#[test]
fn tool_out_sections_fold_independently() {
    // Two text outputs on one tool must fold independently (per-section
    // indices in `out_open`), and the whole-card fold is its own flag.
    let mut chat = Chat::default();
    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "t1".into(),
        title: "cmd".into(),
        description: None,
        kind: crate::acp::ToolKind::Other,
        status: ToolStatus::Completed,
        content: vec![],
    })));
    chat.toggle_tool_output(TranscriptItemId::Live(0), 0);
    let ChatItem::Tool(t) = &chat.items[0] else {
        panic!()
    };
    assert!(t.out_open.contains(&0));
    assert!(!t.out_open.contains(&1));
    assert!(!t.is_open(), "completed + unfolded card stays collapsed");
    chat.toggle_tool(TranscriptItemId::Live(0));
    let ChatItem::Tool(t) = &chat.items[0] else {
        panic!()
    };
    assert!(t.is_open());
}

#[test]
fn only_running_tools_start_open() {
    let call = |status| ToolCall {
        id: "t".into(),
        title: String::new(),
        description: None,
        kind: crate::acp::ToolKind::Other,
        status,
        content: vec![],
    };
    assert!(ToolItem::new(call(ToolStatus::InProgress)).is_open());
    assert!(!ToolItem::new(call(ToolStatus::Completed)).is_open());
    assert!(!ToolItem::new(call(ToolStatus::Pending)).is_open());
    // A failure opens itself too, but through the fold rather than through
    // this -- see `a_failure_opens_itself_and_can_still_be_shut`.
    assert!(!ToolItem::new(call(ToolStatus::Failed)).is_open());
}

/// **A failure opens itself, and the user can still shut it.**
///
/// The second half is the trap. Written as `is_open() = fold || Failed`,
/// which is how the running case is written, a terminal status forces the
/// row open *for ever*: the control that shuts it does nothing, and the one
/// state that most wants a way out is the one with none. Running gets away
/// with the OR because it stops being true on its own.
#[test]
fn a_failure_opens_itself_and_can_still_be_shut() {
    let mut chat = Chat::default();
    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "failed".into(),
        title: "cargo test".into(),
        description: None,
        kind: crate::acp::ToolKind::Execute,
        status: ToolStatus::Failed,
        content: vec![ToolContent::Text("Exit code 1".into())],
    })));

    let open = |chat: &Chat| match &chat.items[0] {
        ChatItem::Tool(tool) => tool.is_open(),
        _ => false,
    };
    // Arriving already failed is how an adapter reports a step it never
    // started, and it opens on the same rule.
    chat.apply(crate::acp::AcpEvent::ToolCall(ToolCall {
        id: "late".into(),
        title: "cargo build".into(),
        description: None,
        kind: crate::acp::ToolKind::Execute,
        status: ToolStatus::Failed,
        content: vec![],
    }));
    let late = chat.items.len() - 1;
    assert!(
        matches!(&chat.items[late], ChatItem::Tool(t) if t.is_open()),
        "a failure opens itself"
    );
    chat.toggle_tool(TranscriptItemId::Live(late));
    assert!(
        matches!(&chat.items[late], ChatItem::Tool(t) if !t.is_open()),
        "and shuts when told to"
    );

    // And a step that *becomes* a failure mid-turn does the same.
    chat.apply(crate::acp::AcpEvent::ToolCall(ToolCall {
        id: "running".into(),
        title: "cargo test".into(),
        description: None,
        kind: crate::acp::ToolKind::Execute,
        status: ToolStatus::InProgress,
        content: vec![],
    }));
    chat.apply(crate::acp::AcpEvent::ToolUpdate(
        crate::acp::ToolCallUpdate {
            id: "running".into(),
            status: Some(ToolStatus::Failed),
            title: None,
            description: None,
            content: None,
        },
    ));
    let broke = chat.items.len() - 1;
    assert!(
        matches!(&chat.items[broke], ChatItem::Tool(t) if t.is_open()),
        "a step that breaks opens itself"
    );
    chat.toggle_tool(TranscriptItemId::Live(broke));
    assert!(
        matches!(&chat.items[broke], ChatItem::Tool(t) if !t.is_open()),
        "and shuts when told to"
    );
    let _ = open;
}

#[test]
fn plan_updates_replace_in_place_per_turn() {
    // The agent republishes the full checklist on every change: within one
    // turn the card updates in place (fold survives); a new turn gets its
    // own card.
    let entry = |s: &str, st| PlanEntry {
        content: s.into(),
        status: st,
    };
    let mut chat = Chat::default();
    chat.push_user("go".into(), Vec::new());
    chat.apply(AcpEvent::Plan(vec![entry("a", PlanStatus::Pending)]));
    chat.toggle_tool(TranscriptItemId::Live(1)); // user opens the card
    chat.apply(AcpEvent::Plan(vec![
        entry("a", PlanStatus::Completed),
        entry("b", PlanStatus::InProgress),
    ]));
    let plans: Vec<&PlanItem> = chat
        .items
        .iter()
        .filter_map(|i| match i {
            ChatItem::Plan(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(plans.len(), 1, "same turn replaces in place");
    assert_eq!(plans[0].entries.len(), 2);
    assert!(plans[0].fold, "user fold survives the update");
    assert!(plans[0].is_open(), "in_progress force-opens");

    chat.push_user("next".into(), Vec::new());
    chat.apply(AcpEvent::Plan(vec![entry("c", PlanStatus::Pending)]));
    let count = chat
        .items
        .iter()
        .filter(|i| matches!(i, ChatItem::Plan(_)))
        .count();
    assert_eq!(count, 2, "a new turn gets its own card");
}

#[test]
fn streamed_chunks_append_to_one_block() {
    let mut md = Md::parse("```rust\nlet x = 1;\n");
    md.push("```\n\ntext\n");
    assert_eq!(md.source, "```rust\nlet x = 1;\n```\n\ntext\n");
}

#[test]
fn turn_answer_combines_a_turn_and_marks_the_last_block() {
    // A turn split across a tool: two agent blocks bracketing one tool call.
    let mut chat = Chat::default();
    let mut user = UserMsg::text("hi");
    user.sent_at = Some(100);
    user.completed_at = Some(112);
    chat.items.push(ChatItem::User(user));
    chat.items.push(ChatItem::Agent(Md::parse("first")));
    chat.items.push(ChatItem::notice("a tool ran"));
    chat.items.push(ChatItem::Agent(Md::parse("second")));

    // Copy from either block takes the whole turn, not the fragment…
    let a = chat.turn_answer(TranscriptItemId::Live(1)).unwrap();
    let b = chat.turn_answer(TranscriptItemId::Live(3)).unwrap();
    assert_eq!(
        chat.turn_prose(TranscriptItemId::Live(1)),
        "first\n\nsecond"
    );
    assert_eq!(
        chat.turn_prose(TranscriptItemId::Live(3)),
        "first\n\nsecond"
    );
    // …but only the trailing block is the turn's last (one Copy per turn).
    assert!(!a.is_last);
    assert!(b.is_last);
    // Idle transcript → not the active streaming turn.
    assert!(!b.is_active);
    assert_eq!(b.elapsed_secs, Some(12));
    // A non-agent index has no turn answer.
    assert!(chat.turn_answer(TranscriptItemId::Live(0)).is_none());
    assert!(chat.turn_answer(TranscriptItemId::Live(2)).is_none());
}

/// The card carries its hunks, and a later report of the same tool call
/// replaces them. The view draws what is here and computes nothing: an edit
/// script is quadratic in the worst case, and a redraw happens for every
/// notch of a scroll wheel.
#[test]
fn a_card_holds_the_hunks_its_edit_produced() {
    let diff = |new: &str| ToolContent::Diff {
        path: "a.rs".into(),
        old: Some("x\ny\n".into()),
        new: new.into(),
    };
    let mut chat = Chat::default();
    chat.apply(AcpEvent::ToolCall(ToolCall {
        id: "t1".into(),
        title: "edit".into(),
        description: None,
        kind: crate::acp::ToolKind::Edit,
        status: ToolStatus::InProgress,
        content: vec![diff("x\ny\nz\n")],
    }));

    let hunks = |chat: &Chat| match &chat.items[0] {
        ChatItem::Tool(t) => t.diff_rows.get(&0).cloned().unwrap_or_default(),
        _ => panic!("expected a tool card"),
    };
    assert!(
        hunks(&chat).contains(&crate::diff::Row::Added("z".to_string())),
        "the added line is in the card's hunks"
    );

    chat.apply(AcpEvent::ToolUpdate(crate::acp::ToolCallUpdate {
        id: "t1".into(),
        title: None,
        description: None,
        status: Some(ToolStatus::Completed),
        content: Some(vec![diff("x\ny\nw\n")]),
    }));
    let after = hunks(&chat);
    assert!(after.contains(&crate::diff::Row::Added("w".to_string())));
    assert!(
        !after.contains(&crate::diff::Row::Added("z".to_string())),
        "a re-reported edit replaces the hunks rather than adding to them"
    );
}

#[test]
fn line_change_counts_reports_actual_edits() {
    assert_eq!(line_change_counts(Some("a\nb\nc\n"), "a\nx\nc\n"), (1, 1));
    assert_eq!(line_change_counts(Some("a\nb\n"), "a\nb\nc\n"), (1, 0));
    assert_eq!(line_change_counts(Some("a\nb\n"), "a\n"), (0, 1));
    assert_eq!(line_change_counts(Some("same\n"), "same\n"), (0, 0));
    assert_eq!(line_change_counts(None, "new\nfile\n"), (2, 0));
}

#[test]
fn history_turns_keep_label_and_copy_metadata() {
    let mut chat = Chat::default();
    chat.history
        .push(ChatItem::User(UserMsg::text("old prompt")));
    chat.history.push(ChatItem::Agent(Md::parse("first")));
    chat.history.push(ChatItem::notice("tool finished"));
    chat.history.push(ChatItem::Agent(Md::parse("second")));

    let first = chat.turn_answer(TranscriptItemId::History(1)).unwrap();
    let last = chat.turn_answer(TranscriptItemId::History(3)).unwrap();
    assert!(!first.is_last);
    assert!(last.is_last);
    assert!(!last.is_active);
    assert_eq!(
        chat.turn_prose(TranscriptItemId::History(3)),
        "first\n\nsecond",
        "a resumed turn copies out of history, not out of the live tail"
    );
}

#[test]
fn turn_answer_marks_the_streaming_turn_active() {
    let mut chat = Chat::default();
    chat.items.push(ChatItem::User(UserMsg::text("hi")));
    chat.items.push(ChatItem::Agent(Md::parse("streaming")));
    // No user prompt after it + a turn in flight ⇒ active (Copy hidden).
    chat.busy = true;
    assert!(
        chat.turn_answer(TranscriptItemId::Live(1))
            .unwrap()
            .is_active
    );
    // Once the turn settles it is copyable.
    chat.busy = false;
    assert!(
        !chat
            .turn_answer(TranscriptItemId::Live(1))
            .unwrap()
            .is_active
    );
}

#[test]
fn finishing_a_turn_stamps_only_the_latest_timed_prompt() {
    let mut chat = Chat::default();
    chat.items.push(ChatItem::User(UserMsg::text("legacy")));
    let mut current = UserMsg::text("current");
    current.sent_at = Some(40);
    chat.items.push(ChatItem::User(current));

    chat.finish_active_turn(47);

    let ChatItem::User(legacy) = &chat.items[0] else {
        panic!("expected legacy prompt")
    };
    let ChatItem::User(current) = &chat.items[1] else {
        panic!("expected current prompt")
    };
    assert_eq!(legacy.completed_at, None);
    assert_eq!(current.completed_at, Some(47));
}

#[test]
fn terminal_output_trim_lands_on_char_boundary() {
    // Force the retained-tail cut to land mid-'→' (3 bytes each): one ASCII
    // byte shifts every char boundary to 1+3k, and the raw cut offset is
    // not on that grid. The old byte-slice trim panicked here.
    let mut chat = Chat::default();
    let big = "→".repeat((MAX_TERM_BYTES / 3) + 100);
    chat.apply(AcpEvent::TerminalOutput {
        terminal_id: "t1".into(),
        chunk: format!("a{big}"),
    });
    let view = &chat.terminals["t1"];
    assert!(view.output.len() <= MAX_TERM_BYTES);
    assert!(view.output.chars().all(|c| c == '→'));
}

#[test]
fn load_history_resets_the_live_transcript() {
    // Restart keeps the chat; loading the archive over live items must not
    // leave both — the view chains history ⧺ items (double render) and the
    // next save would double the archive.
    let mut chat = Chat::default();
    chat.items.push(ChatItem::User(UserMsg::text("hi")));
    chat.items.push(ChatItem::Agent(Md::parse("reply")));
    chat.load_history(vec![ChatItem::User(UserMsg::text("hi"))], "sid-1".into(), 1);
    assert!(chat.items.is_empty());
    assert_eq!(chat.history.len(), 1);
    assert!(chat.replay_pending());
    assert_eq!(chat.session_id.as_deref(), Some("sid-1"));
}

#[test]
fn user_prompt_keeps_unreplayed_history() {
    // A resume whose `session/load` replays nothing: the first user prompt
    // must keep the loaded history (it is the only copy of the
    // conversation), while real replayed content still consumes it.
    let mut chat = resumed_chat(1);
    chat.push_user("new prompt".into(), Vec::new());
    assert_eq!(chat.history.len(), 1, "history must survive a user prompt");
    assert!(!chat.replay_pending());
    // And the prompt is added to the conversation, not written as the whole
    // of it: one line, after the one already there.
    let write = chat.flush().expect("the prompt is new");
    assert_eq!(write.lines.len(), 1);
    assert!(!write.rewrite);

    let mut chat = resumed_chat(1);
    chat.apply(AcpEvent::AgentChunk("replayed".into()));
    assert!(chat.history.is_empty(), "a real replay still drops history");
}

/// A conversation of `n` messages, resumed from a store it will never be
/// written to — every assertion here is about what a save *would* carry.
fn resumed_chat(n: usize) -> Chat {
    let mut chat = Chat::new(
        1,
        PathBuf::from("/r"),
        "Claude".into(),
        Some(PathBuf::from("/store")),
    );
    chat.resume_from(ConversationSnapshot {
        session_id: "sid-1".into(),
        title: None,
        updated: 1,
        prefs: Prefs::default(),
        items: (0..n)
            .map(|i| ChatItem::User(UserMsg::text(format!("message {i}"))))
            .collect(),
        written: n,
        complete: true,
    });
    chat
}

/// The headline rule: a replay that stops halfway is not written down.
///
/// A `session/load` re-delivers the conversation as ordinary content, so the
/// first piece of it takes the adopted copy off the screen — and at that
/// moment an adapter that dies leaves two messages standing where fifty
/// were. What `items` holds then is a re-delivery of lines already on disk,
/// so nothing about it can be added to the file without saying those lines
/// twice or writing a fragment as though it were the whole.
#[test]
fn an_interrupted_replay_is_never_written() {
    let mut chat = resumed_chat(3);
    chat.apply(AcpEvent::AgentChunk("first replayed answer".into()));
    // On screen the adopted copy is gone, exactly as before.
    assert!(chat.history.is_empty());
    assert!(chat.flush().is_none(), "and nothing is written from it");

    // The adapter then dies. Closing the session still writes nothing, so
    // the three messages on disk stay three messages.
    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    assert!(chat.flush().is_none());
}

/// A resume that succeeds can still deliver less than the file holds.
///
/// The replay is the conversation as the *agent* holds it, and the file
/// holds things the agent has no reason to send back — the tool cards, the
/// plans, the reasoning. Trading the full transcript for the agent's summary
/// of it would be permanent, so the shorter side never wins, however the
/// load went.
#[test]
fn a_short_replay_loses_to_the_file_even_when_the_load_succeeded() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = resumed_chat(4);
    chat.apply(AcpEvent::AgentChunk("a summary of all that".into()));
    chat.apply(AcpEvent::Connected { tx, resumed: true });

    assert_eq!(chat.history.len(), 4, "the conversation is back on screen");
    assert!(chat.flush().is_none(), "and the file is left as it was");
}

/// …and once the load has answered with everything, the replay *is* the
/// record — which means the file has to become it rather than gain it.
#[test]
fn a_completed_replay_rewrites_the_file_with_what_was_replayed() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = resumed_chat(2);
    chat.apply(AcpEvent::UserChunk("message 0".into()));
    chat.apply(AcpEvent::AgentChunk("answer".into()));
    chat.apply(AcpEvent::Connected { tx, resumed: true });

    assert!(chat.history.is_empty());
    assert!(!chat.replay_pending());
    let write = chat.flush().expect("the replay has to be written down");
    assert!(write.rewrite, "spliced onto the old lines, it would seam");
    assert_eq!(write.lines.len(), 2, "the replayed transcript, not the old");
    // And it is written once: the mark now says the file holds it.
    assert!(chat.flush().is_none());
}

/// A resume the adapter could not honour puts the conversation back.
///
/// The client falls back to a fresh session and says so; the file it failed
/// to load is the only copy there is, and what the adapter managed to
/// stream before failing is a fragment of it.
#[test]
fn a_failed_resume_puts_the_conversation_back() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = resumed_chat(3);
    chat.apply(AcpEvent::UserChunk("message 0".into()));
    chat.apply(AcpEvent::Error("session/load failed".into()));
    chat.apply(AcpEvent::Connected { tx, resumed: false });

    assert_eq!(chat.history.len(), 3, "the conversation is back on screen");
    assert_eq!(
        chat.items.len(),
        1,
        "and the fragment is gone, but not the reason"
    );
    assert!(matches!(chat.items[0], ChatItem::Notice { .. }));
    // The three are already on disk; only the reason is new.
    let write = chat.flush().expect("the notice is worth keeping");
    assert!(!write.rewrite);
    assert_eq!(write.lines.len(), 1);
}

/// A replay longer than the file is the better record, and survives
/// settling. That happens when a previous run died between the last save
/// and the end of a turn: the agent has the turn, the file does not.
#[test]
fn a_replay_that_delivered_more_is_kept() {
    let mut chat = resumed_chat(2);
    for i in 0..3 {
        chat.apply(AcpEvent::UserChunk(format!("message {i}")));
        chat.apply(AcpEvent::AgentChunk("answer".into()));
    }
    chat.push_user("carry on".into(), Vec::new());

    assert!(chat.history.is_empty(), "the shorter copy is not put back");
    let write = chat.flush().unwrap();
    assert!(write.rewrite);
    assert_eq!(write.lines.len(), 7);
}

/// A transcript that came back as its tail must never be written back over
/// the file it is a tail of.
#[test]
fn a_bounded_transcript_never_rewrites_the_file() {
    let mut chat = resumed_chat(2);
    chat.bounded = true;
    for i in 0..3 {
        chat.apply(AcpEvent::UserChunk(format!("message {i}")));
        chat.apply(AcpEvent::AgentChunk("answer".into()));
    }
    chat.push_user("carry on".into(), Vec::new());

    let write = chat.flush().unwrap();
    assert!(!write.rewrite, "the file is longer than what is on screen");
    assert_eq!(write.lines.len(), 1, "only the new prompt is added");
}

/// A restart carries the mark, so the replacement session continues the
/// file rather than writing the conversation into it again.
#[test]
fn a_restart_carries_the_mark() {
    let mut chat = resumed_chat(2);
    chat.push_user("and one more".into(), Vec::new());

    let snapshot = chat.take_snapshot().expect("there is a conversation");
    assert_eq!(snapshot.items.len(), 3);
    assert_eq!(snapshot.written, 2, "two of the three are already on disk");
    // The chat it came from has nothing left to write, which is what keeps
    // its own drop from saying all of it a second time.
    assert!(chat.flush().is_none());

    let mut next = Chat::new(
        2,
        PathBuf::from("/r"),
        "Claude".into(),
        Some(PathBuf::from("/store")),
    );
    next.resume_from(snapshot);
    let write = next.flush().expect("the unwritten prompt still is");
    assert_eq!(write.lines.len(), 1);
}

#[test]
fn replayed_user_messages_split_across_intervening_content() {
    let mut chat = Chat::default();
    // One message replayed in two chunks merges into one bubble…
    chat.apply(AcpEvent::UserChunk("hello ".into()));
    chat.apply(AcpEvent::UserChunk("world".into()));
    assert_eq!(
        chat.items
            .iter()
            .filter(|i| matches!(i, ChatItem::User(_)))
            .count(),
        1
    );
    // …but a second prompt after agent content is its own bubble.
    chat.apply(AcpEvent::AgentChunk("reply".into()));
    chat.apply(AcpEvent::UserChunk("second prompt".into()));
    assert_eq!(
        chat.items
            .iter()
            .filter(|i| matches!(i, ChatItem::User(_)))
            .count(),
        2
    );
}

/// A two-question elicitation: one single-select with an "Other" box, one
/// multi-select without.
fn ask_item() -> AskItem {
    let choice = |v: &str| crate::acp::ElicitChoice {
        value: v.into(),
        label: v.to_uppercase(),
        description: None,
    };
    AskItem::new(Elicitation {
        rpc_id: serde_json::json!(1),
        tool_call_id: None,
        message: "pick".into(),
        fields: vec![
            crate::acp::ElicitField {
                key: "question_0".into(),
                title: None,
                description: None,
                kind: ElicitKind::Select(vec![choice("a"), choice("b")]),
                custom_key: Some("question_0_custom".into()),
            },
            crate::acp::ElicitField {
                key: "question_1".into(),
                title: None,
                description: None,
                kind: ElicitKind::MultiSelect(vec![choice("x"), choice("y")]),
                custom_key: None,
            },
        ],
    })
}

#[test]
fn tab_clamps_and_answered_tracks_picks_and_text() {
    let mut a = ask_item();
    assert_eq!(a.active_field(), 0);
    a.tab = 1;
    assert_eq!(a.active_field(), 1);
    a.tab = 9; // a stale cursor never points off the end of the form
    assert_eq!(a.active_field(), 1);

    assert!(!a.field_answered(0) && !a.field_answered(1));
    a.toggle(1, 0);
    assert!(a.field_answered(1));
    a.set_custom(0, "  ".into()); // blank text isn't an answer
    assert!(!a.field_answered(0));
    a.set_custom(0, "mine".into());
    assert!(a.field_answered(0));
}

#[test]
fn single_select_replaces_multi_select_toggles() {
    let mut a = ask_item();
    a.toggle(0, 0);
    a.toggle(0, 1);
    assert_eq!(a.picked[0], vec![1]); // replaced, not accumulated
    a.toggle(1, 0);
    a.toggle(1, 1);
    assert_eq!(a.picked[1], vec![0, 1]);
    a.toggle(1, 0); // toggling an on choice turns it off
    assert_eq!(a.picked[1], vec![1]);
}

/// The card's forward button: enabled by *this* question carrying an
/// answer, and reading Submit only on the last one.
#[test]
fn forward_needs_this_question_answered_and_ends_on_the_last() {
    let mut a = ask_item();
    assert!(!a.field_answered(0), "nothing picked yet, so Next refuses");
    assert!(!a.is_last(0));
    a.toggle(0, 0);
    assert!(a.field_answered(0));
    // Question one being answered says nothing about question two.
    assert!(!a.field_answered(1));
    assert!(a.is_last(1), "the second of two is where Submit appears");
}

#[test]
fn skipping_drops_the_answer_and_walks_on() {
    let mut a = ask_item();
    a.toggle(0, 0);
    a.set_custom(0, "mine".into());
    // Not stepped over: a half-typed line left behind would be sent.
    assert!(!a.skip_field(0), "there is a question after this one");
    assert_eq!(a.tab, 1);
    assert!(!a.field_answered(0));
    assert_eq!(a.custom[0], "");
    // The last question has nowhere to walk to, so skipping it settles the
    // form instead.
    assert!(a.skip_field(1));
}

#[test]
fn a_tab_jumped_back_to_still_holds_its_answer() {
    let mut a = ask_item();
    a.toggle(0, 1);
    a.go_to(1);
    a.toggle(1, 0);
    a.go_to(0);
    assert_eq!(a.picked[0], vec![1], "the earlier pick survived the trip");
    assert_eq!(a.cursor, 0, "and the keyboard starts at the top of it");
}

#[test]
fn the_cursor_walks_the_choices_and_the_typed_answer_as_one_list() {
    let mut a = ask_item();
    // Two choices and an "Other" box.
    assert_eq!(a.row_count(0), 3);
    assert_eq!(a.row(0, 2), Some(AskRow::Custom));
    assert_eq!(a.row(0, 3), None, "a number nobody offered names nothing");
    // The second field has no "Other" box, so it is choices alone.
    assert_eq!(a.row_count(1), 2);
    assert_eq!(a.row(1, 2), None);

    assert_eq!(a.cursor_row(0), Some(AskRow::Choice(0)));
    a.move_cursor(0, 1);
    assert_eq!(a.cursor_row(0), Some(AskRow::Choice(1)));
    a.move_cursor(0, 1);
    assert_eq!(a.cursor_row(0), Some(AskRow::Custom));
    a.move_cursor(0, 1);
    assert_eq!(a.cursor_row(0), Some(AskRow::Choice(0)), "it wraps");
    a.move_cursor(0, -1);
    assert_eq!(a.cursor_row(0), Some(AskRow::Custom), "both ways");
    // A cursor left past the end of a shorter question lands on its last
    // row rather than on nothing.
    assert_eq!(a.cursor, 2);
    assert_eq!(a.cursor_row(1), Some(AskRow::Choice(1)));
}

#[test]
fn answers_use_wire_values_and_skip_empty_fields() {
    let mut a = ask_item();
    a.toggle(0, 1);
    assert_eq!(
        a.answers(),
        vec![("question_0".to_string(), ElicitValue::Text("b".into()))],
    );
    a.toggle(1, 0);
    assert_eq!(
        a.answers()[1],
        (
            "question_1".to_string(),
            ElicitValue::List(vec!["x".into()])
        ),
    );
    // The summary is the *labels*, not the wire values.
    assert_eq!(a.summary(), "B, X");
}

#[test]
fn typed_text_answers_under_the_custom_key_and_clears_the_pick() {
    let mut a = ask_item();
    a.toggle(0, 0);
    a.set_custom(0, "  something else  ".into());
    assert!(a.picked[0].is_empty());
    assert_eq!(
        a.answers(),
        vec![(
            "question_0_custom".to_string(),
            ElicitValue::Text("something else".into())
        )],
    );
    assert_eq!(a.summary(), "something else");
    // …and picking again drops the text, so the two never both apply.
    a.toggle(0, 0);
    assert_eq!(a.custom[0], "");
    assert_eq!(
        a.answers(),
        vec![("question_0".into(), ElicitValue::Text("a".into()))]
    );
}

#[test]
fn quick_form_is_one_single_select_only() {
    let a = ask_item();
    assert!(!a.is_quick()); // two questions
    assert!(!a.has_answer());
    let mut one = AskItem::new(Elicitation {
        fields: vec![a.req.fields[0].clone()],
        ..a.req.clone()
    });
    assert!(one.is_quick());
    one.toggle(0, 0);
    assert!(one.has_answer());
}

#[test]
fn a_parked_question_reads_as_awaiting_the_user() {
    let mut chat = Chat::default();
    chat.apply(AcpEvent::Elicitation(ask_item().req));
    assert!(chat.awaiting_permission());
    assert_eq!(chat.pending_asks().len(), 1);
    // Answering settles the card and empties the sticky bar.
    chat.answer_ask(0, false);
    assert!(!chat.awaiting_permission());
    assert!(chat.pending_asks().is_empty());
}

/// A chat wired to a request channel, with the drain end returned so a test
/// can read back what `reapply_prefs` sent to the adapter.
/// `submit` is the one rule both front ends route through, so the refusals
/// matter as much as the send: each one is a way a prompt could be silently
/// lost or double-counted if a front end reimplemented the guard.
#[test]
fn submit_sends_a_prompt_and_opens_a_turn() {
    let (mut chat, mut rx) = chat_with_tx();

    assert!(chat.submit("  build it  ", &[]));
    assert!(chat.busy, "a sent prompt opens a turn");
    assert!(matches!(
        rx.try_recv(),
        Ok(AcpRequest::Prompt { text, .. }) if text == "build it"
    ));
    assert!(matches!(chat.items.last(), Some(ChatItem::User(u)) if u.text == "build it"));
}

#[test]
fn submit_refuses_and_records_nothing() {
    let (mut chat, mut rx) = chat_with_tx();

    // Blank with no attachments.
    assert!(!chat.submit("   ", &[]));
    // A turn already in flight.
    chat.busy = true;
    assert!(!chat.submit("second", &[]));
    chat.busy = false;
    // No live channel.
    chat.tx = None;
    assert!(!chat.submit("third", &[]));

    assert!(
        chat.items.is_empty(),
        "a refused prompt leaves no transcript"
    );
    assert!(!chat.busy);
    assert!(rx.try_recv().is_err());
}

/// The blocker is what a front end draws on Send, so it has to name the
/// *same* refusals `submit` acts on. Answering "Empty" while `submit`
/// refused for a dead channel is a Send that looks fixable by typing.
#[test]
fn the_blocker_names_every_refusal_submit_makes() {
    let (mut chat, _rx) = chat_with_tx();

    assert_eq!(chat.submit_blocker("   ", &[]), Some(SubmitBlock::Empty));
    assert_eq!(chat.submit_blocker("go", &[]), None);

    // A path that is not there reads back as unreadable, which is the
    // state the tray marks in the danger tint.
    let unreadable = StagedAttachment::inspect(
        PathBuf::from("/tmp/onehand-no-such-file.png"),
        crate::attachment::AttachmentSource::Picker,
    );
    assert_eq!(
        chat.submit_blocker("go", std::slice::from_ref(&unreadable)),
        Some(SubmitBlock::UnreadableAttachment(
            "onehand-no-such-file.png".to_string()
        )),
        "the file is named, so the tray does not have to be searched"
    );

    // A turn in flight outranks the prompt's own problems: Stop is the
    // only thing the user can do about it.
    chat.busy = true;
    assert_eq!(chat.submit_blocker("   ", &[]), Some(SubmitBlock::Busy));
    chat.busy = false;

    chat.tx = None;
    assert_eq!(
        chat.submit_blocker("go", &[]),
        Some(SubmitBlock::NotConnected)
    );
}

/// Only a prompt this app sent is counted. An adapter can deliver user
/// chunks of its own mid-turn, and each lands in the transcript as a user
/// row; counting those as prompts is how a run nobody touched was reported
/// as taken over by hand.
#[test]
fn a_user_chunk_from_the_agent_is_not_a_prompt_sent() {
    let (mut chat, _rx) = chat_with_tx();
    assert_eq!(chat.prompts_sent, 0);
    assert!(chat.submit("first", &[]));
    assert_eq!(chat.prompts_sent, 1);
    chat.apply(AcpEvent::AgentChunk("working".into()));
    chat.apply(AcpEvent::UserChunk("echoed by the adapter".into()));
    assert_eq!(
        chat.items
            .iter()
            .filter(|item| matches!(item, ChatItem::User(_)))
            .count(),
        2,
        "the transcript shows it"
    );
    assert_eq!(chat.prompts_sent, 1, "but nobody sent it");
}

/// A prompt written mid-turn goes when the turn does -- and only then, and
/// only once. Flushed anywhere but at the end of a turn it either races the
/// turn it was queued behind or never leaves at all.
#[test]
fn a_queued_prompt_goes_out_when_the_turn_ends() {
    let (mut chat, mut rx) = chat_with_tx();
    assert!(chat.submit("first", &[]));
    assert!(chat.busy);
    assert!(matches!(rx.try_recv(), Ok(AcpRequest::Prompt { .. })));

    assert!(chat.queue("second", &[]), "a turn in flight is what queues");
    assert!(rx.try_recv().is_err(), "queued is not sent");
    assert_eq!(chat.items.len(), 1, "and not in the transcript either");

    chat.apply(AcpEvent::TurnEnded {
        stop_reason: String::new(),
    });
    assert!(matches!(
        rx.try_recv(),
        Ok(AcpRequest::Prompt { text, .. }) if text == "second"
    ));
    assert!(chat.busy, "the queued prompt opened its own turn");
    assert!(chat.queued.is_none(), "and left the queue empty");
}

/// The queue is for the one blocker that time fixes. Everything else is
/// still a refusal, or a prompt would sit waiting for a turn to end and
/// then be refused all over again, silently.
#[test]
fn only_a_running_turn_queues() {
    let (mut chat, _rx) = chat_with_tx();
    assert!(!chat.queue("idle", &[]), "nothing is in the way");

    chat.busy = true;
    assert!(!chat.queue("   ", &[]), "an empty prompt is still empty");
    let unreadable = StagedAttachment::inspect(
        PathBuf::from("/tmp/onehand-no-such-file.png"),
        crate::attachment::AttachmentSource::Picker,
    );
    assert!(!chat.queue("go", std::slice::from_ref(&unreadable)));
    assert!(chat.queued.is_none());

    assert!(chat.queue("first", &[]));
    assert!(
        chat.queue("second", &[]),
        "the later prompt is the one meant"
    );
    assert_eq!(chat.unqueue().map(|q| q.text).as_deref(), Some("second"));
}

fn chat_with(items: Vec<ChatItem>) -> Chat {
    let mut chat = Chat::new(1, PathBuf::from("/tmp/p"), "Claude Code".to_string(), None);
    chat.items = items;
    chat
}

fn agent(source: &str) -> ChatItem {
    ChatItem::Agent(Md::parse(source))
}

/// A short answer goes whole and unmarked: an ellipsis in front of a
/// complete reply is the notification claiming something was cut off.
#[test]
fn a_short_answer_is_carried_whole() {
    let chat = chat_with(vec![agent("  Done — the test passes now.  ")]);
    assert_eq!(
        chat.answer_tail(200).as_deref(),
        Some("Done — the test passes now.")
    );
}

/// The point of the whole thing: an answer opens by restating the problem
/// and closes by saying what was done, so the end is the half worth sending.
#[test]
fn a_long_answer_is_carried_from_its_end() {
    // The closing paragraph is most of what the excerpt can hold, so its
    // break is inside the search window and the excerpt starts on it.
    let last = "So the fix is one line in the parser, and a test now covers it.";
    let source = format!("{}\n\n{last}", "x".repeat(400));
    let tail = chat_with(vec![agent(&source)]).answer_tail(80).unwrap();
    assert_eq!(tail, format!("…{last}"));
}

/// A boundary hunted far enough would throw away most of what was asked
/// for, so past a quarter of the excerpt the cut simply stands. One
/// unbroken run that long is a URL or a blob, and starting inside one costs
/// nothing worth the rest of the answer.
#[test]
fn a_distant_boundary_is_not_worth_the_text_it_would_cost() {
    let source = format!("{}\n\nend", "y".repeat(300));
    let tail = chat_with(vec![agent(&source)]).answer_tail(100).unwrap();
    assert!(tail.starts_with("…y"), "got {tail:?}");
    assert!(tail.ends_with("end"));
    // Still about the length that was asked for, rather than three letters.
    assert!(
        tail.chars().count() > 90,
        "got {} chars",
        tail.chars().count()
    );
}

/// In prose the cut almost always lands inside a word, and the space after
/// it is a character or two away — so the common case is an excerpt that
/// opens on a whole word for almost no cost.
#[test]
fn an_excerpt_does_not_open_in_the_middle_of_a_word() {
    let source = "the quick brown fox jumps over the lazy dog and keeps on going";
    // 20 characters back from the end lands on the "d" of "dog"; the space
    // one character later is what the excerpt actually starts after.
    let tail = chat_with(vec![agent(source)]).answer_tail(20).unwrap();
    assert_eq!(tail, "…and keeps on going");
    assert!(source.ends_with(tail.trim_start_matches('…')));
}

/// A turn can end on a tool call or be cancelled before the agent says
/// anything. Inventing a sentence for that is worse than the headline alone.
#[test]
fn a_turn_with_no_prose_has_no_tail() {
    assert_eq!(chat_with(vec![]).answer_tail(100), None);
    assert_eq!(chat_with(vec![agent("   \n  ")]).answer_tail(100), None);
    assert_eq!(
        chat_with(vec![ChatItem::User(UserMsg::text("hi"))]).answer_tail(100),
        None
    );
}

/// **The turn, not the transcript.** A turn that ran a tool and said
/// nothing must not reach back past the prompt that started it and announce
/// the *previous* turn's closing paragraph as this one's result -- a wrong
/// answer in the shape of a right one, which the reader has no way to catch.
#[test]
fn a_silent_turn_does_not_borrow_the_last_one_s_summary() {
    let chat = chat_with(vec![
        ChatItem::User(UserMsg::text("first ask")),
        agent("All done — the parser is fixed."),
        ChatItem::User(UserMsg::text("second ask")),
        ChatItem::notice("a tool ran"),
    ]);
    assert_eq!(chat.answer_tail(200), None);
}

/// The *last* answer of the turn, not the first: a turn that spoke, ran a
/// tool and spoke again ends on the second one, and that is the summary.
#[test]
fn the_last_answer_of_the_turn_is_the_one_that_is_carried() {
    let chat = chat_with(vec![
        ChatItem::User(UserMsg::text("go")),
        agent("Let me look at the parser."),
        ChatItem::notice("a tool ran"),
        agent("Fixed it."),
    ]);
    assert_eq!(chat.answer_tail(200).as_deref(), Some("Fixed it."));
}

/// Cutting by bytes would split a multi-byte character and produce an
/// excerpt that is not text at all. Every boundary here is a character.
#[test]
fn a_tail_is_cut_by_characters_and_not_by_bytes() {
    let source = "Đã sửa xong phần phân tích cú pháp của trình biên dịch nhé";
    let tail = chat_with(vec![agent(source)]).answer_tail(10).unwrap();
    assert!(tail.ends_with("nhé"), "got {tail:?}");
    assert!(source.ends_with(tail.trim_start_matches('…')));
}

/// Mode and config options are two shapes in the protocol and two controls
/// on the composer; to anything describing them from outside they are one
/// question. Mode leads, and takes the reserved id.
#[test]
fn selectors_flatten_the_mode_and_the_config_groups() {
    let mut chat = Chat::default();
    chat.modes = vec![
        Mode {
            id: "default".into(),
            name: "Default".into(),
        },
        Mode {
            id: "plan".into(),
            name: "Plan".into(),
        },
    ];
    chat.current_mode = Some("plan".into());
    chat.config_options = vec![ConfigOption {
        id: "effort".into(),
        name: "Effort".into(),
        current: None,
        choices: vec![ConfigChoice {
            value: "high".into(),
            name: "High".into(),
            description: None,
        }],
    }];

    let found = chat.selectors();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].id, MODE_SELECTOR);
    assert_eq!(found[0].name, "Mode");
    assert_eq!(found[0].current.as_deref(), Some("plan"));
    assert_eq!(found[1].id, "effort");

    // A session that offers no modes shows no mode picker, rather than an
    // empty one.
    chat.modes.clear();
    assert_eq!(chat.selectors().len(), 1);
}

/// The mode key walks the list and wraps; a mode the list no longer holds
/// lands on the first, and a single mode is nowhere to go.
#[test]
fn cycle_mode_wraps_and_recovers() {
    let mode = |id: &str| Mode {
        id: id.into(),
        name: id.into(),
    };
    let mut chat = Chat::default();
    chat.modes = vec![mode("default"), mode("plan"), mode("auto")];
    chat.current_mode = Some("plan".into());
    assert_eq!(chat.cycle_mode(), Some("auto"));
    assert_eq!(chat.cycle_mode(), Some("default"));

    chat.current_mode = Some("gone".into());
    assert_eq!(chat.cycle_mode(), Some("default"));

    chat.modes.truncate(1);
    assert_eq!(chat.cycle_mode(), None);
}

/// The two halves go down different paths -- mode is a first-class field of
/// the protocol and the rest are a config group -- and picking the wrong one
/// sends a request the adapter has no reading for.
#[test]
fn choosing_sends_the_request_the_picker_belongs_to() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.tx = Some(tx);
    chat.modes = vec![Mode {
        id: "plan".into(),
        name: "Plan".into(),
    }];
    chat.config_options = vec![ConfigOption {
        id: "effort".into(),
        name: "Effort".into(),
        current: None,
        choices: vec![ConfigChoice {
            value: "high".into(),
            name: "High".into(),
            description: None,
        }],
    }];

    assert_eq!(
        chat.choose(MODE_SELECTOR, 0).as_deref(),
        Some("Mode → Plan")
    );
    assert!(matches!(rx.try_recv(), Ok(AcpRequest::SetMode(m)) if m == "plan"));
    assert_eq!(chat.current_mode.as_deref(), Some("plan"));

    assert_eq!(
        chat.choose("effort", 0).as_deref(),
        Some("Effort → High"),
        "the sentence is read from the picker, not from what the caller thought it chose"
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(AcpRequest::SetConfigOption { config_id, value }) if config_id == "effort" && value == "high"
    ));
    assert_eq!(chat.config_options[0].current.as_deref(), Some("high"));
}

/// What the agent offers is live, so a button sitting in a chat can point at
/// something that has moved. Guessing at the nearest one would change a
/// model nobody asked for, quietly -- and send nothing to say it had.
#[test]
fn choosing_something_that_is_not_there_changes_nothing() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.tx = Some(tx);
    chat.modes = vec![Mode {
        id: "plan".into(),
        name: "Plan".into(),
    }];

    assert_eq!(chat.choose(MODE_SELECTOR, 9), None, "no such choice");
    assert_eq!(chat.choose("model", 0), None, "no such group");
    assert_eq!(chat.current_mode, None);
    assert!(rx.try_recv().is_err(), "nothing may reach the adapter");
}

/// Until the handshake lands there is no agent to be doing anything, so
/// that is what the status says -- ahead of every other answer it could
/// give. A resumed conversation puts its archive on screen the moment it is
/// picked, which is several seconds before anything can be sent to it.
#[test]
fn connecting_outranks_everything_the_status_could_say() {
    let mut chat = Chat::new(1, PathBuf::from("/tmp/p"), "Claude Code".to_string(), None);
    assert_eq!(
        chat.link,
        Link::Connecting,
        "a fresh chat has no adapter yet"
    );
    assert_eq!(
        chat.activity_status().as_deref(),
        Some("Connecting to Claude Code…")
    );

    // Even mid-turn: a turn cannot be in flight down a channel that is not
    // up yet, and saying "Working…" there is the pane claiming otherwise.
    chat.busy = true;
    assert_eq!(
        chat.activity_status().as_deref(),
        Some("Connecting to Claude Code…")
    );

    chat.link = Link::Connected;
    assert_eq!(chat.activity_status().as_deref(), Some("Working…"));
    chat.busy = false;
    assert_eq!(
        chat.activity_status(),
        None,
        "connected and idle says nothing"
    );
}

use crate::acp::PermissionOption;

fn permission(title: &str) -> PermissionRequest {
    PermissionRequest {
        rpc_id: serde_json::Value::from(7),
        tool_call_id: None,
        kind: crate::acp::ToolKind::Execute,
        title: title.into(),
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
    }
}

/// All four halves of a resume have to land. Each one that does not is a
/// silent loss: the transcript, the user's own title, the selector state
/// the adapter will *not* rebuild, and the date the rail shows.
#[test]
fn resuming_adopts_transcript_title_prefs_and_date() {
    let mut chat = Chat::default();
    chat.resume_from(ConversationSnapshot {
        session_id: "sess-9".into(),
        title: Some("Ship the parser".into()),
        updated: 1_700_000_000,
        prefs: Prefs {
            mode: Some("plan".into()),
            config: vec![ConfigPick {
                id: "effort".into(),
                value: "high".into(),
            }],
        },
        items: Vec::new(),
        written: 0,
        complete: true,
    });

    assert_eq!(chat.session_id.as_deref(), Some("sess-9"));
    assert_eq!(chat.custom_title.as_deref(), Some("Ship the parser"));
    assert_eq!(chat.pending_mode.as_deref(), Some("plan"));
    assert_eq!(
        chat.pending_config,
        vec![("effort".to_string(), "high".to_string())]
    );
    // The rail dates a resumed session from its last real turn, not from
    // the moment it was reopened.
    assert_eq!(chat.last_activity, Some(1_700_000_000));
    assert!(chat.replay_pending(), "a resume arms the replay window");
}

/// Cancelling must resolve parked permissions *before* it cancels: ACP
/// leaves a `session/request_permission` dangling otherwise, and the card
/// keeps buttons whose click would answer a request nobody is waiting on.
#[test]
fn cancelling_resolves_parked_permissions_first() {
    let (mut chat, mut rx) = chat_with_tx();
    chat.busy = true;
    chat.items.push(ChatItem::Permission(PermItem {
        req: permission("rm -rf build"),
        resolved: None,
        expanded: false,
    }));

    chat.cancel_turn();

    assert!(
        matches!(
            rx.try_recv(),
            Ok(AcpRequest::PermissionResponse {
                option_id: None,
                ..
            })
        ),
        "the parked permission is answered before the cancel"
    );
    assert!(matches!(rx.try_recv(), Ok(AcpRequest::Cancel)));
    assert!(!chat.awaiting_permission());
}

/// Answering has to reach the adapter *and* mark the card. Losing either
/// half is how a turn ends up parked forever with live-looking buttons.
#[test]
fn answering_a_permission_replies_and_records_the_choice() {
    let (mut chat, mut rx) = chat_with_tx();
    chat.items.push(ChatItem::Permission(PermItem {
        req: permission("rm -rf build"),
        resolved: None,
        expanded: false,
    }));

    chat.answer_permission(0, "allow-1");

    assert!(matches!(
        rx.try_recv(),
        Ok(AcpRequest::PermissionResponse { option_id: Some(id), .. }) if id == "allow-1"
    ));
    // The card records the *name*, the wire carries the id.
    assert!(matches!(
        chat.items.first(),
        Some(ChatItem::Permission(p)) if p.resolved.as_deref() == Some("Allow once")
    ));
    assert!(!chat.awaiting_permission());
}

/// **The fold is a count of the agent's own newlines.** A command at the
/// threshold is drawn whole and offers nothing to open: a control that
/// reveals the one line it was already hiding is a control that reads as
/// broken.
#[test]
fn a_command_folds_only_past_the_threshold() {
    let at = PermItem {
        req: permission(&"echo\n".repeat(COMMAND_FOLD_LINES)),
        resolved: None,
        expanded: false,
    };
    assert!(!at.is_long());
    assert_eq!(at.shown_lines(), (vec!["echo"; COMMAND_FOLD_LINES], 0));

    let over = PermItem {
        req: permission(&"echo\n".repeat(COMMAND_FOLD_LINES + 3)),
        resolved: None,
        expanded: false,
    };
    assert!(over.is_long());
    let (shown, hidden) = over.shown_lines();
    assert_eq!(shown.len(), COMMAND_FOLD_LINES);
    assert_eq!(hidden, 3);
}

/// **Copy hands back the whole command, fold or no fold.** It is offered on
/// a collapsed block precisely so a long script can be read somewhere
/// else, and one that stopped where the block does would hand back
/// something that runs to a different end than the one being approved.
/// The lines come back in the agent's own order and wording -- tabs,
/// blank lines and non-ASCII included.
#[test]
fn copying_a_collapsed_command_takes_all_of_it() {
    let script = format!(
        "#!/bin/sh\n{}\techo 'đã xong ✓'\n",
        "printf 'x'\n".repeat(200)
    );
    let item = PermItem {
        req: permission(&script),
        resolved: None,
        expanded: false,
    };

    assert_eq!(item.command(), script);
    assert_eq!(item.shown_lines().0.len(), COMMAND_FOLD_LINES);

    let long_line = "A".repeat(2_000);
    let one = PermItem {
        req: permission(&long_line),
        resolved: None,
        expanded: false,
    };
    // One real line however wide: wrapping is the view's problem and must
    // not become a fold.
    assert!(!one.is_long());
    assert_eq!(one.command(), long_line);
}

/// Opening is the item's own state, so the card can be rebuilt from
/// scratch every frame without losing what the reader opened.
#[test]
fn opening_a_command_block_survives_in_the_item() {
    let mut chat = Chat::default();
    chat.items.push(ChatItem::Permission(PermItem {
        req: permission(&"echo\n".repeat(20)),
        resolved: None,
        expanded: false,
    }));

    chat.toggle_permission(TranscriptItemId::Live(0));
    let Some(ChatItem::Permission(p)) = chat.items.first() else {
        panic!("the permission is gone");
    };
    assert!(p.expanded);
    assert_eq!(p.shown_lines(), (vec!["echo"; 20], 0));
}

/// A second click on an answered card would echo an rpc id the adapter has
/// already resolved -- and after a restart, one it never issued at all.
#[test]
fn answering_twice_sends_only_once() {
    let (mut chat, mut rx) = chat_with_tx();
    chat.items.push(ChatItem::Permission(PermItem {
        req: permission("rm -rf build"),
        resolved: None,
        expanded: false,
    }));

    chat.answer_permission(0, "allow-1");
    chat.answer_permission(0, "reject-1");

    assert!(rx.try_recv().is_ok());
    assert!(rx.try_recv().is_err(), "the second click must not reply");
    assert!(matches!(
        chat.items.first(),
        Some(ChatItem::Permission(p)) if p.resolved.as_deref() == Some("Allow once")
    ));
}

fn chat_with_tx() -> (Chat, tokio::sync::mpsc::UnboundedReceiver<AcpRequest>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.tx = Some(tx);
    chat.modes = vec![
        Mode {
            id: "default".into(),
            name: "Default".into(),
        },
        Mode {
            id: "plan".into(),
            name: "Plan".into(),
        },
    ];
    chat.current_mode = Some("default".into());
    chat.config_options = vec![
        ConfigOption {
            id: "effort".into(),
            name: "Effort".into(),
            current: Some("default".into()),
            choices: vec![
                crate::acp::ConfigChoice {
                    value: "default".into(),
                    name: "Default".into(),
                    description: None,
                },
                crate::acp::ConfigChoice {
                    value: "high".into(),
                    name: "High".into(),
                    description: None,
                },
            ],
        },
        ConfigOption {
            id: "model".into(),
            name: "Model".into(),
            current: Some("opus[1m]".into()),
            choices: vec![crate::acp::ConfigChoice {
                value: "opus".into(),
                name: "Opus".into(),
                description: None,
            }],
        },
    ];
    (chat, rx)
}

#[test]
fn resume_replays_mode_and_effort_but_never_model() {
    let (mut chat, mut rx) = chat_with_tx();
    chat.arm_prefs(
        Some("plan".into()),
        vec![
            ("effort".into(), "high".into()),
            // The archive recorded the picker alias; on resume the option's
            // current is the verbatim live id — a naive diff would re-push
            // it, which is exactly what we must NOT do.
            ("model".into(), "opus".into()),
        ],
    );
    chat.reapply_prefs();

    let mut sent = Vec::new();
    while let Ok(req) = rx.try_recv() {
        sent.push(req);
    }
    // Mode + effort replayed; model deliberately absent.
    assert!(sent
        .iter()
        .any(|r| matches!(r, AcpRequest::SetMode(m) if m == "plan")));
    assert!(sent.iter().any(|r| matches!(
        r,
        AcpRequest::SetConfigOption { config_id, value } if config_id == "effort" && value == "high"
    )));
    assert!(!sent.iter().any(
        |r| matches!(r, AcpRequest::SetConfigOption { config_id, .. } if config_id == "model")
    ));
    // Optimistic UI: the effort selector reflects the restored pick at once.
    assert_eq!(chat.config_options[0].current.as_deref(), Some("high"));
    assert_eq!(chat.current_mode.as_deref(), Some("plan"));
}

#[test]
fn resume_skips_values_already_current_or_no_longer_offered() {
    let (mut chat, mut rx) = chat_with_tx();
    chat.arm_prefs(
        Some("default".into()), // already the current mode → no resend
        vec![
            ("effort".into(), "default".into()), // already current → skip
            ("phantom".into(), "x".into()),      // option gone → skip
            ("effort".into(), "bogus".into()),   // value not offered → skip
        ],
    );
    // Only the last write per id survives the arm list; use a fresh arm for
    // the "not offered" case to keep the assertion unambiguous.
    chat.pending_config = vec![("effort".into(), "bogus".into())];
    chat.reapply_prefs();
    assert!(rx.try_recv().is_err()); // nothing sent
}

#[test]
fn fresh_session_fallback_drops_armed_prefs_unused() {
    let (mut chat, mut rx) = chat_with_tx();
    chat.arm_prefs(Some("plan".into()), vec![("effort".into(), "high".into())]);
    // A `session/new` fallback: resumed=false must not replay anything.
    chat.apply(AcpEvent::Connected {
        tx: chat.tx.clone().unwrap(),
        resumed: false,
    });
    assert!(rx.try_recv().is_err());
    assert!(chat.pending_mode.is_none());
    assert!(chat.pending_config.is_empty());
}

/// The three states have to be distinguishable, because the rail draws a
/// different thing for each: nothing while connecting, nothing once live,
/// and a danger dot once lost. Reading `tx.is_none()` instead would make
/// "coming up" and "died" the same answer.
#[test]
fn link_separates_coming_up_from_gone() {
    let (mut chat, _rx) = chat_with_tx();
    // `chat_with_tx` hands the channel over directly, so the reducer has
    // not seen a handshake yet.
    assert_eq!(chat.link, Link::Connecting);
    assert!(chat.tx.is_some(), "a channel alone must not mean connected");

    chat.apply(AcpEvent::Connected {
        tx: chat.tx.clone().unwrap(),
        resumed: false,
    });
    assert_eq!(chat.link, Link::Connected);

    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    assert_eq!(chat.link, Link::Lost);
    assert!(chat.tx.is_none());
    assert!(!chat.busy, "a lost adapter cannot still be mid-turn");
}

/// Advertising modes or commands is bookkeeping, not conversation. A view
/// that rebuilt its run layout for these would rebuild it several times
/// during a handshake, before there is anything to lay out.
#[test]
fn session_metadata_does_not_touch_the_transcript() {
    let (mut chat, _rx) = chat_with_tx();
    let before = chat.revision();

    let outcome = chat.apply(AcpEvent::ModeChanged("plan".into()));

    assert!(!outcome.transcript_changed);
    assert_eq!(chat.revision(), before, "nothing rendered moved");
}

/// The case a count of items cannot see. Two chunks of one answer leave the
/// transcript exactly one item long both times, while what that item *says*
/// is different -- so anything cached against a length would go on showing
/// the first chunk.
#[test]
fn streaming_into_one_block_still_counts_as_a_change() {
    let (mut chat, _rx) = chat_with_tx();
    chat.apply(AcpEvent::AgentChunk("half ".into()));
    let (after_first, len) = (chat.revision(), chat.items.len());

    let outcome = chat.apply(AcpEvent::AgentChunk("an answer".into()));

    assert!(outcome.transcript_changed);
    assert_eq!(chat.items.len(), len, "still one answer block");
    assert_ne!(chat.revision(), after_first, "and it is not the same block");
}

/// The reducer says the turn settled, rather than every caller matching on
/// the event a second time to find out.
/// A turn ending settles the steps it left in flight.
///
/// Nothing more arrives for a call the adapter never finished -- a
/// cancelled turn is the ordinary way that happens -- so a step left
/// running stays that way for the rest of the conversation, and everything
/// downstream reads it as live.
#[test]
fn a_turn_ending_settles_what_it_left_running() {
    let mut chat = Chat::new(1, std::path::PathBuf::from("/tmp/p"), "a".into(), None);
    chat.busy = true;
    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "slow".into(),
        title: "cargo build".into(),
        description: None,
        kind: crate::acp::ToolKind::Execute,
        status: ToolStatus::InProgress,
        content: Vec::new(),
    })));

    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "cancelled".into(),
    });

    let ChatItem::Tool(tool) = &chat.items[0] else {
        panic!("the step is still there");
    };
    // Failed and not completed: what is known is that it never reported
    // finishing, and a card claiming a write went through is the one
    // reading a transcript cannot recover from.
    assert_eq!(tool.call.status, ToolStatus::Failed);
    assert!(
        tool.elapsed_secs.is_some(),
        "it stopped, so it has a length"
    );
}

/// A prompt beyond the ones a run sent, or one waiting behind the turn, is
/// somebody else driving the session.
#[test]
fn a_prompt_beyond_the_ones_sent_is_somebody_elses() {
    let (mut chat, _rx) = chat_with_tx();
    assert!(!chat.prompted_beyond(0));
    assert!(chat.submit("the run's", &[]));
    assert!(!chat.prompted_beyond(1));
    assert!(chat.prompted_beyond(0));
    chat.busy = true;
    assert!(chat.queue("a person's, queued behind the turn", &[]));
    assert!(chat.prompted_beyond(1));
}

/// Whether the last turn was cancelled is kept, so something driving the
/// session can tell a Stop from a turn that finished.
#[test]
fn a_cancelled_turn_is_told_from_a_finished_one() {
    let (mut chat, _rx) = chat_with_tx();
    assert!(!chat.cancelled);
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "cancelled".into(),
    });
    assert!(chat.cancelled);
    assert!(chat.submit("next", &[]));
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(!chat.cancelled);

    // A Stop the adapter answers with an error still ends a turn as
    // "end_turn"; the cancel asked for is what counts, until the next prompt.
    chat.busy = true;
    chat.cancel_turn();
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(chat.cancelled);
    assert!(chat.submit("again", &[]));
    assert!(!chat.cancelled);
}

#[test]
fn the_turn_ending_is_reported_once() {
    let (mut chat, _rx) = chat_with_tx();
    assert!(
        !chat
            .apply(AcpEvent::AgentChunk("working".into()))
            .turn_ended
    );

    let outcome = chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });

    assert!(outcome.turn_ended);
    assert!(outcome.transcript_changed, "the turn's card settles too");
    assert!(!chat.busy);
}

/// The reducer reports the *moment* a session starts waiting on the user,
/// which is not a question the transcript can answer.
///
/// `awaiting_permission` stays true for as long as the card is unanswered,
/// so a caller reading it would say "waiting" again on every chunk that
/// arrived afterwards -- and something that says a session is blocked, out
/// loud and outside the window, must say it once per ask.
#[test]
fn an_ask_is_reported_once_and_says_which_kind_it_was() {
    let (mut chat, _rx) = chat_with_tx();
    assert!(chat
        .apply(AcpEvent::AgentChunk("working".into()))
        .asked_user
        .is_none());

    let parked = chat.apply(AcpEvent::Permission(permission("Run cargo test")));
    assert_eq!(parked.asked_user, Some(UserAsk::Permission));

    // The card is still up, so the transcript still reads as waiting --
    // and the next event must not announce it a second time.
    let after = chat.apply(AcpEvent::AgentChunk("still here".into()));
    assert!(chat.awaiting_permission());
    assert!(after.asked_user.is_none());

    let asked = chat.apply(AcpEvent::Elicitation(ask_item().req));
    assert_eq!(asked.asked_user, Some(UserAsk::Question));
}

/// The two asks are named apart. Both stop the turn dead and draw the same
/// mark, so the sentence is the only thing that says which one is waiting --
/// and telling someone an agent wants "approval" when it asked them a
/// question sends them looking for a button that is not there.
#[test]
fn each_ask_names_itself_and_the_agent() {
    assert_eq!(
        UserAsk::Permission.headline("Claude Code"),
        "Claude Code is waiting for your approval"
    );
    assert_eq!(
        UserAsk::Question.headline("Claude Code"),
        "Claude Code has a question for you"
    );
}

#[test]
fn a_turn_that_said_nothing_answers_nothing_rather_than_the_turn_before() {
    let mut chat = Chat::default();
    chat.items.push(ChatItem::User(UserMsg::text("plan it")));
    chat.items.push(ChatItem::Agent(Md::parse("the plan")));
    let from = chat.items.len();
    chat.items
        .push(ChatItem::User(UserMsg::text("now change it")));
    chat.items.push(ChatItem::notice("a tool ran"));
    assert_eq!(chat.prose_since(from), "");
    chat.items.push(ChatItem::Agent(Md::parse("changed")));
    assert_eq!(chat.prose_since(from), "changed");
    assert_eq!(chat.prose_since(from + 10), "");
}

/// The moment an agent says which modes it offers is reported, so the app can
/// keep the list for starts that ask before the agent comes up again.
#[test]
fn the_modes_an_agent_offers_are_reported_when_it_says_them() {
    let (mut chat, _rx) = chat_with_tx();
    assert!(!chat.apply(AcpEvent::AgentChunk("hi".into())).modes_offered);
    let offered = chat.apply(AcpEvent::Modes {
        current: Some("default".into()),
        available: vec![crate::acp::Mode {
            id: "default".into(),
            name: "Default".into(),
        }],
    });
    assert!(offered.modes_offered);
    assert_eq!(chat.modes[0].id, "default");
}
