use super::{place_of, selection_after};

/// Closing a tab ahead of the selected one must move the selection back by
/// one, not leave the index where it was.
///
/// This is the case clamping got wrong and nothing caught: three shells with
/// the middle one on screen, close the first, and an index of 1 now names
/// the *third* — so the panel silently swapped to the one shell the user had
/// neither been reading nor asked to close.
#[test]
fn selection_follows_its_own_tab() {
    assert_eq!(
        selection_after(1, &[0], 2),
        0,
        "closing ahead pulls it back"
    );
    assert_eq!(
        selection_after(1, &[2], 2),
        1,
        "closing behind moves nothing"
    );
    assert_eq!(
        selection_after(2, &[0, 1], 1),
        0,
        "two ahead, two steps back"
    );
}

/// The selected tab going itself falls to whatever took its place, and to
/// the last tab when it was the last.
#[test]
fn closing_the_selected_tab_falls_to_a_neighbour() {
    assert_eq!(
        selection_after(1, &[1], 2),
        1,
        "the tab that shifted up into it"
    );
    assert_eq!(
        selection_after(2, &[2], 2),
        1,
        "nothing after it, so the one before"
    );
    assert_eq!(
        selection_after(0, &[0], 0),
        0,
        "the last tab leaves nothing to select"
    );
}

/// A sweep is the same rule: what matters is how many went *ahead* of the
/// selection, not how many went.
#[test]
fn a_sweep_counts_only_what_was_ahead() {
    assert_eq!(selection_after(3, &[0, 4], 3), 2);
    assert_eq!(selection_after(3, &[4, 5], 4), 3);
}

/// A closing question answered after the strip changed finds the shell
/// it asked about, or nothing at all, and the selection stays on the shell
/// it was on either way.
#[test]
fn an_answer_finds_its_tab_by_id_or_nothing() {
    // Three shells with ids 1, 2 and 3, the third selected; the question
    // is about the second.
    let (mut ids, mut active) = (vec![1, 2, 3], 2);
    // The first exits while the question is open.
    ids.remove(0);
    active = selection_after(active, &[0], ids.len());
    assert_eq!((ids[active], place_of(&ids, 2)), (3, Some(0)));
    // The answer closes the second, and the third stays selected.
    let at = place_of(&ids, 2).unwrap();
    ids.remove(at);
    active = selection_after(active, &[at], ids.len());
    assert_eq!(ids[active], 3);
    // A tab already gone answers nothing, so nothing else closes.
    assert_eq!(place_of(&ids, 2), None);
}
