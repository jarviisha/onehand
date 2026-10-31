//! How the transcript's items are planned into rows: turns closed, runs grouped.

use super::*;

/// Put a row closing every finished turn into `plan`.
///
/// **Derived here and never stored.** The diffs it adds up are already in the
/// transcript and in the archive, so a summary written down beside them is a
/// second copy that can disagree with the first -- and `items.jsonl` is
/// appended to and never revisited, so a copy written at the end of a turn
/// could not be corrected if it ever did. Recomputing it is a walk of the
/// turn's own steps, which is work the cluster lines above it are already
/// doing.
///
/// **A cancelled turn still gets one.** What was written before the stop is on
/// disk exactly as if the turn had run to the end, and that is the moment a
/// reader most needs to be told which files those were.
pub(super) fn close_turns(
    chat: &Chat,
    plan: Vec<RunPlan>,
    is_open: &impl Fn(TranscriptItemId, bool) -> bool,
) -> Vec<RunPlan> {
    let mut out: Vec<RunPlan> = Vec::with_capacity(plan.len());
    // The turn being walked: where it began, and what it has done so far.
    let mut turn: Option<(TranscriptItemId, Vec<TranscriptItemId>)> = None;

    // **The last finished turn opens itself; every older one is a line.** The
    // block answers "what did that do to my tree", which is a question about
    // the turn that just ended -- and a conversation that kept every one of
    // them open would be a column of tables with the reading between them.
    // Sending the next prompt is what puts the one above away, which is the
    // moment the reader stopped asking.
    //
    // **Which is why the fold is asked with its default in hand.** A reader who
    // decided about this block keeps that decision; one who never touched it
    // follows the rule above. Recorded the other way round -- a set of
    // exceptions to whatever the default happens to be -- the recorded fact
    // changes meaning when the default moves under it, and a block closed while
    // it was newest sprang open the moment the next prompt went out.
    let closer = |anchor: TranscriptItemId, body: &[TranscriptItemId], newest: bool| {
        let items: Vec<&ChatItem> = body.iter().filter_map(|&t| item(chat, t)).collect();
        onehand_core::chat::turn_changes(&items).map(|changes| RunPlan {
            members: Vec::new(),
            strip: None,
            changes: Some(ChangePlan {
                anchor,
                changes,
                body: body.to_vec(),
                opens_itself: newest,
            }),
            open: is_open(anchor, newest),
            kind: RunKind::Compact,
        })
    };

    // Which prompt begins the last turn, since that is the one that opens.
    let newest = plan
        .iter()
        .rev()
        .find(|run| run.kind == RunKind::Prompt)
        .and_then(|run| run.members.first().copied());

    for run in plan {
        if run.kind == RunKind::Prompt {
            if let Some((anchor, body)) = turn.take()
                && let Some(row) = closer(anchor, &body, Some(anchor) == newest)
            {
                out.push(row);
            }
            turn = run.members.first().map(|&head| (head, run.members.clone()));
        } else if let Some((_, body)) = turn.as_mut() {
            body.extend(run.members.iter().copied());
        }
        out.push(run);
    }
    // The last turn closes only once it has stopped: a running one is still
    // writing, and a total that grows under the eye is not a summary.
    if !chat.busy
        && let Some((anchor, body)) = turn
        && let Some(row) = closer(anchor, &body, Some(anchor) == newest)
    {
        out.push(row);
    }
    out
}

/// The transcript as one addressed sequence: read-only history first, then the
/// live tail — minus whatever is currently pinned above the composer.
///
/// A card the agent is parked on is drawn once, where it can be answered
/// without scrolling to it. Filtering *after* the positions are handed out is
/// what keeps the ids stable: the answered card reappears at the place it
/// always had, rather than at the place it would have if it had never been
/// away.
pub(super) fn addressed(chat: &Chat) -> impl Iterator<Item = (TranscriptItemId, &ChatItem)> {
    chat.history
        .iter()
        .enumerate()
        .map(|(i, item)| (TranscriptItemId::History(i), item))
        .chain(
            chat.items
                .iter()
                .enumerate()
                .map(|(i, item)| (TranscriptItemId::Live(i), item)),
        )
        .filter(|(_, item)| !is_pinned(item))
}

/// Whether two runs at the same position draw the same row.
///
/// Everything that decides what the row *is*, which is what the list's cached
/// height is a measurement of. The group is left out because it is a function
/// of the members, and the summary is a function of their contents -- so a step
/// that settles inside a strip changes the summary and is caught there.
pub(super) fn draws_the_same(was: &RunPlan, now: &RunPlan) -> bool {
    was.members == now.members
        && was.open == now.open
        && was.kind == now.kind
        && was.strip.as_ref().map(|s| &s.summary) == now.strip.as_ref().map(|s| &s.summary)
        // **Compared in full, unlike a cluster's.** The two rows that carry no
        // members at all -- the line saying a turn is running and the line
        // saying what it wrote -- are otherwise indistinguishable from each
        // other and from every other empty run, so without this the list keeps
        // a one-line height for a row that has become a list of files.
        && was.changes.as_ref().map(|c| (c.anchor, &c.changes))
            == now.changes.as_ref().map(|c| (c.anchor, &c.changes))
}

