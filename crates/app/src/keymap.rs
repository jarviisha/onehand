//! The app command registry, persisted overrides and the Settings editor.
//!
//! Contexts belong to commands, not user input. Rebinding replaces only this
//! registry's actions, preserving the component library and terminal bindings.

use std::collections::BTreeMap;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, App, AppContext, BorrowAppContext, Context, Entity, Focusable as _, InteractiveElement,
    IntoElement, KeyBinding, Keystroke, ParentElement, Render, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Input, InputState};
use gpui_component::notification::Notification;
use gpui_component::{ActiveTheme, Sizable as _, StyledExt, WindowExt as _};
use onehand_core::config::AppConfig;

use crate::shell::*;
use crate::state::Shared;

gpui::actions!(keymap_editor, [SaveShortcut, CancelShortcut]);

type Overrides = BTreeMap<String, Vec<String>>;

pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub defaults: &'static [&'static str],
    pub context: &'static str,
    pub scope: &'static str,
    action: fn() -> Box<dyn Action>,
}

/// The commands that act once per physical press; the shell's latch names
/// them by these, so a renamed id cannot leave the latch holding a stale one.
pub const RESTART: &str = "restart";
pub const CLOSE_SESSION: &str = "close_session";
pub const NEW_SESSION: &str = "new_session";

macro_rules! command {
    ($id:expr, $label:literal, [$($key:literal),*], $action:expr) => {
        command!($id, $label, [$($key),*], $action, "Shell && !Dialog", "Application, including terminal")
    };
    ($id:expr, $label:literal, [$($key:literal),*], $action:expr, $context:literal, $scope:literal) => {
        Command { id: $id, label: $label, defaults: &[$($key),*],
            context: $context, scope: $scope, action: || Box::new($action) }
    };
}

