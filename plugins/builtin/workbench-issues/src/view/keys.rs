//! The Issues mode's single-key shortcuts, and when a key is one.

/// What a key asks of the mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Key {
    /// `j`: the next row in the list.
    Next,
    /// `k`: the row before.
    Previous,
    /// Enter: show the first row when nothing is shown yet.
    Open,
    /// `/`: put the caret in the search box.
    Search,
    /// `e`: edit the issue shown.
    Edit,
    /// `c`: start a new issue.
    New,
    /// `o`: open the issue shown on its forge.
    OpenOnForge,
    /// Escape in the search box: hand the keys back to the list.
    LeaveSearch,
}

/// Where the keys are going inside the mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    /// The list or the issue being read: letters are shortcuts.
    Panel,
    /// The search box: letters are typed, and Escape leaves it.
    Search,
    /// The form, open or typed in: every key is the writer's. A shortcut there
    /// would type nothing and, for the ones that change what is shown, throw
    /// away what was written.
    Field,
}

/// The shortcut `key` is, pressed with `focus` where it is. `key` is the
/// character typed, or the key's name for Enter and Escape; `chorded` is a
/// press held with Ctrl, Alt or the platform key, which belongs to the app's
/// own bindings and never to these.
pub(super) fn shortcut(key: &str, chorded: bool, focus: Focus) -> Option<Key> {
    if chorded {
        return None;
    }
    match focus {
        Focus::Field => None,
        Focus::Search => (key == "escape").then_some(Key::LeaveSearch),
        Focus::Panel => match key {
            "j" => Some(Key::Next),
            "k" => Some(Key::Previous),
            "enter" => Some(Key::Open),
            "/" => Some(Key::Search),
            "e" => Some(Key::Edit),
            "c" => Some(Key::New),
            "o" => Some(Key::OpenOnForge),
            _ => None,
        },
    }
}

/// A tooltip that names its shortcut: `New issue (C)`.
pub(super) fn keyed(text: &str, key: &str) -> String {
    format!("{text} ({})", key.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_shortcuts_only_where_nothing_is_typed() {
        for key in ["j", "k", "e", "c", "o", "/", "enter"] {
            assert!(shortcut(key, false, Focus::Panel).is_some(), "{key}");
            assert_eq!(shortcut(key, false, Focus::Search), None, "{key}");
            assert_eq!(shortcut(key, false, Focus::Field), None, "{key}");
        }
    }

    #[test]
    fn escape_leaves_the_search_and_nothing_else() {
        assert_eq!(
            shortcut("escape", false, Focus::Search),
            Some(Key::LeaveSearch)
        );
        assert_eq!(shortcut("escape", false, Focus::Field), None);
        assert_eq!(shortcut("escape", false, Focus::Panel), None);
    }

    #[test]
    fn a_chord_or_a_capital_is_not_a_shortcut() {
        assert_eq!(shortcut("j", true, Focus::Panel), None);
        assert_eq!(shortcut("J", false, Focus::Panel), None);
    }
}
