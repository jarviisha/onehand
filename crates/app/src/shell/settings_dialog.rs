use super::Shell;
use super::confirm::Ask;
use crate::settings::{AgentCheck, AgentDraft, DraftShift, SettingsPage};
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity, ParentElement, Window, WindowAppearance};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::{DialogClose, DialogFooter};
use gpui_component::notification::Notification;
use gpui_component::{Theme, ThemeMode, WindowExt as _};
use onehand_core::config::{AgentSpec, AppConfig, Appearance};

impl Shell {
    // ── Agents ──────────────────────────────────────────────────────────────

    /// The global agent menu. Definitions are process-wide; a session keeps a
    /// clone of the spec it was spawned with.
    pub fn agents<'a>(&self, cx: &'a App) -> &'a [AgentSpec] {
        &Shared::global(cx).agents
    }

    pub fn agent_draft(&self) -> &AgentDraft {
        &self.agent_draft
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_return_focus = window.focused(cx);
        self.held_commands.clear();
        self.settings_open = true;
        self.settings_note = None;
        self.keymap_editor
            .update(cx, |editor, _| editor.forget_note());
        self.settings_focus.focus(window, cx);
        cx.notify();
    }

    /// Close Settings, unless that would throw away something not saved.
    ///
    /// An agent form or a shortcut left mid-edit is asked about first, and only
    /// then: a question on every close teaches the user to click through it,
    /// and closing with nothing pending has nothing to lose.
    pub fn request_close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pending = self.agent_draft.dirty(&Shared::global(cx).agents, cx)
            || self.keymap_editor.read(cx).dirty(cx);
        if !pending {
            self.close_settings(window, cx);
            return;
        }
        let shell = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let shell = shell.clone();
            alert
                .title("Discard unsaved changes?")
                .description(
                    "An agent or a shortcut is still being edited. \
                     Closing Settings now throws those changes away.",
                )
                // Ours rather than the library's default pair, for the reason
                // every button in this app is ours: the library draws its own
                // with the arrow cursor.
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(
                                crate::controls::action("keep-settings-edit")
                                    .ghost()
                                    .label("Keep editing"),
                            ),
                        )
                        .child(
                            crate::controls::action("discard-settings-edit")
                                .danger()
                                .label("Discard")
                                .on_click(move |_, window: &mut Window, cx: &mut App| {
                                    window.close_dialog(cx);
                                    shell.update(cx, |shell: &mut Self, cx| {
                                        shell.agent_draft.clear(window, cx);
                                        shell.keymap_editor.update(cx, |editor, cx| {
                                            if editor.editing() {
                                                editor.cancel(window, cx);
                                            }
                                        });
                                        shell.close_settings(window, cx);
                                    });
                                }),
                        ),
                )
        });
    }

    /// Whether Settings is on screen.
    pub(crate) fn settings_shown(&self) -> bool {
        self.settings_open
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_open {
            return;
        }
        self.settings_open = false;
        // An unbound workspace writes nothing, so a flag set there is never
        // taken; it must not outlive the Settings that set it.
        self.workspace_note_wanted = false;
        if let Some(focus) = self.settings_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.chat
                .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
        }
        cx.notify();
    }

    /// The word Settings shows beside its ✕ for `page`. The shortcut editor
    /// writes the keymap itself, so it keeps its own; every other page's
    /// writes come through [`Shell::report_write`].
    pub fn settings_note(&self, page: SettingsPage, cx: &App) -> Option<Result<(), String>> {
        match page {
            SettingsPage::Shortcuts => self.keymap_editor.read(cx).note(),
            _ => self.settings_note.clone(),
        }
    }

    /// Say how a write went: a failure as a notification wherever it came
    /// from, and -- when `noted`, which is to say Settings asked for it -- the
    /// outcome either way, for Settings to show beside its ✕.
    pub(super) fn report_write<E: std::fmt::Display>(
        &mut self,
        what: &str,
        saved: Result<(), E>,
        noted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = saved.map_err(|e| e.to_string());
        if let Err(e) = &saved {
            window.push_notification(Notification::error(format!("{what} not saved — {e}")), cx);
        }
        if noted && self.settings_open {
            self.settings_note = Some(saved);
        }
    }

    pub fn settings_focus(&self) -> gpui::FocusHandle {
        self.settings_focus.clone()
    }

    pub fn keymap_editor(&self) -> Entity<crate::keymap::Editor> {
        self.keymap_editor.clone()
    }

    pub fn settings_page(&self) -> SettingsPage {
        self.settings_page
    }

    pub fn show_settings_page(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        self.settings_page = page;
        // A word about the last write belongs to the page it was made on, and
        // so does a write still waiting on its debounce.
        self.settings_note = None;
        self.workspace_note_wanted = false;
        self.keymap_editor
            .update(cx, |editor, _| editor.forget_note());
        cx.notify();
    }

    pub fn edit_agent(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spec) = Shared::global(cx).agents.get(idx).cloned() else {
            return;
        };
        self.agent_draft.load(idx, &spec, window, cx);
        cx.notify();
    }

    pub fn clear_agent_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agent_draft.clear(window, cx);
        cx.notify();
    }

    /// Ask, then remove agent `idx`.
    pub(crate) fn confirm_delete_agent(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(spec) = cx.global::<Shared>().agents.get(idx).cloned() else {
            return;
        };
        let name = spec.name.clone();
        let ask = Ask {
            id: "delete-agent",
            title: format!("Delete {name}?").into(),
            description: "It leaves the agent list. Sessions already running on it carry on."
                .into(),
            act: "Delete",
            ..Default::default()
        };
        // By the whole spec, not by position: the list can be reordered while
        // the question is open, and two agents may share a name.
        self.ask(ask, window, cx, move |shell, window, cx| {
            let at = cx.global::<Shared>().agents.iter().position(|a| *a == spec);
            if let Some(idx) = at {
                shell.delete_agent(idx, window, cx);
            }
        });
    }

    /// Remove an agent, and move the form off the hole it leaves.
    ///
    /// The rule is `settings::draft_shift`, which says why a position left
    /// uncorrected here is a form that saves over the wrong agent.
    fn delete_agent(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.update_global::<Shared, _>(|shared, _| {
            if idx < shared.agents.len() {
                shared.agents.remove(idx);
            }
        });
        match crate::settings::draft_shift(self.agent_draft.editing, idx) {
            DraftShift::Keep => {}
            // The fields go too, not just the index: left filled under a button
            // that now reads "Add", the form offers to recreate the agent the
            // user has just deleted.
            DraftShift::Clear => self.agent_draft.clear(window, cx),
            DraftShift::MoveTo(editing) => self.agent_draft.editing = Some(editing),
        }
        self.persist_agents(window, cx);
        cx.notify();
    }

    /// Move an agent to the front of the list, which is what makes it the one
    /// *New session* starts.
    pub fn make_default_agent(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let moved = cx.update_global::<Shared, _>(|shared, _| {
            if idx == 0 || idx >= shared.agents.len() {
                return false;
            }
            let spec = shared.agents.remove(idx);
            shared.agents.insert(0, spec);
            true
        });
        if !moved {
            return;
        }
        self.agent_draft.editing =
            crate::settings::draft_after_promote(self.agent_draft.editing, idx);
        self.persist_agents(window, cx);
        cx.notify();
    }

    /// What the last *Test* filed under `key` found, if there was one.
    pub fn agent_check(&self, key: &str) -> Option<&AgentCheck> {
        self.agent_checks.get(key)
    }

    /// Look for an agent's command the way starting a session would, off the UI
    /// loop, without starting anything.
    pub fn check_agent(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(spec) = Shared::global(cx).agents.get(idx).cloned() else {
            return;
        };
        let (key, command) = (crate::settings::check_key(&spec), spec.command);
        // A session starts its agent in the project's root, so a relative
        // command is looked for from there: the project on screen is the one
        // the next *New session* would start in.
        let root = self
            .window
            .workspace
            .active_root()
            .map(|root| root.path.clone());
        self.agent_checks.insert(key.clone(), AgentCheck::Running);
        cx.notify();
        cx.spawn(async move |shell, cx| {
            let found = cx
                .background_executor()
                .spawn({
                    let command = command.clone();
                    async move {
                        let path = std::env::var_os("PATH");
                        onehand_core::config::find_command(
                            &command,
                            path.as_deref(),
                            root.as_deref(),
                        )
                    }
                })
                .await;
            shell
                .update(cx, |shell, cx| {
                    let check = match found {
                        Some(at) => AgentCheck::Found(at),
                        None => AgentCheck::Missing,
                    };
                    shell.agent_checks.insert(key, check);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Commit the form. Adds or replaces depending on `editing`.
    pub fn save_agent_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spec) = self.agent_draft.to_spec(cx) else {
            return;
        };
        let editing = self.agent_draft.editing;
        cx.update_global::<Shared, _>(|shared, _| match editing {
            Some(idx) if idx < shared.agents.len() => shared.agents[idx] = spec,
            _ => shared.agents.push(spec),
        });
        self.agent_draft.clear(window, cx);
        self.persist_agents(window, cx);
        cx.notify();
    }

    // ── Appearance ──────────────────────────────────────────────────────────

    /// Which mode the user has chosen. App-wide, like the theme it selects.
    pub fn appearance(&self, cx: &App) -> Appearance {
        Shared::global(cx).appearance
    }

    /// Change it, put it on screen, and remember it.
    ///
    /// Saved to the same file the agent list is in, through the edit-in-place
    /// path that keeps every other section — so choosing a mode never costs
    /// somebody their agents. A failed write still leaves the choice showing:
    /// it is what the user asked for, and saying so beats reverting the screen
    /// under them.
    pub fn set_appearance(
        &mut self,
        choice: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if Shared::global(cx).appearance == choice {
            return;
        }
        cx.update_global::<Shared, _>(|shared, _| shared.appearance = choice);
        apply_appearance(choice, Some(window), cx);

        let path = Shared::global(cx).config_path.clone();
        let saved = AppConfig::update_in_place(&path, |cfg| cfg.appearance = choice);
        self.report_write("Appearance", saved, true, window, cx);
        cx.notify();
    }

    /// Write the agent list back to the file the config was loaded from,
    /// preserving every other section.
    fn persist_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = Shared::global(cx);
        let (path, agents) = (shared.config_path.clone(), shared.agents.clone());
        let saved = AppConfig::update_in_place(&path, |cfg| cfg.agents = agents);
        self.report_write("Agents", saved, true, window, cx);
    }
}

/// Draw everything in the chosen mode.
///
/// The component library ships both palettes and switching between them is one
/// call; what this adds is the three-way choice around it. *System* is resolved
/// here rather than stored, so the answer is whatever the desktop says at the
/// moment it is asked — the window's own reading when there is a window, since
/// the platform's app-wide value can still be its default while the desktop is
/// being queried in the background.
///
/// Two things have to be repaired afterwards. The theme carries the monospace
/// family, which the boot scan resolved against what is installed, and applying
/// a mode re-applies a whole theme config over it. And a mode is global while a
/// refresh is per window, so every other window would keep painting the old
/// palette until something else happened to make it redraw.
///
/// Telling the platform as well is what keeps a window's native border and
/// title bar in step with a forced mode; it does nothing on Linux, and passing
/// `None` for *System* is what hands tracking back to the desktop.
pub(super) fn apply_appearance(choice: Appearance, window: Option<&mut Window>, cx: &mut App) {
    let system = window
        .as_ref()
        .map(|window| window.appearance())
        .unwrap_or_else(|| cx.window_appearance());
    let mode = match choice {
        Appearance::System => ThemeMode::from(system),
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    };

    cx.set_window_appearance(match choice {
        Appearance::System => None,
        Appearance::Light => Some(WindowAppearance::Light),
        Appearance::Dark => Some(WindowAppearance::Dark),
    });
    Theme::change(mode, window, cx);

    // Changing the mode swaps in the whole theme config, which carries the
    // library's own monospace family; put back the one this machine resolved.
    if let Some(family) = Shared::global(cx).mono_family.clone() {
        Theme::global_mut(cx).mono_font_family = family.into();
    }
    // **A dock draws no divider; the thing inside it draws its own edge.**
    //
    // The library paints a permanent 1px rule in the hairline colour down the
    // seam of every resizable split — between the rail and the docks, and
    // between the conversation and each dock. Every panel on the other side of
    // one of those seams already marks it: the Workbench is a change of surface,
    // the terminal draws a hairline along its top, and the rail is a change of
    // surface too — the reading surface against the well, a pair the ramp's own
    // tests hold at 1.14 or better in either palette, which is what makes a
    // fill an edge rather than a tint. So
    // the rule was a second line beside a first, which reads as a seam that
    // could not decide where it was.
    //
    // Only the *resting* colour goes. Dragging still paints `active_handle`,
    // which is the one moment the seam is the thing being looked at.
    //
    // It has to be written here, after `Theme::change`: that call rebuilds the
    // Base layer's copy of the theme from scratch, so anything written onto it
    // beforehand is thrown away. This is the single place a mode is applied, at
    // boot and on every change, which is what keeps one write enough.
    gpui_base::Theme::global_mut(cx).resizable.handle = gpui::transparent_black();
    // The code editor paints its line numbers on an opaque gutter, in the
    // field's own background when the syntax theme names none: the reading
    // surface, a dark band down a dock that is a step lighter. The only editor
    // with line numbers is the Workbench's, so the gutter takes the dock's
    // surface. Written here for the reason the handle is.
    let theme = gpui_component::Theme::global_mut(cx);
    let mut highlight = (*theme.highlight_theme).clone();
    highlight.style.editor_gutter_background = Some(theme.tiles);
    theme.highlight_theme = std::sync::Arc::new(highlight);
    cx.refresh_windows();
}
