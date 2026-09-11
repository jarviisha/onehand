//! The modal windows: settings, the conversation rename, the worktree split.
//!
//! Each is a gpui-component `Dialog`, which owns the overlay, the focus trap
//! and the Esc handling — none of that is worth hand-rolling, and a
//! hand-rolled backdrop is where an "at most one open" invariant has to be
//! enforced by hand. Its `trigger` mode ties the
//! dialog to the control that opens it -- so the invariant is structural here
//! rather than something to remember to maintain.

use crate::controls::Refuses as _;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, Window, div, px,
    relative,
};
use gpui_component::button::{ButtonGroup, ButtonVariants};
use gpui_component::dialog::{Dialog, DialogClose, DialogTitle};
use gpui_component::input::{Input, InputState};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable as _, StyledExt,
};
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

/// What deleting an agent does to a form left open on one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DraftShift {
    /// The form is about an agent the deletion did not move.
    Keep,
    /// The agent being edited is the one that went.
    Clear,
    /// The form's agent slid down into the hole above it.
    MoveTo(usize),
}

/// Where a form open on `editing` belongs once the agent at `removed` is gone.
///
/// `AgentDraft::editing` is a *position* in the agent list, and deleting shifts
/// every position after the one removed. Left alone, a form opened on the third
/// agent while the first is deleted goes on pointing at index 2 -- a different
/// agent now, which Save then overwrites. Deleting the agent being edited is
/// worse: the index falls off the end and Save appends, putting back the agent
/// the user had just asked to be rid of.
///
/// A position and not an id because an agent is identified by a name the user
/// writes and renaming one is the whole point of the form. So the rule is to
/// correct the position at the one place the list shifts, and it is pure so
/// that place can be checked without a window.
pub fn draft_shift(editing: Option<usize>, removed: usize) -> DraftShift {
    match editing {
        Some(editing) if editing == removed => DraftShift::Clear,
        Some(editing) if editing > removed => DraftShift::MoveTo(editing - 1),
        Some(_) | None => DraftShift::Keep,
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
                // Not the bundled `replace`, which is a find-and-replace mark:
                // it reads as swapping this agent for another one rather than
                // as opening it in the form below.
                .icon(Icon::new(crate::icons::Icon::SquarePen))
                .tooltip("Edit")
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
                // Not the bundled `delete`, which is the backspace *key* --
                // "erase the character behind the caret", drawn beside a button
                // that removes a saved agent for good.
                .icon(Icon::new(crate::icons::Icon::Trash))
                .tooltip("Delete")
                .on_click({
                    let shell = shell.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        shell.update(cx, |shell, cx| shell.delete_agent(idx, window, cx));
                    }
                }),
        )
}

