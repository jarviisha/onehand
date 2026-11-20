//! The bottom terminal panel: the user's own shell.
//!
//! Distinct from the ACP terminals the agent runs — those are a byte stream the
//! transcript renders (`chat::transcript`) and need no PTY widget at all. This
//! is the one the user types into.
//!
//! Scope is **per project root**: each root owns its tab set, and switching
//! roots swaps the whole thing — a shell belongs to a project, not to a window.
//!
//! Every tab here is a login shell. **Neovim is not one of them** — it is a mode
//! of the Workbench, because that is the panel about files, and a tab named
//! `nvim` sitting between two called `zsh` says the editor is a kind of shell.
//! PTY/grid ownership is shared with the Neovim crate through
//! `onehand-terminal-ui`.
//!
//! A project on a host has its shells there, run by the host service and
//! read over the host's attach (`host`): they outlive this window, so a tab
//! is a view of one rather than its owner.

mod host;
mod lock;
mod shells;

pub(crate) use shells::{CloseShellRow, ShellRowDown, ShellRowUp};

use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, ScrollHandle, Styled, Window, div,
};
use gpui_base::dock::{Panel, PanelEvent};
use gpui_component::StyledExt;
use gpui_component::button::ButtonVariants as _;
use onehand_core::host::client::HostConn;
use onehand_core::host::wire::HostTerm;
use onehand_core::workspace::HostName;
use onehand_terminal_ui::{Program, PtyTab, TerminalThemeKey, spawn_pty, terminal_palette};
use std::collections::HashMap;
use std::path::PathBuf;

/// Where a tab set belongs: a project's folder, with the host it is on. A
/// folder on a host and one here of the same path are two projects, and
/// neither's shells are the other's.
type Home = (Option<HostName>, PathBuf);

/// The tab set one project root has open.
#[derive(Default)]
struct RootShells {
    tabs: Vec<Tab>,
    active: usize,
    /// The id the next tab takes.
    next: u64,
}

/// One shell, and the id it keeps for its life.
///
/// **By id and not by place**, because a question can be on screen while the
/// strip changes under it: a tab before it exits, or the window moves to
/// another project. The answer looks its tab up again by the id it was asked
/// about, and a place would by then name a different shell.
struct Tab {
    id: u64,
    shell: PtyTab,
    /// The host's terminal this tab shows, and the attach it is read over.
    on_host: Option<(HostConn, HostTerm)>,
    /// The shell's row in the dock's list, a tab stop, so the keyboard can
    /// stand on a row and its close shows there.
    row: FocusHandle,
}

impl RootShells {
    /// Add `shell` as the newest tab, and select it.
    fn push(&mut self, shell: PtyTab, on_host: Option<(HostConn, HostTerm)>, row: FocusHandle) {
        self.tabs.push(Tab {
            id: self.next,
            shell,
            on_host,
            row,
        });
        self.next += 1;
        self.active = self.tabs.len() - 1;
    }

    /// Drop every tab `goes` answers true for, and keep the selection on the
    /// same *shell* it was on.
    ///
    /// **One function because the index arithmetic is the whole thing, and it
    /// was wrong in both places that had written it out.** Clamping the old
    /// index into the shorter list keeps a number, not a shell: with three tabs
    /// and the middle one selected, closing the *first* left the index at 1 and
    /// the panel showing the third — the one shell the user had not been
    /// looking at and had not asked to close. What the index has to lose is one
    /// step per tab removed ahead of it, and the clamp is only for the case
    /// where the selected tab is itself among them.
    ///
    /// Answers whether anything went, since both callers have work to do only
    /// then.
    fn drop_where(&mut self, mut goes: impl FnMut(usize, &mut Tab) -> bool) -> bool {
        let mut dropped = Vec::new();
        let mut ix = 0;
        self.tabs.retain_mut(|tab| {
            let drop = goes(ix, tab);
            if drop {
                dropped.push(ix);
            }
            ix += 1;
            !drop
        });
        if dropped.is_empty() {
            return false;
        }
        self.active = selection_after(self.active, &dropped, self.tabs.len());
        true
    }
}

