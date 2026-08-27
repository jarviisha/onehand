//! The modal windows: agent manager, workspace settings, help.
//!
//! Each is a gpui-component `Dialog`, which owns the overlay, the focus trap
//! and the Esc handling — none of that is worth hand-rolling, and a
//! hand-rolled backdrop is where an "at most one open" invariant has to be
//! enforced by hand. Its `trigger` mode ties the
//! dialog to the control that opens it -- so the invariant is structural here
//! rather than something to remember to maintain.

use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, SharedString, Styled,
    Window, div,
};
use gpui_component::button::{ButtonGroup, ButtonVariants};
use gpui_component::dialog::Dialog;
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme, Disableable, Icon, IconName, Selectable, StyledExt};
use onehand_core::config::{AgentSpec, Appearance};

/// The add/edit form's fields. `editing` is `Some(i)` when an existing agent is
/// being changed and `None` when a new one is being added, so one form serves
/// both without a second "mode" flag to keep in step with it.
pub struct AgentDraft {
    pub editing: Option<usize>,
    pub name: Entity<InputState>,
    pub command: Entity<InputState>,
    pub args: Entity<InputState>,
}

impl AgentDraft {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            editing: None,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Claude Code")),
            command: cx.new(|cx| InputState::new(window, cx).placeholder("npx")),
            args: cx.new(|cx| {
                InputState::new(window, cx).placeholder("-y @agentclientprotocol/claude-agent-acp")
            }),
        }
    }

    /// Load an existing agent into the form for editing.
    pub fn load(&mut self, idx: usize, spec: &AgentSpec, window: &mut Window, cx: &mut App) {
        self.editing = Some(idx);
        self.name
            .update(cx, |s, cx| s.set_value(&spec.name, window, cx));
        self.command
            .update(cx, |s, cx| s.set_value(&spec.command, window, cx));
        self.args
            .update(cx, |s, cx| s.set_value(spec.args_line(), window, cx));
    }

    /// Clear the form back to "adding a new agent".
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.editing = None;
        for field in [&self.name, &self.command, &self.args] {
            field.update(cx, |s, cx| s.set_value("", window, cx));
        }
    }

    /// The spec this form describes, or `None` while it is not saveable.
    ///
    /// Name and command must both be non-blank -- that is the rule Save is
    /// greyed out by. Args split on whitespace.
    pub fn to_spec(&self, cx: &App) -> Option<AgentSpec> {
        let name = self.name.read(cx).value().trim().to_string();
        let command = self.command.read(cx).value().trim().to_string();
        if name.is_empty() || command.is_empty() {
            return None;
        }
        Some(AgentSpec {
            name,
            command,
            // Core's parser, paired with `args_line` on the way in, so an
            // argument containing a space survives being edited.
            args: onehand_core::config::split_args(&self.args.read(cx).value()),
        })
    }
}

/// One row in the agent list: name + command, with edit and delete actions.
///
/// Takes the shell handle rather than a `Context<Shell>` because the dialog's
/// `content` builder is an `Fn` -- it re-runs on every render and only ever has
/// an `&mut App`.
fn agent_row(shell: &Entity<Shell>, idx: usize, spec: &AgentSpec, cx: &App) -> impl IntoElement {
    let name = SharedString::from(spec.name.clone());
    let command = SharedString::from(if spec.args.is_empty() {
        spec.command.clone()
    } else {
        format!("{} {}", spec.command, spec.args.join(" "))
    });

    div()
        .h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .py_1()
        .child(
            div().v_flex().flex_1().child(div().child(name)).child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(command),
            ),
        )
        .child(
            crate::controls::action(("edit-agent", idx))
                .ghost()
                .icon(Icon::new(IconName::Replace))
                .on_click({
                    let shell = shell.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        shell.update(cx, |shell, cx| shell.edit_agent(idx, window, cx));
                    }
                }),
        )
        .child(
            crate::controls::action(("delete-agent", idx))
                .ghost()
                .icon(Icon::new(IconName::Delete))
                .on_click({
                    let shell = shell.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        shell.update(cx, |shell, cx| shell.delete_agent(idx, window, cx));
                    }
                }),
        )
}

/// A labelled form field.
fn field(label: &'static str, state: &Entity<InputState>) -> impl IntoElement {
    div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(div().text_xs().child(label))
        .child(Input::new(state))
}