/// A dialog's name, and the ✕ that closes it.
///
/// Both sit in the dialog's *content* rather than in its own `title` and
/// `close_button` slots, for two separate reasons that happen to share one
/// answer.
///
/// **The name.** A dialog opened from a trigger rebuilds itself from nothing on
/// every press, and what survives that is its content builder, its style and its
/// props — not its title, header or footer, which are elements and so cannot be
/// cloned into a closure that runs again on each open. A name set through the
/// slot is therefore dropped in silence on any dialog the rail opens, and the
/// content builder is the only slot left to put it in. Every dialog here goes
/// through this one, trigger or not: a rule half the call sites follow is the
/// rule the next call site forgets.
///
/// **The ✕.** The library builds its own out of a plain library button inside
/// the dialog element, so it never passes through the app's action wrapper and
/// ends up the single control on a dialog drawing the arrow cursor while
/// everything inside it answers the pointer. Turning that one off and drawing
/// ours puts it on the line that already carries the name.
///
/// It still closes through the library's own `DialogClose`, which dispatches the
/// dialog's cancel action — the same path the built-in took, so the handlers
/// that clear a half-finished rename or worktree still run. The fixed box around
/// it is what contains that element's `size_full`, which would otherwise take
/// the whole row away from the name beside it.
fn title_row(name: &'static str) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .w_full()
        .child(
            DialogTitle::new()
                .min_w_0()
                // The library sets this title's line height to exactly one em,
                // and `truncate` clips to the box -- so every descender is cut
                // off at the baseline, which is the "g" in *Settings* losing
                // its tail. The refinement lands after the library's own, so
                // asking for the room back here is enough.
                .line_height(relative(1.3))
                .truncate()
                .child(name),
        )
        .child(
            div().flex_none().size_6().child(
                DialogClose::new().child(
                    crate::controls::action("dialog-close")
                        .small()
                        .ghost()
                        .icon(Icon::new(IconName::Close)),
                ),
            ),
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

/// The agent page: the global agent menu a new session spawns from, and the
/// form that adds to it.
fn agents_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let specs = shell.agents(cx).to_vec();
    let draft = shell.agent_draft();
    let (name, command, args) = (
        draft.name.clone(),
        draft.command.clone(),
        draft.args.clone(),
    );
    let saveable = draft.to_spec(cx).is_some();
    let editing = draft.editing.is_some();

    let rows = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| agent_row(handle, i, spec, cx).into_any_element())
        .collect::<Vec<_>>();
    let (clear, save) = (handle.clone(), handle.clone());

    div()
        .v_flex()
        .gap_3()
        .w_full()
        .child(page_title("Agents"))
        .children(rows)
        .child(field("Name", &name))
        .child(field("Command", &command))
        .child(field("Args", &args))
        .child(
            div()
                .h_flex()
                .gap_2()
                .w_full()
                .child(
                    crate::controls::action("save-agent")
                        .primary()
                        // Disabled until name and command are both non-blank:
                        // an agent missing either cannot be launched.
                        .refuses(!saveable)
                        .label(if editing { "Save" } else { "Add" })
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            save.update(cx, |shell, cx| shell.save_agent_draft(window, cx));
                        }),
                )
                .child(
                    crate::controls::action("clear-agent")
                        .ghost()
                        .label(if editing { "Cancel edit" } else { "Clear" })
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            clear.update(cx, |shell, cx| shell.clear_agent_draft(window, cx));
                        }),
                ),
        )
        .into_any_element()
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

/// Which page Settings is showing.
///
/// Four pages because there are four groups of control, and three of them used
/// to be their own surface: the agent list and the keyboard table were separate
/// dialogs behind separate rail rows, so "where is that setting" had three
/// answers and which one was right depended on which row somebody remembered.
/// One dialog, one way in, and the rail's footer is one row instead of three.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum SettingsPage {
    /// Appearance first, and it is the default, because it is the only page
    /// here that changes what the app *looks* like and so the one somebody
    /// opening Settings without a specific errand came for.
    #[default]
    Appearance,
    Workspace,
    Agents,
    Shortcuts,
}

impl SettingsPage {
    pub const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Workspace,
        Self::Agents,
        Self::Shortcuts,
    ];

    /// The name in the nav and at the head of the page, which are one string on
    /// purpose: a nav that says one word and a heading that says another leaves
    /// the reader working out whether they landed where they clicked.
    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Workspace => "Workspace",
            Self::Agents => "Agents",
            Self::Shortcuts => "Shortcuts",
        }
    }
}

