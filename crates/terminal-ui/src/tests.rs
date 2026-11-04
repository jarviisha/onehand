use super::foreground_busy;

/// A shell at its prompt is its own foreground group; a command in front
/// is a different one; and a terminal that will not say is neither.
#[test]
fn busy_is_a_foreground_group_other_than_the_shell() {
    assert_eq!(foreground_busy(Some(100), Some(100)), Some(false));
    assert_eq!(foreground_busy(Some(100), Some(240)), Some(true));
    assert_eq!(foreground_busy(Some(100), None), None);
    assert_eq!(foreground_busy(None, Some(240)), None);
}
