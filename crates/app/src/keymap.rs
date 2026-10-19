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
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme, StyledExt};

use crate::state::Shared;

gpui::actions!(keymap_editor, [SaveShortcut, CancelShortcut]);

type Overrides = BTreeMap<String, Vec<String>>;

#[derive(Clone)]
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

// The table itself, one entry per command, in a file of its own: it is
// the part that grows with every key the app learns.
mod commands;
pub use commands::COMMANDS;

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
    check(COMMANDS, overrides)
}

/// The focus stack a command's context sits in, named by the context its
/// predicate starts from.
///
/// A window command (`Shell…`) is in every stack. The composer's two scopes
/// nest, the suggestion list's inside the card's, so they are one stack. The
/// rail's list, the Issues page, the Plugins list and the two blocking cards
/// never hold one another, so each is a stack of its own, and one key may serve
/// one command in each.
fn stack(context: &str) -> &str {
    match head(context) {
        "ChatComposer" => "ChatComposerCard",
        head => head,
    }
}

/// The context a command's predicate starts from: its region.
fn head(context: &str) -> &str {
    context
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
}

/// Whether a binding in `context` fires with `contexts` as the focus stack,
/// outermost first: at any depth of it, as the keymap matches.
fn fires(context: &str, contexts: &[&str]) -> bool {
    let contexts: Vec<_> = contexts
        .iter()
        .map(|c| gpui::KeyContext::parse(c).unwrap())
        .collect();
    gpui::KeyBindingContextPredicate::parse(context)
        .is_ok_and(|predicate| predicate.depth_of(&contexts).is_some())
}