/// Where the selection lands once `dropped` have gone and `left` remain.
///
/// Separate and pure because it is the rule rather than the removal, and a rule
/// about which tab the user is looking at is exactly the kind that regresses in
/// silence -- nothing crashes, the panel simply shows a different shell than the
/// one it was showing, which reads as having clicked something.
fn selection_after(active: usize, dropped: &[usize], left: usize) -> usize {
    let ahead = dropped.iter().filter(|&&ix| ix < active).count();
    (active - ahead).min(left.saturating_sub(1))
}

/// Whether the shells' busy state the watch last saw differs from `now`'s:
/// any one shell starting or ending a command, or a shell gone or added.
/// Pure so a test can pin the case the dock-wide answer misses, a second
/// shell turning busy while the first already is.
fn busy_changed(seen: &[(u64, bool)], now: &[(u64, bool)]) -> bool {
    seen != now
}

/// Where the tab `id` is now in `ids`, if it is still there.
///
/// Pure for the reason [`selection_after`] is: a closing question answered
/// after the strip changed has to find the shell it asked about, or nothing.
fn place_of(ids: &[u64], id: u64) -> Option<usize> {
    ids.iter().position(|&each| each == id)
}

pub struct TerminalPanel {
    focus_handle: FocusHandle,
    root: Option<Home>,
    shells: HashMap<Home, RootShells>,
    /// How the attach for the host project on screen is coming along.
    host: host::Reach,
    /// Surfaced when a shell cannot be started at all.
    status: Option<String>,
    /// The grid's reading size. Not a rem scale like the other panels: a
    /// terminal is a measured glyph grid, so the zoom is a font size, and
    /// changing it re-measures the cell and resizes the PTY.
    zoom: crate::zoom::Zoom,
    terminal_theme: TerminalThemeKey,
    /// Whether this panel is the one currently blown up to the whole frame.
    ///
    /// Pushed down by the shell, which owns the `DockArea` and the fact. Here
    /// only because the strip draws the control that toggles it, and a toggle
    /// that cannot see its own state draws the same icon in both — so pressing
    /// it to restore looks like pressing it to maximize again.
    maximized: bool,
    /// Each shell of the active set, by id, with whether a command was in
    /// its foreground when the watch last looked, so the watch notifies
    /// only when one of them changed: a row's dot is per shell, and the
    /// dock-wide answer does not flip while another shell stays busy.
    running_seen: Vec<(u64, bool)>,
    /// Whether the watch on [`Self::running`] is going: from the first shell
    /// opened until no project has one left.
    watching: bool,
    /// The folders here nothing may be typed into now, with why.
    locked: HashMap<PathBuf, String>,
    /// The shell list's scroll, and the project, shell and count it was
    /// last scrolled for: a new or chosen shell is brought into view once,
    /// so the list stays where the pointer scrolled it until the next.
    tab_scroll: ScrollHandle,
    tab_shown: Option<(Home, usize, usize)>,
}

/// How often the watch asks the shells whether a command is in front.
///
/// Asked on a clock, not on output, because neither half of the question is
/// tied to output: a command can start or end without printing a byte, and a
/// grid behind a closed dock stops notifying after its last draw. One ioctl
/// per shell, and nothing redraws unless the answer changed.
const RUNNING_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

