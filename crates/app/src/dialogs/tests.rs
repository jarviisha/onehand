/// No dialog here names itself through the library's own title slot.
///
/// A dialog opened from a trigger is rebuilt when that trigger is pressed,
/// out of its content builder, its style and its props. Its title, header
/// and footer are elements, which cannot be cloned into a builder that runs
/// again on every open, so they do not survive the trip -- and every dialog
/// the rail opened lost its name and its buttons that way, in silence,
/// while otherwise working.
///
/// The name goes in the content instead, and on every dialog rather than
/// only the triggered ones: one shape is what stops the next dialog picking
/// the wrong one. This is here because nothing else says no -- the slot
/// exists, compiles, and does nothing.
#[test]
fn no_dialog_names_itself_through_the_library_slot() {
    // Assembled at run time so this test is not a match for itself.
    let slot = format!(".{}(", "title");
    for (file, text) in [
        ("dialogs.rs", include_str!("../dialogs.rs")),
        ("dialogs/issue.rs", include_str!("issue.rs")),
    ] {
        for (n, line) in text.lines().enumerate() {
            assert!(
                !line.contains(&slot),
                "{file}:{}: names the dialog through the library's title \
                 slot, which a triggered dialog drops on its way back open. \
                 Put the name in the content instead.\n    {}",
                n + 1,
                line.trim()
            );
        }
    }
}