pub const COMMANDS: &[Command] = &[
    command!("settings", "Open Settings", ["ctrl-,"], OpenSettings),
    command!(
        "toggle_rail",
        "Show / hide navigation rail",
        ["ctrl-shift-b"],
        ToggleRail
    ),
    command!(
        "toggle_workbench",
        "Show / hide Workbench (previous mode)",
        ["ctrl-shift-j"],
        ToggleWorkbenchVisibility
    ),
    command!(
        "editor",
        "Open / focus Editor",
        ["ctrl-shift-e"],
        ToggleWorkbench
    ),
    command!(
        "markdown",
        "Open / focus Markdown",
        ["ctrl-shift-m"],
        ToggleMarkdown
    ),
    command!(
        "neovim",
        "Open / focus Neovim",
        ["ctrl-shift-n"],
        OpenNeovim
    ),
    // Unshifted on purpose: gpui names a key by the keysym the layout produces
    // with the modifiers applied, so shift over the backtick arrives as
    // `ctrl-~` and a `ctrl-shift-`` binding never matches anything.
    command!(
        "terminal",
        "Show / hide terminal",
        ["ctrl-`"],
        ToggleTerminal
    ),
    command!(
        "composer",
        "Focus composer",
        ["ctrl-shift-a"],
        FocusComposer
    ),
    command!(
        NEW_SESSION,
        "New session (default agent, current project)",
        ["ctrl-shift-o"],
        NewSession
    ),
    command!(
        RESTART,
        "Restart agent (confirm again mid-turn)",
        ["ctrl-shift-r"],
        RestartSession
    ),
    command!(
        CLOSE_SESSION,
        "Close session (confirm again mid-turn)",
        ["ctrl-shift-w"],
        CloseSession
    ),
    command!(
        "run_workflow",
        "Run a workflow on the current project",
        [],
        RunWorkflow
    ),
    command!("show_tasks", "Show the Tasks page", [], ShowTasks),
    command!(
        "maximize",
        "Maximize focused panel / restore",
        ["ctrl-shift-k"],
        ToggleMaximize
    ),
    command!(
        "save",
        "Save open file",
        ["ctrl-s"],
        SaveFile,
        "Shell && !Terminal && !Dialog",
        "Outside terminal"
    ),
    command!(
        "zoom_in",
        "Zoom focused panel in",
        ["ctrl-=", "ctrl-+"],
        ZoomIn
    ),
    command!("zoom_out", "Zoom focused panel out", ["ctrl--"], ZoomOut),
    command!(
        "zoom_reset",
        "Reset focused panel zoom",
        ["ctrl-0"],
        ZoomReset
    ),
    command!(
        "next_session",
        "Next session (most recent first)",
        ["ctrl-tab"],
        NextSession
    ),
    command!(
        "previous_session",
        "Previous session",
        ["ctrl-shift-tab"],
        PrevSession
    ),
    command!(
        "session_1",
        "Session 1",
        ["ctrl-1"],
        SelectSession { index: 0 }
    ),
    command!(
        "session_2",
        "Session 2",
        ["ctrl-2"],
        SelectSession { index: 1 }
    ),
    command!(
        "session_3",
        "Session 3",
        ["ctrl-3"],
        SelectSession { index: 2 }
    ),
    command!(
        "session_4",
        "Session 4",
        ["ctrl-4"],
        SelectSession { index: 3 }
    ),
    command!(
        "session_5",
        "Session 5",
        ["ctrl-5"],
        SelectSession { index: 4 }
    ),
    command!(
        "session_6",
        "Session 6",
        ["ctrl-6"],
        SelectSession { index: 5 }
    ),
    command!(
        "session_7",
        "Session 7",
        ["ctrl-7"],
        SelectSession { index: 6 }
    ),
    command!(
        "session_8",
        "Session 8",
        ["ctrl-8"],
        SelectSession { index: 7 }
    ),
    command!(
        "session_9",
        "Session 9",
        ["ctrl-9"],
        SelectSession { index: 8 }
    ),
    command!(
        "focus_rail_search",
        "Search sessions and projects in the rail",
        ["ctrl-k"],
        crate::rail::FocusRailSearch,
        "Shell && !Terminal && !Dialog",
        "Outside terminal"
    ),
    command!(
        "rail_next_session",
        "Next session in the rail",
        ["alt-down"],
        crate::rail::RailNext,
        "Shell && !Terminal && !Dialog",
        "Outside terminal"
    ),
    command!(
        "rail_previous_session",
        "Previous session in the rail",
        ["alt-up"],
        crate::rail::RailPrev,
        "Shell && !Terminal && !Dialog",
        "Outside terminal"
    ),
    command!(
        "rail_open",
        "Open the session, or fold the project",
        ["enter"],
        crate::rail::RailOpen,
        "Rail",
        "Rail list"
    ),
    command!(
        "rail_up",
        "Move to the row above",
        ["up"],
        crate::rail::RailUp,
        "Rail",
        "Rail list"
    ),
    command!(
        "rail_down",
        "Move to the row below",
        ["down"],
        crate::rail::RailDown,
        "Rail",
        "Rail list"
    ),
    command!(
        "rail_fold",
        "Fold the project",
        ["left"],
        crate::rail::RailFold,
        "Rail",
        "Rail list"
    ),
    command!(
        "rail_unfold",
        "Unfold the project",
        ["right"],
        crate::rail::RailUnfold,
        "Rail",
        "Rail list"
    ),
    // `A > B` scores at `B`'s depth, so these tie with the input's own arrow
    // and Tab bindings and win by being registered later. The composer adds
    // `ChatComposer` only while a list is open, so otherwise the keys still
    // move the caret.
    command!(
        "completion_previous",
        "Previous suggestion",
        ["up"],
        CompletionPrev,
        "ChatComposer > Input",
        "Composer suggestions"
    ),
    command!(
        "completion_next",
        "Next suggestion",
        ["down"],
        CompletionNext,
        "ChatComposer > Input",
        "Composer suggestions"
    ),
    command!(
        "completion_accept",
        "Accept suggestion",
        ["tab"],
        CompletionAccept,
        "ChatComposer > Input",
        "Composer suggestions (Enter also accepts)"
    ),
    command!(
        "paste",
        "Paste text / attach image or file",
        ["ctrl-v"],
        PasteHere,
        "ChatComposerCard > Input",
        "Composer"
    ),
    command!(
        "cycle_mode",
        "Next session mode",
        ["shift-tab"],
        CycleMode,
        "ChatComposerCard > Input",
        "Composer"
    ),
];

impl Command {
    pub fn keys(&self, overrides: &Overrides) -> Vec<String> {
        overrides
            .get(self.id)
            .cloned()
            .unwrap_or_else(|| self.defaults.iter().map(|s| s.to_string()).collect())
    }

