//! The window's root view.
//!
//! The rail plus a `DockArea`: what the window is made of, and everything that
//! is about the window rather than about one panel.

use crate::chat::ChatPane;
use crate::settings::{AgentDraft, HostDraft, SettingsPage};
use crate::state::WorkspaceWindow;
use crate::terminal::TerminalPanel;
use crate::workbench::Workbench;
use gpui::{App, Entity, SharedString};
use gpui_component::ResizableState;
use gpui_component::dock::DockArea;
use gpui_component::input::InputState;
use onehand_core::workspace::HostName;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

mod board_settings;
mod boot;
mod build_hosts;
mod carry;
mod check_commands;
mod cleanup;
mod confirm;
mod conversation;
mod routes;
pub(crate) use confirm::{Confirm, confirm_on};
mod dock_rules;
mod dock_skin;
mod docks;
mod drafts;
mod fork;
mod frame;
mod git_sweep;
mod host_end;
mod host_project;
pub(crate) use host_project::{Browse, Way};
pub(crate) use offload::offload_refusal;
mod issue_work;
mod issues_page;
mod off_screen;
mod offload;
mod page_start;
mod pages;
mod presentation;
mod render;
mod retry;
#[cfg(test)]
pub(crate) mod rig;
mod roots;
mod sessions;
mod settings_page;
pub(crate) use settings_page::apply_appearance;
mod storage;
mod workbench_slot;
mod workers;
mod workflows;
mod worktrees;
pub use crate::instance::hold_or_forward;
pub use boot::{boot, open_or_focus, seed_workspace};
pub use drafts::{BranchDraft, Draft, WorktreeDraft};
pub use workflows::WorkflowLauncher;
pub(crate) use workflows::{brief, first_agent};

/// Whether the window may close now; it may ask first and close later itself.
type CloseGuard = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut gpui::App) -> bool>;

gpui::actions!(
    onehand,
    [
        ToggleRail,
        ToggleMarkdown,
        ToggleWorkbench,
        ToggleWorkbenchVisibility,
        OpenSettings,
        SaveFile,
        ToggleTerminal,
        OpenNeovim,
        FocusComposer,
        NewSession,
        RestartSession,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        NextSession,
        PrevSession,
        CloseSession,
        ToggleMaximize,
        CompletionNext,
        CompletionPrev,
        CompletionAccept,
        CycleMode,
        RunWorkflow,
        ShowTasks,
        ShowSchedules,
        ToggleHostBuilds
    ]
);

/// Switch to the *n*-th session of the active root, by position.
///
/// Positional rather than most-recent, deliberately: `Ctrl+3` is muscle memory
/// for a place in the list, and a list that reorders itself under that key is
/// the one thing it must not do. `Ctrl+Tab` is the recency half.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = onehand, no_json)]
pub struct SelectSession {
    pub index: usize,
}

/// What the app calls itself to the desktop.
///
/// Three things have to agree on this exact string or the app draws a generic
/// placeholder instead of its icon: the window's `app_id`, the base name of the
/// installed desktop entry, and that entry's `StartupWMClass`. It is a
/// reverse-DNS-free plain name because that is what the installed entry is
/// named, and the two are compared literally.
///
/// It is the project's own name, the same string as the binary, the icon file
/// and the per-user config directory. A desktop identity is
/// first-come-first-served — two apps announcing one name share an entry, an
/// icon and a slot in the dock, and whichever installs last overwrites the
/// other's — so this name being the project's means nothing else may install an
/// entry under it. The front end this one replaced held it for a while, which is
/// why the string is worth a comment at all.
const APP_ID: &str = "onehand";

/// How long a project must stay selected before its agent is started ahead of
/// the session that would run it.
///
/// Starting one is not free and stopping one is not quiet: the adapter is a
/// node process that also brings up whatever tool servers it was configured
/// with, and killing it mid-handshake makes it die noisily rather than exit.
/// Clicking down a list of projects must therefore not leave a spawn and a kill
/// behind each row it passed through — the wait this is here to hide only
/// exists for the project the user *stops* on.
const WARM_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// Install the registry's defaults and saved overrides after component bindings.
pub fn init_keymap(cx: &mut App) {
    crate::keymap::init(cx);
}

/// Which way a zoom command steps.
#[derive(Clone, Copy)]
enum ZoomStep {
    In,
    Out,
    Reset,
}

