use super::Viewport;
use gpui::{ListOffset, px};
use onehand_core::acp::{PermissionRequest, ToolCall, ToolKind, ToolStatus};
use onehand_core::chat::{
    ActivityGroup, Chat, ChatItem, Md, PermItem, ToolItem, TranscriptItemId, UserMsg,
};
use std::path::PathBuf;

fn read(title: &str) -> ChatItem {
    read_with_status(title, ToolStatus::Completed)
}

fn read_with_status(title: &str, status: ToolStatus) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: title.into(),
        title: title.into(),
        description: None,
        kind: ToolKind::Read,
        status,
        content: Vec::new(),
    }))
}

/// An answer, then two reads that fold into one activity cluster.
fn chat() -> Chat {
    let mut chat = Chat::new(1, PathBuf::from("/tmp/project"), "claude".to_string(), None);
    chat.items = vec![
        ChatItem::Agent(Md::parse("here is what I found")),
        read("Read src/a.rs"),
        read("Read src/b.rs"),
    ];
    chat
}

fn wrote(path: &str, old: &str, new: &str) -> ChatItem {
    ChatItem::Tool(ToolItem::new(ToolCall {
        id: format!("write {path}"),
        title: format!("Write {path}"),
        description: None,
        kind: ToolKind::Edit,
        status: ToolStatus::Completed,
        content: vec![onehand_core::acp::ToolContent::Diff {
            path: path.to_string(),
            old: Some(old.to_string()),
            new: new.to_string(),
        }],
    }))
}

/// A block closed by hand stays closed once its turn stops being newest.
///
/// The default moves -- a summary opens itself while its turn is the last
/// and is a line afterwards -- so a fold recorded as "the exception to the
/// default" records a fact that changes meaning underneath it. Written that
/// way, a block the reader closed sprang open the moment the next prompt
/// went out.
#[test]
fn a_turn_summary_keeps_the_answer_it_was_given() {
    let mut chat = chat();
    chat.items.push(ChatItem::User(UserMsg::text("fix it")));
    chat.items.push(wrote("src/lib.rs", "a\nb\n", "1\n2\n3\n"));

    // The reader closes it while it is the newest turn.
    let closed = std::cell::RefCell::new(std::collections::HashMap::new());
    let decide =
        |anchor: TranscriptItemId, default: bool| *closed.borrow().get(&anchor).unwrap_or(&default);

    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, decide);
    assert_eq!(viewport.run(4).map(|r| r.open), Some(true), "newest opens");

    closed.borrow_mut().insert(TranscriptItemId::Live(3), false);
    viewport.replan(&chat, 1, |_| false, decide);
    assert_eq!(viewport.run(4).map(|r| r.open), Some(false));

    // A second turn arrives. The first is no longer the newest, and the
    // answer it was given still holds.
    chat.items.push(ChatItem::User(UserMsg::text("now this")));
    chat.items.push(wrote("src/main.rs", "x\n", "x\ny\n"));
    viewport.replan(&chat, 2, |_| false, decide);
    assert_eq!(
        viewport.run(4).map(|r| r.open),
        Some(false),
        "closed by hand, and it stays closed"
    );
}

/// A finished turn that wrote something closes on a row of its own, and a
/// turn still writing does not.
///
/// A total that grows under the eye is not a summary, and the line saying
/// the turn is running is already there saying so.
#[test]
fn a_finished_turn_closes_on_what_it_wrote() {
    let mut chat = chat();
    chat.items.push(ChatItem::User(UserMsg::text("fix it")));
    chat.items.push(wrote("src/lib.rs", "a\nb\n", "1\n2\n3\n"));
    chat.busy = true;

    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    // 0: answer · 1: the old cluster · 2: the prompt · 3: the write ·
    // 4: the line saying the turn is running. Nothing closes it yet.
    assert_eq!(viewport.run(4).map(|r| r.changes.is_some()), Some(false));
    assert!(viewport.run(5).is_none());

    chat.busy = false;
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    let closing = viewport.run(4).and_then(|run| run.changes.as_ref());
    let closing = closing.expect("the turn closes on what it wrote");
    // Hung off the prompt that began the turn, which is what folds it.
    assert_eq!(closing.anchor, TranscriptItemId::Live(3));
    assert_eq!(closing.changes.files.len(), 1);
    assert_eq!((closing.changes.added, closing.changes.removed), (3, 2));
    // The block the last finished turn gets opens itself; every older one
    // is a line until somebody asks.
    assert_eq!(viewport.run(4).map(|r| r.open), Some(true));
    assert!(viewport.run(5).is_none());
}