/// The agent manager: the global agent menu a new session spawns from.
pub fn agent_manager(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let handle = cx.entity();
    let specs = shell.agents(cx).to_vec();

    let draft = shell.agent_draft();
    let (name, command, args) = (
        draft.name.clone(),
        draft.command.clone(),
        draft.args.clone(),
    );
    let saveable = draft.to_spec(cx).is_some();
    let editing = draft.editing.is_some();

    Dialog::new(cx)
        .trigger(crate::rail::rail_row(
            "open-agents",
            IconName::Bot,
            "Agents",
            cx,
        ))
        .title("Agents")
        .content(move |content, _, cx: &mut App| {
            let rows = specs
                .iter()
                .enumerate()
                .map(|(i, spec)| agent_row(&handle, i, spec, cx).into_any_element())
                .collect::<Vec<_>>();
            content.child(
                div()
                    .v_flex()
                    .gap_3()
                    .w_full()
                    .children(rows)
                    .child(field("Name", &name))
                    .child(field("Command", &command))
                    .child(field("Args", &args)),
            )
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                .child(
                    crate::controls::action("clear-agent")
                        .ghost()
                        .label(if editing { "Cancel edit" } else { "Clear" })
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.clear_agent_draft(window, cx);
                            }),
                        ),
                )
                .child(
                    crate::controls::action("save-agent")
                        .primary()
                        // Disabled until name and command are both non-blank:
                        // an agent missing either cannot be launched.
                        .disabled(!saveable)
                        .label(if editing { "Save" } else { "Add" })
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.save_agent_draft(window, cx);
                            }),
                        ),
                ),
        )
}

/// The light/dark/system picker.
///
/// A button group rather than three loose buttons: the choice is one value with
/// three answers, and a segmented control is the shape that says so -- one
/// pressed, the others available, no state where none or two are chosen.
///
/// App-wide, unlike everything else in the dialog it sits in, because the theme
/// it selects is a global: two windows cannot be drawn in two modes.
fn appearance_picker(shell: &Entity<Shell>, current: Appearance) -> impl IntoElement + use<> {
    let shell = shell.clone();
    ButtonGroup::new("appearance")
        .outline()
        .children(Appearance::ALL.into_iter().enumerate().map(|(i, choice)| {
            crate::controls::action(("appearance", i))
                .label(choice.label())
                .selected(choice == current)
        }))
        .on_click(
            move |clicked: &Vec<usize>, window: &mut Window, cx: &mut App| {
                let Some(choice) = clicked
                    .first()
                    .and_then(|i| Appearance::ALL.get(*i))
                    .copied()
                else {
                    return;
                };
                shell.update(cx, |shell, cx| shell.set_appearance(choice, window, cx));
            },
        )
}

/// Settings: the appearance, then the workspace's own name, storage-folder
/// binding, and the Workspaces section (new / open / recents).
///
/// There is no in-window workspace switcher by design -- one window hosts
/// exactly one workspace, so opening another one opens another window.
pub fn workspace_settings(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let handle = cx.entity();
    let appearance = shell.appearance(cx);
    let name = shell.workspace_name_input().clone();
    let storage = shell
        .storage_dir()
        .map(|dir| SharedString::from(dir.display().to_string()));
    let bound = storage.is_some();
    let current = shell.storage_dir().cloned();
    let recents = shell.recents(cx);

    Dialog::new(cx)
        .trigger(crate::rail::rail_row(
            "open-settings",
            IconName::Settings,
            "Settings",
            cx,
        ))
        .title("Settings")
        .content(move |content, _, cx: &mut App| {
            let storage = storage.clone();
            let rows = recents
                .iter()
                .enumerate()
                .map(|(i, dir)| {
                    // The window's own directory is marked and unclickable --
                    // "open" would be a no-op that looks like a failure.
                    let is_current = current.as_ref() == Some(dir);
                    let label = SharedString::from(dir.display().to_string());
                    let dir = dir.clone();
                    let handle = handle.clone();
                    crate::controls::action(("recent", i))
                        .ghost()
                        .w_full()
                        .disabled(is_current)
                        .label(if is_current {
                            SharedString::from(format!("{label}  (current)"))
                        } else {
                            label
                        })
                        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            handle.update(cx, |shell, cx| shell.open_recent(dir.clone(), cx));
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>();

            content.child(
                div()
                    .v_flex()
                    .gap_3()
                    .w_full()
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(div().text_xs().child("Appearance"))
                            .child(appearance_picker(&handle, appearance)),
                    )
                    .child(field("Workspace name", &name))
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(div().text_xs().child("Storage folder"))
                            .child(div().text_color(cx.theme().muted_foreground).child(
                                storage.unwrap_or_else(|| {
                                    // An unbound workspace persists nothing;
                                    // say so rather than showing a blank.
                                    SharedString::from("Not bound — nothing is saved")
                                }),
                            )),
                    )
                    .child(div().text_xs().child("Workspaces"))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .w_full()
                            .child({
                                let handle = handle.clone();
                                crate::controls::action("new-workspace")
                                    .ghost()
                                    .label("New workspace…")
                                    .on_click(
                                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                            handle.update(cx, |shell, cx| shell.new_workspace(cx));
                                        },
                                    )
                            })
                            .child({
                                let handle = handle.clone();
                                crate::controls::action("open-workspace")
                                    .ghost()
                                    .label("Open workspace…")
                                    .on_click(
                                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                            handle.update(cx, |shell, cx| shell.open_workspace(cx));
                                        },
                                    )
                            }),
                    )
                    .children(rows),
            )
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                .child(
                    crate::controls::action("unbind-storage")
                        .ghost()
                        .disabled(!bound)
                        .label("Unbind")
                        .on_click(
                            cx.listener(|shell: &mut Shell, _: &ClickEvent, window, cx| {
                                shell.unbind_storage(window, cx);
                            }),
                        ),
                )
                .child(
                    crate::controls::action("bind-storage")
                        .primary()
                        .label("Choose folder…")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.pick_storage_dir(cx);
                        })),
                ),
        )
}

