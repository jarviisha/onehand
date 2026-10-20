use super::*;

#[test]
fn editor_layout_thresholds() {
    assert!(matches!(editor_layout(gpui::rems(68.)), EditorLayout::Three));
    assert!(matches!(editor_layout(gpui::rems(67.9)), EditorLayout::Stacked));
    assert!(matches!(editor_layout(gpui::rems(48.)), EditorLayout::Stacked));
    assert!(matches!(editor_layout(gpui::rems(47.9)), EditorLayout::OneAtATime));
}
