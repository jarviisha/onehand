use super::metrics::{
    BUTTON_H, DETAIL_OPEN_H, HAIR_GAP, LINE_H, MARK_SIZE, PART_GAP, STACK_GAP, THUMB_H, THUMB_W,
};
use super::parts::Object;
use super::parts::RowMark;
use super::strip::activity_identity;
use super::tool::is_error_line;
use super::*;
use onehand_core::acp::ToolCall;
use onehand_core::acp::{ToolContent, ToolKind, ToolStatus};
use onehand_core::chat::activity;
use onehand_core::chat::{ChatItem, Md, Thought, ToolItem, TranscriptItemId};

fn thought(secs: u64) -> ChatItem {
    ChatItem::Thought(Thought {
        md: Md::parse("…"),
        started: None,
        elapsed_secs: Some(secs),
        fold: None,
    })
}

fn tool(kind: ToolKind, title: &str) -> ChatItem {
    tool_with_status(kind, title, ToolStatus::Completed)
}

fn tool_with_status(kind: ToolKind, title: &str, status: ToolStatus) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: title.into(),
        title: title.into(),
        description: None,
        kind,
        status,
        content: Vec::new(),
    }))
}

fn targets(items: &[ChatItem]) -> Vec<(TranscriptItemId, &ChatItem)> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| (TranscriptItemId::Live(i), item))
        .collect()
}

/// Every named step, at the app's own rem size.
fn steps() -> Vec<(&'static str, f32)> {
    vec![
        ("HAIR_GAP", HAIR_GAP.0),
        ("TIGHT_GAP", TIGHT_GAP.0),
        ("STACK_GAP", STACK_GAP.0),
        ("PART_GAP", PART_GAP.0),
        ("BLOCK_GAP", BLOCK_GAP.0),
        ("TURN_GAP", TURN_GAP.0),
        ("TEXT", TEXT.0),
        ("TEXT_SM", TEXT_SM.0),
        ("BUTTON_H", BUTTON_H.0),
        ("LINE_H", LINE_H.0),
        ("MARK_SIZE", MARK_SIZE.0),
        ("THUMB_W", THUMB_W.0),
        ("THUMB_H", THUMB_H.0),
        ("DETAIL_OPEN_H", DETAIL_OPEN_H.0),
    ]
}

/// Nothing in the transcript is sized off the scale.
///
/// The point of a scale is that the *next* value is chosen from it rather
/// than measured by eye, and nothing about a rem constant stops somebody
/// writing `rems(0.7)`. This is what says so out loud — and it fails on the
/// value's name, so the failure names the step that left the ladder rather
/// than printing a number.
#[test]
fn every_step_is_on_the_scale() {
    // Half a step is a step nobody can see, so the ladder skips no rung it
    // does not use and admits no rung between two it does.
    const SCALE: &[f32] = &[
        2., 4., 6., 8., 10., 12., 14., 16., 18., 20., 24., 28., 32., 40., 44., 62., 64., 92., 320.,
    ];
    for (name, rems) in steps() {
        let px = rems * 16.;
        assert!(
            SCALE.iter().any(|step| (step - px).abs() < 0.01),
            "{name} is {px}px, which is not a step of the scale"
        );
    }
}

/// What is inside a thing is closer than what surrounds it, at every level.
///
/// This is the one rule the whole arrangement rests on: it is what makes a
/// turn boundary readable without a rule across the column, and the first
/// thing to break when one gap is nudged to fix the look of one block.
#[test]
fn the_gaps_nest_and_so_do_the_corners() {
    let ladder = [
        ("HAIR_GAP", HAIR_GAP.0),
        ("TIGHT_GAP", TIGHT_GAP.0),
        ("STACK_GAP", STACK_GAP.0),
        ("PART_GAP", PART_GAP.0),
        ("BLOCK_GAP", BLOCK_GAP.0),
        ("TURN_GAP", TURN_GAP.0),
    ];
    for pair in ladder.windows(2) {
        let [(inner, a), (outer, b)] = pair else {
            unreachable!()
        };
        assert!(a < b, "{inner} must stay under {outer}");
    }

    // What the agent did reads a step under what it said.
    const { assert!(TEXT_SM.0 < TEXT.0, "TEXT_SM must stay under TEXT") };
}

#[test]
fn every_activity_group_has_its_own_name_and_icon() {
    let groups = [
        activity::ActivityGroup::Explored,
        activity::ActivityGroup::Changed,
        activity::ActivityGroup::Ran,
        activity::ActivityGroup::Verified,
        activity::ActivityGroup::Reasoned,
        activity::ActivityGroup::Other,
    ];
    let identities = groups.map(activity_identity);
    let mut names = identities.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    // The identity already carries the asset path, which is the fact this
    // test is about: two groups drawing the same SVG is the collision worth
    // catching, whatever the two names in front of it are -- and the six
    // are no longer even drawn from one enum.
    let mut icons = identities
        .iter()
        .map(|(_, icon)| icon.clone())
        .collect::<Vec<_>>();
    icons.sort_unstable();
    icons.dedup();

    assert_eq!(names.len(), groups.len());
    assert_eq!(icons.len(), groups.len());
}

