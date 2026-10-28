use super::*;
use crate::acp::{AcpEvent, ToolCall, ToolKind};
use crate::chat::model::UserMsg;
use std::path::PathBuf;

fn spawned(id: &str) -> AcpEvent {
    AcpEvent::Background(BackgroundReport::Spawned {
        id: id.into(),
        name: format!("build {id}"),
        kind: "bash".into(),
    })
}

fn ended(id: &str) -> AcpEvent {
    AcpEvent::Background(BackgroundReport::Ended { id: id.into() })
}

fn usage(used: u64, size: u64) -> AcpEvent {
    AcpEvent::Usage(Usage {
        used,
        size,
        limit: None,
        cycle_end: false,
        marks_cycles: false,
    })
}

fn limit(kind: Option<&str>, utilization: Option<f32>, status: LimitStatus) -> AcpEvent {
    AcpEvent::Usage(Usage {
        used: 1,
        size: 10,
        limit: Some(RateLimit {
            status,
            utilization,
            resets_at: None,
            kind: kind.map(str::to_string),
        }),
        cycle_end: false,
        marks_cycles: false,
    })
}

#[test]
fn a_spawned_item_is_listed_until_it_ends() {
    let mut chat = Chat::default();
    chat.apply(spawned("t1"));
    assert_eq!(chat.background().len(), 1);
    assert_eq!(chat.background()[0].name, "build t1");
    assert_eq!(chat.background()[0].kind, "bash");
    chat.apply(ended("t1"));
    assert!(chat.background().is_empty());
}

#[test]
fn a_repeated_spawn_is_listed_once() {
    let mut chat = Chat::default();
    chat.apply(spawned("t1"));
    chat.apply(spawned("t1"));
    assert_eq!(chat.background().len(), 1);
}

#[test]
fn an_end_for_an_unknown_item_is_ignored() {
    let mut chat = Chat::default();
    chat.apply(spawned("t1"));
    chat.apply(ended("nobody"));
    assert_eq!(chat.background().len(), 1);
}

#[test]
fn a_spawn_without_kind_or_name_falls_back() {
    let mut chat = Chat::default();
    chat.apply(AcpEvent::Background(BackgroundReport::Spawned {
        id: "t1".into(),
        name: String::new(),
        kind: String::new(),
    }));
    chat.apply(AcpEvent::Background(BackgroundReport::Spawned {
        id: "t2".into(),
        name: String::new(),
        kind: "agent".into(),
    }));
    let items = chat.background();
    assert_eq!(
        (items[0].name.as_str(), items[0].kind.as_str()),
        ("task", "task")
    );
    assert_eq!(
        (items[1].name.as_str(), items[1].kind.as_str()),
        ("agent", "agent")
    );
}

#[test]
fn background_reports_do_not_change_the_transcript() {
    let mut chat = Chat::default();
    assert!(!chat.apply(spawned("t1")).transcript_changed);
    assert!(!chat.apply(usage(1, 10)).transcript_changed);
}

#[test]
fn background_outlives_the_turn() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.tx = Some(tx);
    chat.busy = true;
    chat.apply(spawned("t1"));
    chat.cancel_turn();
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "cancelled".into(),
    });
    assert_eq!(chat.background().len(), 1);
}

#[test]
fn stop_background_asks_the_adapter_and_leaves_the_list() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::default();
    chat.tx = Some(tx);
    chat.apply(spawned("t1"));
    chat.stop_background("t1");
    assert!(matches!(
        rx.try_recv(),
        Ok(AcpRequest::StopBackground { id }) if id == "t1"
    ));
    assert_eq!(chat.background().len(), 1, "the adapter's report ends it");
}

#[test]
fn a_lost_adapter_clears_background_and_meters() {
    let mut chat = Chat::default();
    chat.apply(spawned("t1"));
    chat.apply(usage(84_000, 200_000));
    chat.apply(limit(Some("five_hour"), Some(0.5), LimitStatus::Allowed));
    chat.apply(AcpEvent::Disconnected("adapter exited".into()));
    assert!(chat.background().is_empty());
    assert_eq!(chat.context_meter(), None);
    assert_eq!(chat.limit_meter(), None);
}