/// The conversation-rename window.
///
/// **No trigger.** Every other dialog here is opened by a control that can
/// carry one, so `Dialog::trigger` ties the two together and "at most one open"
/// is structural. This one is opened from a menu entry, which is gone by the
/// time the dialog would appear — a `Dialog` built without a trigger renders
/// already open, so the shell decides whether it exists at all.
///
/// Until this existed the rename was unreachable: core could name a
/// conversation and the archive could store the name, and nothing anywhere
/// called either, so a conversation was stuck with the summary guessed from its
/// first prompt for good.
pub fn rename_session(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let input = shell.rename_input().clone();
    let resettable = shell.rename_is_override(cx);

    Dialog::new(cx)
        .title("Rename conversation")
        .content(move |content, _, _: &mut App| {
            content.child(div().v_flex().gap_1().w_full().child(Input::new(&input)))
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                // Offered only when there is an override to drop. On a
                // conversation that never had one this button would look like
                // it clears the title, and it does not: the title comes back,
                // derived from the first prompt.
                .when(resettable, |row| {
                    row.child(
                        crate::controls::action("reset-title")
                            .ghost()
                            .label("Use the automatic title")
                            .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                                shell.reset_conversation_title(cx);
                            })),
                    )
                })
                .child(
                    crate::controls::action("cancel-rename")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_rename(cx);
                        })),
                )
                .child(
                    crate::controls::action("save-rename")
                        .primary()
                        .label("Rename")
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.commit_rename(cx);
                        })),
                ),
        )
        // Esc and the close button both mean the same thing here, and both have
        // to clear the state that is putting this on screen -- otherwise the
        // dialog dismisses itself and the next frame renders it straight back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_rename(cx);
        }))
}