/// **A cluster of one is still a cluster.** It was the exception —
/// a lone step drew itself bare — which meant the transcript had two ways
/// of saying the same thing and which one a reader got depended on whether
/// the agent happened to do a second thing afterwards.
#[test]
fn one_step_is_still_a_cluster() {
    let items = vec![tool(ToolKind::Read, "Read src/a.rs")];
    let runs = runs(&targets(&items));
    assert!(
        matches!(runs.as_slice(), [Run::Activity { members }] if members.len() == 1),
        "a lone step is a cluster of one"
    );
}

/// **A cluster is bounded by the agent's words and by nothing else.** Kinds
/// of work used to bound it too, so three reads and a command between one
/// paragraph and the next drew two headers — two claims about one stretch
/// of work, with nothing between them to explain the seam.
#[test]
fn every_kind_of_work_between_two_paragraphs_is_one_cluster() {
    let items = vec![
        tool(ToolKind::Read, "Read src/a.rs"),
        tool(ToolKind::Read, "Read src/b.rs"),
        tool(ToolKind::Execute, "cargo build"),
        tool(ToolKind::Edit, "src/a.rs"),
    ];
    let runs = runs(&targets(&items));
    assert!(
        matches!(runs.as_slice(), [Run::Activity { members }] if members.len() == 4),
        "{} runs, wanted one",
        runs.len()
    );
}

/// **A step's status does not decide which cluster it is in.** Running work
/// used to sit outside, and move in when it finished — so the row count
/// changed every few seconds mid-turn, and a step that had been a card
/// became a line in a list somebody was already reading.
#[test]
fn what_a_step_is_doing_does_not_move_it_between_clusters() {
    let items = vec![
        tool_with_status(ToolKind::Read, "Read src/pending.rs", ToolStatus::Pending),
        tool_with_status(ToolKind::Read, "Read src/live-a.rs", ToolStatus::InProgress),
        tool_with_status(ToolKind::Read, "Read src/failed.rs", ToolStatus::Failed),
        tool(ToolKind::Read, "Read src/done-a.rs"),
        tool(ToolKind::Read, "Read src/done-b.rs"),
    ];
    let runs = runs(&targets(&items));
    assert!(
        matches!(runs.as_slice(), [Run::Activity { members }] if members.len() == 5),
        "{} runs, wanted one",
        runs.len()
    );
}

#[test]
fn a_prompt_breaks_a_run() {
    // A user prompt is not activity, so the steps either side of it belong
    // to different turns and must not be folded into one strip.
    let items = vec![
        tool(ToolKind::Read, "Read src/a.rs"),
        tool(ToolKind::Read, "Read src/b.rs"),
        ChatItem::notice("interrupted"),
        tool(ToolKind::Read, "Read src/c.rs"),
    ];
    let runs = runs(&targets(&items));
    assert!(
        matches!(
            runs.as_slice(),
            [
                Run::Activity { members: before },
                Run::Single(_),
                Run::Activity { members: after }
            ] if before.len() == 2 && after.len() == 1
        ),
        "the notice between them is what makes two clusters of one"
    );
}

#[test]
fn summary_aggregates_repeated_steps() {
    let items = [thought(3), thought(3), thought(4), thought(5), thought(6)];
    let bodies: Vec<&ChatItem> = items.iter().collect();
    assert_eq!(activity_summary(&bodies), "21s reasoning");

    let reads = [
        tool(ToolKind::Read, "Read src/a.rs"),
        tool(ToolKind::Read, "Read src/b.rs"),
    ];
    let bodies: Vec<&ChatItem> = reads.iter().collect();
    assert_eq!(activity_summary(&bodies), "2 files");

    let failed_reads = [
        tool(ToolKind::Read, "Read src/a.rs"),
        tool_with_status(ToolKind::Read, "Read src/b.rs", ToolStatus::Failed),
    ];
    // **The state is no longer in the summary**, and neither is the verb.
    // The row this lands in carries both, two columns to the left.
    let bodies: Vec<&ChatItem> = failed_reads.iter().collect();
    assert_eq!(activity_summary(&bodies), "2 files");

    let multi_edit = ChatItem::Tool(ToolItem::new(ToolCall {
        id: "multi-edit".into(),
        title: "Edit project config".into(),
        description: None,
        kind: ToolKind::Edit,
        status: ToolStatus::Completed,
        content: vec![
            ToolContent::Diff {
                path: "package.json".into(),
                old: Some("{}".into()),
                new: "{\"type\":\"module\"}".into(),
            },
            ToolContent::Diff {
                path: "tsconfig.json".into(),
                old: Some("{}".into()),
                new: "{\"module\":\"NodeNext\"}".into(),
            },
        ],
    }));
    assert_eq!(activity_summary(&[&multi_edit]), "2 files");
}