impl TerminalPanel {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            root: None,
            shells: HashMap::new(),
            host: host::Reach::default(),
            status: None,
            zoom: crate::zoom::Zoom::default(),
            terminal_theme: TerminalThemeKey::current(cx),
            maximized: false,
            running_seen: Vec::new(),
            watching: false,
            tab_scroll: ScrollHandle::new(),
            tab_shown: None,
            locked: HashMap::new(),
        })
    }

    /// Start watching [`Self::running`], unless the watch is already going.
    ///
    /// Nothing else about the panel changes when a command starts or ends in
    /// a shell, so the header's dot -- shown only while the dock is closed --
    /// would keep whatever it last read. The watch notifies this panel when
    /// the answer flips, which is what the shell observes. It ends once no
    /// project has a shell, and with the panel.
    fn watch_running(&mut self, cx: &mut Context<Self>) {
        if self.watching {
            return;
        }
        self.watching = true;
        cx.spawn(async move |panel, cx| {
            loop {
                cx.background_executor().timer(RUNNING_EVERY).await;
                let go_on = panel.update(cx, |panel: &mut Self, cx| {
                    if panel.shells.values().all(|set| set.tabs.is_empty()) {
                        panel.watching = false;
                        panel.running_seen.clear();
                        return false;
                    }
                    let busy = panel.busy_shells();
                    if busy_changed(&panel.running_seen, &busy) {
                        panel.running_seen = busy;
                        cx.notify();
                    }
                    true
                });
                if !matches!(go_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Take the shell's word for whether this panel fills the frame.
    ///
    /// Guarded on both sides: the shell pushes this from paths that run whether
    /// or not anything changed, and a repaint here is the whole window.
    pub fn set_maximized(&mut self, maximized: bool, cx: &mut Context<Self>) {
        if self.maximized == maximized {
            return;
        }
        self.maximized = maximized;
        cx.notify();
    }

    /// How many shells are open on the folder `root` of this machine.
    pub fn shells_in(&self, root: &std::path::Path) -> usize {
        self.shells
            .get(&(None, root.to_path_buf()))
            .map_or(0, |shells| shells.tabs.len())
    }

    /// Every shell's process id, with the project it was opened on. A tab on
    /// a host has no process here, so none is listed for it.
    pub fn processes(&self) -> Vec<(PathBuf, u32)> {
        self.shells
            .iter()
            .flat_map(|((_, root), shells)| {
                shells
                    .tabs
                    .iter()
                    .filter_map(|tab| Some((root.clone(), tab.shell.process_id()?)))
            })
            .collect()
    }

    /// The folder whose shells are on show.
    #[cfg(test)]
    pub(crate) fn root(&self) -> Option<&std::path::Path> {
        self.root.as_ref().map(|(_, root)| root.as_path())
    }

    /// Show the shells of `root`, on `host` when it is there: those the
    /// host keeps for it are asked for, and opened as tabs.
    pub fn set_root(&mut self, root: PathBuf, host: Option<HostName>, cx: &mut Context<Self>) {
        let home = (host, root);
        // What was asked of another project's host, and a shell asked for
        // there, are not this one's.
        if self.root.as_ref() != Some(&home) {
            self.host = host::Reach::default();
        }
        self.root = Some(home);
        self.show_host(cx);
        cx.notify();
    }

    /// Point at no project at all.
    pub fn clear_root(&mut self, cx: &mut Context<Self>) {
        self.root = None;
        self.host = host::Reach::default();
        cx.notify();
    }

    /// Open a shell on the active root.
    ///
    /// Spawned lazily: a workspace with a dozen roots must not start a dozen
    /// shells nobody asked for. Lazy about *roots*, though, and not about the
    /// clock — a launch that restores an open dock does start the one shell
    /// that dock is for (`Shell::fill_open_terminal`), because leaving the
    /// terminal open is the request, and greeting it with a button asking
    /// whether a terminal is wanted asks a question already answered.
    ///
    /// Always a new one — shells are what somebody opens several of on purpose,
    /// one per thing they are watching.
    pub fn open_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(home) = self.root.clone() else {
            return;
        };
        let root = match &home {
            (Some(_), _) => return self.open_on_host(window.window_handle(), cx),
            (None, root) => root.clone(),
        };

        let panel = cx.entity().downgrade();
        let spawned = spawn_pty(
            &root,
            Program::Shell,
            crate::zoom::term_font_size(self.zoom),
            crate::theme::dock_surface(cx),
            cx,
            move |window, cx| {
                let _ = panel.update(cx, |panel: &mut Self, cx| panel.reap(window, cx));
            },
        );
        // Beside open tabs a failed start is a press that failed, said once
        // as a toast; with none, the dock has nothing to show but why, which
        // stands under its *New terminal* until a start goes through.
        let had_tabs = self.has_shell();
        match spawned {
            Ok(shell) => {
                self.shells.entry(home).or_default().push(
                    shell,
                    None,
                    cx.focus_handle().tab_stop(true),
                );
                self.apply_locks();
                self.status = None;
                self.watch_running(cx);
            }
            Err(e) if had_tabs => {
                use gpui_component::WindowExt as _;
                window.push_notification(crate::toast::failed("Opening a shell", e), cx);
            }
            Err(e) => self.status = Some(e),
        }
        cx.notify();
    }

    /// Drop every tab whose shell has exited.
    ///
    /// Reached from a tab announcing its own death, but written as a sweep over
    /// all of them rather than a removal of the one that spoke: `finished` asks
    /// the process, so a child that died without anything noticing — killed from
    /// another terminal, or gone while its root was off screen — is collected
    /// on the next sweep rather than left as a grid nobody can type into.
    ///
    /// The caret is the other half. A grid dropped while it holds focus leaves
    /// the window pointing at an element no frame contains, and GPUI resolves a
    /// key along the path down to the *focused* node — so every shortcut,
    /// including the one that would reopen this panel, stops working. Focus is
    /// only moved when it was inside this panel to begin with: a shell exiting
    /// in the background must not take the caret away from whatever the user is
    /// actually doing.
    pub fn reap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let held_focus = self.focus_handle.contains_focused(window, cx);
        let mut removed = false;
        for set in self.shells.values_mut() {
            removed |= set.drop_where(|_, tab| tab.shell.finished());
        }
        if !removed {
            return;
        }
        if held_focus {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Close tab `idx` of the project on screen, ending its shell — at once
    /// when it sits at its prompt, after asking when a command is running in
    /// it or the system will not say.
    ///
    /// **A running command is never ended unasked**: a build, a server, an
    /// editor or an `ssh` session is work the ✕ beside it does not name.
    pub fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(tab) = self.shells.get(&root).and_then(|set| set.tabs.get(idx)) else {
            return;
        };
        let id = tab.id;
        let description = match tab.shell.busy() {
            Some(false) => return self.close_tab_id(&root, id, window, cx),
            Some(true) => "A command is still running in it. Closing the terminal ends it.",
            None => {
                "Whether a command is running in it cannot be told. Closing the terminal ends \
                 whatever is."
            }
        };
        crate::shell::confirm_on(
            cx.entity(),
            crate::shell::Confirm {
                id: "close-terminal",
                title: "Close this terminal?".into(),
                description: description.into(),
                act: "Close",
                ..Default::default()
            },
            window,
            cx,
            move |panel, window, cx| panel.close_tab_id(&root, id, window, cx),
        );
    }

    /// Close the tab `id` of `root`'s set, ending its shell, if it is still
    /// there: one that exited or was closed while the question was open is
    /// gone, and nothing else closes in its place.
    ///
    /// Takes the window for the same reason [`Self::reap`] does, and it is not
    /// optional here: the ✕ is pressed with the caret in the very grid about to
    /// be dropped, and a focused entity that leaves the frame leaves the window
    /// pointing at a node no frame contains — which takes every shortcut with
    /// it, including the one that would reopen this panel. The exit callback
    /// cannot cover it, because a grid that is no longer drawn never runs one.
    /// Asked *before* the drop, since a handle that is not on screen cannot
    /// answer, and only acted on when focus was inside this panel to begin
    /// with.
    fn close_tab_id(&mut self, root: &Home, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let held_focus = self.focus_handle.contains_focused(window, cx);
        let Some(set) = self.shells.get_mut(root) else {
            return;
        };
        let ids: Vec<u64> = set.tabs.iter().map(|tab| tab.id).collect();
        let Some(at) = place_of(&ids, id) else {
            return;
        };
        // Dropping the tab drops its PTY, which ends the child. A host's
        // shell is asked to end, as nothing here holds it.
        if let Some((conn, term)) = &set.tabs[at].on_host {
            conn.term_close(*term);
        }
        let closed = set.drop_where(|i, _| i == at);
        if closed && held_focus && self.root.as_ref() == Some(root) {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    pub fn select_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(set) = self.root.as_ref().and_then(|r| self.shells.get_mut(r))
            && idx < set.tabs.len()
        {
            set.active = idx;
        }
        cx.notify();
    }

    /// Drop every shell this panel holds for `root`, on `host` if it is there.
    ///
    /// Dropping the tabs drops their PTYs, which is what ends the children --
    /// removing a project must not leave its shells running with nothing on
    /// screen pointing at them. A host's shells are only let go of: they run
    /// on there, for whoever adds the folder again.
    pub fn forget_root(
        &mut self,
        root: &std::path::Path,
        host: Option<HostName>,
        cx: &mut Context<Self>,
    ) {
        let home = (host, root.to_path_buf());
        // ponytail: a host tab's reader thread waits on until its terminal
        // next prints or exits, or the attach ends; an unwatch call would end
        // it at once if those threads ever pile up.
        self.shells.remove(&home);
        if self.root.as_ref() == Some(&home) {
            self.clear_root(cx);
        }
        cx.notify();
    }

    /// Step the grid's font size, re-measuring every live shell.
    ///
    /// Applied to the whole panel, not just the active tab: the tabs are one
    /// terminal in the user's head, and switching tabs must not switch size.
    /// Newly spawned shells pick the factor up in [`Self::open_shell`].
    pub fn set_zoom(&mut self, zoom: crate::zoom::Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        let size = crate::zoom::term_font_size(zoom);
        for set in self.shells.values() {
            for tab in &set.tabs {
                tab.shell.set_font_size(size, cx);
            }
        }
        cx.notify();
    }

    pub fn zoom(&self) -> crate::zoom::Zoom {
        self.zoom
    }

    /// Recolour every live grid after an app appearance change.
    fn sync_theme(&mut self, cx: &mut Context<Self>) {
        let current = TerminalThemeKey::current(cx);
        if self.terminal_theme == current {
            return;
        }
        self.terminal_theme = current;
        let colors = terminal_palette(crate::theme::dock_surface(cx), cx);
        for set in self.shells.values() {
            for tab in &set.tabs {
                tab.shell.set_palette(colors.clone(), cx);
            }
        }
    }

    /// Whether the active root already has a shell open, so a shortcut that
    /// reveals the panel can spawn the first one without spawning a second on
    /// every later press.
    pub fn has_shell(&self) -> bool {
        self.active_set().is_some_and(|set| !set.tabs.is_empty())
    }

    /// Whether a shell of the active root has a command in its foreground: a
    /// build, a server, an editor. A shell idle at its prompt is not running
    /// anything, and one whose system will not say is not counted either.
    pub fn running(&self) -> bool {
        self.busy_shells().iter().any(|&(_, busy)| busy)
    }

    /// Each shell of the active root by id, with whether a command is in its
    /// foreground; one whose system will not say counts as not.
    fn busy_shells(&self) -> Vec<(u64, bool)> {
        self.active_set().map_or_else(Vec::new, |set| {
            set.tabs
                .iter()
                .map(|tab| (tab.id, tab.shell.busy() == Some(true)))
                .collect()
        })
    }

    /// Focus the active shell, not the panel: the point of reaching for the
    /// terminal is to type into it.
    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        let handle = self
            .active_set()
            .and_then(|set| set.tabs.get(set.active))
            .map(|tab| tab.shell.view().read(cx).focus_handle().clone());
        match handle {
            Some(handle) => handle.focus(window, cx),
            None => self.focus_handle.focus(window, cx),
        }
    }

    fn active_set(&self) -> Option<&RootShells> {
        self.root.as_ref().and_then(|r| self.shells.get(r))
    }
}

/// The behaviour half of the dock's panel trait alone, as the conversation's
/// is: the strip of shells is this panel's chrome, and the dock draws none.
impl Panel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "Terminal"
    }
}

impl EventEmitter<PanelEvent> for TerminalPanel {}

/// What this panel asks the shell for, having no way to do it itself.
///
/// Taking the dock off screen is the shell's: it owns the `DockArea`, and it is
/// the half that files the open state under the project being left. One variant,
/// because there is exactly one thing the strip cannot do from in here.
pub enum TerminalPanelEvent {
    /// Put the dock away. Not *toggle* — the button is drawn only where the
    /// panel is already showing, so a press means one thing, and routing it
    /// through the three-state toggle would leave it focusing the terminal
    /// instead of closing it whenever the caret was somewhere else.
    Hide,
    /// Fill the frame with this panel, or put it back.
    ///
    /// A toggle where [`Self::Hide`] is not, and for the opposite reason: this
    /// button *is* drawn in both states, because a panel filling the frame
    /// still shows the strip it was pressed from.
    ///
    /// It names this panel, which the key it shares an effect with does not —
    /// `Ctrl+Shift+K` maximizes whatever holds the caret, and a control sitting
    /// in the terminal's own strip that blew up the conversation instead
    /// because that is where the user was typing would be a button lying about
    /// its own location.
    ToggleMaximize,
}

impl EventEmitter<TerminalPanelEvent> for TerminalPanel {}

impl Focusable for TerminalPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalPanel {
    /// The panel's key context. This is the one place the app deliberately
    /// steps *out* of the way: `Ctrl+S` is bound `Shell && !Terminal`, so a
    /// program in the PTY keeps it.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_theme(cx);
        div()
            .size_full()
            // Filling its dock's box, which paints the surface: the grid is
            // handed the same one as its background, so a shell is the panel
            // rather than a plate laid on it.
            .v_flex()
            .overflow_hidden()
            // Tracked here because this panel is mounted bare. A `TabPanel` calls
            // `track_focus` on the panel it holds, which is what normally puts
            // that handle in the focus tree; without a tab group nothing does,
            // and `contains_focused` would answer "no" however deep in the grid
            // the caret is -- silently pointing the three-state panel keymap and
            // every "which panel is this" question at some other panel.
            .track_focus(&self.focus_handle)
            .key_context("Terminal")
            .child(self.body(window, cx))
    }
}