impl ZoomStep {
    fn apply(self, zoom: &mut crate::zoom::Zoom) {
        match self {
            Self::In => zoom.zoom_in(),
            Self::Out => zoom.zoom_out(),
            Self::Reset => zoom.reset(),
        }
    }
}

/// What the rail shows for one session.
///
/// Both halves come from the conversation, which lives in the chat pane, so
/// they are fetched together: the rail is rebuilt only when this changes, and a
/// value that carried one half would leave the other able to change unseen —
/// a conversation earning its title mid-stream would never reach the rail.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RailSession {
    /// The one state worth a dot, if any.
    pub signal: Option<crate::chat::pane::SessionSignal>,
    /// The conversation's own name, `None` until its first prompt.
    pub title: Option<SharedString>,
    /// When the session took its signal, which the row says as an age. In
    /// memory only: a launch starts every clock again.
    pub since: std::time::Instant,
    /// How many of the agent's background items are alive.
    pub background: usize,
    /// How many subagents are working, nested ones included.
    pub subagents: usize,
    /// Whether a turn is open, as opposed to only background work running.
    pub in_turn: bool,
}

/// Which panel a panel-scoped command addresses.
///
/// GPUI has a focus tree, so the live answer is a *query* rather than a value
/// something has to remember to update. `Shell::last_panel` only fills the gap
/// where focus is somewhere that is not a panel at all -- the rail, a dialog,
/// nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusedPanel {
    Chat,
    Workbench,
    Terminal,
}

impl FocusedPanel {
    /// What this panel is called in something a user reads.
    ///
    /// "Conversation" rather than "Chat" or "Agent": it is what the pane's own
    /// header, the rail's session rows and the export both call it, and a panel
    /// with three names in three places is three panels to whoever is reading.
    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "Conversation",
            Self::Workbench => "Workbench",
            Self::Terminal => "Terminal",
        }
    }
}