    fn binding(&self, key: &str) -> KeyBinding {
        KeyBinding::load(
            key,
            (self.action)(),
            Some(
                gpui::KeyBindingContextPredicate::parse(self.context)
                    .unwrap()
                    .into(),
            ),
            false,
            None,
            &gpui::DummyKeyboardMapper,
        )
        .expect("validated shortcut")
    }
}

fn bindings(overrides: &Overrides) -> Vec<KeyBinding> {
    COMMANDS
        .iter()
        .flat_map(|command| {
            command
                .keys(overrides)
                .into_iter()
                .map(|key| command.binding(&key))
        })
        .collect()
}

fn owned(binding: &KeyBinding) -> bool {
    COMMANDS
        .iter()
        .any(|command| (command.action)().name() == binding.action().name())
}

/// Single strokes only: chords would delay typing and require another editor UI.
fn parse_key(raw: &str) -> Result<Keystroke, String> {
    if raw.split_whitespace().count() != 1 {
        return Err(
            "Use one key combination per shortcut; put alternatives in separate entries".into(),
        );
    }
    let key = Keystroke::parse(raw).map_err(|_| format!("Invalid shortcut: {raw}"))?;
    let named = matches!(
        key.key.as_str(),
        "tab"
            | "enter"
            | "escape"
            | "space"
            | "backspace"
            | "delete"
            | "insert"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
            | "up"
            | "down"
            | "left"
            | "right"
    );
    let function = key
        .key
        .strip_prefix('f')
        .and_then(|s| s.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
    if key.key.chars().count() != 1 && !named && !function {
        return Err(format!("Unknown key in shortcut: {raw}"));
    }
    if key.modifiers.shift
        && key.key.chars().count() == 1
        && !key.key.chars().next().unwrap().is_alphabetic()
    {
        return Err(
            "Use the shifted symbol without Shift, for example ctrl-+ instead of ctrl-shift-=."
                .into(),
        );
    }
    Ok(key)
}

fn validate(overrides: &Overrides) -> Result<(), String> {
    for id in overrides.keys() {
        if !COMMANDS.iter().any(|c| c.id == id) {
            return Err(format!("Unknown command: {id}"));
        }
    }
    let mut seen: Vec<(Keystroke, &str, &str)> = Vec::new();
    for command in COMMANDS {
        for raw in command.keys(overrides) {
            let key = parse_key(&raw)?;
            // A key may serve two commands only when one is the rail list's and
            // the other a field's: the list holds no field, so the two are never
            // in one focus stack. A window command is everywhere, and the
            // composer's scopes nest in each other; anything else would rest on
            // registration order.
            let overlap = |context: &str| {
                context == command.context
                    || context.starts_with("Shell")
                    || command.context.starts_with("Shell")
                    || (context != "Rail" && command.context != "Rail")
            };
            if let Some((_, other, _)) = seen
                .iter()
                .find(|(k, _, context)| *k == key && overlap(context))
            {
                return Err(format!(
                    "{raw} conflicts between {other} and {}",
                    command.label
                ));
            }
            if command.context.starts_with("Shell")
                && !(key.modifiers.control
                    || key.modifiers.alt
                    || key.modifiers.platform
                    || key.modifiers.function
                    || key.key.starts_with('f') && key.key.len() > 1)
            {
                return Err(format!(
                    "{} needs Ctrl, Alt, Super, or a function key",
                    command.label
                ));
            }
            if ["ctrl-shift-c", "ctrl-shift-v", "escape"]
                .iter()
                .any(|reserved| Keystroke::parse(reserved).unwrap() == key)
            {
                return Err(format!(
                    "{raw} is reserved for clipboard or closing dialogs"
                ));
            }
            seen.push((key, command.label, command.context));
        }
    }
    Ok(())
}

/// The key a command answers to now, the first of them, for a control that
/// names it.
pub fn first_key(id: &str, cx: &App) -> Option<Keystroke> {
    let command = COMMANDS.iter().find(|command| command.id == id)?;
    let keys = command.keys(&Shared::global(cx).keymap);
    Keystroke::parse(keys.first()?).ok()
}

/// A released physical key ends the once-per-press latch even when
/// modifiers were released first (the release can have different modifiers).
pub fn released(id: &str, key: &str, overrides: &Overrides) -> bool {
    COMMANDS
        .iter()
        .find(|command| command.id == id)
        .is_some_and(|command| {
            command
                .keys(overrides)
                .iter()
                .any(|raw| parse_key(raw).is_ok_and(|stroke| stroke.key.eq_ignore_ascii_case(key)))
        })
}

fn validate_controls(overrides: &Overrides, existing: &[KeyBinding]) -> Result<(), String> {
    validate(overrides)?;
    for command in COMMANDS {
        for raw in command.keys(overrides) {
            let key = parse_key(&raw)?;
            if command
                .defaults
                .iter()
                .any(|default| parse_key(default).unwrap() == key)
            {
                continue;
            }
            if existing
                .iter()
                .filter(|b| !owned(b) && !b.action().as_any().is::<gpui::NoAction>())
                .any(|binding| binding.match_keystrokes(std::slice::from_ref(&key)) == Some(false))
            {
                return Err(format!(
                    "{raw} is already used by an editor or navigation control. Choose another shortcut."
                ));
            }
        }
    }
    Ok(())
}

/// Remove the previous app bindings instead of stacking NoAction overrides:
/// this returns an old shortcut to the input/PTY that originally owned it.
fn replace(existing: Vec<KeyBinding>, overrides: &Overrides) -> Vec<KeyBinding> {
    existing
        .into_iter()
        .filter(|b| !owned(b))
        .chain(bindings(overrides))
        .collect()
}

fn install(cx: &mut App) {
    let existing = cx.key_bindings().borrow().bindings().cloned().collect();
    let next = replace(existing, &Shared::global(cx).keymap);
    cx.clear_key_bindings();
    cx.bind_keys(next);
}

#[derive(Default)]
struct LoadWarning(Option<String>);
impl gpui::Global for LoadWarning {}

/// Ids a command answered to before it was renamed, and the id it has now.
/// Without this an override saved under the old id fails validation, and that
/// throws the person's whole keymap away.
const RENAMED: &[(&str, &str)] = &[("run_pipeline", "run_workflow")];

fn rename_old_ids(overrides: &mut Overrides) {
    for (old, new) in RENAMED {
        if let Some(keys) = overrides.remove(*old) {
            overrides.entry((*new).to_string()).or_insert(keys);
        }
    }
}

pub fn init(cx: &mut App) {
    cx.update_global::<Shared, _>(|shared, _| rename_old_ids(&mut shared.keymap));
    let existing: Vec<_> = cx.key_bindings().borrow().bindings().cloned().collect();
    let warning = validate_controls(&Shared::global(cx).keymap, &existing).err();
    if let Some(error) = &warning {
        eprintln!("onehand: keymap ignored: {error}");
        cx.update_global::<Shared, _>(|shared, _| shared.keymap.clear());
    }
    cx.set_global(LoadWarning(warning));
    // Suppress the library's focus traversal only inside a PTY. These are
    // routing rules, not editable commands, and are installed just once.
    cx.bind_keys(fixed_bindings());
    install(cx);
}

fn fixed_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("enter", SaveShortcut, Some("ShortcutEditor > Input")),
        KeyBinding::new("escape", CancelShortcut, Some("ShortcutEditor > Input")),
        KeyBinding::new("tab", gpui::NoAction, Some("Terminal")),
        KeyBinding::new("shift-tab", gpui::NoAction, Some("Terminal")),
    ]
}