/// **A path is split so the name can lead.** A reader scanning a column of
/// them is looking for the file; the directory above it is only there for
/// the times two names are the same, and at one weight it takes the eye
/// first every time it is long.
#[test]
fn a_row_splits_a_path_and_leaves_a_command_whole() {
    let file = Object::path("crates/app/src/chat/pane.rs");
    assert_eq!(file.dir.as_deref(), Some("crates/app/src/chat/"));
    assert_eq!(file.name.as_ref(), "pane.rs");

    let bare = Object::path("README.md");
    assert_eq!(bare.dir, None);
    assert_eq!(bare.name.as_ref(), "README.md");

    // A command holds slashes and is not a path: cut at the last one it
    // would quote something nobody ran.
    let command = Object::path("cargo test -p onehand --manifest-path ./Cargo.toml");
    assert_eq!(command.dir, None);
    assert!(command.name.contains("cargo test"));
}

/// Output is read for the line that went wrong, so that line is the one
/// the ink follows — not the row it sits in. A count of none in a passing
/// summary is not a failure.
#[test]
fn the_line_that_went_wrong_is_the_one_marked() {
    assert!(is_error_line("error[E0433]: failed to resolve"));
    assert!(is_error_line("  FAILED: 1 test"));
    assert!(is_error_line("panicked at src/lib.rs:4"));
    assert!(is_error_line("test backoff_caps ... FAILED"));
    assert!(is_error_line(
        "thread 'backoff_caps' panicked at 'elapsed 5.2s > 5s'"
    ));
    assert!(is_error_line("test result: FAILED. 11 passed; 1 failed"));
    assert!(is_error_line("error[E0308]: mismatched types"));
    assert!(!is_error_line("test result: ok. 12 passed; 0 failed"));
    assert!(!is_error_line("test backoff_caps ... ok"));
    assert!(!is_error_line("test result: ok. 34 passed"));
    assert!(!is_error_line("   Compiling onehand v0.1.0"));
}

/// **A thought and a settled exchange are lines of their own.** The agent
/// reasoning is read as its words, and an answered question is the person
/// speaking; neither is one more step folded into the work around it.
#[test]
fn a_thought_or_an_answer_stands_between_clusters() {
    let items = vec![
        tool(ToolKind::Read, "Read src/a.rs"),
        thought(2),
        tool(ToolKind::Read, "Read src/b.rs"),
    ];
    let runs = runs(&targets(&items));
    assert!(
        matches!(
            runs.as_slice(),
            [Run::Activity { .. }, Run::Single(_), Run::Activity { .. }]
        ),
        "the thought splits the work either side of it"
    );
}

/// The mark a run's row carries reads its ending, not its worst moment.
#[test]
fn a_fixed_failure_is_not_an_alarm() {
    use onehand_core::chat::{Outcome, RunOutcome};
    let mark = |outcome| RowMark::of_run(RunOutcome { outcome, errors: 0 });
    assert!(matches!(mark(Outcome::Clean), RowMark::Done));
    assert!(matches!(mark(Outcome::Recovered), RowMark::Recovered));
    assert!(matches!(mark(Outcome::Failed), RowMark::Failed));
    assert!(matches!(mark(Outcome::Running), RowMark::Running));
}

/// **Two clusters never stand next to each other.**
///
/// It is the rule the whole arrangement rests on: a cluster is bounded by
/// the agent's words, so two of them with nothing between is one stretch of
/// work claiming to be two — and drawn, that is two muted lines a reader has
/// to work out the seam between.
#[test]
fn two_clusters_never_stand_next_to_each_other() {
    let items = vec![
        ChatItem::User(onehand_core::chat::UserMsg::text("go")),
        tool(ToolKind::Read, "a.rs"),
        thought(2),
        tool(ToolKind::Execute, "cargo build"),
        tool_with_status(ToolKind::Read, "b.rs", ToolStatus::InProgress),
        ChatItem::Agent(Md::parse("done")),
        tool(ToolKind::Edit, "a.rs"),
        ChatItem::notice("interrupted"),
        tool(ToolKind::Read, "c.rs"),
    ];
    let runs = runs(&targets(&items));
    let mut previous_was_cluster = false;
    for run in &runs {
        let cluster = matches!(run, Run::Activity { .. });
        assert!(
            !(cluster && previous_was_cluster),
            "two clusters with nothing between them"
        );
        previous_was_cluster = cluster;
    }
    // And what does bound one is prose, a prompt, a thought or a notice:
    // four clusters here.
    assert_eq!(
        runs.iter()
            .filter(|run| matches!(run, Run::Activity { .. }))
            .count(),
        4
    );
}