pub struct Shell {
    window: WorkspaceWindow,
    /// Whether the rail is off screen entirely.
    ///
    /// Hidden, not narrowed. `Ctrl+Shift+B` used to squeeze the rail to a 48px
    /// icon column, and at that width every project row is the same folder
    /// glyph: ten projects became ten identical squares, so the one thing the
    /// rail exists to tell you -- which project, which session -- was exactly
    /// what the narrow form could not say. What the width was wanted for was
    /// the width itself, so it is given back whole.
    rail_hidden: bool,
    /// The rail | dock split, which is where the rail's width lives.
    ///
    /// Held by the shell rather than left to the element's own keyed state:
    /// the width is persisted per workspace, so something that outlives a
    /// frame has to be able to read it back, and the rail is not rendered at
    /// all while it is hidden or a panel is maximized -- state living in the
    /// element would be reset by the first frame it is missing from.
    rail_split: Entity<ResizableState>,
    /// The agent add/edit form.
    agent_draft: AgentDraft,
    /// Whether the agent form is open over the page.
    agent_form: bool,
    /// The field Settings adds a host by.
    host_draft: HostDraft,
    /// Each project's check command field, by root, made as its project
    /// page comes up.
    check_inputs: HashMap<PathBuf, Entity<InputState>>,
    /// Why the workflow last picked as a project's board was refused, and
    /// for which project's folder.
    board_refused: Option<(PathBuf, String)>,
    /// Which of its pages Settings is showing: on the shell, with the drafts,
    /// so leaving Settings and coming back finds it where it was.
    ///
    /// Not persisted: a launch that opened Settings on the agent list would be
    /// one where the appearance, the thing most likely to be looked for, is a
    /// page away.
    settings_page: SettingsPage,
    /// The project Settings' Projects page was last pointed at, by folder.
    settings_project: Option<std::path::PathBuf>,
    /// The sends under way from this window's project pages.
    page_starts: onehand_core::workspace::page_starts::PageStarts,
    /// The sessions a page started off screen, waiting on their agent, with
    /// the page's project, by uid.
    page_waits: std::collections::HashMap<u64, gpui::Subscription>,
    /// Held by Settings' nav and focused when it opens.
    settings_focus: gpui::FocusHandle,
    keymap_editor: Entity<crate::keymap::Editor>,
    settings_view: Entity<crate::settings::SettingsView>,
    held_commands: std::collections::HashSet<&'static str>,
    /// What closing the window asks first, from the compositor's close and the
    /// title bar's alike: `true` lets it close.
    close_guard: CloseGuard,
    /// The workspace-rename field.
    workspace_name: Entity<InputState>,
    /// The session whose name is being edited, if any.
    ///
    /// By uid, not by position: the rename outlives its own dialog frame, and a
    /// session closed elsewhere must not hand its index -- and with it the name
    /// being typed -- to whichever session slides into that place.
    ///
    /// `Some` is also what puts the dialog on screen. gpui-component's `Dialog`
    /// renders open when it carries no trigger, so "is it showing" is this
    /// field rather than a second piece of state to keep in step with it.
    renaming: Option<u64>,
    /// The conversation-rename field.
    rename_input: Entity<InputState>,
    /// The project being renamed, and the field its new name is typed in,
    /// while that dialog is up.
    project_rename: Option<(onehand_core::project::ProjectId, Entity<InputState>)>,
    /// Each project folder's issues file, as the Workbench and the Issues
    /// page were last told it.
    told_issue_files: HashMap<PathBuf, onehand_core::workbench::KeptIssues>,
    /// The project being split onto a branch of its own, if that dialog is
    /// open. `Some` is what puts it on screen, the same as the rename above.
    worktree_draft: Option<WorktreeDraft>,
    branch_draft: Option<BranchDraft>,
    /// The add-a-project-on-a-host form, while it is up.
    host_project: Option<host_project::HostProjectDraft>,
    /// What here waits on each host, and the form's attaches.
    host_waits: onehand_core::host::waits::WindowWaits,
    /// The listening to each host's wait (on its being back, or on its busy
    /// sessions) that something here waits on; dropping one leaves the wait.
    host_listening: BTreeMap<(HostName, bool), gpui::Task<()>>,
    /// The offload form, while it is up.
    offload_draft: Option<offload::OffloadDraft>,
    /// The form that adds or changes one of a project's build hosts.
    build_host_draft: Option<build_hosts::BuildHostDraft>,
    /// The form that adds or changes one of a project's routed commands.
    route_draft: Option<routes::RouteDraft>,
    /// The commands each project folder suggests routing, read once a folder.
    route_suggestions: HashMap<PathBuf, Vec<&'static str>>,
    /// The folders of this window's offloaded projects whose writers are
    /// off, with the line said under them, as last told to the docks.
    offload_locks: HashMap<PathBuf, String>,
    /// The workflow launcher, while it is on screen. Opened from a menu entry
    /// or a key, so this being `Some` is what puts it up.
    workflow_launcher: Option<WorkflowLauncher>,
    /// What held the keyboard when one of the shell's own dialogs opened, so
    /// it gets the keyboard back once the dialog is gone: those dialogs are
    /// mounted by the shell rather than by the library, which would do this
    /// itself. `None` when nothing held it, and the conversation takes it.
    dialog_return: Option<gpui::WeakFocusHandle>,
    /// The field the new branch name is typed into.
    branch_input: Entity<InputState>,
    /// The new branch's name field.
    worktree_branch: Entity<InputState>,

