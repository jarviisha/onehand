//! Where one cluster of activity ends and the next begins, and what a
//! section of one says: decided here so the two front ends cannot disagree
//! about either.

use crate::chat::activity::{self, ActivityKind};
use crate::chat::model::{ChatItem, TranscriptItemId};

/// A stretch of the transcript as it is drawn: one item on its own, or every
/// adjacent step between two of the agent's paragraphs, which is one cluster
/// and draws one muted line.
pub enum Cluster {
    Single(TranscriptItemId),
    Activity { members: Vec<TranscriptItemId> },
}

/// Split the transcript into clusters: either a single item, or a stretch of
/// adjacent steps that folds into one line.
pub fn clusters(items: &[(TranscriptItemId, &ChatItem)]) -> Vec<Cluster> {
    let mut out: Vec<Cluster> = Vec::new();
    for &(target, item) in items {
        match (is_activity(item), out.last_mut()) {
            // Still the same stretch of work: extend it.
            (true, Some(Cluster::Activity { members })) => members.push(target),
            (true, _) => out.push(Cluster::Activity {
                members: vec![target],
            }),
            (false, _) => out.push(Cluster::Single(target)),
        }
    }
    out
}

/// Whether an item is part of what the agent *did* rather than what it said.
///
/// **Every status, and no kind boundary.** What a cluster is bounded by is the
/// agent's own words: everything between two paragraphs is one thing it did
/// between saying two things, however many kinds of work that took and whatever
/// state each is in.
///
/// **Tools only.** A thought is the agent reasoning, which is read as words of
/// its own rather than counted as work, and a settled question or grant is the
/// person answering: each is a line of its own between the clusters, where it
/// happened, rather than one more row folded inside them.
fn is_activity(item: &ChatItem) -> bool {
    match item {
        ChatItem::Tool(_) => true,
        ChatItem::User(_)
        | ChatItem::Agent(_)
        | ChatItem::Thought(_)
        | ChatItem::Plan(_)
        | ChatItem::Permission(_)
        | ChatItem::Ask(_)
        | ChatItem::Subagent(_)
        | ChatItem::Notice { .. } => false,
    }
}

/// A semantic, counted summary of the work in one activity run.
///
/// Per-target phrases make the folded form almost as expensive to scan as its
/// expanded rows (`Inspected a · Inspected b · Inspected c`). Counts preserve
/// what happened while letting one glance answer how much happened.
pub fn section_summary(members: &[&ChatItem]) -> String {
    let mut counts = [0usize; 10];
    let mut reason_secs = 0u64;
    for item in members {
        let kind = match item {
            ChatItem::Thought(th) => {
                counts[8] += 1;
                if let Some(secs) = th.elapsed_secs {
                    reason_secs += secs;
                }
                continue;
            }
            ChatItem::Tool(tool) => {
                let p = activity::presentation(tool);
                if p.kind == ActivityKind::Change {
                    // One MultiEdit is one tool step but can touch many files;
                    // the summary names files, so count its distinct paths.
                    counts[3] += tool.diff_summary.len().max(1);
                    continue;
                }
                p.kind
            }
            _ => continue,
        };
        let index = match kind {
            ActivityKind::Inspect => 0,
            ActivityKind::Search => 1,
            ActivityKind::Fetch => 2,
            ActivityKind::Change => 3,
            ActivityKind::Test => 4,
            ActivityKind::Check => 5,
            ActivityKind::Build => 6,
            ActivityKind::Run => 7,
            ActivityKind::Reason => 8,
            ActivityKind::Other => 9,
        };
        counts[index] += 1;
    }

    // **Nouns, with no verb on any of them.** The row this lands in already
    // carries one, in full ink and a weight up, two columns to the left -- so
    // a phrase that opened with its own gave `Ran · Ran 7 commands`, which is
    // the same word twice within an inch and the count pushed a column right
    // for it.
    let mut phrases = Vec::new();
    let counted = [
        (0, "file", "files"),
        (1, "search", "searches"),
        (2, "resource", "resources"),
        (3, "file", "files"),
        (4, "test", "tests"),
        (5, "check", "checks"),
        (6, "target", "targets"),
        (7, "command", "commands"),
        (9, "tool", "tools"),
    ];
    for (index, singular, plural) in counted {
        let count = counts[index];
        if count > 0 {
            phrases.push(format!(
                "{count} {}",
                if count == 1 { singular } else { plural }
            ));
        }
    }
    if counts[8] > 0 {
        phrases.push(match reason_secs > 0 {
            true => format!("{reason_secs}s reasoning"),
            false => "reasoning".to_string(),
        });
    }

    let mut label = phrases.join(" · ");
    if label.is_empty() {
        label.push_str("activity");
    }
    label
}

#[cfg(test)]
mod tests;