mod editor;
pub use editor::Editor;

#[cfg(test)]
mod tests {
    use super::editor::edit_changes;
    use super::*;
    use gpui::{KeyContext, Keymap};

    fn action_at(map: &Keymap, key: &str, contexts: &[&str]) -> Option<String> {
        let contexts: Vec<_> = contexts
            .iter()
            .map(|s| KeyContext::parse(s).unwrap())
            .collect();
        let (matches, pending) =
            map.bindings_for_input(&[Keystroke::parse(key).unwrap()], &contexts);
        assert!(!pending);
        matches
            .first()
            .map(|binding| binding.action().name().to_string())
    }

    #[test]
    fn shortcut_editor_enter_and_escape_win_only_inside_its_input() {
        let mut installed = vec![KeyBinding::new(
            "enter",
            gpui_component::input::SelectAll,
            Some("Input"),
        )];
        installed.extend(fixed_bindings());
        let map = Keymap::new(replace(installed, &Overrides::new()));
        assert_eq!(
            action_at(&map, "enter", &["Dialog", "ShortcutEditor", "Input"]),
            Some(SaveShortcut.name().into())
        );
        assert_eq!(
            action_at(&map, "escape", &["Dialog", "ShortcutEditor", "Input"]),
            Some(CancelShortcut.name().into())
        );
        assert_eq!(
            action_at(&map, "enter", &["Dialog", "Input"]),
            Some(gpui_component::input::SelectAll.name().into())
        );
    }