/// Split a project onto a branch of its own, as a git worktree.
///
/// The folder is **shown, not typed**. It is derived from the branch name, so a
/// second field holding it would be a copy of a reading -- one that either
/// fights every keystroke in the field above it or silently stops following it.
/// What is left for the user to decide is the part the derivation cannot know:
/// which folder to put it under, and that is a picker rather than a path to
/// spell out.
pub fn new_worktree(shell: &Shell, cx: &mut Context<Shell>) -> Dialog {
    let input = shell.worktree_branch().clone();
    let Some(draft) = shell.worktree_draft() else {
        return Dialog::new(cx);
    };
    let (label, error, busy) = (draft.label.clone(), draft.error.clone(), draft.busy);
    let target = shell
        .worktree_target(cx)
        .map(|dir| SharedString::from(dir.display().to_string()));
    let named = target.is_some();
    let (muted, danger) = (
        cx.theme().muted_foreground,
        crate::theme::status_ink(cx).danger,
    );

    Dialog::new(cx)
        .title("New worktree")
        .content(move |content, _, _: &mut App| {
            content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("A second checkout of {label}, on its own branch.")),
                    )
                    .child(Input::new(&input))
                    // Where it lands, in full. A worktree is a folder that
                    // appears on disk without anyone browsing to it, so the one
                    // thing this form owes the reader is the path it is about
                    // to create -- before it exists, not in a toast afterwards.
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(target.clone().unwrap_or_else(|| {
                                SharedString::from("Name the branch to see where its folder goes.")
                            })),
                    )
                    .when_some(error.clone(), |col, why| {
                        col.child(div().text_xs().text_color(danger).child(why))
                    }),
            )
        })
        .footer(
            div()
                .h_flex()
                .gap_2()
                .justify_end()
                .w_full()
                .child(
                    crate::controls::action("worktree-parent")
                        .ghost()
                        .label("Put it somewhere else…")
                        .disabled(busy)
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.pick_worktree_parent(cx);
                        })),
                )
                .child(
                    // Spent while git is working, for the same reason Create is
                    // and one more: the command cannot be called back, so a
                    // Cancel that still offered itself would be promising to
                    // undo something already happening on disk.
                    crate::controls::action("cancel-worktree")
                        .ghost()
                        .label("Cancel")
                        .disabled(busy)
                        .on_click(cx.listener(|shell: &mut Shell, _: &ClickEvent, _, cx| {
                            shell.cancel_worktree(cx);
                        })),
                )
                .child({
                    // Spent while git is working and while there is no name to
                    // work from: cloning a working tree takes long enough that
                    // a button still offering itself invites the second press
                    // that would ask for the same folder twice.
                    let create =
                        crate::controls::action("create-worktree")
                            .primary()
                            .label(if busy {
                                "Creating…"
                            } else {
                                "Create worktree"
                            });
                    match busy || !named {
                        true => crate::controls::resting(create).disabled(true),
                        false => create.on_click(cx.listener(
                            |shell: &mut Shell, _: &ClickEvent, _, cx| {
                                shell.commit_worktree(cx);
                            },
                        )),
                    }
                }),
        )
        // Esc and the close button have to clear what is putting this on screen,
        // or the dialog dismisses itself and the next frame renders it back.
        .on_close(cx.listener(|shell: &mut Shell, _, _, cx| {
            shell.cancel_worktree(cx);
        }))
}

/// One row of the Help window's shortcut table.
pub struct Shortcut {
    /// How the row is written for a human.
    pub label: &'static str,
    pub what: &'static str,
    /// The bindings behind it, spelled exactly as [`crate::shell::init_keymap`]
    /// spells them. This is what `keymap_and_help_agree` checks, so a binding
    /// added without a row here fails the build rather than going unfindable.
    /// Empty for a key the app does not bind at all.
    ///
    /// Read only by that test, which is the point of it: `label` is an
    /// editorial summary (one row covers `Ctrl+1…9`), so it cannot be derived
    /// from this, and this cannot be derived from it. Two spellings of one fact
    /// is a drift risk, so the test also checks they agree.
    #[allow(dead_code, reason = "the keymap contract, checked by tests")]
    pub keys: &'static [&'static str],
}

/// The keyboard-shortcut list.
///
/// Only bindings that exist in this build are listed -- an aspirational table
/// is worse than a short one.
///
/// Two bindings are absent on purpose: `Ctrl+Shift+P` (the command palette is a
/// feature — a command registry plus a filtered popup — not a keymap entry) and
/// `Ctrl+Shift+N` (Neovim mode was cut from this build along with the hardest
/// terminal parity work, so there is nothing for the key to open).
pub const SHORTCUTS: &[Shortcut] = &[
    Shortcut {
        label: "Ctrl+Shift+B",
        what: "Show or hide the navigation rail",
        keys: &["ctrl-shift-b"],
    },
    Shortcut {
        label: "Ctrl+Shift+O",
        what: "Workbench — Editor",
        keys: &["ctrl-shift-o"],
    },
    Shortcut {
        label: "Ctrl+Shift+E",
        what: "Workbench — Files",
        keys: &["ctrl-shift-e"],
    },
    Shortcut {
        label: "Ctrl+Shift+`",
        what: "Toggle the terminal",
        keys: &["ctrl-shift-`"],
    },
    Shortcut {
        label: "Ctrl+Shift+A",
        what: "Focus the composer",
        keys: &["ctrl-shift-a"],
    },
    Shortcut {
        label: "Ctrl+Shift+F",
        what: "Find in the transcript",
        keys: &["ctrl-shift-f"],
    },
    Shortcut {
        label: "Ctrl+Shift+R",
        what: "Restart the agent (twice, mid-turn)",
        keys: &["ctrl-shift-r"],
    },
    Shortcut {
        label: "Ctrl+Shift+W",
        what: "Close the session on screen (twice, mid-turn)",
        keys: &["ctrl-shift-w"],
    },
    Shortcut {
        label: "Ctrl+Shift+K",
        what: "Maximize the focused panel / restore",
        keys: &["ctrl-shift-k"],
    },
    Shortcut {
        label: "Ctrl+S",
        what: "Save the open file (not in the terminal)",
        keys: &["ctrl-s"],
    },
    Shortcut {
        label: "Up / Down",
        what: "Walk the composer's completion list (Enter takes it)",
        keys: &["up", "down"],
    },
    Shortcut {
        label: "Ctrl+V",
        what: "Paste — an image or a file becomes an attachment",
        keys: &["ctrl-v"],
    },
    Shortcut {
        label: "Ctrl+1…9",
        what: "Switch session by position",
        keys: &[
            "ctrl-1", "ctrl-2", "ctrl-3", "ctrl-4", "ctrl-5", "ctrl-6", "ctrl-7", "ctrl-8",
            "ctrl-9",
        ],
    },
    Shortcut {
        label: "Ctrl+Tab / Ctrl+Shift+Tab",
        what: "Cycle sessions, most recent first",
        keys: &["ctrl-tab", "ctrl-shift-tab"],
    },
    Shortcut {
        label: "Ctrl+= / Ctrl+-",
        what: "Zoom the focused panel in / out",
        keys: &["ctrl-=", "ctrl-+", "ctrl--"],
    },
    Shortcut {
        label: "Ctrl+0",
        what: "Reset the focused panel's zoom",
        keys: &["ctrl-0"],
    },
    Shortcut {
        // Handled inside the vendored terminal's key path, not by the app
        // keymap: the app must not bind these, or the PTY would never see a
        // paste (see `vendor/gpui-terminal/src/view.rs`).
        label: "Ctrl+Shift+C / V",
        what: "Copy / paste in the terminal",
        keys: &[],
    },
];

