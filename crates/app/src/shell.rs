//! The window's root view.
//!
//! The rail plus a `DockArea`: what the window is made of, and everything that
//! is about the window rather than about one panel.

use crate::chat::ChatPane;
use crate::settings::{AgentCheck, AgentDraft, SettingsPage};
use crate::state::WorkspaceWindow;
use crate::terminal::TerminalPanel;
use crate::workbench::Workbench;
use gpui::{App, Entity, SharedString};
use gpui_component::ResizableState;
use gpui_component::dock::DockArea;
use gpui_component::input::InputState;
use std::collections::HashMap;
use std::path::PathBuf;

mod boot;
mod docks;
mod drafts;
mod issue_work;
mod pipelines;
mod remote_runs;
mod render;
mod roots;
mod sessions;
mod settings_dialog;
mod storage;
pub use boot::{boot, open_or_focus, seed_workspace};
pub use drafts::{BranchDraft, Draft, WorktreeDraft};
pub use pipelines::PipelineLauncher;

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
        PasteHere,
        CycleMode,
        RunPipeline
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

/// How long the panel arrangement must sit still before it is written.
///
/// Long enough that a drag is one write rather than one per frame, short
/// enough that quitting right after a resize still saves it.
const SAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(600);

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
    /// The pipeline template form, while one is open.
    pipeline_draft: Option<crate::settings::PipelineDraft>,
    /// Each project's check command field in Settings, by root, made as
    /// Settings opens.
    check_inputs: HashMap<PathBuf, Entity<InputState>>,
    /// Which page the Settings dialog is showing.
    ///
    /// On the shell rather than inside the dialog because a triggered dialog is
    /// rebuilt from its content closure on every frame it is open, so it has
    /// nowhere of its own to keep a selection.
    ///
    /// Not persisted: a launch that opened Settings on the agent list would be
    /// one where the appearance, the thing most likely to be looked for, is a
    /// page away.
    settings_page: SettingsPage,
    settings_open: bool,
    settings_return_focus: Option<gpui::FocusHandle>,
    /// Held by Settings' body and focused when it opens.
    settings_focus: gpui::FocusHandle,
    keymap_editor: Entity<crate::keymap::Editor>,
    settings_view: Entity<crate::settings::SettingsView>,
    /// What each agent's last *Test* found, by `settings::check_key`.
    agent_checks: HashMap<String, AgentCheck>,
    /// How the last write made from the Settings page showing went.
    settings_note: Option<Result<(), String>>,
    /// The next workspace write was asked for from Settings, so how it goes is
    /// Settings' to say. Workspace writes also come from runs, pins and the
    /// dock, and one of those landing while Settings is open is not a change
    /// the page in front of the user made.
    workspace_note_wanted: bool,
    held_commands: std::collections::HashSet<&'static str>,
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
    /// The project being split onto a branch of its own, if that dialog is
    /// open. `Some` is what puts it on screen, the same as the rename above.
    worktree_draft: Option<WorktreeDraft>,
    branch_draft: Option<BranchDraft>,
    /// The open-issue picker, while it is on screen. Opened from a project's
    /// menu, which is gone by the time the list arrives, so this being `Some`
    /// is what puts the dialog up — like the rename and the worktree forms.
    issue_picker: Option<IssuePicker>,
    /// The pipeline launcher, while it is on screen. Opened from a menu entry
    /// or a key, so this being `Some` is what puts it up.
    pipeline_launcher: Option<PipelineLauncher>,
    /// The field the new branch name is typed into.
    branch_input: Entity<InputState>,
    /// The new branch's name field.
    worktree_branch: Entity<InputState>,

    /// A pending workspace write. Replacing it cancels the one before, which
    /// is the whole debounce — see [`Shell::save_workspace_soon`].
    _pending_save: Option<gpui::Task<()>>,
    /// A pending agent pre-start, cancelled the same way — see
    /// [`Shell::warm_default_agent`].
    _pending_warm: Option<gpui::Task<()>>,
    /// Bumped on every `git status` sweep, so a slow one that started earlier
    /// cannot land on top of a fast one that started later.
    git_generation: u64,
    /// The rail's session rows as of the last repaint, so a chat notify that
    /// changes nothing the rail shows does not cost a rail rebuild.
    rail_sessions: Vec<(u64, RailSession)>,
    /// The conversations with a live session here, as the Workbench was last
    /// told them.
    live_conversations: Vec<String>,
    /// Which of the rail's two lists is showing.
    ///
    /// Not persisted: it is where the user is looking right now, and a launch
    /// that came up on the flat list would be one where the project tree — the
    /// thing that says what a workspace *is* — had to be found before anything
    /// else could be read.
    rail_tab: crate::rail::RailTab,
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
    /// Whether the terminal dock was open, per project root.
    ///
    /// **The terminal is per project all the way down, so its dock is too.**
    /// Its tabs, its shells and its working directory are all keyed by root, and
    /// none of them come along when the selection moves — so a dock that stayed
    /// open across a switch was showing the new project an empty panel where the
    /// old project's shells had been, which reads as the terminal having lost
    /// them rather than as their having been left behind.
    ///
    /// A root nobody has opened it in is closed, not "however it was left in the
    /// project before". Inheriting would reproduce the same empty panel on every
    /// project newly arrived at, which is the thing this exists to stop.
    terminal_open: HashMap<PathBuf, bool>,
    /// Which root the bottom dock's current open/closed state belongs to.
    ///
    /// The handover has to read the dock *live* — the state is changed by a key,
    /// by the conversation's header, by a rail menu entry and by the dock's own
    /// chrome, and a memory updated at each of those has four places to forget.
    /// Knowing whose state is on screen is enough to file it correctly at the one
    /// moment it matters, which is the switch itself.
    terminal_root: Option<PathBuf>,
    /// Whether the Workbench was open when the workspace page put it away, so
    /// leaving the page puts it back.
    workbench_aside: bool,
    /// Session uids per root, most recently viewed first. What `Ctrl+Tab`
    /// walks.
    mru: HashMap<PathBuf, Vec<u64>>,
    /// A `Ctrl+Tab` cycle in flight, if any.
    tab_cycle: Option<TabCycle>,
    /// A root whose removal is armed, waiting for the confirming click.
    ///
    /// Only armed for a root that has live sessions: removing those kills their
    /// agents mid-turn, which is not something one stray click on a small
    /// target should be able to do. A root with nothing running just goes.
    pending_remove: Option<usize>,
    /// A session whose closing is armed, waiting for the confirming click.
    ///
    /// By uid rather than by position: the arming has to survive the list
    /// shifting under it, and a session closed elsewhere must not hand its
    /// index -- and with it its confirmation -- to whichever session slides
    /// into that place.
    pending_close: Option<u64>,
    /// The panel currently filling the whole frame, rail included.
    ///
    /// Only the *app* direction is tracked here, because only that direction
    /// is something the shell does: the content-only direction is the dock's
    /// own zoom, driven by the button in each panel's tab bar, and the rail is
    /// what tells the two apart.
    app_maximized: Option<FocusedPanel>,
    /// The three dockable regions. The rail is deliberately *not* in here --
    /// it is app chrome, not a panel (see [`crate::panels`]).
    dock: Entity<DockArea>,
    /// The centre panel, kept by handle so selecting a session in the rail can
    /// reach it. The dock owns its placement; the shell owns what it shows.
    chat: Entity<ChatPane>,
    /// The right dock, kept for the same reason: switching roots swaps its
    /// tabs and its tree.
    workbench: Entity<Workbench>,
    /// The bottom dock. Per-root shells, so it follows the selection too.
    terminal: Entity<TerminalPanel>,
}

/// What reading a project's open issues came to: the issues and whether the
/// list was cut at its bound, or why they could not be read.
pub type PickerAnswer = Result<crate::unattended::Pickable, String>;

/// The open issues of one project, being read or read.
pub struct IssuePicker {
    root: PathBuf,
    /// The project's name, for the dialog's heading.
    pub project: SharedString,
    /// `None` while `gh` is being asked; then the issues and whether the list
    /// was cut at its bound, or why they could not be read. Shared rather than
    /// owned, because the dialog is rebuilt every frame and a hundred issues
    /// with their bodies is not a thing to copy sixty times a second.
    pub found: Option<std::rc::Rc<PickerAnswer>>,
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