#[test]
fn quiet_steps_collapse_into_one_row() {
    let mut viewport = Viewport::default();
    viewport.replan(&chat(), 0, |_| false, |_, default| default);

    assert_eq!(viewport.run(0).map(|r| r.members.len()), Some(1));
    assert_eq!(
        viewport.run(1).map(|r| r.members.len()),
        Some(2),
        "two reads are one run"
    );
    let strip = viewport.run(1).and_then(|run| run.strip.as_ref()).unwrap();
    assert_eq!(strip.sections.len(), 1, "two reads are one kind of work");
    assert_eq!(strip.sections[0].group, ActivityGroup::Explored);
    // The one sentence the muted line says: kinds of work in the order
    // they happened, each with a count.
    assert_eq!(strip.summary.plain(), "Read 2 files");
    assert!(viewport.run(2).is_none(), "three items, two rows");
}

/// A scroll target is named as an *item* and the list scrolls to *rows*. An
/// item folded inside a strip still has a row to be taken to -- the strip's.
#[test]
fn an_item_inside_a_strip_still_names_a_row() {
    let mut viewport = Viewport::default();
    viewport.replan(&chat(), 0, |_| false, |_, default| default);

    assert_eq!(viewport.run_of(TranscriptItemId::Live(0)), Some(0));
    assert_eq!(viewport.run_of(TranscriptItemId::Live(1)), Some(1));
    assert_eq!(viewport.run_of(TranscriptItemId::Live(2)), Some(1));
    assert_eq!(
        viewport.run_of(TranscriptItemId::Live(9)),
        None,
        "an item that is not there names no row"
    );
}

fn permission(resolved: Option<&str>) -> ChatItem {
    ChatItem::Permission(PermItem {
        req: PermissionRequest {
            rpc_id: Default::default(),
            tool_call_id: None,
            kind: ToolKind::Execute,
            title: "Run `rm -rf build`?".to_string(),
            options: Vec::new(),
        },
        resolved: resolved.map(str::to_string),
        expanded: false,
    })
}

/// A card the agent is parked on is drawn above the composer, so the
/// transcript leaves it out -- otherwise the one thing the user has to
/// answer is on screen twice, and answering one copy leaves the other
/// looking live.
#[test]
fn an_unanswered_blocking_card_is_not_in_the_transcript() {
    let mut chat = chat();
    chat.items.push(permission(None));
    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);

    assert_eq!(
        viewport.run_of(TranscriptItemId::Live(3)),
        None,
        "the pending card has no row of its own"
    );
}

/// Answered, it takes the place it always had -- the record of what was
/// decided, in the order it was decided.
#[test]
fn an_answered_one_returns_to_where_it_was() {
    let mut chat = chat();
    chat.items.push(permission(Some("Allow once")));
    chat.items.push(ChatItem::Agent(Md::parse("done")));
    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);

    let card = viewport.run_of(TranscriptItemId::Live(3));
    let after = viewport.run_of(TranscriptItemId::Live(4));
    assert!(card.is_some(), "a resolved card is part of the transcript");
    assert!(card < after, "and it sits before what came after it");
}

/// A frame that changed nothing about the conversation keeps the layout it
/// has. `render` runs for a hover, a keystroke in the composer, a panel
/// drag -- none of which is a reason to walk the transcript again.
#[test]
fn an_unchanged_transcript_is_not_laid_out_twice() {
    let mut chat = chat();
    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert_eq!(viewport.run(1).map(|r| r.open), Some(false));

    // Someone opened the strip, but the viewport was not told: same
    // revision, same folds count, so the answer it already has stands.
    viewport.replan(&chat, 0, |_| true, |_, default| default);
    assert_eq!(viewport.run(1).map(|r| r.open), Some(false));

    // Toggling a fold is a change, and it is counted.
    viewport.replan(&chat, 1, |_| true, |_, default| default);
    assert_eq!(viewport.run(1).map(|r| r.open), Some(true));

    // So is the transcript growing.
    chat.items.push(read("Read src/c.rs"));
    viewport.replan(&chat, 1, |_| true, |_, default| default);
    assert_eq!(viewport.run(1).map(|r| r.members.len()), Some(3));
}