fn check(commands: &[Command], overrides: &Overrides) -> Result<(), String> {
    for id in overrides.keys() {
        if !commands.iter().any(|c| c.id == id) {
            return Err(format!("Unknown command: {id}"));
        }
    }
    let mut seen: Vec<(Keystroke, &str, &str)> = Vec::new();
    for command in commands {
        let window_wide = stack(command.context) == "Shell";
        let region = head(command.context);
        // Fires in its region, and still fires with a field inside it focused.
        let reaches_fields = fires(command.context, &["Shell", region])
            && fires(command.context, &["Shell", region, "Input"]);
        for raw in command.keys(overrides) {
            let key = parse_key(&raw)?;
            // Two commands on one key in one focus stack would be decided by
            // registration order, so that is the clash; in two stacks that
            // never hold each other, each key reaches only its own.
            if let Some((_, other, _)) = seen.iter().find(|(k, _, context)| {
                *k == key
                    && (window_wide
                        || stack(context) == "Shell"
                        || stack(context) == stack(command.context))
            }) {
                return Err(format!(
                    "{raw} conflicts between {other} and {}",
                    command.label
                ));
            }
            let modified = key.modifiers.control
                || key.modifiers.alt
                || key.modifiers.platform
                || key.modifiers.function
                || key.key.starts_with('f') && key.key.len() > 1;
            if window_wide && !modified {
                return Err(format!(
                    "{} needs Ctrl, Alt, Super, or a function key",
                    command.label
                ));
            }
            // A plain key a region handles must not land while a field in that
            // region holds the caret, or typing fires it.
            if !window_wide && !modified && reaches_fields {
                return Err(format!(
                    "{raw} for {} would fire while typing; exclude Input from its context",
                    command.label
                ));
            }
            if ["ctrl-shift-c", "ctrl-shift-v"]
                .iter()
                .any(|reserved| Keystroke::parse(reserved).unwrap() == key)
            {
                return Err(format!("{raw} is reserved for the clipboard"));
            }
            // Escape belongs to dialogs and fields. A region may take it only
            // where neither is in the stack.
            if key == Keystroke::parse("escape").unwrap()
                && (window_wide
                    || fires(command.context, &["Shell", region, "Input"])
                    || fires(command.context, &["Shell", "Dialog", region]))
            {
                return Err(format!(
                    "{raw} is reserved for closing dialogs and leaving fields"
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
    use super::*;
    use crate::shell::*;
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
        // Settings hides the editor, so its save does not reach through it.
        assert_eq!(
            action_at(&map, "ctrl-s", &["Shell", "Chat", "Settings", "Input"]),
            None
        );
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
        // The Issues page steps its list with the arrows, but never while
        // the caret is in its search; Esc closes what is open over the list.
        let page = ["Shell", "Chat", "Issues"];
        assert_eq!(
            action_at(&map, "up", &page),
            Some(onehand_workbench_issues::IssuesUp.name().into())
        );
        assert_eq!(
            action_at(&map, "up", &[&page[..], &["Input"]].concat()),
            None
        );
        assert_eq!(
            action_at(&map, "escape", &page),
            Some(onehand_workbench_issues::IssuesClose.name().into())
        );
        // The question card walks on the arrows and takes a number, but the
        // free-text box inside it keeps every key; the permission card answers
        // Enter and Esc.
        let card = ["Shell", "Chat", "QuestionCard"];
        assert_eq!(
            action_at(&map, "2", &card),
            Some(
                crate::chat::transcript::QuestionPick { index: 1 }
                    .name()
                    .into()
            )
        );
        assert_eq!(
            action_at(&map, "2", &[&card[..], &["Input"]].concat()),
            None
        );
        assert_eq!(
            action_at(&map, "escape", &["Shell", "Chat", "PermissionCard"]),
            Some(crate::chat::transcript::PermissionDeny.name().into())
        );
        // A Plugins row opens on Enter and flips on Space, but never while a
        // field holds the caret.
        let list = ["Shell", "Workbench", "PluginList"];
        assert_eq!(
            action_at(&map, "enter", &list),
            Some(onehand_workbench_plugins::PluginOpen.name().into())
        );
        assert_eq!(
            action_at(&map, "space", &list),
            Some(onehand_workbench_plugins::PluginFlip.name().into())
        );
        assert_eq!(
            action_at(&map, "space", &[&list[..], &["Input"]].concat()),
            None
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
    /// is a stack of its own, so its keys may repeat a field's.
    #[test]
    fn the_composers_scopes_are_one_stack() {
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

    /// One command on `j` in each region that takes keys of its own, each
    /// kept out of the fields inside it.
    fn regions() -> Vec<(&'static str, Command)> {
        vec![
            (
                "Issues",
                command!(
                    "t_issues",
                    "Issues j",
                    ["j"],
                    crate::rail::RailUp,
                    "Issues && !Input",
                    "Issues"
                ),
            ),
            (
                "PluginList",
                command!(
                    "t_plugins",
                    "Plugins j",
                    ["j"],
                    crate::rail::RailDown,
                    "PluginList && !Input",
                    "Plugins list"
                ),
            ),
            (
                "QuestionCard",
                command!(
                    "t_question",
                    "Question j",
                    ["j"],
                    crate::rail::RailFold,
                    "QuestionCard && !Input",
                    "Question card"
                ),
            ),
            (
                "PermissionCard",
                command!(
                    "t_permission",
                    "Permission j",
                    ["j"],
                    crate::rail::RailUnfold,
                    "PermissionCard && !Input",
                    "Permission card"
                ),
            ),
        ]
    }

    /// `COMMANDS` and `extra`, as the validator would see them registered.
    fn with(extra: impl IntoIterator<Item = Command>) -> Vec<Command> {
        COMMANDS.iter().cloned().chain(extra).collect()
    }

    #[test]
    fn a_key_may_serve_one_command_in_each_region() {
        let all = with(regions().into_iter().map(|(_, command)| command));
        assert_eq!(check(&all, &Overrides::new()), Ok(()));
    }

    #[test]
    fn a_key_shared_by_a_region_and_a_window_command_fails() {
        let all = with([command!(
            "t_issues",
            "Issues new",
            ["ctrl-shift-o"],
            crate::rail::RailUp,
            "Issues && !Input",
            "Issues"
        )]);
        assert!(check(&all, &Overrides::new()).is_err());
    }

    #[test]
    fn a_plain_region_key_must_stay_out_of_fields() {
        let typed = with([command!(
            "t",
            "j",
            ["j"],
            crate::rail::RailUp,
            "Issues",
            "Issues"
        )]);
        assert!(check(&typed, &Overrides::new()).is_err());
        let modified = with([command!(
            "t",
            "j",
            ["alt-j"],
            crate::rail::RailUp,
            "Issues",
            "Issues"
        )]);
        assert_eq!(check(&modified, &Overrides::new()), Ok(()));
    }

    #[test]
    fn escape_is_a_regions_only_where_no_dialog_or_field_is() {
        let on = |context: &'static str| {
            let command = Command {
                id: "t",
                label: "Esc",
                defaults: &["escape"],
                context,
                scope: "",
                action: || Box::new(crate::rail::RailUp),
            };
            check(&with([command]), &Overrides::new())
        };
        assert_eq!(on("PluginList && !Dialog && !Input"), Ok(()));
        for context in [
            "PluginList && !Input",
            "PluginList && !Dialog",
            "ChatComposer > Input",
            "Shell && !Dialog",
        ] {
            assert!(on(context).is_err(), "escape accepted in {context}");
        }
    }

    #[test]
    fn a_regions_key_fires_in_its_region_only() {
        let regions = regions();
        let escape = command!(
            "t_escape",
            "Esc",
            ["escape"],
            crate::rail::RailOpen,
            "PluginList && !Dialog && !Input",
            "Plugins list"
        );
        let all = with(regions.iter().map(|(_, c)| c.clone()).chain([escape]));
        check(&all, &Overrides::new()).unwrap();
        let map = Keymap::new(
            all.iter()
                .flat_map(|c| c.keys(&Overrides::new()).into_iter().map(|k| c.binding(&k)))
                .collect(),
        );
        for (region, command) in &regions {
            let own = Some((command.action)().name().to_string());
            assert_eq!(action_at(&map, "j", &["Shell", region]), own, "{region}");
            assert_eq!(
                action_at(&map, "j", &["Shell", region, "Input"]),
                None,
                "{region}"
            );
            assert_eq!(
                action_at(&map, "j", &["Shell", "Dialog", "Input"]),
                None,
                "{region}"
            );
            assert_eq!(
                action_at(&map, "j", &["Shell", "Terminal"]),
                None,
                "{region}"
            );
            // The window's commands still reach a region, and a field in it.
            assert_eq!(
                action_at(&map, "ctrl-shift-o", &["Shell", region, "Input"]),
                Some(NewSession.name().into()),
                "{region}"
            );
        }
        let esc = Some(crate::rail::RailOpen.name().to_string());
        assert_eq!(action_at(&map, "escape", &["Shell", "PluginList"]), esc);
        assert_eq!(
            action_at(&map, "escape", &["Shell", "PluginList", "Input"]),
            None
        );
        assert_eq!(
            action_at(&map, "escape", &["Shell", "PluginList", "Dialog"]),
            None
        );
    }
}
