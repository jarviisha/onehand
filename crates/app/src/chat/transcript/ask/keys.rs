//! What a question card's keys do: take a row, skip, and step to the next field.

use super::*;

/// Take one row of the question showing at `field`.
///
/// **One function behind the click and the key**, because they are the same
/// gesture: a row aimed at with the pointer and a row walked to with the arrows
/// both end in the same answer, and two copies of "what picking does" is how a
/// number key comes to leave the typed answer sitting in a box the click would
/// have emptied.
pub(super) fn ask_take(
    session: &Entity<ChatSession>,
    idx: usize,
    field: usize,
    row: onehand_core::chat::AskRow,
    window: &mut Window,
    cx: &mut App,
) {
    use onehand_core::chat::AskRow;

    match row {
        // The typed answer is a row of the list, so reaching it is reaching its
        // box: the keyboard hands the caret over rather than answering for the
        // user, which is the one row that cannot be settled by arriving at it.
        AskRow::Custom => {
            let state = session.read(cx).ask_input(idx, field).cloned();
            if let Some(state) = state {
                let handle = state.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                session.update(cx, |s, cx| {
                    if let Some(rows) = s.chat.ask_at(idx).map(|item| item.row_count(field)) {
                        let mut place = s.ask_place(idx);
                        place.point_at(rows.saturating_sub(1));
                        s.set_ask_place(idx, place);
                    }
                    cx.notify();
                });
            }
        }
        AskRow::Choice(option) => session.update(cx, |s, cx| {
            // Picking is the user choosing the agent's wording over their own,
            // so the box goes with it -- model and widget together, since
            // neither clears the other.
            s.clear_ask_input(idx, field, window, cx);
            // A one-question single-select has nothing left to decide, so the
            // drive commits it on the press.
            s.ask_pick(idx, field, option, cx);
            cx.notify();
        }),
    }
}

/// Pass on the question showing at `field`, and settle the form if it was the
/// last one.
///
/// What is sent then is whatever the *other* questions hold: a form skipped
/// through to the end with nothing filled in is a refusal, and one with answers
/// above the skipped question is those answers. Declining a form that carries
/// work already done would throw it away at the last press.
pub(super) fn ask_skip(
    session: &Entity<ChatSession>,
    idx: usize,
    field: usize,
    window: &mut Window,
    cx: &mut App,
) {
    session.update(cx, |s, cx| {
        s.clear_ask_input(idx, field, window, cx);
        s.ask_skip(idx, field, cx);
        cx.notify();
    });
}

/// Move the form on from `field`, or submit it where that was the last
/// question.
pub(super) fn ask_advance(session: &Entity<ChatSession>, idx: usize, field: usize, cx: &mut App) {
    session.update(cx, |s, cx| {
        s.ask_advance(idx, field, cx);
        cx.notify();
    });
}

/// Move the question card's cursor `delta` rows. The walk takes the caret
/// back off whichever row was last clicked, or the cursor and the focus point
/// at two different rows and Enter answers the one the eye is not on.
pub(super) fn walk(
    session: &Entity<ChatSession>,
    idx: usize,
    active: usize,
    delta: isize,
    card: &gpui::FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    window.focus(card, cx);
    session.update(cx, |s, cx| {
        let mut place = s.ask_place(idx);
        if let Some(item) = s.chat.ask_at(idx) {
            place.move_cursor(item, active, delta);
        }
        s.set_ask_place(idx, place);
        cx.notify();
    });
}