/// **A step in flight is in the cluster it happened in**, and stays there
/// when it finishes.
///
/// It used to be outside: a running tool was its own row until it settled,
/// at which point it folded into the strip above it. So the row count
/// changed every few seconds mid-turn, the reader's position moved with it,
/// and a step that had been a tall card became a line in a list somebody
/// was already reading. A cluster is bounded by the agent's words, not by
/// what each step inside it happens to be doing.
#[test]
fn a_step_in_flight_is_in_the_cluster_it_happened_in() {
    let mut chat = chat();
    chat.items.push(ChatItem::User(UserMsg::text("and now?")));
    chat.items.push(read("Read src/c.rs"));
    chat.items.push(read("Read src/d.rs"));
    chat.items
        .push(ChatItem::Agent(Md::parse("first checkpoint")));
    chat.items
        .push(read_with_status("Read src/e.rs", ToolStatus::InProgress));
    chat.items
        .push(read_with_status("Read src/f.rs", ToolStatus::InProgress));
    chat.busy = true;

    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);

    // 0: old answer · 1: old reads · 2: prompt · 3: the completed reads of
    // the live turn · 4: checkpoint · 5: the two reads changing right now,
    // which are one cluster like any other.
    assert_eq!(viewport.run(1).map(|r| r.open), Some(false));
    assert_eq!(viewport.run(3).map(|r| r.open), Some(false));
    assert_eq!(
        viewport.run(5).map(|r| r.members.as_slice()),
        Some([TranscriptItemId::Live(7), TranscriptItemId::Live(8)].as_slice())
    );
    // **Collapsed, even while it is running.** A cluster that opened itself
    // would push the answer above it up the panel every time a turn started
    // work, and shut again when it stopped.
    assert_eq!(viewport.run(5).map(|r| r.open), Some(false));
    // 6 is the line saying the turn is still going, which every live
    // plan ends on.
    assert_eq!(viewport.run(6).map(|r| r.members.is_empty()), Some(true));
    assert!(viewport.run(7).is_none(), "and nothing after it");

    // They settle. **Nothing about the layout moves**: the same run, the
    // same members, the same fold. What changes is the sentence that run's
    // line says about itself, which is a row redrawn rather than a row
    // appearing or going.
    let before = viewport.run(5).map(|r| r.members.len());
    for item in &mut chat.items[7..=8] {
        let ChatItem::Tool(tool) = item else {
            unreachable!();
        };
        tool.call.status = ToolStatus::Completed;
    }
    chat.busy = false;
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert_eq!(viewport.run(5).map(|r| r.members.len()), before);
    assert_eq!(viewport.run(5).map(|r| r.open), Some(false));
    assert!(viewport.run(6).is_none());
}

/// A question just asked is taken to the top of the panel, so the answer
/// arrives in the space under it instead of pushing it off the screen.
/// **The clip is taken off once, and off the right one of the two.**
///
/// The transcript is clipped at its foot so a row never shows sliced either
/// side of the composer. The held answer is measured from the list's own
/// viewport, which is already that clipped box; the bare answer is a
/// constant that has never heard of it. Taking the clip off both left the
/// list short of the padding a scroll to the tail needs — so it never came
/// to rest, the prompt stayed held, and the jump-to-latest pill appeared on
/// every turn while the transcript stopped following.
#[test]
fn the_clip_comes_off_the_floor_and_not_off_a_measurement() {
    use super::room_for;

    // The bare answer: a constant, so the clip is its caller's to remove.
    assert_eq!(room_for(px(200.), px(50.), None), px(150.));
    assert_eq!(room_for(px(200.), px(0.), None), px(200.));
    assert_eq!(
        room_for(px(30.), px(50.), None),
        px(0.),
        "a clip taller than the floor leaves no room rather than a negative one"
    );

    // The measured answer already has the clip inside it, so growing the
    // clip must not shrink it — that double subtraction is the whole bug.
    let held = px(420.);
    for cut in [px(0.), px(50.), px(120.)] {
        assert_eq!(
            room_for(px(200.), cut, Some(held)),
            held,
            "the clip was taken off a measurement that already had it out"
        );
    }

    // It is still a floor: a turn asking for less than what is left after
    // the clip gets what is left.
    assert_eq!(room_for(px(200.), px(50.), Some(px(10.))), px(150.));
}

