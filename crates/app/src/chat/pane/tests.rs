use super::body::away_from_tail;
use super::runs::lead_gap;
use super::runs::working_word;
use super::{
    BLOCK_GAP, COMPACT_GAP, SessionSignal, restart_needs_arming, switching_away, waits_alone,
};
use crate::chat::viewport::{self, RunKind};
use gpui::rems;
use onehand_core::chat::{Link, TranscriptItemId};

/// The running line always has a word, including where the model is quiet.
///
/// `Chat::activity_status` says nothing while a thought or a tool is live,
/// because the transcript's own block a few lines up is already saying it.
/// The running line sits *below* those blocks and says nothing else, so
/// without a word of its own the two states a reader most wants named came
/// out as a mark and a clock.
#[test]
fn a_running_line_is_never_wordless() {
    use onehand_core::acp::{ToolCall, ToolKind, ToolStatus};
    use onehand_core::chat::{Chat, ChatItem, Md, Thought, ToolItem};

    let mut chat = Chat::new(
        1,
        std::path::PathBuf::from("/tmp/project"),
        "claude".to_string(),
        None,
    );
    chat.busy = true;
    // Past the handshake: while it is still connecting the model has a
    // sentence of its own and this never runs.
    chat.link = Link::Connected;

    chat.items.push(ChatItem::Thought(Thought {
        md: Md::parse("weighing it up"),
        started: None,
        elapsed_secs: None,
        expanded: false,
    }));
    assert_eq!(chat.activity_status(), None, "the model stays quiet");
    assert_eq!(working_word(&chat).as_deref(), Some("Thinking…"));

    chat.items.push(ChatItem::Tool(ToolItem::new(ToolCall {
        id: "w".into(),
        title: "Write src/lib.rs".into(),
        description: None,
        kind: ToolKind::Edit,
        status: ToolStatus::InProgress,
        content: Vec::new(),
    })));
    assert_eq!(chat.activity_status(), None);
    let word = working_word(&chat).expect("a step in flight names itself");
    assert!(word.contains("src/lib.rs"), "{word}");

    // A settled transcript has nothing to say, and says nothing.
    chat.busy = false;
    chat.items.clear();
    assert_eq!(working_word(&chat), None);
}

/// The two sides of a prompt are one space, so they are one number —
/// whatever sits above the prompt and whatever follows it.
///
/// This is the property the old layout could not hold. Spacing hung off the
/// *upper* run, so the gap above a prompt was that run's own bottom padding
/// plus the prompt's, and it therefore changed with what happened to
/// precede it while the gap below never did.
#[test]
fn a_prompt_opens_a_turn_and_does_not_close_one() {
    // Above: the widest boundary in the conversation, whatever precedes it
    // -- that space is what a reader scrolling back finds the last question
    // by.
    for above in [RunKind::Block, RunKind::Compact, RunKind::Prompt] {
        assert_eq!(
            lead_gap(Some(above), RunKind::Prompt),
            rems(BLOCK_GAP.0 * 2.),
            "{above:?} above a prompt"
        );
    }
    // Below: an ordinary block gap, because what follows is the *reply*.
    // Symmetrical, the two said the prompt belonged to neither side.
    for below in [RunKind::Block, RunKind::Compact] {
        assert_eq!(
            lead_gap(Some(RunKind::Prompt), below),
            BLOCK_GAP,
            "{below:?} below a prompt"
        );
    }
    // **A prompt under a prompt is still a turn opening**, and opening wins
    // over closing: somebody who asked twice without waiting asked two
    // questions, and the space has to say so from the side that knows.
    assert_eq!(
        lead_gap(Some(RunKind::Prompt), RunKind::Prompt),
        rems(BLOCK_GAP.0 * 2.),
    );
}

/// Index rows close ranks with each other and with nothing else. The old
/// layout gave the *answer* after a folded strip the strip's own compact
/// cadence, gluing prose to a row it has nothing to do with.
#[test]
fn only_two_index_rows_close_ranks() {
    assert_eq!(
        lead_gap(Some(RunKind::Compact), RunKind::Compact),
        COMPACT_GAP
    );
    assert_eq!(
        lead_gap(Some(RunKind::Compact), RunKind::Block),
        BLOCK_GAP,
        "an answer under a folded strip is a block boundary"
    );
    assert_eq!(lead_gap(Some(RunKind::Block), RunKind::Compact), BLOCK_GAP);
    assert_eq!(lead_gap(Some(RunKind::Block), RunKind::Block), BLOCK_GAP);
}

