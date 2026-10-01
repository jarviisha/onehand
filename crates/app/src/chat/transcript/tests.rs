use super::metrics::{
    BUTTON_H, CHEVRON_MARK, CHEVRON_SLOT, CODE_TEXT, DETAIL_INSET, DETAIL_OPEN_H, DIFF_COLUMNS,
    DIFF_NUM_PAD, DIFF_NUM_W, DIFF_SIGN_W, DIFF_TEXT_PAD, FRAME_PAD, HAIR_GAP, KIND_ICON, LINE_H,
    MARK_SIZE, MARK_SLOT, MONO_ADVANCE, OBJECT_TEXT, PART_GAP, PILL_H, PILL_PAD_X, PILL_PAD_Y,
    PLAN_BOX, ROW_PAD_X, ROW_PAD_Y, SMOKE_DIFF, SMOKE_OUT, STACK_GAP, STATUS_DOT, TEXT_PAD_X,
    TEXT_PAD_Y, THUMB_H, THUMB_W, VERB_TEXT,
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
        expanded: false,
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
        ("FRAME_PAD", FRAME_PAD.0),
        ("BLOCK_GAP", BLOCK_GAP.0),
        ("TURN_GAP", TURN_GAP.0),
        ("TEXT_PAD_X", TEXT_PAD_X.0),
        ("TEXT_PAD_Y", TEXT_PAD_Y.0),
        ("ROW_PAD_Y", ROW_PAD_Y.0),
        ("ROW_PAD_X", ROW_PAD_X.0),
        ("STATUS_DOT", STATUS_DOT.0),
        ("KIND_ICON", KIND_ICON.0),
        ("CHEVRON_SLOT", CHEVRON_SLOT.0),
        ("PILL_PAD_Y", PILL_PAD_Y.0),
        ("PILL_PAD_X", PILL_PAD_X.0),
        ("PILL_H", PILL_H.0),
        ("BUTTON_H", BUTTON_H.0),
        ("LINE_H", LINE_H.0),
        ("MARK_SLOT", MARK_SLOT.0),
        ("MARK_SIZE", MARK_SIZE.0),
        ("DIFF_NUM_PAD", DIFF_NUM_PAD.0),
        ("DIFF_SIGN_W", DIFF_SIGN_W.0),
        ("DIFF_TEXT_PAD", DIFF_TEXT_PAD.0),
        ("PLAN_BOX", PLAN_BOX.0),
        ("THUMB_W", THUMB_W.0),
        ("THUMB_H", THUMB_H.0),
        ("SMOKE_DIFF", SMOKE_DIFF.0),
        ("SMOKE_OUT", SMOKE_OUT.0),
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

    // **A row's words start after its padding, its state disc and its kind
    // icon**, and whatever that row opens is set in to the same place —
    // written as a sum in one spot and a number in the other, the two
    // drifted the first time either column moved.
    assert_eq!(
        DETAIL_INSET.0,
        ROW_PAD_X.0 + STATUS_DOT.0 + PART_GAP.0 + KIND_ICON.0 + PART_GAP.0
    );
    // The marks and the type, each inside what holds it: a disc inside the
    // slot that keeps its column (which is what lets a spinner take its
    // place), the arrow quietest of the three, and what a row did it *to*
    // a step under what it says it did.
    let inside = [
        ("STATUS_DOT", STATUS_DOT.0, "KIND_ICON", KIND_ICON.0),
        ("CHEVRON_MARK", CHEVRON_MARK.0, "KIND_ICON", KIND_ICON.0),
        ("OBJECT_TEXT", OBJECT_TEXT.0, "VERB_TEXT", VERB_TEXT.0),
    ];
    for (inner, a, outer, b) in inside {
        assert!(a < b, "{inner} must stay under {outer}");
    }
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
    let items = vec![thought(3)];
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
        thought(2),
    ];
    let runs = runs(&targets(&items));
    assert!(
        matches!(runs.as_slice(), [Run::Activity { members }] if members.len() == 5),
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
/// the ink follows — not the row it sits in.
#[test]
fn the_line_that_went_wrong_is_the_one_marked() {
    assert!(is_error_line("error[E0433]: failed to resolve"));
    assert!(is_error_line("  FAILED: 1 test"));
    assert!(is_error_line("panicked at src/lib.rs:4"));
    assert!(!is_error_line("test result: ok. 34 passed"));
    assert!(!is_error_line("   Compiling onehand v0.1.0"));
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
    // And what does bound one is prose, a prompt, or a notice — three
    // boundaries here, so four clusters.
    assert_eq!(
        runs.iter()
            .filter(|run| matches!(run, Run::Activity { .. }))
            .count(),
        3
    );
}

/// **A line of this project's own code fits the reading column.**
///
/// The column used to be set by prose alone, which left a diff 74 columns
/// wide against the 100 `rustfmt` writes at — so nearly every line of
/// nearly every diff wrapped, in the one block somebody opens the
/// transcript to read when something has broken. The cap is derived from
/// that instead, and this is what keeps it derived: shift any inset between
/// the column and the text and the sum moves, rather than the diff quietly
/// getting tighter.
#[test]
fn a_hundred_columns_of_this_projects_code_fits() {
    // Every inset between the edge of the column and a diff's first
    // character, at the deepest place one is drawn: inside a child row's
    // own detail.
    // Every inset between the edge of the column and a diff's first
    // character: the row's own detail inset, the frame's right margin, the
    // detail box's border, and the diff's number and sign columns.
    let chrome =
        DETAIL_INSET.0 + ROW_PAD_X.0 + 2. / 16. + DIFF_NUM_W.0 + DIFF_SIGN_W.0 + DIFF_TEXT_PAD.0;
    let text = CODE_TEXT.0 * MONO_ADVANCE * DIFF_COLUMNS;
    assert!(
        CONTENT_COLUMN.0 >= chrome + text,
        "{DIFF_COLUMNS} columns need {:.2}rem and the column caps at {:.2}rem",
        chrome + text,
        CONTENT_COLUMN.0
    );
}