/// One row in the nav column.
///
/// A div and not the app's button wrapper, for the reason the rail's rows are:
/// a full-width [`gpui_component::button::Button`] centres its own content and
/// that is not style-refinable from outside, so a column of them reads as a row
/// of banners rather than as a list. No icons, unlike the rail — four words in
/// a column need no second alphabet to be told apart, and every icon added here
/// would be one chosen for a category rather than for a thing.
fn nav_row(page: SettingsPage, current: SettingsPage, handle: &Entity<Shell>, cx: &App) -> Div {
    let (accent, accent_fg, muted, radius) = (
        cx.theme().accent,
        cx.theme().accent_foreground,
        cx.theme().muted_foreground,
        cx.theme().radius,
    );
    let handle = handle.clone();
    let selected = page == current;

    div().w_full().child(
        div()
            .id(page.label())
            .w_full()
            .px_2()
            .py_1()
            .rounded(radius)
            .text_sm()
            .cursor_pointer()
            .map(|row| match selected {
                true => row.bg(accent).text_color(accent_fg),
                false => row
                    .text_color(muted)
                    .hover(move |row| row.bg(accent.opacity(0.5))),
            })
            .child(page.label())
            .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                handle.update(cx, |shell, cx| shell.show_settings_page(page, cx));
            }),
    )
}

/// The heading a page opens with -- the same word its nav row carries.
fn page_title(name: &'static str) -> impl IntoElement {
    div().text_lg().font_semibold().child(name)
}

/// How wide Settings would like to be, and how wide it is allowed to be.
///
/// Two columns need the room, but the library positions a dialog by
/// subtracting half its width from half the viewport's and never clamps, so a
/// box wider than the window starts at a negative x -- the nav column is off
/// the left edge and the ✕ off the right, with no scroll to bring either back.
/// The floor is what the app's own controls need to stay pressable; below that
/// the window is smaller than any dialog and something has to overflow.
fn width_within(window: &Window) -> gpui::Pixels {
    (window.viewport_size().width - px(48.)).clamp(px(360.), px(720.))
}

/// How tall the nav and the page are, against the same frame.
///
/// The subtraction is the room the dialog needs around this box: the title row
/// above it, the padding, and the tenth of the viewport the library drops it
/// from the top.
fn body_height(window: &Window) -> gpui::Pixels {
    (window.viewport_size().height - px(220.)).clamp(px(200.), px(420.))
}

/// Settings: appearance, this workspace, the agent menu, the keymap.
///
/// **A nav column and a page, not one scroll.** What is in here belongs to
/// three different scopes -- the theme is app-wide, the name and the storage
/// binding are this workspace's, the agent list is every workspace's -- and
/// stacked in one column the only thing saying so was a row of `text_xs`
/// labels. A page per scope is the shape that says it without a sentence.
///
/// **There is no footer, and no control here sits away from what it acts on.**
/// The pair that binds and unbinds storage was in one, three items below the
/// folder it names -- and a `.primary()` button at the foot of a settings
/// dialog reads as *Save*, while that one opens a folder picker and re-points
/// where the workspace is written.
pub fn settings(window: &Window, cx: &mut Context<Shell>) -> Dialog {
    let handle = cx.entity();

    Dialog::new(cx)
        // Clamped to the frame, because the library centres the box on the
        // viewport by subtracting half this width from half the window's -- so
        // a width wider than the window puts the left edge at a negative x, and
        // the nav column goes off the side of the screen with nothing to scroll
        // it back. Read at build time, which is the frame the trigger is
        // pressed in; the props the dialog keeps are cloned when it opens.
        .w(width_within(window))
        .trigger(crate::rail::rail_row(
            "open-settings",
            IconName::Settings,
            "Settings",
            cx,
        ))
        .close_button(false)
        .content(move |content, window: &mut Window, cx: &mut App| {
            // Read per build rather than captured once: the content of an open
            // dialog is rebuilt every frame, which is what makes the nav work
            // at all -- a page captured here would be the one that was showing
            // when the dialog opened, for as long as it stayed open.
            let current = handle.read(cx).settings_page();
            let body = body_height(window);
            let nav = SettingsPage::ALL
                .into_iter()
                .map(|page| nav_row(page, current, &handle, cx).into_any_element())
                .collect::<Vec<_>>();
            let page = match current {
                SettingsPage::Appearance => appearance_page(&handle, cx),
                SettingsPage::Workspace => workspace_page(&handle, cx),
                SettingsPage::Agents => agents_page(&handle, cx),
                SettingsPage::Shortcuts => shortcuts_page(cx),
            };

            content.child(title_row("Settings")).child(
                div()
                    .h_flex()
                    .items_start()
                    .gap_4()
                    .w_full()
                    // One height for every page, so the box does not grow to
                    // whatever the keymap table needs and shrink back on the
                    // way out -- which reads as the window jumping rather than
                    // as a page changing. Re-read each frame, unlike the width:
                    // this one is inside the content, so it does follow a
                    // window resized while the dialog is open.
                    .h(body)
                    .child(
                        div()
                            .v_flex()
                            .gap_0p5()
                            .flex_none()
                            .w(px(150.))
                            .h_full()
                            .pr_3()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .children(nav),
                    )
                    .child(
                        // The page scrolls, not the dialog: the nav has to stay
                        // reachable from the bottom of a long page, and the
                        // keymap is longer than any window this opens in.
                        div()
                            .id("settings-page")
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .pr_1()
                            .overflow_y_scroll()
                            .child(page),
                    ),
            )
        })
}