    /// The workspace drive's wait for a burst of changes to end, while one
    /// is armed. Replaced by the next; the drive drops a wait it superseded.
    _pending_save: Option<gpui::Task<()>>,
    /// The docks' layout as last saved, so a change to it, and nothing else,
    /// schedules a save.
    layout_saved: Option<onehand_core::config::PanelLayout>,
    /// Whether a shell of the active project is running a command, for the
    /// dot on the title bar's terminal toggle.
    terminal_live: bool,
    /// A pending agent pre-start, cancelled the same way — see
    /// [`Shell::warm_default_agent`].
    _pending_warm: Option<gpui::Task<()>>,
    /// The attach a host project on screen waits on for its files; replaced
    /// on every arrival, so one for a project left behind stops.
    _host_files: Option<gpui::Task<()>>,
    /// The rail's session rows as of the last repaint, so a chat notify that
    /// changes nothing the rail shows does not cost a rail rebuild.
    rail_sessions: Vec<(u64, RailSession)>,
    /// The conversations with a live session here, as the Workbench was last
    /// told them.
    live_conversations: Vec<String>,
    /// The words of each refusal this window put up, in order, for a test:
    /// a toast's own words cannot be read back off the window.
    #[cfg(test)]
    refused: Vec<&'static str>,
    /// How many times this window asked for the workspace to be saved, for
    /// a test: with no storage bound, the ask itself writes nothing.
    #[cfg(test)]
    saves_asked: usize,
    /// The rail's filter, search and keyboard row, which no launch keeps.
    rail: crate::rail::RailState,
    palette: crate::palette::Palette,
    /// Whether each project's sessions are showing under it in the rail.
    ///
    /// **Here and not in the row, because the row does not live long enough.**
    /// gpui keeps an element's state only across frames the key is *accessed*
    /// in, so every project's fold died the moment the rail drew the flat list
    /// instead — and coming back re-seeded each row from scratch, springing a
    /// folded project open and snapping an unfolded one shut. The window
    /// outlives both lists; a row drawn in one of them does not.
    ///
    /// By path, like pinning, and for the same reason: two checkouts of one
    /// repository share a folder name and are two different projects. Absent
    /// means untouched, which is why this is a map rather than a set of the
    /// open ones — the answer for a project nobody has folded depends on
    /// whether it is the selected one.
    folds: HashMap<PathBuf, bool>,
    /// The panel a panel-scoped command falls back to when focus is not in one.
    last_panel: FocusedPanel,
    /// The terminal dock's state per project, and whose is on screen.
    terminal_homes: dock_rules::TerminalHomes,
    /// Whether the Workbench was open when the workspace page put it away, so
    /// leaving the page puts it back.
    page_hid_workbench: bool,
    /// The page Settings was opened over, or `None` for a session, so closing
    /// Settings goes back to it.
    settings_from: Option<crate::chat::pane::PageKind>,
    /// Session uids per root, most recently viewed first. What `Ctrl+Tab`
    /// walks.
    mru: HashMap<PathBuf, Vec<u64>>,
    /// A `Ctrl+Tab` cycle in flight, if any.
    tab_cycle: Option<TabCycle>,
    /// The panel currently filling the whole frame, rail included.
    ///
    /// Only the *app* direction is tracked here, because only that direction
    /// is something the shell does: the content-only direction is the dock's
    /// own zoom, driven by the button in each panel's tab bar, and the rail is
    /// what tells the two apart.
    app_maximized: Option<FocusedPanel>,
    /// How the area beside the rail was shown on the last frame.
    presentation: presentation::Presentation,
    /// The Workbench width this shell last drew the dock at, so a width the
    /// dock reports that differs from it is a drag, and becomes the width the
    /// person wants.
    workbench_drawn: Option<gpui::Pixels>,
    /// The terminal height this shell last drew the dock at, for the same
    /// reason: a height the dock reports that differs from it is a drag.
    terminal_drawn: Option<gpui::Pixels>,
    /// The Workbench stepped aside from taking the whole area, so the
    /// conversation could be read: still open as far as the person is
    /// concerned, and back as a split once the window has room for one.
    stepped_aside: bool,
    /// The three dockable regions. The rail is deliberately *not* in here --
    /// it is app chrome, not a panel (see [`crate::panels`]).
    dock: Entity<DockArea>,
    /// The centre panel, kept by handle so selecting a session in the rail can
    /// reach it. The dock owns its placement; the shell owns what it shows.
    chat: Entity<ChatPane>,
    /// The centre's right slot, kept for the same reason: switching roots
    /// swaps its tabs and its tree.
    workbench: Entity<Workbench>,
    /// The bottom dock. Per-root shells, so it follows the selection too.
    terminal: Entity<TerminalPanel>,
    /// The Issues page, made once so it is found as it was left whenever it
    /// is picked again.
    issues_page: onehand_issues::Page,
    /// The Workflows page, made once for the same reason: a workflow being
    /// written is found as it was left.
    workflows_page: Entity<crate::workflows_page::WorkflowsPage>,
    /// The Schedules page, made once so its filter is found as it was left.
    schedules_page: Entity<crate::schedules_page::SchedulesPage>,
    /// The Tasks page, made once for the same reason.
    tasks_page: Entity<crate::tasks_page::TasksPage>,
    /// The workspace page, made once likewise.
    workspace_page: Entity<crate::workspace_page::WorkspacePage>,
}

/// A `Ctrl+Tab` walk in progress.
struct TabCycle {
    /// Recency order, frozen for the walk's life.
    order: Vec<u64>,
    /// Where in `order` the walk currently is.
    pos: usize,
    modifiers: gpui::Modifiers,
}

impl TabCycle {
    fn held(&self, current: gpui::Modifiers) -> bool {
        let start = self.modifiers;
        let any = start.control || start.alt || start.platform || start.function;
        any && (!start.control || current.control)
            && (!start.alt || current.alt)
            && (!start.platform || current.platform)
            && (!start.function || current.function)
    }
}

/// The shell entity type, for callers that need to name it.
pub type ShellEntity = Entity<Shell>;

#[cfg(test)]
mod tests;