#[test]
fn context_meter_rounds_and_clamps() {
    let mut chat = Chat::default();
    chat.apply(usage(84_000, 200_000));
    assert_eq!(
        chat.context_meter(),
        Some(Meter {
            label: "Context".into(),
            percent: Some(42),
            level: Level::Calm,
            tip: "Context: 84k of 200k tokens".into(),
        })
    );
    chat.apply(usage(250, 200));
    let meter = chat.context_meter().unwrap();
    assert_eq!((meter.percent, meter.level), (Some(100), Level::Warning));
    assert_eq!(meter.tip, "Context: 250 of 200 tokens");
    chat.apply(usage(5, 0));
    assert_eq!(chat.context_meter(), None);
}

#[test]
fn context_meter_warns_at_eighty() {
    let mut chat = Chat::default();
    chat.apply(usage(160_000, 200_000));
    assert_eq!(chat.context_meter().unwrap().level, Level::Warning);
    chat.apply(usage(159_999, 200_000));
    assert_eq!(chat.context_meter().unwrap().level, Level::Calm);
}

#[test]
fn limit_meter_follows_status_when_worse() {
    let mut chat = Chat::default();
    chat.apply(limit(Some("five_hour"), Some(0.30), LimitStatus::Rejected));
    let meter = chat.limit_meter().unwrap();
    assert_eq!((meter.percent, meter.level), (Some(30), Level::Danger));
    chat.apply(limit(Some("five_hour"), Some(0.30), LimitStatus::Warning));
    assert_eq!(chat.limit_meter().unwrap().level, Level::Warning);
    chat.apply(limit(Some("five_hour"), Some(0.90), LimitStatus::Allowed));
    assert_eq!(chat.limit_meter().unwrap().level, Level::Warning);
}

#[test]
fn limit_meter_clamps_and_survives_missing_utilization() {
    let mut chat = Chat::default();
    chat.apply(limit(Some("five_hour"), Some(1.4), LimitStatus::Rejected));
    assert_eq!(chat.limit_meter().unwrap().percent, Some(100));
    chat.apply(limit(Some("five_hour"), None, LimitStatus::Warning));
    let meter = chat.limit_meter().unwrap();
    assert_eq!((meter.percent, meter.level), (None, Level::Warning));
    assert_eq!(meter.tip, "5-hour limit");
}

#[test]
fn a_rejected_limit_without_utilization_has_no_percent() {
    let mut chat = Chat::default();
    chat.apply(limit(Some("five_hour"), None, LimitStatus::Rejected));
    let meter = chat.limit_meter().unwrap();
    assert_eq!((meter.percent, meter.level), (None, Level::Danger));
    assert_eq!(meter.tip, "5-hour limit");
}

#[test]
fn a_calm_limit_without_a_figure_says_nothing() {
    let mut chat = Chat::default();
    chat.apply(limit(Some("five_hour"), None, LimitStatus::Allowed));
    assert_eq!(chat.limit_meter(), None);
}

#[test]
fn limit_meter_shows_the_fullest_kind() {
    let mut chat = Chat::default();
    chat.apply(limit(Some("five_hour"), Some(0.78), LimitStatus::Allowed));
    chat.apply(limit(Some("seven_day"), Some(0.40), LimitStatus::Allowed));
    let meter = chat.limit_meter().unwrap();
    assert_eq!(meter.percent, Some(78));
    assert_eq!(meter.tip, "5-hour limit 78%\nWeekly limit 40%");
}

#[test]
fn limit_tip_names_each_kind_and_its_reset() {
    let mut chat = Chat::default();
    chat.apply(AcpEvent::Usage(Usage {
        used: 1,
        size: 10,
        limit: Some(RateLimit {
            status: LimitStatus::Allowed,
            utilization: Some(0.1),
            resets_at: Some(1_800_000_000),
            kind: Some("seven_day_opus".into()),
        }),
        cycle_end: false,
        marks_cycles: false,
    }));
    chat.apply(limit(
        Some("seven_day_sonnet"),
        Some(0.2),
        LimitStatus::Allowed,
    ));
    chat.apply(limit(Some("overage"), Some(0.05), LimitStatus::Allowed));
    let at = chrono::DateTime::from_timestamp(1_800_000_000, 0)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%H:%M");
    assert_eq!(
        chat.limit_meter().unwrap().tip,
        format!("Weekly Opus limit 10%, resets {at}\nWeekly Sonnet limit 20%\noverage 5%")
    );
}

