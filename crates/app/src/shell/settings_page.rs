use super::Shell;
use super::confirm::Confirm;
use crate::settings::{AgentCheck, AgentDraft, DraftShift, SettingsPage};
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity, Focusable as _, Window, WindowAppearance};
use gpui_component::{Theme, ThemeMode};
use onehand_core::config::{AgentEdit, AgentSpec, Appearance};

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

    /// Show Settings in the agent pane, on the page it was left on, with the
    /// keyboard on its nav. The docks go aside as for every page.
    ///
    /// **A page, so nothing guards leaving it.** What it holds saves as it
    /// changes; the agent form is a dialog, so the page is not left while it
    /// is open, and a shortcut being edited is the shell's, kept until its own
    /// *Save* or *Cancel*: leave and come back, and it is as it was.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.held_commands.clear();
        self.docks_aside(window, cx);
        let view: gpui::AnyView = self.settings_view.clone().into();
        self.chat
            .update(cx, |pane, cx| pane.show_settings(view, window, cx));
        self.settings_focus.focus(window, cx);
        cx.notify();
    }

    /// Whether Settings is what the centre of the window shows.
    pub(crate) fn settings_shown(&self, cx: &App) -> bool {
        self.chat.read(cx).showing_settings()
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
        cx.notify();
    }

    /// Open the agent form on agent `idx`, or empty for a new one.
    pub fn open_agent_form(
        &mut self,
        idx: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = idx.and_then(|idx| Shared::global(cx).agents.get(idx).cloned());
        self.remember_focus(window, cx);
        match (idx, spec) {
            (Some(idx), Some(spec)) => self.agent_draft.load(idx, &spec, window, cx),
            (Some(_), None) => return,
            (None, _) => self.agent_draft.clear(window, cx),
        }
        self.agent_form = true;
        self.agent_draft.name.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Put the agent form away, its fields with it.
    pub fn close_agent_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agent_form = false;
        self.agent_draft.clear(window, cx);
        cx.notify();
    }

    /// Make the form describe a built-in agent, or one written out in full.
    pub fn set_agent_kind(
        &mut self,
        kind: Option<onehand_core::config::Preset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_draft.set_kind(kind, window, cx);
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
        let ask = Confirm {
            id: "delete-agent",
            title: format!("Delete {name}?").into(),
            description: "It leaves the agent list. Sessions already running on it carry on."
                .into(),
            act: "Delete",
            ..Default::default()
        };
        // By the whole spec, not by position: the list can be reordered while
        // the question is open, and two agents may share a name.
        self.confirm(ask, window, cx, move |shell, window, cx| {
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
        let Some(spec) = Shared::global(cx).agents.get(idx).cloned() else {
            return;
        };
        match crate::settings::draft_shift(self.agent_draft.editing, idx) {
            DraftShift::Keep => {}
            // The fields go too, not just the index: left filled under a button
            // that now reads "Add", the form offers to recreate the agent the
            // user has just deleted.
            DraftShift::Clear => self.close_agent_form(window, cx),
            DraftShift::MoveTo(editing) => self.agent_draft.editing = Some(editing),
        }
        self.persist_agents(AgentEdit::Remove(spec), window, cx);
        cx.notify();
    }

    /// Move an agent to the front of the list, which is what makes it the one
    /// *New session* starts.
    pub fn make_default_agent(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx == 0 {
            return;
        }
        let Some(spec) = Shared::global(cx).agents.get(idx).cloned() else {
            return;
        };
        self.agent_draft.editing =
            crate::settings::draft_after_promote(self.agent_draft.editing, idx);
        self.persist_agents(AgentEdit::Promote(spec), window, cx);
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
        let key = crate::settings::check_key(&spec);
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
                .spawn(async move {
                    let path = std::env::var_os("PATH");
                    match spec.preset() {
                        // A built-in agent is its binary, looked for where a
                        // start looks for it, and asked its version.
                        Some(preset) => onehand_core::config::find_command(
                            preset.info().binary,
                            path.as_deref(),
                            None,
                        )
                        .map(|at| {
                            let version = version_of(&at);
                            (at, version)
                        }),
                        None => onehand_core::config::find_command(
                            &spec.command,
                            path.as_deref(),
                            root.as_deref(),
                        )
                        .map(|at| (at, None)),
                    }
                })
                .await;
            shell
                .update(cx, |shell, cx| {
                    let check = match found {
                        Some((at, version)) => AgentCheck::Found(at, version),
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
        let Ok(spec) = self.agent_draft.to_spec(cx) else {
            return;
        };
        let old = self
            .agent_draft
            .editing
            .and_then(|idx| Shared::global(cx).agents.get(idx).cloned());
        self.close_agent_form(window, cx);
        self.persist_agents(AgentEdit::Put { old, new: spec }, window, cx);
        cx.notify();
    }

    /// Add `preset`, offered because it is on this machine with no entry yet,
    /// under its own name or the first free one.
    pub fn add_found_agent(
        &mut self,
        preset: onehand_core::config::Preset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new = AgentSpec::of(preset, &Shared::global(cx).agents);
        self.persist_agents(AgentEdit::Put { old: None, new }, window, cx);
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
    /// somebody their agents. Shown at once and written after; a write that
    /// fails puts the mode back, as every change to a settings file does.
    pub fn set_appearance(
        &mut self,
        choice: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = Shared::global(cx).appearance;
        if before == choice {
            return;
        }
        cx.update_global::<Shared, _>(|shared, _| shared.appearance = choice);
        apply_appearance(choice, Some(window), cx);
        crate::saves::write_config(
            "appearance",
            "Saving the appearance",
            move |cfg| cfg.appearance = choice,
            Some(crate::saves::Change {
                before,
                after: choice,
                put: |choice, window, cx| {
                    cx.update_global::<Shared, _>(|shared, _| shared.appearance = choice);
                    apply_appearance(choice, Some(window), cx);
                },
            }),
            window,
            cx,
        );
        cx.notify();
    }

    /// Make `edit` to the agent list on screen, then to the one in the file
    /// the config was loaded from, preserving every other section and any
    /// entry written there by hand since.
    fn persist_agents(&mut self, edit: AgentEdit, window: &mut Window, cx: &mut Context<Self>) {
        cx.update_global::<Shared, _>(|shared, _| edit.apply(&mut shared.agents));
        // Off the UI thread and in order; the form already waited on its
        // press, so a failure is said and the list stands.
        crate::saves::write_config(
            "agents",
            "Saving the agents",
            move |cfg| edit.apply(&mut cfg.agents),
            None::<crate::saves::Change<()>>,
            window,
            cx,
        );
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
/// One thing has to be repaired afterwards: the theme carries the monospace
/// family, which the boot scan resolved against what is installed, and applying
/// a mode re-applies a whole theme config over it.
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

    // Written after `Theme::change`, because that call rebuilds the theme from
    // its config and drops anything written before it, and through
    // `Theme::update`, which hands the result on to the layer under the
    // library: rendered Markdown reads its monospace family there, so a
    // family written straight into the global never reached a code block.
    let mono = Shared::global(cx).mono_family.clone();
    Theme::update(cx, |theme| {
        // The config carries the library's own monospace family; put back
        // the one this machine resolved.
        if let Some(family) = mono {
            theme.mono_font_family = family.into();
        }
        // The code editor paints its line numbers on an opaque gutter, in the
        // field's own background when the syntax theme names none: the
        // reading surface, a dark band down a dock that is a step lighter.
        // The only editor with line numbers is the Workbench's, so the gutter
        // takes the dock's surface. Its numbers are a margin read only when
        // looked for, so they rest in the muted ink; the caret's line keeps
        // the full one, so where the caret is still reads down the column.
        let mut highlight = (*theme.highlight_theme).clone();
        highlight.style.editor_gutter_background = Some(theme.sidebar);
        highlight.style.editor_line_number = Some(theme.muted_foreground);
        highlight.style.editor_active_line_number = Some(theme.foreground);
        theme.highlight_theme = std::sync::Arc::new(highlight);
    });
}

/// The first line `<binary> --version` prints, when it prints one.
fn version_of(binary: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}