    #[test]
    fn an_override_under_a_renamed_id_reaches_the_new_one() {
        let keys = |k: &str| vec![k.to_string()];
        let mut old = Overrides::from([("run_pipeline".to_string(), keys("ctrl-alt-p"))]);
        rename_old_ids(&mut old);
        assert_eq!(
            old,
            Overrides::from([("run_workflow".to_string(), keys("ctrl-alt-p"))])
        );
        assert!(validate(&old).is_ok());
        let mut both = Overrides::from([
            ("run_pipeline".to_string(), keys("ctrl-alt-p")),
            ("run_workflow".to_string(), keys("ctrl-alt-w")),
        ]);
        rename_old_ids(&mut both);
        assert_eq!(
            both,
            Overrides::from([("run_workflow".to_string(), keys("ctrl-alt-w"))])
        );
    }

    /// A shortcut opened and left as it was is not an unsaved change, so
    /// closing Settings over it must not ask; one whose keys differ is.
    #[test]
    fn a_shortcut_edit_is_pending_only_when_its_keys_changed() {
        let current = vec!["ctrl-shift-b".to_string()];
        assert!(!edit_changes("ctrl-shift-b", &current));
        assert!(!edit_changes("  ctrl-shift-b  ", &current));
        assert!(edit_changes("ctrl-shift-x", &current));
        assert!(edit_changes("ctrl-shift-b ctrl-alt-b", &current));
        // Emptied is a change: it unassigns the command.
        assert!(edit_changes("", &current));
        assert!(!edit_changes("   ", &[]));
    }

    #[test]
    fn defaults_dispatch_by_context() {
        validate(&Overrides::new()).unwrap();
        let map = Keymap::new(bindings(&Overrides::new()));
        assert_eq!(
            action_at(&map, "ctrl-shift-j", &["Shell", "Terminal"]),
            Some(ToggleWorkbenchVisibility.name().into())
        );
        assert_eq!(
            action_at(&map, "ctrl-shift-e", &["Shell", "Workbench", "Input"]),
            Some(ToggleWorkbench.name().into())
        );
        assert_eq!(
            action_at(&map, "ctrl-s", &["Shell", "Workbench", "Input"]),
            Some(SaveFile.name().into())
        );
        assert_eq!(action_at(&map, "ctrl-s", &["Shell", "Terminal"]), None);
        assert_eq!(
            action_at(&map, "ctrl-shift-w", &["Shell", "Dialog", "Input"]),
            None
        );
        assert_eq!(
            action_at(&map, "ctrl-shift-o", &["Shell", "Terminal"]),
            Some(NewSession.name().into())
        );
        assert_eq!(
            action_at(&map, "ctrl-shift-o", &["Shell", "Dialog", "Input"]),
            None
        );
        assert_eq!(
            action_at(&map, "ctrl-,", &["Shell"]),
            Some(OpenSettings.name().into())
        );
        assert_eq!(
            action_at(&map, "down", &["Shell", "ChatComposerCard", "Input"]),
            None
        );
        assert_eq!(
            action_at(
                &map,
                "down",
                &["Shell", "ChatComposerCard", "ChatComposer", "Input"]
            ),
            Some(CompletionNext.name().into())
        );
        assert_eq!(
            action_at(&map, "shift-tab", &["Shell", "ChatComposerCard", "Input"]),
            Some(CycleMode.name().into())
        );
        // The rail's list and the composer's suggestions share the arrows,
        // each only where it has focus.
        assert_eq!(
            action_at(&map, "down", &["Shell", "Rail"]),
            Some(crate::rail::RailDown.name().into())
        );
    }