#[test]
fn meter_labels_name_the_window() {
    let label = |kind: Option<&str>| {
        let mut chat = Chat::default();
        chat.apply(limit(kind, Some(0.5), LimitStatus::Allowed));
        chat.limit_meter().unwrap().label
    };
    assert_eq!(label(Some("five_hour")), "5h limit");
    assert_eq!(label(Some("seven_day")), "Weekly limit");
    assert_eq!(label(Some("seven_day_opus")), "Weekly limit");
    assert_eq!(label(Some("overage")), "Limit");
    assert_eq!(label(None), "Limit");
    let mut chat = Chat::default();
    chat.apply(usage(1, 10));
    assert_eq!(chat.context_meter().unwrap().label, "Context");
}

#[test]
fn no_reports_no_meters() {
    let chat = Chat::default();
    assert_eq!(chat.context_meter(), None);
    assert_eq!(chat.limit_meter(), None);
    assert!(chat.background().is_empty());
}

#[test]
fn a_cycle_end_is_reported() {
    let mut chat = Chat::default();
    let outcome = chat.apply(AcpEvent::Usage(Usage {
        used: 1,
        size: 10,
        limit: None,
        cycle_end: true,
        marks_cycles: true,
    }));
    assert!(outcome.cycle_ended);
    assert!(!chat.apply(usage(1, 10)).cycle_ended);
}

#[test]
fn a_refused_stop_is_reported() {
    let mut chat = Chat::default();
    let outcome = chat.apply(AcpEvent::StopRefused {
        why: "no such task".into(),
    });
    assert_eq!(outcome.stop_refused.as_deref(), Some("no such task"));
    assert_eq!(chat.apply(usage(1, 10)).stop_refused, None);
}

#[test]
fn unsaved_between_turns_waits_for_a_settled_tail() {
    let mut chat = Chat::new(
        1,
        PathBuf::from("/r"),
        "Claude".into(),
        Some(PathBuf::from("/store")),
    );
    chat.session_id = Some("sid-1".into());
    chat.link = Link::Connected;
    chat.items.push(ChatItem::User(UserMsg::text("go")));
    chat.busy = true;
    chat.apply(AcpEvent::AgentChunk {
        text: "done".into(),
        message: None,
    });
    assert!(!chat.unsaved_between_turns(), "not while busy");
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(chat.flush().is_some());
    assert!(!chat.unsaved_between_turns(), "everything is written");

    chat.apply(AcpEvent::AgentChunk {
        text: "a task finished".into(),
        message: None,
    });
    assert!(chat.unsaved_between_turns());
    assert!(
        matches!(chat.items.last(), Some(ChatItem::Agent(md)) if md.source == "a task finished"),
        "text after a written block starts its own"
    );

    chat.apply(AcpEvent::ToolCall(ToolCall {
        id: "c1".into(),
        title: "Read".into(),
        kind: ToolKind::Read,
        status: ToolStatus::Pending,
        content: Vec::new(),
        description: None,
    }));
    assert!(
        !chat.unsaved_between_turns(),
        "a step still in flight waits"
    );
    // Never written: the store is a path nobody owns.
    chat.store = None;
}

/// A chat whose turn is over and written, the adapter still live.
fn settled_chat() -> Chat {
    let mut chat = Chat::new(
        1,
        PathBuf::from("/r"),
        "Codex".into(),
        Some(PathBuf::from("/store")),
    );
    chat.session_id = Some("sid-1".into());
    chat.link = Link::Connected;
    chat.items.push(ChatItem::User(UserMsg::text("go")));
    chat.busy = true;
    chat.apply(AcpEvent::AgentChunk {
        text: "done".into(),
        message: None,
    });
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(chat.flush().is_some());
    // Never written: the store is a path nobody owns.
    chat.store = None;
    chat
}

#[test]
fn a_lost_adapter_leaves_nothing_to_save_between_turns() {
    let mut chat = settled_chat();
    chat.apply(AcpEvent::AgentChunk {
        text: "a task finished".into(),
        message: None,
    });
    assert!(chat.unsaved_between_turns());
    chat.apply(AcpEvent::Disconnected(String::new()));
    assert!(
        !chat.unsaved_between_turns(),
        "the failure line is not work between turns"
    );
}

#[test]
fn a_thought_still_streaming_waits() {
    let mut chat = settled_chat();
    chat.apply(AcpEvent::ThoughtChunk("thinking".into()));
    assert!(!chat.unsaved_between_turns());
    chat.apply(AcpEvent::AgentChunk {
        text: "so".into(),
        message: None,
    });
    assert!(chat.unsaved_between_turns(), "text after it settles it");
}