/// Split a cluster into the stretches of one kind of work the frame lists.
pub(super) fn sections(chat: &Chat, members: &[TranscriptItemId]) -> Vec<Section> {
    let mut out: Vec<Vec<TranscriptItemId>> = Vec::new();
    let mut last: Option<ActivityGroup> = None;
    for &target in members {
        let group = item(chat, target).and_then(transcript::section_group);
        // `None` never extends: a step that stands on its own does so however
        // many of its kind are beside it, so two of them are two sections.
        match (group.is_some() && group == last, out.last_mut()) {
            (true, Some(run)) => run.push(target),
            _ => out.push(vec![target]),
        }
        last = group;
    }
    out.into_iter()
        .map(|members| {
            let bodies: Vec<&ChatItem> = members
                .iter()
                .copied()
                .filter_map(|t| item(chat, t))
                .collect();
            Section {
                group: bodies
                    .first()
                    .and_then(|item| transcript::section_group(item))
                    .unwrap_or(ActivityGroup::Other),
                summary: onehand_core::chat::section_summary(&bodies),
                outcome: run_outcome(&bodies),
                members,
            }
        })
        .collect()
}

/// Whether an item is a blocking card still waiting on the user, and so is
/// drawn above the composer rather than in the transcript.
pub fn is_pinned(item: &ChatItem) -> bool {
    match item {
        ChatItem::Permission(p) => p.resolved.is_none(),
        ChatItem::Ask(a) => a.resolved.is_none(),
        _ => false,
    }
}

/// Whether a run is the one-line record an answered permission or question
/// leaves, which the transcript gathers with its neighbours into one box.
pub fn is_record(chat: &Chat, run: &RunPlan) -> bool {
    run.strip.is_none()
        && run.changes.is_none()
        && matches!(run.members.as_slice(), [target] if matches!(
            item(chat, *target),
            Some(ChatItem::Permission(p)) if p.resolved.is_some()
        ) || matches!(
            item(chat, *target),
            Some(ChatItem::Ask(a)) if a.resolved.is_some()
        ))
}

/// Where the list must redraw from, given the first run that changed: a
/// record draws its share of a box its neighbours decide the ends of, so the
/// record just above a divergence changes too, a record arriving under it
/// moving that box's foot.
pub(super) fn with_record_above(chat: &Chat, plan: &[RunPlan], diverged: usize) -> usize {
    match diverged.checked_sub(1) {
        Some(above) if plan.get(above).is_some_and(|run| is_record(chat, run)) => above,
        _ => diverged,
    }
}

/// The cadence one un-folded item asks for.
///
/// A settled step nobody has opened is the same index entry a folded strip is —
/// it just happened to have no neighbour to fold with, and so is a closed
/// thought or the one line a settled question or grant leaves. Everything else
/// is read rather than scanned, including a failed tool: its status is the
/// reason to stop at it.
pub(super) fn single_kind(item: &ChatItem) -> RunKind {
    match item {
        ChatItem::User(_) => RunKind::Prompt,
        ChatItem::Tool(tool) => {
            let settled = matches!(
                tool.call.status,
                onehand_core::acp::ToolStatus::Completed | onehand_core::acp::ToolStatus::Failed
            );
            if settled && !tool.is_open() {
                RunKind::Compact
            } else {
                RunKind::Block
            }
        }
        ChatItem::Thought(thought) if !thought.is_open() => RunKind::Compact,
        ChatItem::Permission(p) if p.resolved.is_some() && !p.expanded => RunKind::Compact,
        ChatItem::Ask(a) if a.resolved.is_some() && !a.expanded => RunKind::Compact,
        _ => RunKind::Block,
    }
}

/// The runs holding a subagent card: every one when `all`, else only those
/// with the card still working.
pub(super) fn card_runs(chat: &Chat, plan: &[RunPlan], all: bool) -> Vec<usize> {
    plan.iter()
        .enumerate()
        .filter(|(_, run)| {
            run.members.iter().any(|t| {
                matches!(item(chat, *t), Some(ChatItem::Subagent(card))
                    if all || card.state == onehand_core::chat::SubagentState::Working)
            })
        })
        .map(|(ix, _)| ix)
        .collect()
}

/// The item an id addresses.
///
/// A direct index into whichever of the two collections the id names. It used
/// to be a linear search through the whole addressed sequence, run once per
/// member of every activity run and once per frame -- quadratic in the length
/// of the conversation, for a lookup the id already answers.
pub fn item(chat: &Chat, target: TranscriptItemId) -> Option<&ChatItem> {
    match target {
        TranscriptItemId::History(i) => chat.history.get(i),
        TranscriptItemId::Live(i) => chat.items.get(i),
    }
}
