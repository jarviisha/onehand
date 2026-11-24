use super::Shell;
use super::confirm::Confirm;
use crate::settings::{AgentDraft, DraftShift, HostDraft, SettingsPage};
use crate::state::Shared;
use gpui::{App, BorrowAppContext, Context, Entity, Focusable as _, Window, WindowAppearance};
use gpui_component::WindowExt as _;
use gpui_component::{Theme, ThemeMode};
use onehand_core::config::{AgentEdit, AgentSpec, Appearance};
use onehand_core::workspace::HostName;

impl Shell {
    // ── Agents ──────────────────────────────────────────────────────────────

    /// The global agent menu. Definitions are process-wide; a session keeps a
    /// clone of the spec it was spawned with.
    pub fn agents<'a>(&self, cx: &'a App) -> &'a [AgentSpec] {
        &Shared::global(cx).settings.agents
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
        if !self.settings_shown(cx) {
            self.settings_from = self.shown_page(cx);
        }
        self.held_commands.clear();
        // The Projects page draws each project's check command field, made
        // here since a page cannot make one while it draws.
        self.make_check_inputs(window, cx);
        self.read_route_suggestions(cx);
        self.docks_aside(window, cx);
        let view: gpui::AnyView = self.settings_view.clone().into();
        self.chat.update(cx, |pane, cx| {
            pane.show_page(crate::chat::pane::PageKind::Settings, view, window, cx)
        });
        self.settings_focus.focus(window, cx);
        cx.notify();
    }

    /// Open Settings, or close it and go back to what it was opened over: a
    /// page, else the session or project page it covered.
    pub fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_shown(cx) {
            return self.open_settings(window, cx);
        }
        use crate::chat::pane::PageKind;
        match self.settings_from.take() {
            Some(PageKind::Workspace) => self.show_workspace(window, cx),
            Some(PageKind::Tasks) => self.show_tasks(None, window, cx),
            Some(PageKind::Issues) => self.show_issues(window, cx),
            Some(PageKind::Workflows) => self.show_workflows(window, cx),
            Some(PageKind::Schedules) => self.show_schedules(None, window, cx),
            Some(PageKind::Settings) | None => self.show_active_session(window, cx),
        }
        cx.notify();
    }

    /// Whether Settings is what the centre of the window shows.
    pub(crate) fn settings_shown(&self, cx: &App) -> bool {
        self.shown_page(cx) == Some(crate::chat::pane::PageKind::Settings)
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

    /// The project the Projects page shows: the one picked while the window
    /// still offers it, else the first it offers.
    pub(crate) fn settings_project(&self, _cx: &App) -> Option<std::path::PathBuf> {
        let offered = crate::settings::offered_projects(&self.window.workspace.roots);
        crate::settings::shown_project(self.settings_project.as_deref(), &offered)
    }

    /// Show `root` on the Projects page.
    pub(crate) fn pick_settings_project(
        &mut self,
        root: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.make_check_inputs(window, cx);
        self.settings_project = Some(root);
        self.read_route_suggestions(cx);
        cx.notify();
    }

    /// Open Settings on its Projects page, showing the project at `root`.
    pub(crate) fn show_project_settings(
        &mut self,
        root: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pick_settings_project(root, window, cx);
        self.settings_page = SettingsPage::Projects;
        self.open_settings(window, cx);
    }

    /// [`Self::show_project_settings`] for the project at row `root_idx`.
    pub(crate) fn project_settings_of(
        &mut self,
        root_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(root) = self.local_root(root_idx).map(std::path::Path::to_path_buf) {
            self.show_project_settings(root, window, cx);
        }
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
        let spec = idx.and_then(|idx| Shared::global(cx).settings.agents.get(idx).cloned());
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
        let Some(spec) = cx.global::<Shared>().settings.agents.get(idx).cloned() else {
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
            let at = cx
                .global::<Shared>()
                .settings
                .agents
                .iter()
                .position(|a| *a == spec);
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
        let Some(spec) = Shared::global(cx).settings.agents.get(idx).cloned() else {
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
        let Some(spec) = Shared::global(cx).settings.agents.get(idx).cloned() else {
            return;
        };
        self.agent_draft.editing =
            crate::settings::draft_after_promote(self.agent_draft.editing, idx);
        self.persist_agents(AgentEdit::Promote(spec), window, cx);
        cx.notify();
    }

    /// *Test* an agent's command: look for it the way starting a session
    /// would, off the UI loop, without starting anything.
    pub fn check_agent(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(spec) = Shared::global(cx).settings.agents.get(idx).cloned() else {
            return;
        };
        // A session starts its agent in the project's root, so a relative
        // command is looked for from there: the project on screen is the one
        // the next *New session* would start in. A project on a host gives
        // none: its folder names nothing on this machine.
        let root = self
            .local_root(self.window.workspace.active_root)
            .map(std::path::Path::to_path_buf);
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.settings.test(spec, root));
        crate::saves::carry_quietly(fx, cx);
    }

    /// Commit the form. Adds or replaces depending on `editing`.
    pub fn save_agent_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(spec) = self.agent_draft.to_spec(cx) else {
            return;
        };
        let old = self
            .agent_draft
            .editing
            .and_then(|idx| Shared::global(cx).settings.agents.get(idx).cloned());
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
        let new = AgentSpec::of(preset, &Shared::global(cx).settings.agents);
        self.persist_agents(AgentEdit::Put { old: None, new }, window, cx);
        cx.notify();
    }

    // ── Appearance ──────────────────────────────────────────────────────────

    /// Which mode the user has chosen. App-wide, like the theme it selects.
    pub fn appearance(&self, cx: &App) -> Appearance {
        Shared::global(cx).settings.appearance
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
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.settings.set_appearance(choice));
        crate::saves::carry_settings(fx, window, cx);
        cx.notify();
    }

    // ── Hosts ───────────────────────────────────────────────────────────────

    pub fn host_draft(&self) -> &HostDraft {
        &self.host_draft
    }

    pub(crate) fn host_draft_mut(&mut self) -> &mut HostDraft {
        &mut self.host_draft
    }

    /// Each host this window's projects use, to run on or to build on, with
    /// the project's name.
    pub(crate) fn host_users(&self) -> Vec<(HostName, String)> {
        self.window
            .workspace
            .roots
            .iter()
            .flat_map(|root| {
                root.hosts_used()
                    .into_iter()
                    .map(|host| (host.clone(), root.label.clone()))
            })
            .collect()
    }

    /// Put a host picked from the menu into the field.
    pub(crate) fn pick_declared_host(
        &mut self,
        host: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host_draft
            .name
            .update(cx, |input, cx| input.set_value(host, window, cx));
        cx.notify();
    }

    /// *Add* the host the field names: probe it, and declare what the probe
    /// finds. The field is emptied at once, and the probe said under it.
    pub(crate) fn add_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.host_draft.name.read(cx).value().to_string();
        let declared = &Shared::global(cx).settings.hosts;
        if onehand_core::config::add_refusal(&typed, declared).is_some() {
            return;
        }
        let host = HostName(typed.trim().to_string());
        self.host_draft.adding = Some(host.clone());
        self.host_draft
            .name
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.check_host(host, window, cx);
    }

    /// *Check* a host: ask it what it is and what it has, off the UI loop.
    pub(crate) fn check_host(
        &mut self,
        host: HostName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.settings.probe_host(host));
        crate::saves::carry_settings(fx, window, cx);
    }

    /// Ask before a declared host leaves the list. One a project still names
    /// is not asked about: its *Remove* refuses, saying which.
    pub(crate) fn confirm_remove_host(
        &mut self,
        host: HostName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = Confirm {
            id: "remove-host",
            title: format!("Remove {host}?").into(),
            description: "It leaves the host list. Nothing on the host itself is touched.".into(),
            act: "Remove",
            ..Default::default()
        };
        self.confirm(ask, window, cx, move |_, window, cx| {
            // Asked of the drive with who uses the host now, not when the
            // question was put: a project may have named it meanwhile.
            let users: Vec<String> = crate::settings::hosts::host_users(cx)
                .into_iter()
                .find(|(used, _)| *used == host)
                .map(|(_, users)| users)
                .unwrap_or_default();
            let host = host.clone();
            let removed = cx
                .update_global::<Shared, _>(|shared, _| shared.settings.remove_host(host, &users));
            match removed {
                Ok(fx) => crate::saves::carry_settings(fx, window, cx),
                Err(why) => window.push_notification(crate::toast::warning(why), cx),
            }
        });
    }

    /// Make `edit` to the agent list on screen, then to the one in the file
    /// the config was loaded from, preserving every other section and any
    /// entry written there by hand since.
    fn persist_agents(&mut self, edit: AgentEdit, window: &mut Window, cx: &mut Context<Self>) {
        // Off the UI thread and in order; the form already waited on its
        // press, so a failure is said and the list stands.
        let fx = cx.update_global::<Shared, _>(|shared, _| shared.settings.edit_agents(edit));
        crate::saves::carry_settings(fx, window, cx);
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
pub(crate) fn apply_appearance(choice: Appearance, window: Option<&mut Window>, cx: &mut App) {
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
    let gutter = onehand_ui::dock_surface(cx);
    Theme::update(cx, |theme| {
        // The config carries the library's own monospace family; put back
        // the one this machine resolved.
        if let Some(family) = mono {
            theme.mono_font_family = family.into();
        }
        // The code editor paints its line numbers on an opaque gutter, in
        // whatever the syntax theme names. The only editor with line numbers
        // is the Workbench's, so the gutter takes the dock's surface, the
        // box's own: any other fill is a band down the box beside the code,
        // and squares the box's corner at its foot. Its numbers are a margin read only when
        // looked for, so they rest in the muted ink; the caret's line keeps
        // the full one, so where the caret is still reads down the column.
        let mut highlight = (*theme.highlight_theme).clone();
        highlight.style.editor_gutter_background = Some(gutter);
        highlight.style.editor_line_number = Some(theme.muted_foreground);
        highlight.style.editor_active_line_number = Some(theme.foreground);
        theme.highlight_theme = std::sync::Arc::new(highlight);
    });
}

#[cfg(test)]
mod tests;