#[test]
fn only_agent_writing_between_turns_starts_a_quiet_save() {
    let mut chat = settled_chat();
    let outcome = chat.apply(usage(1, 10));
    assert!(!chat.starts_quiet_save(&outcome), "a report is not writing");
    // onehand's own line goes in without the reducer.
    chat.items.push(ChatItem::notice("run note".to_string()));
    let outcome = chat.apply(spawned("t1"));
    assert!(!chat.starts_quiet_save(&outcome));

    let outcome = chat.apply(AcpEvent::AgentChunk {
        text: "a task finished".into(),
        message: None,
    });
    assert!(chat.starts_quiet_save(&outcome));

    chat.busy = true;
    let outcome = chat.apply(AcpEvent::AgentChunk {
        text: "more".into(),
        message: None,
    });
    assert!(!chat.starts_quiet_save(&outcome), "a turn saves its own");
    let outcome = chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(!chat.starts_quiet_save(&outcome), "nor at its end");

    let outcome = chat.apply(AcpEvent::Disconnected(String::new()));
    assert!(
        !chat.starts_quiet_save(&outcome),
        "nor once the adapter is gone"
    );
}

#[test]
fn an_adapter_that_marks_its_cycles_gets_no_quiet_save() {
    let mut chat = settled_chat();
    chat.apply(AcpEvent::Usage(Usage {
        used: 1,
        size: 10,
        limit: None,
        cycle_end: false,
        marks_cycles: true,
    }));
    let outcome = chat.apply(AcpEvent::AgentChunk {
        text: "a task finished".into(),
        message: None,
    });
    assert!(outcome.transcript_changed);
    assert!(!chat.starts_quiet_save(&outcome));
}

#[test]
fn background_work_is_running() {
    let mut chat = Chat::default();
    assert!(!chat.running());
    chat.apply(spawned("t1"));
    assert!(chat.running(), "work outliving its turn");
    chat.apply(ended("t1"));
    assert!(!chat.running());
    chat.busy = true;
    assert!(chat.running(), "a turn open");
}

#[test]
fn a_stream_ending_forgets_the_activity() {
    let mut chat = Chat::default();
    chat.apply(spawned("t1"));
    chat.apply(limit(Some("five_hour"), Some(0.5), LimitStatus::Allowed));
    chat.stream_ended();
    assert!(chat.background().is_empty());
    assert_eq!(chat.context_meter(), None);
    assert_eq!(chat.limit_meter(), None);
}

#[test]
fn a_rejected_limit_without_a_figure_is_shown() {
    let mut chat = Chat::default();
    chat.apply(limit(Some("five_hour"), Some(0.3), LimitStatus::Allowed));
    chat.apply(limit(Some("seven_day"), None, LimitStatus::Rejected));
    let meter = chat.limit_meter().unwrap();
    assert_eq!(meter.label, "Weekly limit");
    assert_eq!(meter.level, Level::Danger);
    assert_eq!(meter.percent, None);
}

#[test]
fn a_pending_rewrite_is_not_work_between_turns() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat = Chat::new(
        1,
        PathBuf::from("/r"),
        "Claude".into(),
        Some(PathBuf::from("/store")),
    );
    chat.resume_from(crate::chat::store::ConversationSnapshot {
        session_id: "sid-1".into(),
        title: None,
        updated: 1,
        prefs: crate::chat::store::Prefs::default(),
        items: vec![ChatItem::User(UserMsg::text("message 0"))],
        written: 1,
        complete: true,
    });
    chat.apply(AcpEvent::UserChunk("message 0".into()));
    chat.apply(AcpEvent::AgentChunk {
        text: "answer".into(),
        message: None,
    });
    chat.apply(AcpEvent::Connected {
        tx,
        resumed: true,
        forkable: false,
    });
    assert!(chat.rewrite, "the replay settled into a rewrite");
    assert!(!chat.busy);
    assert!(!chat.unsaved_between_turns());
    chat.store = None;
}

