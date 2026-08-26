//! The window's root view.
//!
//! The rail plus a `DockArea`: what the window is made of, and everything that
//! is about the window rather than about one panel.

use crate::chat::ChatPane;
use crate::dialogs::AgentDraft;
use crate::state::{OpenWindow, Shared, WorkspaceWindow};
use crate::terminal::TerminalPanel;
use crate::workbench::{Workbench, WorkbenchMode};
use gpui::{
    App, AppContext, BorrowAppContext, Context, Entity, Focusable as _, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, Styled, Window, WindowAppearance, div, px,
};
use gpui_component::dock::{DockArea, DockEvent, DockItem, DockPlacement, PanelStyle};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::notification::Notification;
use gpui_component::{
    ResizableState, Root, StyledExt, Theme, ThemeMode, WindowExt as _, h_resizable, resizable_panel,
};
use onehand_core::config::{AgentSpec, AppConfig, Appearance, PanelLayout};
use onehand_core::config::{Load, WorkspaceConfig};
use onehand_core::gitstat;
use onehand_core::workspace::{self, Workspace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

gpui::actions!(
    onehand,
    [
        ToggleRail,
        ToggleFiles,
        ToggleWorkbench,
        SaveFile,
        ToggleTerminal,
        FocusComposer,
        ToggleFind,
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
        PasteHere
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

/// App commands occupy an exact `Ctrl+Shift` namespace so plain Ctrl keys stay
/// usable inside the embedded terminal and Neovim later.
///
/// **This is where the GPUI port pays off.** GPUI matches key bindings against
/// the focus context stack *before* it delivers the key to whatever is focused
/// (`Window::dispatch_key_event`: bindings first, `finish_dispatch_key_event`
/// -- which runs `on_key_down` -- only if none matched). So a binding declared
/// here reaches the app even while a PTY holds focus, and the terminal simply
/// never sees that keystroke. Nothing in `vendor/gpui-terminal` knows what the
/// app's keymap is, and nothing there has to: a terminal widget that had to be
/// told which combinations to drop would need editing every time this list
/// grows.
///
/// The one deliberate exception is `Ctrl+S`, bound `Shell && !Terminal`: it is
/// outside the `Ctrl+Shift` namespace, so the PTY has a real claim on it.
/// A `!` predicate matches only when that context appears nowhere in the
/// stack, which is exactly "focus is not inside the terminal".
pub fn init_keymap(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("ctrl-shift-b", ToggleRail, None),
        gpui::KeyBinding::new("ctrl-shift-e", ToggleFiles, None),
        gpui::KeyBinding::new("ctrl-shift-o", ToggleWorkbench, None),
        // The outer terminal. Plain ``Ctrl+` `` stays free for whatever is
        // running *inside* a PTY.
        gpui::KeyBinding::new("ctrl-shift-`", ToggleTerminal, None),
        gpui::KeyBinding::new("ctrl-shift-a", FocusComposer, None),
        gpui::KeyBinding::new("ctrl-shift-f", ToggleFind, None),
        gpui::KeyBinding::new("ctrl-shift-r", RestartSession, None),
        // Closing a session is the counterpart to restarting one, and sits next
        // to it in the namespace for that reason.
        gpui::KeyBinding::new("ctrl-shift-w", CloseSession, None),
        gpui::KeyBinding::new("ctrl-shift-k", ToggleMaximize, None),
        // Saving is the one editor gesture nobody will look up, so it stays on
        // plain Ctrl+S -- everywhere except inside the terminal, where the key
        // belongs to whatever is running there.
        gpui::KeyBinding::new("ctrl-s", SaveFile, Some("Shell && !Terminal")),
        // Zoom is app-global on purpose, terminal included: `Ctrl+=` in a PTY
        // is not a key anything reads, and a terminal that could not be made
        // readable would be the one panel that needs it most. The binding
        // simply wins; the PTY never sees these.
        gpui::KeyBinding::new("ctrl-=", ZoomIn, None),
        // The same physical key on layouts that report it shifted.
        gpui::KeyBinding::new("ctrl-+", ZoomIn, None),
        gpui::KeyBinding::new("ctrl--", ZoomOut, None),
        gpui::KeyBinding::new("ctrl-0", ZoomReset, None),
        // Session switching stays app-global over a PTY too: these are how the
        // user leaves a terminal that has their full attention.
        gpui::KeyBinding::new("ctrl-tab", NextSession, None),
        gpui::KeyBinding::new("ctrl-shift-tab", PrevSession, None),
        gpui::KeyBinding::new("ctrl-1", SelectSession { index: 0 }, None),
        gpui::KeyBinding::new("ctrl-2", SelectSession { index: 1 }, None),
        gpui::KeyBinding::new("ctrl-3", SelectSession { index: 2 }, None),
        gpui::KeyBinding::new("ctrl-4", SelectSession { index: 3 }, None),
        gpui::KeyBinding::new("ctrl-5", SelectSession { index: 4 }, None),
        gpui::KeyBinding::new("ctrl-6", SelectSession { index: 5 }, None),
        gpui::KeyBinding::new("ctrl-7", SelectSession { index: 6 }, None),
        gpui::KeyBinding::new("ctrl-8", SelectSession { index: 7 }, None),
        gpui::KeyBinding::new("ctrl-9", SelectSession { index: 8 }, None),
        // Walking the composer's completion list. The input binds both keys
        // itself, at the same depth of the focus stack as this predicate
        // reaches -- `A > B` is scored at the depth of `B` -- so what decides
        // between them is registration order, and the app's keymap is built
        // after the library's. The composer answers to `ChatComposer` **only
        // while a list is open**, so with nothing to walk the keys go back to
        // moving the caret, which is the whole reason for the narrow predicate.
        gpui::KeyBinding::new("up", CompletionPrev, Some("ChatComposer > Input")),
        gpui::KeyBinding::new("down", CompletionNext, Some("ChatComposer > Input")),
        // Paste, taken from the input for the same reason and by the same rule
        // -- except that the composer holds `ChatComposerCard` at all times,
        // because an image on the clipboard is an attachment whatever else is
        // going on. Text is handed straight back to the input, so this only
        // adds a case rather than replacing one.
        gpui::KeyBinding::new("ctrl-v", PasteHere, Some("ChatComposerCard > Input")),
    ]);
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

/// What the status bar reads out of the two docks.
///
/// Both are facts a panel owns and nothing else on screen says: an unsaved
/// buffer is a dot on one tab of a dock that may be closed, and a shell
/// outliving a closed dock has no representation at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PanelFacts {
    /// Open buffers on the active root carrying edits that are not on disk.
    pub unsaved: usize,
    /// Whether the active root has a shell running in the terminal dock.
    pub terminal_live: bool,
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
    /// What the status bar last read out of the two docks.
    ///
    /// The same guard the rail's rows get, and for a sharper reason: the
    /// terminal notifies for every chunk a running command prints, so an
    /// unguarded observer would put a whole window repaint -- rail rebuild
    /// included -- in the output path of `cargo build`. Comparing the two facts
    /// actually drawn is what keeps it out.
    panels: PanelFacts,
    /// Whether the agent list under *New session* is expanded.
    ///
    /// Only ever shown when more than one agent is configured: with one there
    /// is nothing to choose, and a chevron that opens a list of length one is
    /// a control that exists to disappoint.
    agent_menu_open: bool,
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
    /// by the status bar, by a rail menu entry and by the dock's own chrome, and
    /// a memory updated at each of those is a memory with four places to forget.
    /// Knowing whose state is on screen is enough to file it correctly at the one
    /// moment it matters, which is the switch itself.
    terminal_root: Option<PathBuf>,
    /// Session uids per root, most recently viewed first. What `Ctrl+Tab`
    /// walks.
    mru: HashMap<PathBuf, Vec<u64>>,
    /// Session uids across the whole workspace, most recently viewed first.
    ///
    /// Separate from `mru` rather than derived from it: `mru` is per root and
    /// several roots' lists cannot be merged back into one order, because
    /// nothing in them records *when*.
    recent: Vec<u64>,
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

impl Shell {
    pub fn new(workspace: Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `PanelStyle::TabBar` rather than the default `Auto`.
        //
        // `Auto` renders a dock holding a single panel with a "simple title"
        // instead of a tab bar, and that path miscalculates its own height
        // (`dock/tab_panel.rs:775`): `h(px(30.)) + py_2()` under taffy's default
        // `BoxSizing::BorderBox` leaves a 14px content box for a
        // `line_height(rems(1.0))` = 16px line, which its `overflow_hidden`
        // then clips -- descenders come off every title ("Agent" lost the tail
        // of its `g`). Not fixable from `Panel::title`: the clipping element is
        // the parent of whatever that returns. The `TabBar` path sizes
        // correctly, and onehand wants real tabs on two of these three docks
        // anyway -- the Workbench's modes and the terminal's tab per project
        // root.
        let dock = cx.new(|cx| {
            DockArea::new("onehand", Some(1), window, cx).panel_style(PanelStyle::TabBar)
        });
        let weak = dock.downgrade();
        let chat = ChatPane::new(window, cx);
        let workbench = Workbench::new(cx);
        let terminal = TerminalPanel::new(cx);

        // Restored from the workspace, which supplies the built-in arrangement
        // when there is nothing saved: both docks closed, because the
        // conversation is the window's job and a dock nobody asked for is width
        // taken from it.
        let saved = workspace.layout;
        // The saved arrangement holds one workspace-wide answer to "was the
        // terminal open", and the live question is per root -- so it seeds the
        // root that is about to be on screen and no other. Seeding
        // `terminal_root` with it too is what stops the first handover from
        // reading the restored dock as some *other* project's state and closing
        // it on the way in.
        let seed_root = workspace.active_root().map(|root| root.path.clone());
        dock.update(cx, |area, cx| {
            // A bare panel, not a tab group. `DockItem::tab` wraps its panel in
            // a `TabPanel`, whose title bar draws a tab carrying the panel's
            // title -- which for the conversation is the conversation's own
            // name, printed a second time directly above the header that says
            // it. One tab that can never have a sibling is not a tab; it is a
            // duplicate title with a chevron's worth of chrome around it. The
            // Workbench and the terminal keep their tab groups, because theirs
            // hold several tabs and switching between them is what a tab is
            // for.
            //
            // What the tab bar was also carrying moves into the pane's own
            // header, which is where the rest of the session's controls already
            // live -- see `ChatPane::header`.
            let center = DockItem::panel(std::sync::Arc::new(chat.clone()));
            let workbench = DockItem::tab(workbench.clone(), &weak, window, cx);

            area.set_center(center, window, cx);
            area.set_right_dock(
                workbench,
                Some(px(saved.workbench_w)),
                saved.workbench_open,
                window,
                cx,
            );
        });
        // The bottom dock is mounted only while the terminal is showing, so
        // there is nothing to set up here when it is not -- see
        // [`Shell::set_terminal_visible`].
        if saved.terminal_open {
            let item = DockItem::panel(std::sync::Arc::new(terminal.clone()));
            dock.update(cx, |area, cx| {
                area.set_bottom_dock(item, Some(px(saved.terminal_h)), true, window, cx);
            });
        }

        cx.subscribe_in(
            &chat,
            window,
            |shell: &mut Self, _, event: &crate::chat::pane::ChatPaneEvent, window, cx| {
                use crate::chat::pane::ChatPaneEvent as E;
                match event {
                    E::OpenFile(path) => {
                        shell
                            .workbench
                            .update(cx, |panel, cx| panel.open_file(path.clone(), window, cx));
                        // Opening a file is what makes the Workbench worth
                        // showing -- but only if it is closed, since toggling
                        // an open dock would hide the file just asked for.
                        shell.dock.update(cx, |dock, cx| {
                            if !dock.is_dock_open(DockPlacement::Right, cx) {
                                dock.toggle_dock(DockPlacement::Right, window, cx);
                            }
                        });
                    }
                    E::WorkTreeTouched => shell.refresh_worktree(cx),
                    E::ShowRail => shell.show_rail(cx),
                    // On whichever mode it is already carrying, so the button
                    // means "show me the Workbench" rather than "show me the
                    // files" -- the two keys are how a mode is chosen.
                    E::ToggleWorkbench => {
                        let mode = shell.workbench.read(cx).mode();
                        shell.show_workbench(mode, window, cx);
                    }
                    E::Restart => shell.restart_session(window, cx),
                    E::CloseSession => shell.close_active_session(window, cx),
                    E::StartSession { agent, resume } => {
                        shell.start_session(agent.clone(), resume.clone(), window, cx)
                    }
                }
                // A finished turn also changes what the rail's session dots
                // say, and the rail is drawn from a query rather than from
                // state the pane pushes here.
                cx.notify();
            },
        )
        .detach();

        // The rail reads session state through `Shell::session_row`, so it has
        // to be redrawn when that changes -- and the pane, which owns the
        // conversations, has no idea the rail exists.
        //
        // Guarded rather than a bare `notify`: the pane notifies on every
        // streamed chunk, and rebuilding the rail per token to redraw a dot
        // that has not moved is work for nothing. The guard compares the whole
        // of what the rail asks the pane for, so it cannot drift out of step
        // with what is drawn.
        cx.observe(&chat, |shell: &mut Self, _, cx| {
            let sessions = shell.rail_sessions(cx);
            if sessions != shell.rail_sessions {
                shell.rail_sessions = sessions;
                cx.notify();
            }
        })
        .detach();

        // The status bar reads two facts out of the docks that the docks have no
        // idea anyone outside them wants. Guarded the same way and for the same
        // reason as the rail's rows above -- more so for the terminal, which
        // notifies once per chunk of whatever is printing into it.
        cx.observe(&workbench, |shell: &mut Self, _, cx| {
            shell.sync_panel_facts(cx);
        })
        .detach();
        cx.observe(&terminal, |shell: &mut Self, _, cx| {
            shell.sync_panel_facts(cx);
        })
        .detach();

        // Coming back to the window is the other moment everything on screen
        // may have gone stale: the agent kept working while the user was
        // elsewhere, and the badge on the session they are about to read
        // should not still be up when they get there.
        cx.observe_window_activation(window, |shell: &mut Self, window, cx| {
            if !window.is_window_active() {
                return;
            }
            shell.chat.update(cx, |pane, cx| pane.mark_active_seen(cx));
            shell.refresh_worktree(cx);
            cx.notify();
        })
        .detach();

        // *System* means following the desktop for as long as the window is
        // open, not reading it once at boot. That is also what settles the
        // startup race: the platform can only report its default until the
        // desktop has answered the query it makes in the background, and the
        // answer arrives here.
        window
            .observe_window_appearance(|window, cx| {
                let choice = Shared::global(cx).appearance;
                if choice == Appearance::System {
                    apply_appearance(choice, Some(window), cx);
                }
            })
            .detach();

        cx.subscribe(&dock, |shell: &mut Self, _, event: &DockEvent, cx| {
            if matches!(event, DockEvent::LayoutChanged) {
                shell.save_workspace_soon(cx);
                cx.notify();
            }
        })
        .detach();

        let workspace_name =
            cx.new(|cx| InputState::new(window, cx).default_value(workspace.name.clone()));

        // Renaming auto-saves: there is no Save button on the name field, so
        // nothing would ever commit it otherwise -- debounced, because "on
        // change" here means once per keystroke.
        cx.subscribe_in(
            &workspace_name,
            window,
            |shell: &mut Self, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let name = state.read(cx).value().trim().to_string();
                    if !name.is_empty() && name != shell.window.workspace.name {
                        shell.window.workspace.name = name;
                        shell.save_workspace_soon(cx);
                        cx.notify();
                    }
                }
            },
        )
        .detach();

        Self {
            window: WorkspaceWindow::new(workspace),
            rail_hidden: false,
            rail_split: cx.new(|_| ResizableState::default()),
            agent_draft: AgentDraft::new(window, cx),
            workspace_name,
            renaming: None,
            rename_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder("What this conversation is about")
            }),
            dock,
            chat,
            workbench,
            terminal,
            _pending_save: None,
            _pending_warm: None,
            git_generation: 0,
            rail_sessions: Vec::new(),
            panels: PanelFacts::default(),
            agent_menu_open: false,
            last_panel: FocusedPanel::Chat,
            terminal_open: seed_root
                .clone()
                .map(|path| (path, saved.terminal_open))
                .into_iter()
                .collect(),
            terminal_root: seed_root,
            mru: HashMap::new(),
            recent: Vec::new(),
            tab_cycle: None,
            pending_remove: None,
            pending_close: None,
            app_maximized: None,
        }
    }

    /// What the rail draws on a session row.
    ///
    /// Delegated rather than mirrored: the chat pane holds the conversations,
    /// and every attempt so far to keep a copy of this one level up went stale
    /// immediately.
    ///
    /// One call for both halves so there is a single answer to "what does the
    /// rail read?" — which is what [`Self::rail_sessions`] compares to decide
    /// whether a rebuild is worth it.
    pub fn session_row(&self, uid: u64, cx: &App) -> RailSession {
        let pane = self.chat.read(cx);
        RailSession {
            signal: pane.signal(uid, cx),
            title: pane.title_for(uid, cx).map(SharedString::from),
        }
    }

    /// Every session row, in tree order — the whole of what the rail asks the
    /// chat pane for, and therefore a sound repaint key.
    fn rail_sessions(&self, cx: &App) -> Vec<(u64, RailSession)> {
        self.window
            .workspace
            .roots
            .iter()
            .flat_map(|root| root.sessions.iter())
            .map(|session| (session.uid, self.session_row(session.uid, cx)))
            .collect()
    }

    /// The panel a panel-scoped command addresses: whichever holds focus, else
    /// the last one a panel command was aimed at.
    pub fn focused_panel(&self, window: &Window, cx: &App) -> FocusedPanel {
        if self.terminal.focus_handle(cx).contains_focused(window, cx) {
            FocusedPanel::Terminal
        } else if self.workbench.focus_handle(cx).contains_focused(window, cx) {
            FocusedPanel::Workbench
        } else if self.chat.focus_handle(cx).contains_focused(window, cx) {
            FocusedPanel::Chat
        } else {
            self.last_panel
        }
    }

    /// What the status bar draws from the two docks.
    ///
    /// Read fresh at every repaint rather than served from [`Self::panels`]:
    /// that copy exists only to decide whether a panel's notify was worth a
    /// repaint, and a cached value would be one root switch behind.
    pub fn panel_facts(&self, cx: &App) -> PanelFacts {
        PanelFacts {
            unsaved: self
                .window
                .workspace
                .active_root()
                .map(|root| self.workbench.read(cx).unsaved_in(&root.path))
                .unwrap_or(0),
            terminal_live: self.terminal.read(cx).has_shell(),
        }
    }

    /// Repaint only if a panel's notify changed something the bar shows.
    fn sync_panel_facts(&mut self, cx: &mut Context<Self>) {
        let facts = self.panel_facts(cx);
        if facts != self.panels {
            self.panels = facts;
            cx.notify();
        }
    }

    /// Every panel currently being read at something other than 100%, with its
    /// factor. Normally empty.
    ///
    /// Asked of the panels themselves rather than of whichever one holds focus.
    /// Focus moves without telling the window, so a focus-derived reading would
    /// sit on screen showing one panel's factor while another was in front, with
    /// nothing to say it had gone stale — and the reason to show a factor at all
    /// is that a panel left zoomed is easy to forget about.
    pub fn zoomed_panels(&self, cx: &App) -> Vec<(FocusedPanel, f32)> {
        [
            (FocusedPanel::Chat, self.chat.read(cx).zoom()),
            (FocusedPanel::Workbench, self.workbench.read(cx).zoom()),
            (FocusedPanel::Terminal, self.terminal.read(cx).zoom()),
        ]
        .into_iter()
        .map(|(panel, zoom)| (panel, zoom.factor()))
        .filter(|(_, factor)| *factor != 1.0)
        .collect()
    }

    /// Put one panel back to 100%, whether or not it holds focus.
    pub fn reset_zoom(&mut self, panel: FocusedPanel, cx: &mut Context<Self>) {
        self.zoom_panel(panel, ZoomStep::Reset, cx);
    }

    /// Read the dock's current geometry back out.
    ///
    /// Four facts, not `DockArea::dump`. `dump` produces a whole
    /// `DockAreaState`, but *restoring* one rebuilds every panel through
    /// gpui-component's process-global `PanelRegistry` — and onehand's panels
    /// are per window and held by the shell, so the shell would be left holding
    /// handles to orphans, and one global registry could not tell two windows'
    /// panels apart anyway. The arrangement here is fixed by design, so what a
    /// user actually changes is these values (see
    /// `onehand_core::config::PanelLayout`).
    ///
    /// The rail's width comes from the split it lives in rather than from the
    /// dock, because the rail is not in the dock -- but it is the same fact and
    /// belongs in the same snapshot.
    fn dock_layout(&self, cx: &App) -> PanelLayout {
        let dock = self.dock.read(cx);
        let fallback = self.window.workspace.layout;
        let right = dock.right_dock().map(|d| d.read(cx));
        let bottom = dock.bottom_dock().map(|d| d.read(cx));
        PanelLayout {
            workbench_w: right.map_or(fallback.workbench_w, |d| f32::from(d.size())),
            workbench_open: right.is_some_and(|d| d.is_open()),
            terminal_h: bottom.map_or(fallback.terminal_h, |d| f32::from(d.size())),
            terminal_open: bottom.is_some_and(|d| d.is_open()),
            // Kept only when it is a width the rail could actually have been
            // dragged to. The split seeds every panel at its own floor before
            // the first prepaint measures anything, so a snapshot taken in that
            // gap would record a number the user never chose and quietly
            // replace the width they did.
            rail_w: self
                .rail_split
                .read(cx)
                .sizes()
                .first()
                .map(|w| f32::from(*w))
                .filter(|w| (PanelLayout::RAIL_MIN..=PanelLayout::RAIL_MAX).contains(w))
                .unwrap_or(fallback.rail_w),
        }
    }

    /// Write the workspace once the user stops changing it.
    ///
    /// Two callers, one problem: the dock emits `LayoutChanged` on **every
    /// frame of a drag**, and the rename field emits `Change` on **every
    /// keystroke**. Either one writing on the event is a file write per frame
    /// or per character, on the UI thread. Each call replaces
    /// the pending task, and dropping a `Task` cancels it — so the write only
    /// happens after things have been still for [`SAVE_DEBOUNCE`]. The
    /// debounce *is* the replacement; there is no flag to keep in step.
    fn save_workspace_soon(&mut self, cx: &mut Context<Self>) {
        // Nothing to save to, so nothing to schedule. An unbound workspace
        // persists nothing at all -- that is what binding a folder is for.
        if self.window.workspace.storage_dir.is_none() {
            return;
        }
        self._pending_save = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                // `save_workspace` reads the dock itself, so there is one place
                // that knows how to turn the live arrangement into the saved
                // one.
                shell.save_workspace(window, cx);
                shell._pending_save = None;
            });
        }));
    }

    /// Select a root, and show whatever session it was last on.
    pub fn select_root(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.window.workspace.select_root(idx);
        self.show_active_session(window, cx);
        cx.notify();
    }

    /// Select root *and* session in one touch, mirroring
    /// `Message::SelectRootSession`. The rail is session-first: a session row
    /// is the switcher, so it must not take two clicks to reach.
    pub fn select_root_session(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window.workspace.select_root(root_idx);
        self.window.workspace.select_session(session_idx);
        self.show_active_session(window, cx);
        cx.notify();
    }

    /// Point the chat pane at the workspace's active session, spawning its
    /// adapter on first view.
    ///
    /// Lazy: a workspace with a dozen roots must not launch a dozen agent
    /// processes at boot.
    fn show_active_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.active_root() else {
            // No roots at all. The pane has to be told, or removing the last
            // project leaves the centre of the window inviting the user to
            // start a session in a project that is no longer in the workspace.
            self.chat
                .update(cx, |pane, cx| pane.clear_active(None, window, cx));
            // A parked adapter is bound to a project root, and there is no
            // longer one to be bound to -- unlike every other case, this one
            // has no project the spare could still turn out to be for.
            self._pending_warm = None;
            Shared::global(cx).acp.drop_warm();
            return;
        };
        let label = SharedString::from(root.label.clone());
        let path = root.path.clone();
        // Read out of the tree before anything below borrows the shell mutably,
        // rather than reached for again further down.
        let session = root
            .active_session()
            .map(|session| (session.uid, session.spec.clone()));
        // Editor tabs and the file tree are per root, so the Workbench follows
        // the selection rather than mixing two projects' state.
        self.workbench
            .update(cx, |panel, cx| panel.set_root(path.clone(), cx));
        self.terminal
            .update(cx, |panel, cx| panel.set_root(path.clone(), cx));
        self.follow_terminal_dock(&path, window, cx);
        let Some((uid, spec)) = session else {
            // The other two panels have already followed the selection, so the
            // chat must not be the one panel still showing the root the user
            // just left -- the composer would keep prompting that root's agent.
            // The path goes with the label: what the pane draws in place of a
            // conversation is that project's own past ones, and they are keyed
            // by where the project is rather than by what it is called.
            self.chat.update(cx, |pane, cx| {
                pane.clear_active(Some((label, path.clone())), window, cx)
            });
            // Nothing is running on this project and the page now on screen is
            // a list of past conversations over a *New session* button, so the
            // next thing asked of it is almost certainly a session. Start the
            // agent against this root now: bringing one up costs seconds that
            // are entirely the adapter's and the SDK's, and spending them while
            // the user reads the page is spending them for free.
            self.warm_default_agent(path, cx);
            return;
        };
        self.chat.update(cx, |pane, cx| {
            pane.show(uid, path.clone(), &spec, window, cx)
        });
        // This project has a session on screen, so nothing here is waiting on a
        // fresh agent. A pre-start still in its delay is dropped rather than
        // allowed to fire -- but one that has already *started* is left alone.
        // Killing it here is what turned clicking between projects into a spawn
        // and a kill per click, and an adapter cut off mid-handshake does not go
        // quietly. Holding one spare process costs less than that, and the next
        // pre-start for a different project replaces it anyway.
        self._pending_warm = None;
        self.touch_mru(path, uid);
    }

    /// Start the default agent against `root` ahead of the session that would
    /// run it, once the selection has held still for [`WARM_DELAY`].
    ///
    /// The default is the first configured agent -- the same choice *New
    /// session* makes, which is what makes the guess worth acting on. Starting
    /// a *different* agent is a separate rail action, and a miss costs only the
    /// spare process: the adapter is claimed by matching what it was told to
    /// run against what the session asks for, so a mismatch simply spawns.
    fn warm_default_agent(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if Shared::global(cx).agents.is_empty() {
            return;
        }
        // Replacing the task cancels the one before it, which is the debounce.
        self._pending_warm = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(WARM_DELAY).await;
            cx.update(|cx| {
                let shared = Shared::global(cx);
                if let Some(spec) = shared.agents.first() {
                    shared.acp.warm(spec, root);
                }
            });
        }));
    }

    /// Add a project root, picked with the native folder dialog.
    ///
    /// `Workspace::add_root` normalizes the path and *selects* an existing root
    /// rather than making a twin, so picking a folder that is already in the
    /// workspace (or a symlink to one) is a navigation, not a duplicate --
    /// every per-root map in the app is keyed by that path.
    pub fn add_root(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };
            shell
                .update_in(cx, |shell: &mut Self, window, cx| {
                    let idx = shell.window.workspace.add_root(dir);
                    shell.window.workspace.select_root(idx);
                    shell.show_active_session(window, cx);
                    shell.refresh_git(cx);
                    shell.save_workspace(window, cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Remove a project root, and everything the window keeps for it.
    ///
    /// Two-step while the root has sessions: the first choice arms and says so,
    /// the second removes -- the same guard shape as a mid-turn restart, and
    /// for the same reason. Arming a different root replaces the arming rather
    /// than stacking, so the confirmation always belongs to the row just
    /// acted on.
    pub fn remove_root(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(idx) else {
            return;
        };
        let (path, label, live) = (root.path.clone(), root.label.clone(), root.sessions.len());
        // Unsaved editor buffers are the other thing this click destroys, and
        // they have nowhere to go afterwards -- the tab strip they belong to
        // leaves with the root. Guarded on the same second click rather than a
        // second one of its own.
        let unsaved = self.workbench.read(cx).unsaved_in(&path);

        if (live > 0 || unsaved > 0) && self.pending_remove != Some(idx) {
            self.pending_remove = Some(idx);
            let mut losses = Vec::new();
            if live > 0 {
                let s = if live == 1 { "session" } else { "sessions" };
                losses.push(format!("close {live} {s}"));
            }
            if unsaved > 0 {
                let s = if unsaved == 1 { "file" } else { "files" };
                losses.push(format!("discard {unsaved} unsaved {s}"));
            }
            window.push_notification(
                Notification::warning(format!(
                    "Remove {label} and {}? Choose Remove from workspace again to confirm",
                    losses.join(" and ")
                )),
                cx,
            );
            cx.notify();
            return;
        }
        self.pending_remove = None;

        // Sessions go first: dropping a chat session is what kills its adapter,
        // and the workspace tree is where their uids are recorded.
        let uids: Vec<u64> = self.window.workspace.roots[idx]
            .sessions
            .iter()
            .map(|session| session.uid)
            .collect();
        self.chat.update(cx, |pane, cx| {
            for &uid in &uids {
                pane.close(uid, cx);
            }
        });
        self.workbench
            .update(cx, |panel, cx| panel.forget_root(&path, cx));
        self.terminal
            .update(cx, |panel, cx| panel.forget_root(&path, cx));
        self.window.git.remove(&path);
        self.mru.remove(&path);
        self.terminal_open.remove(&path);
        // The dock on screen no longer belongs to anyone. Left naming this root,
        // the next handover would file the live state under a project that is
        // gone and hand a re-added one a state it never chose.
        if self.terminal_root.as_deref() == Some(path.as_path()) {
            self.terminal_root = None;
        }
        self.forget_recent(&uids);
        // A cycle's frozen order can name sessions that no longer exist.
        self.tab_cycle = None;

        self.window.workspace.remove_root(idx);
        window.push_notification(Notification::info(format!("Removed {label}")), cx);
        self.show_active_session(window, cx);
        self.save_workspace(window, cx);
        cx.notify();
    }

    /// Open the rename field on a conversation.
    ///
    /// Prefilled with the name the user set, **not** with the title derived
    /// from the first prompt: accepting a prefilled guess would freeze that
    /// guess in place as an explicit choice, and the derived title is supposed
    /// to keep tracking the conversation.
    pub fn begin_rename(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.chat.read(cx).custom_title(uid, cx).unwrap_or_default();
        self.rename_input
            .update(cx, |state, cx| state.set_value(current, window, cx));
        self.renaming = Some(uid);
        self.rename_input.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Commit whatever is in the rename field, if it is a name at all.
    pub fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.renaming.take() else {
            return;
        };
        let title = self.rename_input.read(cx).value().to_string();
        self.chat.update(cx, |pane, cx| {
            pane.rename(uid, &title, cx);
        });
        cx.notify();
    }

    /// Close the rename field without changing anything.
    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            cx.notify();
        }
    }

    /// Give a conversation its derived title back.
    pub fn reset_conversation_title(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.renaming.take() else {
            return;
        };
        self.chat.update(cx, |pane, cx| pane.reset_title(uid, cx));
        cx.notify();
    }

    /// Restart a *named* session's adapter. Selecting it first is what makes
    /// the guard, the notification and the transcript all be about the session
    /// the user pointed at.
    pub fn restart_session_at(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_root_session(root_idx, session_idx, window, cx);
        self.restart_session(window, cx);
    }

    /// Write a named session's conversation to a Markdown file.
    pub fn export_session_at(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_root_session(root_idx, session_idx, window, cx);
        self.chat.update(cx, |pane, cx| pane.export(cx));
    }

    /// Close the session on screen — the keyboard half of the rail's ✕.
    pub fn close_active_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root_idx = self.window.workspace.active_root;
        let Some(session_idx) = self
            .window
            .workspace
            .active_root()
            .map(|root| root.active_session)
        else {
            return;
        };
        self.close_session(root_idx, session_idx, window, cx);
    }

    /// Close one session and, with it, its agent — the project root stays.
    ///
    /// Until this existed the only way to end a session was to remove the whole
    /// project it belonged to, so a root accumulated agents nothing could stop.
    ///
    /// **Guarded by a second click only while a turn is in flight.** The
    /// transcript is written at the end of every turn, so closing an idle
    /// session costs nothing that is not already on disk and does not deserve a
    /// confirmation; closing one mid-turn throws away the turn that is running,
    /// which is the same loss a mid-turn restart guards against and is guarded
    /// the same way. Arming a different session replaces the arming, so the
    /// confirmation always belongs to the row just clicked.
    pub fn close_session(
        &mut self,
        root_idx: usize,
        session_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.window.workspace.roots.get(root_idx) else {
            return;
        };
        let Some(session) = root.sessions.get(session_idx) else {
            return;
        };
        let (uid, path) = (session.uid, root.path.clone());

        if self.chat.read(cx).turn_in_flight(uid, cx) && self.pending_close != Some(uid) {
            self.pending_close = Some(uid);
            // Named by whatever the row is named by, so the warning and the row
            // it is about read as the same thing.
            let label = self
                .chat
                .read(cx)
                .title_for(uid, cx)
                .unwrap_or_else(|| session.title().to_string());
            window.push_notification(
                Notification::warning(format!(
                    "{label} is mid-turn. Click ✕ again to close it and lose that turn"
                )),
                cx,
            );
            cx.notify();
            return;
        }
        self.pending_close = None;

        // The pane owns the conversation and, through it, the adapter: dropping
        // the session there is what ends the agent process. Nothing else has to
        // be shut down by hand.
        self.chat.update(cx, |pane, cx| pane.close(uid, cx));
        if let Some(order) = self.mru.get_mut(&path) {
            order.retain(|&seen| seen != uid);
        }
        self.forget_recent(&[uid]);
        // A cycle's frozen order can name a session that no longer exists.
        self.tab_cycle = None;

        self.window.workspace.close_session(root_idx, session_idx);
        // No toast: the row leaving the rail and the pane moving to whatever is
        // left say it, where removing a project has to announce itself because
        // most of what it destroys was never on screen.
        self.show_active_session(window, cx);
        cx.notify();
    }

    /// Move `uid` to the front of its root's recency list.
    ///
    /// Skipped while a cycle is running: `Ctrl+Tab` *passing over* a session is
    /// not the user choosing it, and reordering as it goes would make the list
    /// shuffle under the key that is walking it.
    fn touch_mru(&mut self, root: PathBuf, uid: u64) {
        if self.tab_cycle.is_some() {
            return;
        }
        let order = self.mru.entry(root).or_default();
        order.retain(|&seen| seen != uid);
        order.insert(0, uid);
        // The same move, once more across the whole workspace. `Ctrl+Tab` walks
        // *within* a root, so that list cannot answer "where was I before this
        // project" -- which is the question the rail's Recent section is for.
        self.recent.retain(|&seen| seen != uid);
        self.recent.insert(0, uid);
    }

    /// Sessions the user has looked at, most recent first.
    ///
    /// Across every root, unlike [`Self::mru`]. Uids only: what each one is
    /// called and which project it belongs to are the rail's to look up, and
    /// both change under a list that would otherwise have to be told.
    pub fn recent_order(&self) -> &[u64] {
        &self.recent
    }

    /// Drop a session from every recency list.
    fn forget_recent(&mut self, uids: &[u64]) {
        self.recent.retain(|seen| !uids.contains(seen));
    }

    /// The active root's sessions in recency order.
    ///
    /// Sessions the list has never seen (just added, or added in another
    /// window's copy of the tree) go on the end in tree order rather than
    /// being dropped -- an unlisted session must still be reachable.
    fn mru_order(&self) -> Vec<u64> {
        let Some(root) = self.window.workspace.active_root() else {
            return Vec::new();
        };
        let live: Vec<u64> = root.sessions.iter().map(|s| s.uid).collect();
        let mut order: Vec<u64> = self
            .mru
            .get(&root.path)
            .map(|seen| {
                seen.iter()
                    .copied()
                    .filter(|uid| live.contains(uid))
                    .collect()
            })
            .unwrap_or_default();
        let unseen: Vec<u64> = live
            .into_iter()
            .filter(|uid| !order.contains(uid))
            .collect();
        order.extend(unseen);
        order
    }

    /// Switch to the *n*-th session of the active root, by position.
    pub fn select_session(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self
            .window
            .workspace
            .active_root()
            .map(|root| root.sessions.len())
            .unwrap_or(0);
        if index >= count {
            return;
        }
        self.window.workspace.select_session(index);
        self.show_active_session(window, cx);
        cx.notify();
    }

    /// Walk the active root's sessions in recency order, VSCode-style.
    ///
    /// The order is snapshotted when the cycle starts and held until `Ctrl` is
    /// released ([`Self::end_cycle`]). Recomputing it per press would make the
    /// second press walk back to where the first one came from, and the cycle
    /// would ping-pong between two sessions instead of reaching the third.
    fn cycle_session(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_cycle.is_none() {
            let order = self.mru_order();
            if order.len() < 2 {
                return;
            }
            self.tab_cycle = Some(TabCycle { order, pos: 0 });
        }
        let Some(cycle) = self.tab_cycle.as_mut() else {
            return;
        };
        let len = cycle.order.len();
        cycle.pos = if forward {
            (cycle.pos + 1) % len
        } else {
            (cycle.pos + len - 1) % len
        };
        let uid = cycle.order[cycle.pos];

        let index = self
            .window
            .workspace
            .active_root()
            .and_then(|root| root.sessions.iter().position(|s| s.uid == uid));
        if let Some(index) = index {
            self.window.workspace.select_session(index);
            self.show_active_session(window, cx);
            cx.notify();
        }
    }

    /// Commit a cycle: where it stopped becomes the most recent session.
    ///
    /// Fired on `Ctrl` release. Until then nothing about the recency list has
    /// changed, so a cycle the user abandons by pressing on leaves no trace.
    fn end_cycle(&mut self, cx: &mut Context<Self>) {
        if self.tab_cycle.take().is_none() {
            return;
        }
        let landed = self
            .window
            .workspace
            .active_root()
            .and_then(|root| root.active_session().map(|s| (root.path.clone(), s.uid)));
        if let Some((root, uid)) = landed {
            self.touch_mru(root, uid);
        }
        cx.notify();
    }

    /// Whether the agent list under *New session* is showing.
    pub fn agent_menu_open(&self) -> bool {
        self.agent_menu_open
    }

    /// Show or hide the agent list. Only reachable when there is more than one
    /// agent to choose between.
    pub fn toggle_agent_menu(&mut self, cx: &mut Context<Self>) {
        self.agent_menu_open = !self.agent_menu_open;
        cx.notify();
    }

    /// Add a session on the active root using the default agent, and show it.
    ///
    /// The *default* is the first configured agent, which is the Claude Code
    /// built-in unless the user has reordered the list. One agent is the common
    /// case and it must stay one click; choosing a different one is the rail's
    /// agent list (see [`Self::new_session_with`]).
    pub fn new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_session_with(0, window, cx);
    }

    /// Add a session on a *named* root using the default agent.
    ///
    /// The root is selected first, so this cannot start an agent somewhere the
    /// user is not looking: a session is bound to one project root for its
    /// whole life, and starting one on a root the rail is not showing would be
    /// a prompt sent into a project nobody has open.
    pub fn new_session_in(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.window.workspace.select_root(root_idx);
        self.new_session(window, cx);
    }

    /// Open the bottom terminal on a named root.
    ///
    /// Selection first for the same reason `new_session_in` does it: the
    /// terminal is a tab per root, so opening it without switching would put
    /// the user in a shell in one project while every other panel says another.
    pub fn open_terminal_in(
        &mut self,
        root_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_root(root_idx, window, cx);
        // Never the closing direction: this was asked for by name, and a menu
        // entry called *Open terminal* that closes the terminal is a bug that
        // reads as one.
        if !self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx) {
            self.show_terminal(window, cx);
        }
    }

    /// Hold a root at the top of the rail, or let it go.
    ///
    /// Only the drawing order moves; `roots` and every index into it stay
    /// exactly as they were, so nothing keyed by position has to be told.
    pub fn toggle_pin(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.window.workspace.toggle_pin(root_idx);
        self.save_workspace(window, cx);
        cx.notify();
    }

    /// Put a root's path on the clipboard.
    ///
    /// The rail shows a folder's *name*, which is not enough to paste anywhere
    /// or to tell two checkouts of one repository apart.
    pub fn copy_root_path(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(root_idx) else {
            return;
        };
        let path = root.path.display().to_string();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
        window.push_notification(Notification::info(format!("Copied {path}")), cx);
    }

    /// Add a session running `agents[idx]`.
    pub fn new_session_with(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.spawn_session(idx, None, window, cx);
    }

    /// Start a session on the active root, as the project page asks for it: on
    /// a named agent, and optionally opening straight onto an archived
    /// conversation.
    ///
    /// **An unknown agent falls back to the default rather than refusing.** The
    /// name comes off an archive, and the agent that wrote it can since have
    /// been renamed or removed in the agent manager — a conversation the user
    /// can see listed must still be openable, and which agent replays it is the
    /// smaller loss.
    pub fn start_session(
        &mut self,
        agent: Option<SharedString>,
        resume: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let idx = agent
            .and_then(|name| {
                Shared::global(cx)
                    .agents
                    .iter()
                    .position(|spec| spec.name == name.as_ref())
            })
            .unwrap_or(0);
        self.spawn_session(idx, resume, window, cx);
    }

    /// Mint a session on the active root and show it, resuming `archive` if one
    /// was named.
    ///
    /// The archive is handed to the pane *before* the session is shown: showing
    /// is what spawns the adapter, and a resume arriving after that has already
    /// lost — the session would be up on a fresh conversation with the picker
    /// asking which one to open.
    fn spawn_session(
        &mut self,
        idx: usize,
        archive: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_menu_open = false;
        let Some(spec) = Shared::global(cx).agents.get(idx).cloned() else {
            window.push_notification(Notification::warning("No agents configured"), cx);
            return;
        };
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        if self.window.workspace.add_session(spec, uid).is_some() {
            if let Some(archive) = archive {
                self.chat
                    .update(cx, |pane, _| pane.resume_next(uid, archive));
            }
            self.show_active_session(window, cx);
            cx.notify();
        }
    }

    /// Show a Workbench mode, opening the dock if it is closed.
    ///
    /// Three-state: closed opens on that mode and takes focus; open elsewhere (another mode, or focus somewhere
    /// else) switches and takes focus; open, on this mode and already focused
    /// closes. Closing a panel the user is not looking at is the one outcome
    /// nobody presses a key for.
    pub fn show_workbench(
        &mut self,
        mode: WorkbenchMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_panel = FocusedPanel::Workbench;
        let open = self.dock.read(cx).is_dock_open(DockPlacement::Right, cx);
        let showing = self.workbench.read(cx).mode() == mode;
        let focused = self.workbench.focus_handle(cx).contains_focused(window, cx);
        if open && showing && focused {
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
            return;
        }
        self.workbench
            .update(cx, |panel, cx| panel.set_mode(mode, cx));
        if !open {
            self.dock.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Right, window, cx)
            });
        }
        self.workbench
            .update(cx, |panel, cx| panel.focus_active(window, cx));
        cx.notify();
    }

    /// The bottom terminal, on the same three states. A shell is spawned on
    /// first open and never at boot (see [`crate::terminal`]).
    pub fn show_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.last_panel = FocusedPanel::Terminal;
        let open = self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx);
        let focused = self.terminal.focus_handle(cx).contains_focused(window, cx);
        if open && focused {
            self.set_terminal_visible(false, window, cx);
            return;
        }
        self.set_terminal_visible(true, window, cx);
        self.terminal.update(cx, |panel, cx| {
            if !panel.has_shell() {
                panel.open_shell(window, cx);
            }
            panel.focus_active(window, cx);
        });
        cx.notify();
    }

    /// Put the terminal on screen, or take it off.
    ///
    /// **Mounted and unmounted, not opened and closed.** A *closed* bottom dock
    /// still draws a strip of title bar, on the library's own reasoning that the
    /// button to reopen it lives there — and this terminal has no such button to
    /// put on it. What was left was a bare band of chrome across the bottom of
    /// every window, in every project, naming nothing and reopening nothing.
    /// With no bottom dock at all there is nothing to draw, and the way back is
    /// the key and the status bar's Terminal cell.
    fn set_terminal_visible(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        if visible == self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx) {
            return;
        }
        if visible {
            // The panel entity is the same one every time, so the shells, their
            // PTYs and their scrollback all survive being taken off screen --
            // only the dock around them is rebuilt.
            let item = DockItem::panel(std::sync::Arc::new(self.terminal.clone()));
            let height = px(self.window.workspace.layout.terminal_h);
            self.dock.update(cx, |area, cx| {
                area.set_bottom_dock(item, Some(height), true, window, cx);
                cx.notify();
            });
            cx.notify();
            return;
        }
        // The height has to be read back before the dock holding it goes, or
        // every reopen comes up at the built-in default and the drag is lost.
        // It lands in the workspace's own layout, which is what the saved
        // arrangement falls back to while there is no dock to ask.
        if let Some(height) = self
            .dock
            .read(cx)
            .bottom_dock()
            .map(|dock| f32::from(dock.read(cx).size()))
        {
            self.window.workspace.layout.terminal_h = height;
        }
        // A maximized panel cannot be unmounted out from under the zoom: the
        // dock area would be left blown up over something that is no longer
        // mounted, and the key that undoes it is the same key that got here.
        if self.app_maximized == Some(FocusedPanel::Terminal) {
            self.app_maximized = None;
            self.dock
                .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
        }
        self.dock.update(cx, |area, cx| {
            area.remove_bottom_dock(window, cx);
            cx.notify();
        });
        cx.notify();
    }

    /// Hand the terminal dock over from the project being left to the one
    /// arriving.
    ///
    /// Called from the one place the selection actually moves, and it does both
    /// halves there: it files the live state under the root that owns it, then
    /// puts the dock into whatever the incoming root left it in. Doing it at the
    /// handover rather than at each of the four controls that can toggle the
    /// dock is what keeps this from being four places to forget — the dock is
    /// read, never assumed.
    ///
    /// A session switch inside one project is not a handover: the dock on screen
    /// is already this root's, so the live state is filed and nothing moves.
    /// Restoring there would fight a user who had just opened it.
    fn follow_terminal_dock(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let live = self.dock.read(cx).is_dock_open(DockPlacement::Bottom, cx);
        let leaving = self.terminal_root.replace(root.to_path_buf());
        if leaving.as_deref() == Some(root) {
            self.terminal_open.insert(root.to_path_buf(), live);
            return;
        }
        if let Some(leaving) = leaving {
            self.terminal_open.insert(leaving, live);
        }
        // A project the terminal has never been opened in gets it closed. The
        // shells that were on screen a moment ago belong to the project just
        // left and do not come along, so an inherited open dock would greet the
        // new one with an empty panel where they had been.
        let wanted = self.terminal_open.get(root).copied().unwrap_or(false);
        self.set_terminal_visible(wanted, window, cx);
    }

    /// Blow the focused panel up to the whole frame, or put it back.
    ///
    /// The rail goes with it -- that is what makes this the *app* direction
    /// rather than the content one, which each panel's tab bar offers as a
    /// button and which leaves the rail in place. Restoring is the same key,
    /// whichever panel is focused: a maximized panel is the only thing on
    /// screen, so there is nothing else the key could mean.
    fn toggle_maximize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app_maximized.take().is_some() {
            self.dock
                .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
            cx.notify();
            return;
        }
        let panel = self.focused_panel(window, cx);
        self.dock.update(cx, |dock, cx| match panel {
            FocusedPanel::Chat => dock.set_zoomed_in(self.chat.clone(), window, cx),
            FocusedPanel::Workbench => dock.set_zoomed_in(self.workbench.clone(), window, cx),
            FocusedPanel::Terminal => dock.set_zoomed_in(self.terminal.clone(), window, cx),
        });
        self.app_maximized = Some(panel);
        cx.notify();
    }

    /// Step the focused panel's zoom.
    ///
    /// The target is the panel that holds focus, falling back to the last one
    /// a panel command addressed -- so zooming right after a shortcut opened a
    /// panel hits that panel, not whatever was under the mouse.
    fn zoom(&mut self, step: ZoomStep, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.focused_panel(window, cx);
        self.zoom_panel(panel, step, cx);
    }

    /// Step one named panel's zoom.
    ///
    /// The window notifies too, and not only the panel: the status bar says
    /// which panel is off 100% and is drawn by the shell, so a step that told
    /// only the panel would leave that reading a factor behind.
    fn zoom_panel(&mut self, panel: FocusedPanel, step: ZoomStep, cx: &mut Context<Self>) {
        match panel {
            FocusedPanel::Chat => self.chat.update(cx, |pane, cx| {
                step.apply(pane.zoom_mut());
                cx.notify();
            }),
            FocusedPanel::Workbench => self.workbench.update(cx, |panel, cx| {
                step.apply(panel.zoom_mut());
                cx.notify();
            }),
            FocusedPanel::Terminal => self.terminal.update(cx, |panel, cx| {
                let mut zoom = panel.zoom();
                step.apply(&mut zoom);
                panel.set_zoom(zoom, cx);
            }),
        }
        cx.notify();
    }

    /// Restart the active session's adapter, guarded while a turn is running.
    pub fn restart_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.last_panel = FocusedPanel::Chat;
        match self.chat.update(cx, |pane, cx| pane.restart_active(cx)) {
            crate::chat::pane::Restart::Restarted => {
                window.push_notification(Notification::info("Restarting the agent"), cx);
            }
            crate::chat::pane::Restart::Armed => window.push_notification(
                Notification::warning("A turn is running — press Ctrl+Shift+R again to restart"),
                cx,
            ),
            crate::chat::pane::Restart::Nothing => {}
        }
        cx.notify();
    }

    /// Show or hide the rail.
    ///
    /// The chat pane is told, because it is what offers the way back: with the
    /// rail gone the key is the only route to it, and a key nobody has been
    /// told about is not a route.
    pub fn toggle_rail(&mut self, cx: &mut Context<Self>) {
        let hidden = !self.rail_hidden;
        self.rail_hidden = hidden;
        self.chat
            .update(cx, |pane, cx| pane.set_rail_hidden(hidden, cx));
        cx.notify();
    }

    /// Bring the rail back, whatever asked for it.
    fn show_rail(&mut self, cx: &mut Context<Self>) {
        if !self.rail_hidden {
            return;
        }
        self.toggle_rail(cx);
    }

    // ── Agent manager ───────────────────────────────────────────────────────

    /// The global agent menu. Definitions are process-wide; a session keeps a
    /// clone of the spec it was spawned with.
    pub fn agents<'a>(&self, cx: &'a App) -> &'a [AgentSpec] {
        &Shared::global(cx).agents
    }

    pub fn agent_draft(&self) -> &AgentDraft {
        &self.agent_draft
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

    pub fn delete_agent(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.update_global::<Shared, _>(|shared, _| {
            if idx < shared.agents.len() {
                shared.agents.remove(idx);
            }
        });
        self.persist_agents(window, cx);
        cx.notify();
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
        if let Err(e) = AppConfig::update_in_place(&path, |cfg| cfg.appearance = choice) {
            window.push_notification(
                Notification::error(format!("Appearance not saved — {e}")),
                cx,
            );
        }
        cx.notify();
    }

    /// Write the agent list back to the file the config was loaded from,
    /// preserving every other section.
    fn persist_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = Shared::global(cx);
        let (path, agents) = (shared.config_path.clone(), shared.agents.clone());
        if let Err(e) = AppConfig::update_in_place(&path, |cfg| cfg.agents = agents) {
            window.push_notification(Notification::error(format!("Agents not saved — {e}")), cx);
        }
    }

    // ── Workspace settings ──────────────────────────────────────────────────

    /// The conversation-rename field, for the dialog that edits it.
    pub fn rename_input(&self) -> &Entity<InputState> {
        &self.rename_input
    }

    /// Whether the conversation being renamed already carries a name the user
    /// set, which is the only case where *Use the automatic title* has anything
    /// to undo.
    pub fn rename_is_override(&self, cx: &App) -> bool {
        self.renaming
            .is_some_and(|uid| self.chat.read(cx).custom_title(uid, cx).is_some())
    }

    pub fn workspace_name_input(&self) -> &Entity<InputState> {
        &self.workspace_name
    }

    pub fn storage_dir(&self) -> Option<&std::path::PathBuf> {
        self.window.workspace.storage_dir.as_ref()
    }

    /// Drop the storage binding. An unbound workspace persists nothing.
    pub fn unbind_storage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.window.workspace.storage_dir.take();
        self.set_window_identity(None, window, cx);
        // Forget the recent too, or the next launch reopens the workspace that
        // was just unbound -- `recent_workspaces[0]` takes precedence over
        // everything, including the CLI root.
        if let Some(dir) = was {
            cx.update_global::<Shared, _>(|shared, _| {
                shared.recents.forget(&dir);
                if let Err(e) = shared.recents.save() {
                    eprintln!("onehand: failed to save recents: {e}");
                }
            });
        }
        cx.notify();
    }

    /// Point this workspace at a storage folder and write it there.
    ///
    /// One function for all four writes because they are one fact arriving, and
    /// splitting them is what let them drift: binding used to set
    /// `workspace.storage_dir` and nothing else, leaving this window's registry
    /// entry saying `None` forever -- so opening the very same workspace from
    /// recents made a *second* window for it, which is the one thing the
    /// registry exists to prevent.
    fn bind_storage(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.window.workspace.storage_dir = Some(dir.clone());
        self.save_workspace(window, cx);
        self.set_window_identity(Some(dir.clone()), window, cx);
        cx.update_global::<Shared, _>(|shared, _| {
            shared.recents.touch(dir);
            // Recents are a convenience, not state the app depends on:
            // a failed write is logged, never surfaced or fatal.
            if let Err(e) = shared.recents.save() {
                eprintln!("onehand: failed to save recents: {e}");
            }
        });
        cx.notify();
    }

    /// Update what the process-wide registry thinks this window is showing.
    ///
    /// Canonicalized on the way in, exactly as `open_or_focus` does, so a
    /// symlinked or `..`-laden path binds to the same identity it would be
    /// looked up by.
    fn set_window_identity(
        &mut self,
        dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = dir.map(|d| std::fs::canonicalize(&d).unwrap_or(d));
        let handle = window.window_handle();
        cx.update_global::<Shared, _>(|shared, _| {
            if let Some(entry) = shared
                .windows
                .iter_mut()
                .find(|w| w.handle.window_id() == handle.window_id())
            {
                entry.storage_dir = dir;
            }
        });
    }

    /// Bind this workspace to a storage folder, chosen with the native picker.
    ///
    /// **Overwrite guard:** a folder that already holds another workspace's
    /// `onehand-workspace.toml` is never overwritten -- that workspace is opened
    /// (or its window focused) and this one is left as it was. Binding a folder
    /// is a choice about *this* workspace; it must not be a way to lose another
    /// one.
    pub fn pick_storage_dir(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };

            let existing = cx
                .background_executor()
                .spawn({
                    let dir = dir.clone();
                    async move { WorkspaceConfig::load_from(&dir) }
                })
                .await;

            cx.update(|cx| match existing {
                Load::Found(cfg) => open_or_focus(Workspace::from_config(cfg, dir), cx),
                // Something is in that folder and we could not read it. The
                // overwrite guard has to cover this case too, or a workspace
                // config with one bad character is a workspace deleted by a
                // folder picker.
                Load::Unreadable => {
                    shell
                        .update_in(cx, |_, window, cx| {
                            window.push_notification(
                                Notification::error(format!(
                                    "{} already holds a workspace config that cannot be read — \
                                     nothing was changed",
                                    dir.display()
                                )),
                                cx,
                            );
                            cx.notify();
                        })
                        .ok();
                }
                Load::Missing => {
                    shell
                        .update_in(cx, |shell, window, cx| {
                            shell.bind_storage(dir.clone(), window, cx);
                        })
                        .ok();
                }
            });
        })
        .detach();
    }

    /// Pick a folder and open it as a *new* workspace in its own window.
    pub fn new_workspace(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |_, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };
            cx.update(|cx| open_or_focus(Workspace::seeded(dir), cx));
        })
        .detach();
    }

    /// Pick a storage directory holding an `onehand-workspace.toml` and open it.
    pub fn open_workspace(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };
            let loaded = cx
                .background_executor()
                .spawn({
                    let dir = dir.clone();
                    async move { WorkspaceConfig::load_from(&dir) }
                })
                .await;

            match loaded {
                Load::Found(cfg) => {
                    cx.update(|cx| open_or_focus(Workspace::from_config(cfg, dir), cx));
                }
                // Two different sentences on purpose: "there is nothing here"
                // sends the user to a different folder, "I could not read what
                // is here" sends them to fix the file they meant to open.
                Load::Missing | Load::Unreadable => {
                    let missing = matches!(loaded, Load::Missing);
                    shell
                        .update_in(cx, |_, window, cx| {
                            window.push_notification(
                                if missing {
                                    Notification::warning(
                                        "No onehand-workspace.toml in that folder",
                                    )
                                } else {
                                    Notification::error(
                                        "That folder's onehand-workspace.toml could not be read",
                                    )
                                },
                                cx,
                            );
                            cx.notify();
                        })
                        .ok();
                }
            }
        })
        .detach();
    }

    /// Recently opened storage directories, most-recent-first.
    pub fn recents(&self, cx: &App) -> Vec<std::path::PathBuf> {
        Shared::global(cx).recents.recent_workspaces.clone()
    }

    /// Open a recents row: read its config off the UI loop, then funnel into
    /// the single open path so dedup-focus applies here too.
    pub fn open_recent(&mut self, dir: std::path::PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let loaded = cx
                .background_executor()
                .spawn({
                    let dir = dir.clone();
                    async move { WorkspaceConfig::load_from(&dir) }
                })
                .await;

            match loaded {
                Load::Found(cfg) => {
                    cx.update(|cx| open_or_focus(Workspace::from_config(cfg, dir), cx));
                }
                // Gone for good: no such file. Stale, not an error the user
                // caused, so drop it rather than nag.
                Load::Missing => {
                    shell
                        .update(cx, |_shell, cx| {
                            cx.update_global::<Shared, _>(|shared, _| {
                                shared.recents.forget(&dir);
                                let _ = shared.recents.save();
                            });
                            cx.notify();
                        })
                        .ok();
                }
                // Present but unreadable -- an unmounted share, a permission
                // blip, a TOML typo. Every one of those is recoverable, and
                // forgetting the recent is not: say so and keep the row.
                Load::Unreadable => {
                    shell
                        .update_in(cx, |_, window, cx| {
                            window.push_notification(
                                Notification::error(format!(
                                    "Could not read the workspace in {} — the entry was kept",
                                    dir.display()
                                )),
                                cx,
                            );
                            cx.notify();
                        })
                        .ok();
                }
            }
        })
        .detach();
    }

    /// Write the workspace back to its storage folder, if it has one.
    fn save_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.window.workspace.storage_dir.clone() else {
            return;
        };
        // Every write picks the arrangement up here rather than at each call
        // site. Otherwise the *first* save after binding a storage folder --
        // which is a save the user triggered by choosing the folder, not by
        // touching a panel -- would write the arrangement this window started
        // with instead of the one on screen, and the next launch would restore
        // that.
        let layout = self.dock_layout(cx);
        self.window.workspace.layout = layout;

        if let Err(e) = self.window.workspace.to_config().save_to(&dir) {
            window.push_notification(
                Notification::error(format!("Workspace not saved — {e}")),
                cx,
            );
        }
    }

    /// Re-read everything derived from the files on disk.
    ///
    /// Two panels read the working tree and neither notices it change: the
    /// rail's `branch · N changed` and the Files tree. Both were seeded once
    /// and left, which made them stale from the first turn
    /// onwards -- the moment they start being worth reading.
    ///
    /// One call for both, because there is one cause. Cheap enough to run on
    /// every turn end: `git status` is one process per root off the UI loop,
    /// and the rescan covers only the directories currently on screen.
    fn refresh_worktree(&mut self, cx: &mut Context<Self>) {
        self.refresh_git(cx);
        self.workbench.update(cx, |panel, cx| panel.rescan(cx));
    }

    /// Refresh `git status` for every root, off the UI loop.
    ///
    /// Uses core's blocking reader on GPUI's background executor rather than
    /// its tokio wrapper: tokio's process driver is not running under GPUI, so
    /// the async path would panic looking for a reactor.
    pub fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let roots = self
            .window
            .workspace
            .roots
            .iter()
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();

        // Two refreshes can be in flight at once -- a turn ends while the
        // window is being activated, say -- and `git status` on a big repo does
        // not finish in the order it was asked for. Without this, the slower
        // (older) scan lands last and overwrites the newer snapshot.
        self.git_generation = self.git_generation.wrapping_add(1);
        let generation = self.git_generation;

        cx.spawn(async move |shell, cx| {
            let scanned = cx
                .background_executor()
                .spawn(async move {
                    roots
                        .into_iter()
                        .filter_map(|root| {
                            gitstat::read_blocking(&root).map(|status| (root, status))
                        })
                        .collect::<Vec<_>>()
                })
                .await;

            shell
                .update(cx, |shell, cx| {
                    if shell.git_generation != generation {
                        return;
                    }
                    shell.window.git = scanned.into_iter().collect();
                    let git = shell.window.git.clone();
                    shell
                        .workbench
                        .update(cx, |panel, cx| panel.set_git(git, cx));
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A panel maximized in the app direction is the whole window: the rail
        // is not rendered at all rather than rendered at zero width, so
        // nothing of it can catch a click along the edge.
        let rail = (self.app_maximized.is_none() && !self.rail_hidden)
            .then(|| crate::rail::rail(self, &self.window, cx));
        // Gone for the same reason and in the same direction: maximizing a
        // panel in the app direction means the frame *is* that panel, and a
        // strip of chrome across the bottom is the one thing that would still
        // not be it.
        let status = self
            .app_maximized
            .is_none()
            .then(|| crate::statusbar::status_bar(self, &self.window, cx));
        let dock = div().size_full().child(self.dock.clone());
        // With no rail there is no split to drag, so there is no split: an
        // `h_resizable` holding one panel would draw a handle against the
        // window's edge that resizes nothing.
        let body = match rail {
            None => dock.into_any_element(),
            Some(rail) => h_resizable("rail-split")
                .with_state(&self.rail_split)
                .child(
                    // `flex_none`: the panel sets `flex_grow: 1` on itself, and
                    // a rail that grows is a rail that takes whatever the dock
                    // is not using -- which is most of the window.
                    resizable_panel()
                        .size(px(self.window.workspace.layout.rail_w))
                        .size_range(px(PanelLayout::RAIL_MIN)..px(PanelLayout::RAIL_MAX))
                        .flex_none()
                        .child(rail),
                )
                .child(resizable_panel().child(dock))
                // The drag emits per frame, so this goes through the same
                // debounce the dock's own resizing does rather than writing the
                // workspace file on every pixel.
                .on_resize(
                    cx.listener(|shell: &mut Self, _: &Entity<ResizableState>, _, cx| {
                        shell.save_workspace_soon(cx);
                    }),
                )
                .into_any_element(),
        };
        // The status bar sits under the rail *and* the dock, so the frame is a
        // column: the two of them together are its first child, and the bar its
        // second. `min_h_0` is what keeps the bar on screen -- without it the
        // body takes its content's height as a floor and pushes a fixed-height
        // strip off the bottom edge of a short window. The body sizes itself
        // from here (both forms of it are `size_full`), so this wrapper sets no
        // direction of its own.
        let body = div().flex_1().min_h_0().w_full().child(body);
        let frame = div()
            .size_full()
            .v_flex()
            .key_context("Shell")
            .on_action(cx.listener(|shell: &mut Self, _: &ToggleRail, _, cx| {
                shell.toggle_rail(cx);
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleFiles, window, cx| {
                    shell.show_workbench(WorkbenchMode::Files, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleWorkbench, window, cx| {
                    shell.show_workbench(WorkbenchMode::Editor, window, cx);
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &SaveFile, _, cx| {
                shell
                    .workbench
                    .update(cx, |panel, cx| panel.save_active(cx));
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleTerminal, window, cx| {
                    shell.show_terminal(window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &FocusComposer, window, cx| {
                    shell.last_panel = FocusedPanel::Chat;
                    shell
                        .chat
                        .update(cx, |pane, cx| pane.focus_composer(window, cx));
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &ToggleFind, window, cx| {
                shell.last_panel = FocusedPanel::Chat;
                shell
                    .chat
                    .update(cx, |pane, cx| pane.toggle_find(window, cx));
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &RestartSession, window, cx| {
                    shell.restart_session(window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &CloseSession, window, cx| {
                    shell.close_active_session(window, cx);
                }),
            )
            .on_action(cx.listener(|shell: &mut Self, _: &ZoomIn, window, cx| {
                shell.zoom(ZoomStep::In, window, cx);
            }))
            .on_action(cx.listener(|shell: &mut Self, _: &ZoomOut, window, cx| {
                shell.zoom(ZoomStep::Out, window, cx);
            }))
            .on_action(cx.listener(|shell: &mut Self, _: &ZoomReset, window, cx| {
                shell.zoom(ZoomStep::Reset, window, cx);
            }))
            .on_action(
                cx.listener(|shell: &mut Self, _: &NextSession, window, cx| {
                    shell.cycle_session(true, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &PrevSession, window, cx| {
                    shell.cycle_session(false, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, action: &SelectSession, window, cx| {
                    shell.select_session(action.index, window, cx);
                }),
            )
            .on_action(
                cx.listener(|shell: &mut Self, _: &ToggleMaximize, window, cx| {
                    shell.toggle_maximize(window, cx);
                }),
            )
            // Releasing Ctrl is what commits a `Ctrl+Tab` cycle, so the walk
            // has to see modifier changes and not just keys.
            .on_modifiers_changed(cx.listener(
                |shell: &mut Self, event: &gpui::ModifiersChangedEvent, _, cx| {
                    if !event.modifiers.control {
                        shell.end_cycle(cx);
                    }
                },
            ))
            .child(body)
            .children(status);

        // `Root` stores dialogs, sheets and notifications; it does not draw
        // them. Its own `render` puts up the view, the tooltip overlay and the
        // native-menu overlay and stops -- every layer below is the app's to
        // mount, and until this existed `Dialog::trigger` opened a dialog into
        // a list nobody read. That is why the settings, agent-manager and Help
        // dialogs did nothing when clicked.
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .size_full()
            .relative()
            .child(frame)
            // Above the frame, below the library's own layers: this one is
            // rendered by the shell rather than stored in `Root`, because it is
            // opened by a menu entry rather than by a control that can carry a
            // `Dialog::trigger`.
            .children(
                self.renaming
                    .map(|_| crate::dialogs::rename_session(self, cx)),
            )
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}

/// Seed the first workspace: the positional CLI argument is the project root,
/// else the current directory.
pub fn seed_workspace() -> Workspace {
    let root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    Workspace::seeded(root)
}

/// Run the native folder picker off the UI loop.
async fn pick_folder(cx: &mut gpui::AsyncApp) -> Option<std::path::PathBuf> {
    cx.background_executor()
        .spawn(async {
            rfd::FileDialog::new()
                .pick_folder()
                .map(workspace::canon_dir)
        })
        .await
}

/// Open `workspace` in a new window -- unless a window already shows that
/// storage directory, in which case focus it. Storage dirs are canonicalized on
/// the way in, so symlink and `..` aliases deduplicate.
pub fn open_or_focus(workspace: Workspace, cx: &mut App) {
    if let Some(dir) = workspace.storage_dir.clone() {
        if let Some(handle) = Shared::global(cx).window_for(&dir) {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .ok();
            return;
        }
        cx.update_global::<Shared, _>(|shared, _| {
            shared.recents.touch(dir);
            if let Err(e) = shared.recents.save() {
                eprintln!("onehand: failed to save recents: {e}");
            }
        });
    }
    open_window(workspace, cx);
}

/// Open a window for `workspace` and register it.
fn open_window(workspace: Workspace, cx: &mut App) {
    let storage_dir = workspace.storage_dir.clone();
    cx.spawn(async move |cx| {
        let handle = cx
            .open_window(Default::default(), |window, cx| {
                let shell = cx.new(|cx| {
                    let mut shell = Shell::new(workspace, window, cx);
                    shell.refresh_git(cx);
                    // Point every panel at the seeded root before the first
                    // frame. Sessions are not persisted, so there is never one
                    // to connect here -- what this settles is which project the
                    // Workbench, the terminal and the empty chat are about,
                    // which was nothing at all until the first rail click.
                    shell.show_active_session(window, cx);
                    shell
                });
                cx.new(|cx| Root::new(shell, window, cx))
            })
            .expect("failed to open window");

        cx.update(|cx| {
            cx.update_global::<Shared, _>(|shared, _| {
                shared.windows.push(OpenWindow {
                    storage_dir,
                    handle: handle.into(),
                });
            });
        });
    })
    .detach();
}

/// Install global state and open the first window.
pub fn boot(cx: &mut App) {
    let (cfg, config_path) = AppConfig::load_resolved();
    let mono = cfg.font.monospace.clone();
    let appearance = cfg.appearance;
    cx.set_global(Shared::from_config(cfg, config_path));
    init_keymap(cx);
    // Before a mode is chosen, because choosing one applies whichever of the
    // two configs this installs.
    crate::theme::install(cx);
    // Before the font scan, not after: choosing a mode loads a whole theme
    // config, and one naming a font family would land on top of whatever the
    // scan had just resolved.
    apply_appearance(appearance, None, cx);
    use_installed_mono(mono.as_deref(), cx);
    watch_window_close(cx);

    // The most recent storage dir wins over the CLI seed -- reopening where the
    // user left off beats reopening where the shortcut points; a broken or
    // missing recent falls straight through.
    let recent = Shared::global(cx)
        .recents
        .recent_workspaces
        .first()
        .cloned()
        .and_then(|dir| {
            WorkspaceConfig::load_from(&dir)
                .found()
                .map(|cfg| Workspace::from_config(cfg, dir))
        });

    open_or_focus(recent.unwrap_or_else(seed_workspace), cx);
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
fn apply_appearance(choice: Appearance, window: Option<&mut Window>, cx: &mut App) {
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

    if let Some(family) = Shared::global(cx).mono_family.clone() {
        Theme::global_mut(cx).mono_font_family = family.into();
    }
    cx.refresh_windows();
}

/// Point the theme's monospace family at one this machine actually has.
///
/// Everything the transcript sets in mono — diffs, commands, terminal output,
/// `IN`/`OUT` wells, the permission card's command — asks for it by
/// `cx.theme().mono_font_family`, and a family the system does not have is not
/// an error: the text simply comes out in the body face, with nothing on screen
/// or in the log to say the request went nowhere. The component library's
/// default is one hard-coded name per platform, and on Linux that name is
/// DejaVu Sans Mono, which many distributions do not ship — so on those
/// machines every well in the transcript was sans while the code that drew it
/// was, correctly, asking for mono.
///
/// The scan is done once at boot, before any window exists, because the theme
/// is a global — but the answer is kept, since changing the appearance loads a
/// fresh theme config and [`apply_appearance`] has to put it back.
/// `[font].monospace` from the config file is the first preference — this is
/// the one part of that section the app now reads; `size`, `scale`, `sans` and
/// `fallbacks` still parse and go nowhere.
fn use_installed_mono(configured: Option<&str>, cx: &mut App) {
    let installed = cx.text_system().all_font_names();
    // The theme's own default is a preference too, not a thing to override:
    // where it resolves, it is what this platform's users expect to read code
    // in, and it should win over anything merely popular.
    let default = gpui_component::ActiveTheme::theme(cx)
        .mono_font_family
        .to_string();
    let preferred = configured
        .into_iter()
        .chain(std::iter::once(default.as_str()));

    if let Some(family) = onehand_core::config::resolve_monospace(preferred, &installed) {
        gpui_component::Theme::global_mut(cx).mono_font_family = family.clone().into();
        cx.update_global::<Shared, _>(|shared, _| shared.mono_family = Some(family));
    }
}

/// Keep the window registry honest, and end the process with the last window.
///
/// Two jobs, one hook, because they are the same fact arriving:
///
/// - **Prune.** `Shared.windows` exists so that opening a workspace already on
///   screen focuses it instead of duplicating it. A closed window left in that
///   list makes `window_for` hand back a dead handle, and `open_or_focus` then
///   "focuses" nothing and returns -- so *Open workspace…* on a folder whose
///   window was closed silently does nothing at all.
/// - **Quit.** GPUI's Linux backend only stops its event loop when `quit()` is
///   called (`gpui_linux::platform::LinuxPlatform::quit`); there is no
///   quit-on-last-window default. Without this the process outlives its last
///   window, with no UI and no way to reach it.
fn watch_window_close(cx: &mut App) {
    cx.on_window_closed(|cx, closed| {
        cx.update_global::<Shared, _>(|shared, _| {
            shared.windows.retain(|w| w.handle.window_id() != closed);
        });
        // The window is already gone by the time this runs, so the count is
        // the count *after* the close.
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

/// A `Ctrl+Tab` walk in progress.
struct TabCycle {
    /// Recency order, frozen for the walk's life.
    order: Vec<u64>,
    /// Where in `order` the walk currently is.
    pos: usize,
}

/// The shell entity type, for callers that need to name it.
pub type ShellEntity = Entity<Shell>;