/// Opening a group must not move the row that was clicked.
///
/// The gap above a run is a property of the boundary, and opening a group
/// changes nothing about the boundary above its own header — only about
/// what hangs below it. Decided from the run as a whole, that gap tripled
/// the moment the group opened: the header slid down under the pointer
/// that had just clicked it, and every row above appeared to shift.
#[test]
fn opening_a_group_leaves_the_space_above_it_alone() {
    let strip = |open: bool| viewport::RunPlan {
        members: vec![TranscriptItemId::Live(0)],
        strip: Some(viewport::ActivityPlan {
            summary: onehand_core::chat::ClusterSummary::default(),
            sections: Vec::new(),
        }),
        changes: None,
        open,
        // What the layout classifies an opened group as: a block's worth
        // of reading, which is what the run *after* it has to answer to.
        kind: if open {
            RunKind::Block
        } else {
            RunKind::Compact
        },
    };

    assert_eq!(
        lead_gap(Some(RunKind::Compact), strip(true).head_kind()),
        lead_gap(Some(RunKind::Compact), strip(false).head_kind()),
        "the space over a group's own header changed when it opened"
    );
    assert_eq!(
        lead_gap(Some(RunKind::Compact), strip(true).head_kind()),
        COMPACT_GAP
    );
    // What *does* change is the space under it: an opened group is a block,
    // and the index row after it no longer closes ranks with a header it
    // can no longer see the bottom of.
    assert_eq!(strip(true).tail_kind(), RunKind::Block);
    assert_eq!(
        lead_gap(Some(strip(true).tail_kind()), RunKind::Compact),
        BLOCK_GAP
    );
}

/// The way back to the latest shows when the reader is parked above the
/// tail, and only then.
///
/// Both halves have failed. Asked of the list's measured height it was
/// backwards on any long transcript — the rows above the viewport have
/// never been measured, the answer came back "don't know", and the control
/// stayed hidden on exactly the conversations that need it. And a
/// transcript that fits on screen has no bottom to be away from, so a wheel
/// event that moves nothing must not summon a way back to where the reader
/// already is.
#[test]
fn the_way_back_appears_only_when_there_is_somewhere_to_go_back_to() {
    use gpui::{ListAlignment, ListOffset, ListState, px};

    let list = ListState::new(8, ListAlignment::Bottom, px(512.));
    list.set_follow_mode(gpui::FollowMode::Tail);
    assert!(
        !away_from_tail(&list),
        "a list at its tail is not away from it"
    );

    // Parked above the end: `scroll_to` stops the list following.
    list.scroll_to(ListOffset {
        item_ix: 3,
        offset_in_item: px(0.),
    });
    assert!(away_from_tail(&list));

    // Back at the end.
    list.scroll_to_end();
    list.set_follow_mode(gpui::FollowMode::Tail);
    assert!(!away_from_tail(&list));

    // Nothing to scroll: following stopped by hand, but the offset never
    // left the tail.
    let short = ListState::new(2, ListAlignment::Bottom, px(512.));
    short.set_follow_mode(gpui::FollowMode::Tail);
    short.pause_following_tail();
    assert!(
        !away_from_tail(&short),
        "a transcript that fits on screen has no bottom to be away from"
    );
}

/// A conversation is hidden while it comes up for the first time, and never
/// again after that.
///
/// The second half is the one worth a test: a restart drops the adapter and
/// spawns another, so the link goes back to connecting on a transcript the
/// user is in the middle of reading. Blanking it there looks exactly like
/// the restart having thrown the conversation away.
#[test]
fn only_a_conversation_that_was_never_live_is_hidden_while_it_connects() {
    assert!(waits_alone(Link::Connecting, false));
    assert!(
        !waits_alone(Link::Connecting, true),
        "a restart must not blank a transcript being read"
    );
    for link in [Link::Connected, Link::Lost] {
        assert!(!waits_alone(link, false));
        assert!(!waits_alone(link, true));
    }
}

/// The first run rests on the list's own top padding; a gap there would be
/// space between the header's rule and nothing.
#[test]
fn the_top_of_the_transcript_leads_with_nothing() {
    for kind in [RunKind::Block, RunKind::Compact, RunKind::Prompt] {
        assert_eq!(lead_gap(None, kind), gpui::rems(0.));
    }
}

/// Re-selecting the session already on screen is not a switch. It happens
/// on every rail click and on every window activation, so treating it as
/// one would stash the draft out from under somebody still typing it.
#[test]
fn reselecting_the_shown_session_changes_nothing() {
    assert!(!switching_away(Some(7), 7));
}

/// Both of the ways the pane stops showing a conversation count.
///
/// The second one is the case that was missed: after the pane has been
/// cleared, the composer still holds what was typed for the session that
/// was showing, and opening any session at all has to take it away first.
#[test]
fn every_other_move_is_a_switch() {
    assert!(switching_away(Some(7), 8), "one session to another");
    assert!(switching_away(None, 8), "from nothing showing to a session");
}