/// A card still working is not something to save between turns: the save
/// would write nothing, so the gate does not ask for one.
#[test]
fn a_working_card_alone_is_not_unsaved() {
    let mut chat = Chat::new(
        1,
        PathBuf::from("/r"),
        "Claude".into(),
        Some(PathBuf::from("/store")),
    );
    chat.session_id = Some("sid-1".into());
    chat.link = Link::Connected;
    chat.items.push(ChatItem::User(UserMsg::text("go")));
    assert!(chat.flush().is_some());
    chat.apply(AcpEvent::SubagentSpawned {
        id: "c1".into(),
        name: "explore".into(),
        task: "look".into(),
    });
    chat.apply(AcpEvent::AgentChunk {
        text: "after the card".into(),
        message: None,
    });
    assert!(!chat.unsaved_between_turns(), "nothing before the card");
    assert!(chat.flush().is_none(), "and a save writes nothing");
    chat.apply(AcpEvent::SubagentEnded {
        id: "c1".into(),
        end: crate::acp::SubagentEnd::Completed,
    });
    assert!(chat.unsaved_between_turns(), "the ended card is");
    // Never written: the store is a path nobody owns.
    chat.store = None;
}

fn chunk(text: &str) -> AcpEvent {
    AcpEvent::AgentChunk {
        text: text.into(),
        message: None,
    }
}

fn cycle_closed(cycle_end: bool) -> AcpEvent {
    AcpEvent::Usage(Usage {
        used: 1,
        size: 10,
        limit: None,
        cycle_end,
        marks_cycles: true,
    })
}

#[test]
fn an_agent_writing_by_itself_runs_until_a_quiet_spell() {
    let mut chat = settled_chat();
    assert!(!chat.running());
    assert_eq!(chat.own_cycle_wait(), None);
    chat.apply(chunk("a task finished"));
    assert!(chat.running(), "between turns, writing is running");
    assert_eq!(chat.own_cycle_wait(), Some(QUIET));
    assert!(chat.quiet_spell());
    assert!(!chat.running());
    assert!(!chat.quiet_spell(), "nothing left to end");
    chat.store = None;
}

#[test]
fn a_cycle_the_adapter_marks_runs_until_its_end() {
    let mut chat = settled_chat();
    chat.apply(cycle_closed(false));
    chat.apply(chunk("a task finished"));
    assert!(chat.running());
    assert_eq!(chat.own_cycle_wait(), Some(GRACE), "its word is awaited");
    chat.apply(cycle_closed(true));
    assert!(!chat.running());
    chat.store = None;
}

#[test]
fn the_last_background_end_expects_an_answer() {
    let mut chat = settled_chat();
    chat.apply(spawned("t1"));
    chat.apply(spawned("t2"));
    chat.apply(ended("t1"));
    assert_eq!(chat.own_cycle_wait(), None, "t2 still runs");
    chat.apply(ended("t2"));
    assert!(chat.running(), "the answer to it is still to come");
    assert_eq!(chat.own_cycle_wait(), Some(GRACE));
    chat.apply(chunk("the build passed"));
    assert_eq!(chat.own_cycle_wait(), Some(QUIET), "and it came");
    assert!(chat.quiet_spell());
    assert!(!chat.running());

    // An answer that never comes is waited for once.
    chat.apply(spawned("t3"));
    chat.apply(ended("t3"));
    assert!(chat.quiet_spell());
    assert!(!chat.running());
    chat.store = None;
}

#[test]
fn a_step_in_flight_outlasts_a_quiet_spell() {
    let mut chat = settled_chat();
    chat.apply(AcpEvent::ToolCall(ToolCall {
        id: "c9".into(),
        title: "Bash".into(),
        kind: ToolKind::Execute,
        status: ToolStatus::InProgress,
        content: Vec::new(),
        description: None,
    }));
    assert!(chat.running());
    assert!(!chat.quiet_spell(), "the step still runs");
    assert!(chat.running());
    chat.store = None;
}

#[test]
fn no_own_cycle_inside_a_turn_in_a_replay_or_after_the_adapter() {
    let mut chat = settled_chat();
    chat.busy = true;
    chat.apply(spawned("t1"));
    chat.apply(ended("t1"));
    chat.apply(AcpEvent::TurnEnded {
        stop_reason: "end_turn".into(),
    });
    assert!(!chat.running(), "the turn answered it");

    chat.link = Link::Connecting;
    chat.apply(chunk("replayed"));
    assert!(
        !chat.running(),
        "a resume replaying is not the agent at work"
    );

    chat.link = Link::Connected;
    chat.apply(chunk("on its own"));
    chat.apply(AcpEvent::Disconnected(String::new()));
    assert!(!chat.running());
    chat.store = None;
}
