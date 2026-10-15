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
//! PTY/grid ownership is shared with the Neovim plugin through
//! `onehand-terminal-ui`.

use crate::controls::Short as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_terminal_ui::{Program, PtyTab, TerminalThemeKey, spawn_pty, terminal_palette};
use std::collections::HashMap;
use std::path::PathBuf;

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
}

impl RootShells {
    /// Add `shell` as the newest tab, and select it.
    fn push(&mut self, shell: PtyTab) {
        self.tabs.push(Tab {
            id: self.next,
            shell,
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
    fn drop_where(&mut self, mut goes: impl FnMut(usize, &mut PtyTab) -> bool) -> bool {
        let mut dropped = Vec::new();
        let mut ix = 0;
        self.tabs.retain_mut(|tab| {
            let drop = goes(ix, &mut tab.shell);
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

/// Where the tab `id` is now in `ids`, if it is still there.
///
/// Pure for the reason [`selection_after`] is: a closing question answered
/// after the strip changed has to find the shell it asked about, or nothing.
fn place_of(ids: &[u64], id: u64) -> Option<usize> {
    ids.iter().position(|&each| each == id)
}

pub struct TerminalPanel {
    focus_handle: FocusHandle,
    root: Option<PathBuf>,
    shells: HashMap<PathBuf, RootShells>,
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
    /// What [`Self::running`] answered when the watch last looked, so the
    /// watch notifies only when it flips.
    running_seen: bool,
    /// Whether the watch on [`Self::running`] is going: from the first shell
    /// opened until no project has one left.
    watching: bool,
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
            status: None,
            zoom: crate::zoom::Zoom::default(),
            terminal_theme: TerminalThemeKey::current(cx),
            maximized: false,
            running_seen: false,
            watching: false,
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
                        panel.running_seen = false;
                        return false;
                    }
                    let running = panel.running();
                    if running != panel.running_seen {
                        panel.running_seen = running;
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

    /// How many shells are open on `root`.
    pub fn shells_in(&self, root: &std::path::Path) -> usize {
        self.shells.get(root).map_or(0, |shells| shells.tabs.len())
    }

    /// Every shell's process id, with the project it was opened on.
    pub fn processes(&self) -> Vec<(PathBuf, u32)> {
        self.shells
            .iter()
            .flat_map(|(root, shells)| {
                shells
                    .tabs
                    .iter()
                    .filter_map(|tab| Some((root.clone(), tab.shell.process_id()?)))
            })
            .collect()
    }

    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.root = Some(root);
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
        let Some(root) = self.root.clone() else {
            return;
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
                self.shells.entry(root).or_default().push(shell);
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
            removed |= set.drop_where(|_, shell| shell.finished());
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
    fn close_tab_id(
        &mut self,
        root: &PathBuf,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let held_focus = self.focus_handle.contains_focused(window, cx);
        let Some(set) = self.shells.get_mut(root) else {
            return;
        };
        let ids: Vec<u64> = set.tabs.iter().map(|tab| tab.id).collect();
        let Some(at) = place_of(&ids, id) else {
            return;
        };
        // Dropping the tab drops its PTY, which ends the child.
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

    /// Drop every shell this panel holds for `root`.
    ///
    /// Dropping the tabs drops their PTYs, which is what ends the children --
    /// removing a project must not leave its shells running with nothing on
    /// screen pointing at them.
    pub fn forget_root(&mut self, root: &PathBuf, cx: &mut Context<Self>) {
        self.shells.remove(root);
        if self.root.as_ref() == Some(root) {
            self.root = None;
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
        self.active_set()
            .is_some_and(|set| set.tabs.iter().any(|tab| tab.shell.busy() == Some(true)))
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

impl Panel for TerminalPanel {
    fn panel_name(&self) -> &'static str {
        "Terminal"
    }

    /// No content-only maximize, because there is no tab bar to put the
    /// library's button on: this panel is mounted bare, the way the
    /// conversation is.
    ///
    /// The *app* direction is a different question and has an answer — the key,
    /// and a button in the strip below, which is this panel's own chrome rather
    /// than a tab bar it does not have. Filling the frame and filling the dock
    /// area are two controls, and only one of them needs somewhere the library
    /// will draw it.
    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Terminal")
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
            // A block of its own, held off the conversation above, the
            // Workbench beside it and the window's edges by a gap of the
            // reading surface, so a shell never reads as the conversation
            // running on. The gap belongs to the panel: a click in it is a
            // click on the terminal, and the dock's grip runs along its top.
            .p_2()
            .bg(cx.theme().background)
            // Tracked here because this panel is mounted bare. A `TabPanel` calls
            // `track_focus` on the panel it holds, which is what normally puts
            // that handle in the focus tree; without a tab group nothing does,
            // and `contains_focused` would answer "no" however deep in the grid
            // the caret is -- silently pointing the three-state panel keymap and
            // every "which panel is this" question at some other panel.
            .track_focus(&self.focus_handle)
            .key_context("Terminal")
            .child(
                // The grid inside is handed the same surface as its
                // background, so a shell is the block rather than a plate
                // laid on it. Clipped, or the strip's rule would run through
                // the corners the radius cuts.
                div()
                    .size_full()
                    .v_flex()
                    .rounded(cx.theme().radius)
                    .overflow_hidden()
                    .bg(crate::theme::dock_surface(cx))
                    .child(self.body(window, cx)),
            )
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
            return onehand_plugin_host::hint("No project selected", cx);
        }

        let set = self.active_set().filter(|set| !set.tabs.is_empty());
        let active = set.map_or(0, |set| set.active);
        let body = set
            .and_then(|set| set.tabs.get(active))
            .map(|tab| tab.shell.view().clone());
        // A tab is named by the PTY's program alone, which is the same word for
        // every tab a root has open, so three shells came out three tabs
        // reading `zsh`. **Numbered, and only where there is more than one**: a
        // lone tab has nothing to be told apart from and `zsh 1` beside no
        // `zsh 2` is a question about where the rest went.
        //
        // Not the project, which was tried and is the same word on every tab
        // for a stronger reason than the shell is -- this panel draws one
        // root's tabs and only ever that root's, so the part of the name
        // carrying the project was constant by construction, repeated on every
        // tab, and first in line to be cut by the width cap. Which project the
        // terminal is on is the project the whole window is on.
        //
        // Composed here and not in `PtyTab::label`, which the Neovim mode also
        // reads: that mode has one grid and no strip, so a name built to
        // separate siblings would be a name with nothing to separate it from.
        let empty = body.is_none();
        let tabs = set.map_or(&[][..], |set| set.tabs.as_slice());
        let labels: Vec<SharedString> = tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| match tabs.len() {
                1 => tab.shell.label(),
                _ => SharedString::from(format!("{} {}", tab.shell.label(), i + 1)),
            })
            .collect();

        div()
            .size_full()
            .v_flex()
            .child(
                // Drawn here rather than with the library's `TabBar`, whose
                // variants each bring a plate, a capsule, a ring or a border of
                // their own: a tab here is a flat label on the dock, the chosen
                // one on the selected fill, and nothing else. A strip like every
                // other, the shell below it parted from it by the space.
                onehand_plugin_host::strip(cx)
                    .id("terminal-tabs")
                    // The tabs live in a box of their own that scrolls sideways,
                    // and that box is what gives way, so `+` and the controls at
                    // the end stay on screen at any count.
                    .child(
                        div()
                            .id("terminal-tab-list")
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .children(labels.into_iter().enumerate().map(|(i, label)| {
                                onehand_plugin_host::tab_chip(
                                    ("terminal-tab", i),
                                    label.clone(),
                                    label,
                                    i == active,
                                    cx,
                                )
                                .max_w(onehand_plugin_host::TAB_MAX_W)
                                .on_click(cx.listener(move |panel: &mut Self, _, _, cx| {
                                    panel.select_tab(i, cx);
                                }))
                                // Inside the tab, so `stop_propagation` is what
                                // keeps the press that closes a shell from also
                                // selecting the one it closed.
                                .child(
                                    onehand_plugin_host::tab_close(
                                        ("close-shell", i),
                                        "Close this shell",
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        move |panel: &mut Self,
                                              _,
                                              window: &mut Window,
                                              cx: &mut Context<Self>| {
                                            cx.stop_propagation();
                                            panel.close_tab(i, window, cx);
                                        },
                                    )),
                                )
                            })),
                    )
                    // Muted, like the conversation header's own controls: a
                    // ghost button in full ink would be the brightest thing in
                    // the panel. The hover fill brings it back on the one about
                    // to be pressed.
                    .child(
                        onehand_plugin_host::strip_control("add-shell", IconName::Plus, false, cx)
                            .tooltip("New shell in this project")
                            .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                                panel.open_shell(window, cx);
                            })),
                    )
                    // The icon is the state, not the action: the one situation
                    // where it matters is the panel filling the frame and the
                    // user looking for the way back.
                    .child({
                        let full = self.maximized;
                        onehand_plugin_host::strip_control(
                            "maximize-terminal",
                            match full {
                                true => IconName::Minimize,
                                false => IconName::Maximize,
                            },
                            false,
                            cx,
                        )
                        .tooltip_with_action(
                            match full {
                                true => "Back to the dock",
                                false => "Fill the window",
                            },
                            &crate::shell::ToggleMaximize,
                            None,
                        )
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(TerminalPanelEvent::ToggleMaximize);
                        }))
                    })
                    // A minus, as the Workbench's: the shells are not being
                    // ended, which the cross on every tab is for. `Minus` and
                    // never `Dash`, whose stroke is written as literal black and
                    // ignores `text_color`.
                    .child(
                        onehand_plugin_host::strip_control(
                            "hide-terminal",
                            IconName::Minus,
                            false,
                            cx,
                        )
                        .tooltip_with_action(
                            "Hide the terminal",
                            &crate::shell::ToggleTerminal,
                            None,
                        )
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(TerminalPanelEvent::Hide);
                        })),
                    ),
            )
            .children(body.map(|view| {
                div()
                    .flex_1()
                    .min_h_0()
                    // The grid draws from its own top-left corner outward, so
                    // without an inset the first column sits against the dock's
                    // edge. The view measures its own bounds and reports the
                    // cell count back through the PTY resize, so this costs the
                    // shell a column and a row rather than painting over them.
                    .px_3()
                    .py_2()
                    .child(view)
                    .into_any_element()
            }))
            // No shells on this root. **The strip above stays**, and that is the
            // whole point of putting this here rather than returning early: the
            // way out of the dock lives on that row, so closing the last shell
            // used to take it away and leave an open panel with no control
            // inside it to close -- the state the chevron was added for, reached
            // by using the ✕ beside it.
            .when(empty, |panel| {
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
                            onehand_plugin_host::status_line(
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
mod tests {
    use super::{place_of, selection_after};

    /// Closing a tab ahead of the selected one must move the selection back by
    /// one, not leave the index where it was.
    ///
    /// This is the case clamping got wrong and nothing caught: three shells with
    /// the middle one on screen, close the first, and an index of 1 now names
    /// the *third* — so the panel silently swapped to the one shell the user had
    /// neither been reading nor asked to close.
    #[test]
    fn selection_follows_its_own_tab() {
        assert_eq!(
            selection_after(1, &[0], 2),
            0,
            "closing ahead pulls it back"
        );
        assert_eq!(
            selection_after(1, &[2], 2),
            1,
            "closing behind moves nothing"
        );
        assert_eq!(
            selection_after(2, &[0, 1], 1),
            0,
            "two ahead, two steps back"
        );
    }

    /// The selected tab going itself falls to whatever took its place, and to
    /// the last tab when it was the last.
    #[test]
    fn closing_the_selected_tab_falls_to_a_neighbour() {
        assert_eq!(
            selection_after(1, &[1], 2),
            1,
            "the tab that shifted up into it"
        );
        assert_eq!(
            selection_after(2, &[2], 2),
            1,
            "nothing after it, so the one before"
        );
        assert_eq!(
            selection_after(0, &[0], 0),
            0,
            "the last tab leaves nothing to select"
        );
    }

    /// A sweep is the same rule: what matters is how many went *ahead* of the
    /// selection, not how many went.
    #[test]
    fn a_sweep_counts_only_what_was_ahead() {
        assert_eq!(selection_after(3, &[0, 4], 3), 2);
        assert_eq!(selection_after(3, &[4, 5], 4), 3);
    }

    /// A closing question answered after the strip changed finds the shell
    /// it asked about, or nothing at all, and the selection stays on the shell
    /// it was on either way.
    #[test]
    fn an_answer_finds_its_tab_by_id_or_nothing() {
        // Three shells with ids 1, 2 and 3, the third selected; the question
        // is about the second.
        let (mut ids, mut active) = (vec![1, 2, 3], 2);
        // The first exits while the question is open.
        ids.remove(0);
        active = selection_after(active, &[0], ids.len());
        assert_eq!((ids[active], place_of(&ids, 2)), (3, Some(0)));
        // The answer closes the second, and the third stays selected.
        let at = place_of(&ids, 2).unwrap();
        ids.remove(at);
        active = selection_after(active, &[at], ids.len());
        assert_eq!(ids[active], 3);
        // A tab already gone answers nothing, so nothing else closes.
        assert_eq!(place_of(&ids, 2), None);
    }
}