    #[test]
    fn remapping_unbinding_and_reset_return_old_keys_without_accumulating_bindings() {
        let base = vec![
            KeyBinding::new("tab", gpui::NoAction, Some("Terminal")),
            KeyBinding::new("shift-tab", gpui::NoAction, Some("Terminal")),
        ];
        let defaults = replace(base, &Overrides::new());
        let count = defaults.len();
        let mut overrides = Overrides::from([
            ("toggle_workbench".into(), vec!["alt-j".into()]),
            ("editor".into(), vec![]),
        ]);
        validate(&overrides).unwrap();
        let changed = replace(defaults, &overrides);
        let changed = replace(changed, &overrides);
        let map = Keymap::new(changed.clone());
        assert_eq!(
            action_at(&map, "ctrl-shift-j", &["Shell", "Terminal"]),
            None
        );
        assert_eq!(
            action_at(&map, "alt-j", &["Shell", "Terminal"]),
            Some(ToggleWorkbenchVisibility.name().into())
        );
        assert_eq!(action_at(&map, "ctrl-shift-e", &["Shell"]), None);
        assert_eq!(changed.len(), count - 1);
        overrides.clear();
        let reset = replace(changed, &overrides);
        assert_eq!(reset.len(), count);
        assert_eq!(
            action_at(&Keymap::new(reset), "ctrl-shift-j", &["Shell"]),
            Some(ToggleWorkbenchVisibility.name().into())
        );
    }

    #[test]
    fn validation_rejects_collisions_typos_chords_and_terminal_clipboard() {
        for raw in [
            "ctrl-shift-e",
            "shift-ctrl-e",
            "ctrl-shift-v",
            "ctrl-shfit-j",
            "ctrl-k ctrl-j",
            "j",
            "ctrl",
            "",
        ] {
            let overrides = Overrides::from([("toggle_workbench".into(), vec![raw.into()])]);
            assert!(validate(&overrides).is_err(), "accepted {raw:?}");
        }
        assert!(validate(&Overrides::from([("typo".into(), vec![])])).is_err());
        let overrides = Overrides::from([(
            "toggle_workbench".into(),
            vec!["alt-j".into(), "alt-k".into()],
        )]);
        assert!(validate(&overrides).is_ok());
    }

    /// The composer's scopes nest while a suggestion list is open, so one key
    /// in two of them would be decided by registration order; the rail's list
    /// holds no field, so its keys may repeat a field's.
    #[test]
    fn only_the_rail_list_shares_a_key_with_a_field() {
        let overrides = Overrides::from([("cycle_mode".into(), vec!["tab".into()])]);
        assert!(
            validate(&overrides).is_err(),
            "two composer scopes shared tab"
        );
        assert!(
            validate(&Overrides::new()).is_ok(),
            "enter and the arrows clash"
        );
    }

    #[test]
    fn control_shortcuts_cannot_silently_shadow_a_custom_binding() {
        let library = vec![KeyBinding::new(
            "ctrl-a",
            gpui_component::input::SelectAll,
            Some("Input"),
        )];
        let overrides = Overrides::from([("toggle_workbench".into(), vec!["ctrl-a".into()])]);
        assert!(validate_controls(&overrides, &library).is_err());
    }

    #[test]
    fn terminal_tab_passthrough_and_library_bindings_survive_rebinding() {
        let library = vec![
            KeyBinding::new("tab", gpui_component::input::SelectAll, None),
            KeyBinding::new("tab", gpui::NoAction, Some("Terminal")),
            KeyBinding::new("shift-tab", gpui::NoAction, Some("Terminal")),
        ];
        let map = Keymap::new(replace(library, &Overrides::new()));
        assert_eq!(action_at(&map, "tab", &["Shell", "Terminal"]), None);
        assert_eq!(action_at(&map, "shift-tab", &["Shell", "Terminal"]), None);
        assert_eq!(
            action_at(&map, "tab", &["Shell", "Input"]),
            Some(gpui_component::input::SelectAll.name().into())
        );
        assert_eq!(
            action_at(
                &map,
                "tab",
                &["Shell", "ChatComposerCard", "ChatComposer", "Input"]
            ),
            Some(CompletionAccept.name().into())
        );
    }

    #[test]
    fn release_follows_the_remapped_key_even_without_modifiers() {
        let overrides = Overrides::from([(RESTART.into(), vec!["alt-x".into()])]);
        assert!(released(RESTART, "x", &overrides));
        assert!(!released(RESTART, "r", &overrides));
    }
}