impl TerminalPanel {
    fn body(&mut self, _: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Whether there *is* a root is a question about the selection, not
        // about the shell map: a root only gains an entry there once a shell
        // has been spawned in it. Reading the map instead would tell a project
        // that has simply never had a terminal opened -- which is every project
        // on the first frame after a restored layout reopens this dock -- that
        // it is not a project at all, and hide the button that would start one.
        // Whether there *is* a root is a question about the selection, not the
        // shell map, and it is the one case with nothing for a strip to sit
        // over: no project means no shell to start and nowhere to start it.
        if self.root.is_none() {
            return onehand_ui::hint("No project selected", cx);
        }

        let (active, count) = self
            .active_set()
            .map_or((0, 0), |set| (set.active, set.tabs.len()));
        // Asked once per change: the strip reaches the tab as it lays it
        // out, and keeps the asking until a frame has the tab.
        let shown = self.root.clone().map(|root| (root, active, count));
        if shown != self.tab_shown {
            self.tab_scroll.scroll_to_item(active);
            self.tab_shown = shown;
        }
        let set = self.active_set().filter(|set| !set.tabs.is_empty());
        let active = set.map_or(0, |set| set.active);
        let body = set
            .and_then(|set| set.tabs.get(active))
            .map(|tab| tab.shell.view().clone());
        let empty = body.is_none();
        let tabs = set.map_or(&[][..], |set| set.tabs.as_slice());
        let facts: Vec<_> = tabs
            .iter()
            .map(|tab| (tab.shell.label(), tab.shell.busy()))
            .collect();
        let rows = shells::shell_rows(&facts, active);
        let name = rows.get(active).map(|row| row.name.clone());
        let focus: Vec<FocusHandle> = tabs.iter().map(|tab| tab.row.clone()).collect();
        let list = shells::lists_shells(rows.len()).then(|| self.shell_list(rows, focus, cx));

        div()
            .size_full()
            .v_flex()
            .child(self.top_bar(name, cx))
            .children(body.map(|view| {
                div()
                    .flex_1()
                    .min_h_0()
                    .h_flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            // The grid draws from its own top-left corner
                            // outward, so without an inset the first column
                            // sits against the dock's edge. The view measures
                            // its own bounds and reports the cell count back
                            // through the PTY resize, so this costs the shell
                            // a column and a row rather than painting over them.
                            .px_3()
                            .py_2()
                            .child(view),
                    )
                    .children(list)
                    .into_any_element()
            }))
            .children(self.locked_line(cx))
            // No shells on this root. **The strip above stays**, and that is the
            // whole point of putting this here rather than returning early: the
            // way out of the dock lives on that row, so closing the last shell
            // used to take it away and leave an open panel with no control
            // inside it to close -- the state the chevron was added for, reached
            // by using the ✕ beside it.
            // A host project with no tabs yet: what its attach waits on, or
            // why it failed, stands where *New terminal* would.
            .when_some(
                empty.then(|| self.host_said(cx)).flatten(),
                |panel, said| panel.child(said),
            )
            .when(empty && self.host.is_quiet(), |panel| {
                panel.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .v_flex()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            crate::controls::action("new-shell")
                                .short()
                                .primary()
                                .label("New terminal")
                                .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                                    panel.open_shell(window, cx);
                                })),
                        )
                        .children(self.status.clone().map(|status| {
                            onehand_ui::status_line(
                                format!("Opening a shell failed: {status}"),
                                crate::theme::status_ink(cx).danger,
                            )
                        })),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