#[test]
fn a_new_prompt_asks_to_be_held_at_the_top() {
    let mut chat = chat();
    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert!(!viewport.holding(), "nothing was asked");

    chat.items.push(ChatItem::User(UserMsg::text("and now?")));
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert!(viewport.holding());

    // The answer to it is not another question, so the hold stands: it is
    // ended by the answer growing tall enough to need the room, which is a
    // measurement rather than a layout.
    chat.items.push(ChatItem::Agent(Md::parse("this")));
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert!(viewport.holding());
}

/// A transcript adopted from an archive arrives whole, and its last
/// question is not a question just asked -- reopening a conversation
/// belongs at its end, where it was left.
#[test]
fn a_transcript_that_arrives_whole_is_not_held() {
    let mut chat = chat();
    chat.items.push(ChatItem::User(UserMsg::text("and now?")));
    chat.items.push(ChatItem::Agent(Md::parse("this")));

    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert!(!viewport.holding());
}

/// A panel's worth of nothing, so the two positions the hold rests between
/// are the only thing a test is measuring.
const ROOM: super::TopRoom = super::TopRoom {
    head: px(0.),
    floor: px(64.),
};

/// The run a turn is streaming into has to be measured again on every
/// frame, or its height freezes at the first chunk's -- but asking for that
/// must not cost the reader their place inside it.
///
/// Told the row had been *replaced*, the list gave up the offset it held
/// into it, so every arriving chunk put a reader partway down a long answer
/// back at that answer's first line: the whole of a streaming answer was
/// unreadable until it finished.
#[test]
fn the_streaming_run_is_measured_again_without_moving_the_reader() {
    let mut viewport = Viewport::default();
    viewport.replan(&chat(), 0, |_| false, |_, default| default);
    let state = viewport.list_state(true, ROOM);
    let count = state.item_count();
    assert!(count > 0);

    // Partway down the run the answer is arriving in.
    state.scroll_to(ListOffset {
        item_ix: count - 1,
        offset_in_item: px(600.),
    });

    // The next chunk lands: same rows, one of them a different height.
    let state = viewport.list_state(true, ROOM);
    assert_eq!(
        state.item_count(),
        count,
        "measuring a row again is not a change of length"
    );
    assert_eq!(
        state.logical_scroll_top().offset_in_item,
        px(600.),
        "the chunk took the reader back to the top of the answer"
    );
}

/// A step settling at the tail must not move the reader above it.
///
/// Told to reset, the list gave up every measured height and the scroll
/// position on *every* tool that finished -- which mid-turn is every few
/// seconds. It read as the panel flickering: the transcript jumped, the
/// frame hitched re-measuring what was above, and the way-back pill blinked
/// as the anchor landed past the tail for a frame.
///
/// Clustering took the *other* half of that away: a step finishing no
/// longer folds one row into another, because it was already in the cluster
/// the moment it started. The row count holding still is now part of what
/// this asserts rather than something it works around.
#[test]
fn a_step_settling_at_the_tail_leaves_the_reader_where_they_were() {
    let mut chat = chat();
    chat.items
        .push(read_with_status("Read src/c.rs", ToolStatus::InProgress));
    chat.busy = true;

    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    let state = viewport.list_state(true, ROOM);
    let count = state.item_count();

    // Reading the answer at the top, well above the work going on below.
    state.scroll_to(ListOffset {
        item_ix: 0,
        offset_in_item: px(120.),
    });

    // The tool settles. It was in the cluster already, so the only thing
    // that changes is what that cluster's one line says about itself.
    chat.apply(onehand_core::acp::AcpEvent::ToolUpdate(
        onehand_core::acp::ToolCallUpdate {
            id: "Read src/c.rs".to_string(),
            status: Some(ToolStatus::Completed),
            title: None,
            description: None,
            content: None,
        },
    ));
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    let state = viewport.list_state(true, ROOM);

    assert_eq!(
        state.item_count(),
        count,
        "a step finishing is not a row appearing or going"
    );
    let top = state.logical_scroll_top();
    assert_eq!(
        (top.item_ix, top.offset_in_item),
        (0, px(120.)),
        "a fold at the tail moved the reader at the top"
    );
}

