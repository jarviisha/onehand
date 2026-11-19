//! The empty session's greeting: when it is drawn, and what it says.

/// What the heading says.
pub(super) const GREETING: &str = "What are we building?";

/// Whether the pane greets rather than draws a transcript: a connected
/// session with nothing in it and nothing standing over the composer (a
/// reconnect, the queue, staged files, a waiting card). The first item moves
/// the composer to the foot.
pub(super) fn greets(connected: bool, items: usize, over_composer: bool) -> bool {
    connected && items == 0 && !over_composer
}

/// How far above the well's foot the composer rests so it sits at the well's
/// middle. Anchored from the foot, so a popup opening over it grows upward and
/// moves nothing.
pub(super) fn offset(well: gpui::Pixels, composer: gpui::Pixels) -> gpui::Pixels {
    ((well - composer) / 2.).max(gpui::px(0.))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_only_a_connected_empty_session_with_nothing_over_the_composer() {
        assert!(greets(true, 0, false));
        assert!(!greets(false, 0, false)); // connecting or failed
        assert!(!greets(true, 1, false)); // first item moves the composer down
        assert!(!greets(true, 0, true)); // a card or banner stands over the composer
    }

    #[test]
    fn the_composer_rests_at_the_wells_middle_and_never_below_its_foot() {
        use gpui::px;
        assert_eq!(offset(px(600.), px(100.)), px(250.));
        assert_eq!(offset(px(80.), px(100.)), px(0.));
    }
}