pub fn help(cx: &mut Context<Shell>) -> Dialog {
    Dialog::new(cx)
        .trigger(crate::rail::rail_row(
            "open-help",
            IconName::Info,
            "Help",
            cx,
        ))
        .title("Keyboard shortcuts")
        .content(|content, _, cx: &mut App| {
            content.child(
                div().v_flex().gap_2().w_full().children(
                    SHORTCUTS
                        .iter()
                        .map(|shortcut| {
                            div()
                                .h_flex()
                                .w_full()
                                .justify_between()
                                .gap_4()
                                .child(div().child(shortcut.what))
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(shortcut.label),
                                )
                        })
                        .collect::<Vec<_>>(),
                ),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::SHORTCUTS;

    /// The Help window is the whole keymap. A shortcut nobody can find is a
    /// shortcut nobody has, so the table is not documentation of the bindings
    /// -- it is the only way most of them are ever discovered.
    ///
    /// Reads the shell's source rather than the live keymap because binding
    /// requires an `App`, and this catches the failure that actually happens --
    /// someone adds a `KeyBinding` and forgets the table.
    #[test]
    fn keymap_and_help_agree() {
        let source = include_str!("shell.rs");
        let bound: Vec<&str> = source
            .split("KeyBinding::new(\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();

        for shortcut in SHORTCUTS {
            for key in shortcut.keys {
                assert!(
                    bound.contains(key),
                    "help lists {key:?} but init_keymap does not bind it"
                );
            }
        }

        for key in &bound {
            assert!(
                SHORTCUTS.iter().any(|s| s.keys.contains(key)),
                "init_keymap binds {key:?} but the Help window never mentions it"
            );
        }
    }

    /// A row's human label and its machine keys must describe the same key.
    ///
    /// `keymap_and_help_agree` compares `keys` against the keymap and never
    /// looks at `label` — so a row reading "Ctrl+Shift+B" while binding
    /// `ctrl-shift-e` passes it, and the Help window then teaches the wrong
    /// key. Two spellings of one fact need something holding them together.
    ///
    /// Checked against the **first** key only: `label` is an editorial summary
    /// ("Ctrl+1…9" stands for nine bindings, "Ctrl+= / Ctrl+-" hides the
    /// shifted alias), so every part being present is not a property that
    /// holds. What must hold is that the row starts by naming what it binds.
    #[test]
    fn a_rows_label_names_the_key_it_binds() {
        for shortcut in SHORTCUTS {
            let Some(first) = shortcut.keys.first() else {
                continue; // A row for a key the app deliberately does not bind.
            };
            let label = shortcut.label.to_lowercase();
            for part in first.split('-') {
                assert!(
                    label.contains(part),
                    "{:?} binds {first:?} but its label never says {part:?}",
                    shortcut.label
                );
            }
        }
    }
}