/// The appearance page. One control, and it is app-wide -- said on the page,
/// since every other page here is about one workspace and nothing else marks
/// the difference.
fn appearance_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let current = handle.read(cx).appearance(cx);
    div()
        .v_flex()
        .gap_3()
        .w_full()
        .child(page_title("Appearance"))
        .child(appearance_picker(handle, current))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Applies to every window. Following the system keeps up with it."),
        )
        .into_any_element()
}

/// The workspace page: this workspace's name and storage binding, then the two
/// ways to reach another one.
///
/// No workspace is ever *replaced* in place -- one window hosts exactly one
/// workspace, so both buttons at the end open another window, or focus the one
/// already showing that folder.
///
/// **The list of known workspaces is deliberately not here.** The rail's
/// identity row is the switcher, and it draws the same directories better: a
/// row named by its folder with the parent beside it, the one on screen checked
/// and unpickable. A second copy here printed each absolute path whole into a
/// button, uncapped, so the longer the app was used the further it pushed
/// everything else off the bottom. What stays is the binding, which has nowhere
/// else to live.
fn workspace_page(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let shell = handle.read(cx);
    let name = shell.workspace_name_input().clone();
    let storage = shell
        .storage_dir()
        .map(|dir| SharedString::from(dir.display().to_string()));
    let bound = storage.is_some();
    let (bind, unbind, new, open) = (
        handle.clone(),
        handle.clone(),
        handle.clone(),
        handle.clone(),
    );

    div()
        .v_flex()
        .gap_3()
        .w_full()
        .child(page_title("Workspace"))
        .child(field("Workspace name", &name))
        .child(
            div()
                .v_flex()
                .gap_1()
                .w_full()
                .child(div().text_xs().child("Storage folder"))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(storage.unwrap_or_else(|| {
                            // An unbound workspace persists nothing; say so
                            // rather than showing a blank.
                            SharedString::from("Not bound — nothing is saved")
                        })),
                )
                .child(
                    div()
                        .h_flex()
                        .gap_2()
                        .w_full()
                        .child(
                            crate::controls::action("bind-storage")
                                .primary()
                                .label("Choose folder…")
                                .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                    bind.update(cx, |shell, cx| shell.pick_storage_dir(cx));
                                }),
                        )
                        .child(
                            crate::controls::action("unbind-storage")
                                .ghost()
                                .refuses(!bound)
                                .label("Unbind")
                                .on_click(
                                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                                        unbind.update(cx, |shell, cx| {
                                            shell.unbind_storage(window, cx)
                                        });
                                    },
                                ),
                        ),
                ),
        )
        .child(div().text_xs().child("Workspaces"))
        .child(
            div()
                .h_flex()
                .gap_2()
                .w_full()
                .child(
                    crate::controls::action("new-workspace")
                        .ghost()
                        .label("New workspace…")
                        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            new.update(cx, |shell, cx| shell.new_workspace(cx));
                        }),
                )
                .child(
                    crate::controls::action("open-workspace")
                        .ghost()
                        .label("Open workspace…")
                        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            open.update(cx, |shell, cx| shell.open_workspace(cx));
                        }),
                ),
        )
        .into_any_element()
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
        .close_button(false)
        .content(move |content, _, _: &mut App| {
            content
                .child(title_row("Rename conversation"))
                .child(div().v_flex().gap_1().w_full().child(Input::new(&input)))
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
        .close_button(false)
        .content(move |content, _, _: &mut App| {
            content.child(title_row("New worktree")).child(
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
                        .refuses(busy)
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
                        .refuses(busy)
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
/// One binding is absent on purpose: `Ctrl+Shift+P`, because the command
/// palette is a feature — a command registry plus a filtered popup — and not a
/// keymap entry.
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
        label: "Ctrl+Shift+M",
        what: "Workbench — Markdown",
        keys: &["ctrl-shift-m"],
    },
    Shortcut {
        label: "Ctrl+Shift+N",
        what: "Workbench — Neovim",
        keys: &["ctrl-shift-n"],
    },
    Shortcut {
        label: "Ctrl+`",
        what: "Toggle the terminal",
        keys: &["ctrl-`"],
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
        what: "Walk the composer's completion list",
        keys: &["up", "down"],
    },
    Shortcut {
        label: "Tab",
        what: "Take the highlighted row (Enter does too)",
        keys: &["tab"],
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

/// The keymap, and the build that draws it.
///
/// The version rides at the foot of this page rather than on an *About* page of
/// its own: one line is not a page, and a nav entry leading to one line is a
/// click that answers less than the row promised.
fn shortcuts_page(cx: &App) -> AnyElement {
    div()
        .v_flex()
        .gap_3()
        .w_full()
        .child(page_title("Shortcuts"))
        .child(
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
                                    .flex_none()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(shortcut.label),
                            )
                    })
                    .collect::<Vec<_>>(),
            ),
        )
        // The build, where somebody who never opens a terminal can read it.
        // `onehand --version` answers the same question for everybody else.
        .child(
            div()
                .pt_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("onehand {}", env!("CARGO_PKG_VERSION"))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{DraftShift, SHORTCUTS, draft_shift};

    /// Deleting an agent moves a form open on another one with it.
    ///
    /// Every case here is a save that would otherwise land on the wrong agent:
    /// the list shifts under a position the form is still holding.
    #[test]
    fn deleting_an_agent_moves_the_form_off_the_hole() {
        assert_eq!(draft_shift(None, 0), DraftShift::Keep);
        // Deleted below the form: nothing the form points at has moved.
        assert_eq!(draft_shift(Some(0), 2), DraftShift::Keep);
        // Deleted above it: everything after the hole slid down one.
        assert_eq!(draft_shift(Some(2), 0), DraftShift::MoveTo(1));
        assert_eq!(draft_shift(Some(1), 0), DraftShift::MoveTo(0));
        // The agent being edited is the one that went.
        assert_eq!(draft_shift(Some(1), 1), DraftShift::Clear);
    }

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
        for (n, line) in include_str!("dialogs.rs").lines().enumerate() {
            assert!(
                !line.contains(&slot),
                "dialogs.rs:{}: names the dialog through the library's title \
                 slot, which a triggered dialog drops on its way back open. \
                 Put the name in the content instead.\n    {}",
                n + 1,
                line.trim()
            );
        }
    }

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
            // A binding whose action is `NoAction` is the opposite of a
            // shortcut: it exists to take a key away from a binding made
            // somewhere else, so what the user gets is the key doing whatever
            // it would have done with no keymap at all. A row for it would
            // teach a command that does not exist.
            .filter(|rest| {
                !rest
                    .split(')')
                    .next()
                    .is_some_and(|call| call.contains("NoAction"))
            })
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