/// A turn in flight is what makes a restart worth confirming.
#[test]
fn an_idle_session_restarts_on_the_first_press() {
    assert!(!restart_needs_arming(false, None, 1));
    assert!(
        !restart_needs_arming(false, Some(1), 1),
        "a stale arming press on an idle session is not a reason to stop"
    );
}

#[test]
fn a_busy_session_arms_then_confirms() {
    assert!(restart_needs_arming(true, None, 1), "first press arms");
    assert!(
        !restart_needs_arming(true, Some(1), 1),
        "the second press on the same session goes through"
    );
}

/// The whole reason the arming is keyed by session. Arm a restart on one
/// busy conversation, switch to another that is also busy, and that
/// session's first press must still be its own warning -- not the
/// confirmation of a press aimed somewhere else.
#[test]
fn arming_one_session_never_confirms_another() {
    assert!(restart_needs_arming(true, Some(1), 2));
}

/// A healthy, idle, already-read session draws **nothing**. This is the
/// case that makes the other four legible: a rail where every row has a dot
/// is a rail where no dot means anything.
#[test]
fn a_calm_session_carries_no_signal() {
    assert_eq!(
        SessionSignal::pick(Link::Connected, false, false, false),
        None
    );
}

/// Connecting is not failing. Reading `tx.is_none()` would conflate them
/// and paint a danger dot for the second or two every session spends
/// coming up.
#[test]
fn coming_up_is_not_a_signal() {
    assert_eq!(
        SessionSignal::pick(Link::Connecting, false, false, false),
        None
    );
}

#[test]
fn each_state_shows_when_it_is_the_only_one() {
    use SessionSignal::*;
    assert_eq!(
        SessionSignal::pick(Link::Lost, false, false, false),
        Some(Lost)
    );
    assert_eq!(
        SessionSignal::pick(Link::Connected, true, false, false),
        Some(AwaitingUser)
    );
    assert_eq!(
        SessionSignal::pick(Link::Connected, false, true, false),
        Some(Busy)
    );
    assert_eq!(
        SessionSignal::pick(Link::Connected, false, false, true),
        Some(UnseenTurn)
    );
}

/// The whole point of the reduction. Each of these is a real pairing:
/// an adapter that died with a question still parked, a turn that finished
/// unseen on a session that then lost its adapter, a busy session the user
/// has not looked at since its last turn.
#[test]
fn the_more_urgent_state_wins() {
    use SessionSignal::*;
    assert_eq!(
        SessionSignal::pick(Link::Lost, true, false, true),
        Some(Lost),
        "a dead adapter outranks a question nobody can answer any more"
    );
    assert_eq!(
        SessionSignal::pick(Link::Connected, true, true, true),
        Some(AwaitingUser),
        "a parked question outranks busy: only one of them moves on its own"
    );
    assert_eq!(
        SessionSignal::pick(Link::Connected, false, true, true),
        Some(Busy),
        "what is happening now outranks what happened last turn"
    );
}

/// A project's mark is the most urgent of its sessions', on the same
/// ordering a single session uses — otherwise the same shape would mean two
/// different things one row apart.
#[test]
fn a_project_rolls_up_to_its_most_urgent_session() {
    use SessionSignal::*;
    assert_eq!(SessionSignal::most_urgent([]), None);
    assert_eq!(
        SessionSignal::most_urgent([UnseenTurn, Lost, Busy]),
        Some(Lost)
    );
    assert_eq!(
        SessionSignal::most_urgent([UnseenTurn, Busy, AwaitingUser]),
        Some(AwaitingUser)
    );
    assert_eq!(
        SessionSignal::most_urgent([UnseenTurn, Busy]),
        Some(Busy),
        "a project with a turn running says so over a stale badge"
    );
}

/// A session with several facts true at once and a project holding those
/// same facts one per session must land on the same mark. They are the same
/// question asked at two altitudes, and `rank` is the single answer.
#[test]
fn one_session_and_one_project_agree() {
    use SessionSignal::*;
    for (link, awaiting, busy, unseen) in [
        (Link::Lost, true, false, true),
        (Link::Connected, true, true, true),
        (Link::Connected, false, true, true),
        (Link::Connected, false, false, true),
    ] {
        let parts = [
            (link == Link::Lost).then_some(Lost),
            awaiting.then_some(AwaitingUser),
            busy.then_some(Busy),
            unseen.then_some(UnseenTurn),
        ];
        assert_eq!(
            SessionSignal::pick(link, awaiting, busy, unseen),
            SessionSignal::most_urgent(parts.into_iter().flatten()),
        );
    }
}