/// A step that settles on its own changes its row's shape without changing
/// how many rows there are, and the list has to be told either way.
///
/// It was told neither: the note of where the plan diverged was only read
/// when the count moved, and was written over by the next replan. So a run
/// that turned from a tall card into a one-line index row kept the card's
/// measured height, and the splice that came later began past it -- the
/// content height stayed inflated, and scrolling up landed on the wrong row
/// and jumped when that row was finally measured again.
#[test]
fn a_row_that_changed_shape_is_told_to_the_list_too() {
    let mut chat = chat();
    chat.items.push(ChatItem::Agent(Md::parse("then this")));
    chat.items
        .push(read_with_status("Read src/c.rs", ToolStatus::InProgress));
    chat.busy = true;

    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    let _ = viewport.list_state(true, ROOM);
    // 0: answer · 1: the cluster · 2: answer · 3: the running read, which
    // is a cluster of one · 4: the line saying the turn is still going.
    assert_eq!(viewport.changed_from, 5, "nothing left to tell the list");

    // The read settles. It is alone between two paragraphs either way, so
    // it stays one run and only what its line says changes.
    chat.apply(onehand_core::acp::AcpEvent::ToolUpdate(
        onehand_core::acp::ToolCallUpdate {
            id: "Read src/c.rs".to_string(),
            status: Some(ToolStatus::Completed),
            title: None,
            description: None,
            content: None,
        },
    ));
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert_eq!(viewport.changed_from, 3, "the row that changed shape");

    // A later replan must not paint over a divergence still owed to the
    // list -- the note is the earliest one, not the newest.
    chat.items
        .push(read_with_status("Read src/d.rs", ToolStatus::InProgress));
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    assert_eq!(
        viewport.changed_from, 3,
        "an append is not the whole answer"
    );

    let _ = viewport.list_state(true, ROOM);
    assert_eq!(viewport.changed_from, 5, "told, and the note cleared");
}

/// The way back to the latest lands on the held question, and not on the
/// end of the transcript.
///
/// The activity is arriving in the room under that question, so the two are
/// ordinarily one place -- but only once the room has been measured against
/// a turn. Until then it is a whole panel of padding, and the end of the
/// transcript is the end of *that*: a position with no row on screen and
/// none measured either, so nothing is left to shrink the room back down
/// and take the reader off an empty panel.
#[test]
fn jumping_to_the_latest_lands_on_the_held_question() {
    let mut chat = chat();
    let mut viewport = Viewport::default();
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    let _ = viewport.list_state(false, ROOM);

    chat.items.push(ChatItem::User(UserMsg::text("and now?")));
    viewport.replan(&chat, 0, |_| false, |_, default| default);
    let state = viewport.list_state(true, ROOM);
    assert!(viewport.holding(), "a question just asked is held");
    let question = viewport.run_of(TranscriptItemId::Live(3)).unwrap();

    // The reader scrolls off the question, which is what puts the way back
    // on screen in the first place.
    state.scroll_to(ListOffset {
        item_ix: 0,
        offset_in_item: px(0.),
    });

    viewport.jump_to_latest();
    let back = state.logical_scroll_top();
    assert_eq!(
        (back.item_ix, back.offset_in_item),
        (question, px(0.)),
        "the way back has to name a row, not the end of the padding"
    );
    assert!(
        viewport.holding(),
        "resting on the question again takes the control off the screen"
    );
}

/// With no question held, the latest activity really is the tail, and the
/// list has to be handed back to following it -- or the next chunk arrives
/// under a reader who asked to be taken to it.
#[test]
fn jumping_with_nothing_held_follows_the_tail_again() {
    let mut viewport = Viewport::default();
    viewport.replan(&chat(), 0, |_| false, |_, default| default);
    let state = viewport.list_state(false, ROOM);
    state.scroll_to(ListOffset {
        item_ix: 0,
        offset_in_item: px(0.),
    });
    assert!(!state.is_following_tail());

    viewport.jump_to_latest();
    assert!(state.is_following_tail());
}

/// The assumption the scroll target rests on: opening a strip changes what
/// a row *draws*, never how many rows there are. If it moved them, every
/// row index taken before an unfold would point somewhere else after it.
#[test]
fn unfolding_moves_no_row() {
    let (chat, mut folded, mut open) = (chat(), Viewport::default(), Viewport::default());
    folded.replan(&chat, 0, |_| false, |_, default| default);
    open.replan(&chat, 0, |_| true, |_, default| default);

    for item in 0..3 {
        let target = TranscriptItemId::Live(item);
        assert_eq!(folded.run_of(target), open.run_of(target));
    }
    assert_eq!(folded.run(1).map(|r| r.open), Some(false));
    assert_eq!(open.run(1).map(|r| r.open), Some(true));
}
