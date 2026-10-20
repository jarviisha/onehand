use super::*;
use crate::acp::{ToolCall, ToolContent, ToolKind, ToolStatus};
use crate::chat::model::{Md, Thought, ToolItem};

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

/// **A cluster of one is still a cluster.** It was the exception —
/// a lone step drew itself bare — which meant the transcript had two ways
/// of saying the same thing and which one a reader got depended on whether
/// the agent happened to do a second thing afterwards.
#[test]
fn one_step_is_still_a_cluster() {
    let items = vec![tool(ToolKind::Read, "Read src/a.rs")];
    let runs = clusters(&targets(&items));
    assert!(
        matches!(runs.as_slice(), [Cluster::Activity { members }] if members.len() == 1),
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
    let runs = clusters(&targets(&items));
    assert!(
        matches!(runs.as_slice(), [Cluster::Activity { members }] if members.len() == 4),
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
    let runs = clusters(&targets(&items));
    assert!(
        matches!(runs.as_slice(), [Cluster::Activity { members }] if members.len() == 5),
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
    let runs = clusters(&targets(&items));
    assert!(
        matches!(
            runs.as_slice(),
            [
                Cluster::Activity { members: before },
                Cluster::Single(_),
                Cluster::Activity { members: after }
            ] if before.len() == 2 && after.len() == 1
        ),
        "the notice between them is what makes two clusters of one"
    );
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
    let runs = clusters(&targets(&items));
    assert!(
        matches!(
            runs.as_slice(),
            [
                Cluster::Activity { .. },
                Cluster::Single(_),
                Cluster::Activity { .. }
            ]
        ),
        "the thought splits the work either side of it"
    );
}

#[test]
fn summary_aggregates_repeated_steps() {
    let items = [thought(3), thought(3), thought(4), thought(5), thought(6)];
    let bodies: Vec<&ChatItem> = items.iter().collect();
    assert_eq!(section_summary(&bodies), "21s reasoning");

    let reads = [
        tool(ToolKind::Read, "Read src/a.rs"),
        tool(ToolKind::Read, "Read src/b.rs"),
    ];
    let bodies: Vec<&ChatItem> = reads.iter().collect();
    assert_eq!(section_summary(&bodies), "2 files");

    let failed_reads = [
        tool(ToolKind::Read, "Read src/a.rs"),
        tool_with_status(ToolKind::Read, "Read src/b.rs", ToolStatus::Failed),
    ];
    // **The state is no longer in the summary**, and neither is the verb.
    // The row this lands in carries both, two columns to the left.
    let bodies: Vec<&ChatItem> = failed_reads.iter().collect();
    assert_eq!(section_summary(&bodies), "2 files");

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
    assert_eq!(section_summary(&[&multi_edit]), "2 files");
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
        ChatItem::User(crate::chat::model::UserMsg::text("go")),
        tool(ToolKind::Read, "a.rs"),
        thought(2),
        tool(ToolKind::Execute, "cargo build"),
        tool_with_status(ToolKind::Read, "b.rs", ToolStatus::InProgress),
        ChatItem::Agent(Md::parse("done")),
        tool(ToolKind::Edit, "a.rs"),
        ChatItem::notice("interrupted"),
        tool(ToolKind::Read, "c.rs"),
    ];
    let runs = clusters(&targets(&items));
    let mut previous_was_cluster = false;
    for run in &runs {
        let cluster = matches!(run, Cluster::Activity { .. });
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
            .filter(|run| matches!(run, Cluster::Activity { .. }))
            .count(),
        4
    );
}
