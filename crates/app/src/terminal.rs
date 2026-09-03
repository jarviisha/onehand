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

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, px,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_terminal_ui::{Program, PtyTab, TerminalThemeKey, spawn_pty, terminal_palette};
use std::collections::HashMap;
use std::path::PathBuf;

/// The tab set one project root has open.
#[derive(Default)]
struct RootShells {
    tabs: Vec<PtyTab>,
    active: usize,
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
}

impl TerminalPanel {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            focus_handle: cx.focus_handle(),
            root: None,
            shells: HashMap::new(),
            status: None,
            zoom: crate::zoom::Zoom::default(),
            terminal_theme: TerminalThemeKey::current(cx),
        })
    }

    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.root = Some(root);
        cx.notify();
    }

    /// Open a shell on the active root.
    ///
    /// Spawned lazily and never at boot: a workspace with a dozen roots must
    /// not start a dozen shells nobody asked for. Always a new one — shells are
    /// what somebody opens several of on purpose, one per thing they are
    /// watching.
    pub fn open_shell(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };

        let panel = cx.entity().downgrade();
        let spawned = spawn_pty(
            &root,
            Program::Shell,
            crate::zoom::term_font_size(self.zoom),
            cx,
            move |window, cx| {
                let _ = panel.update(cx, |panel: &mut Self, cx| panel.reap(window, cx));
            },
        );
        match spawned {
            Ok(shell) => {
                let set = self.shells.entry(root).or_default();
                set.tabs.push(shell);
                set.active = set.tabs.len() - 1;
                self.status = None;
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
            let before = set.tabs.len();
            set.tabs.retain_mut(|tab| !tab.finished());
            if set.tabs.len() != before {
                set.active = set.active.min(set.tabs.len().saturating_sub(1));
                removed = true;
            }
        }
        if !removed {
            return;
        }
        if held_focus {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    pub fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(set) = self.root.as_ref().and_then(|r| self.shells.get_mut(r))
            && idx < set.tabs.len()
        {
            // Dropping the tab drops its PTY, which ends the child.
            set.tabs.remove(idx);
            set.active = set.active.min(set.tabs.len().saturating_sub(1));
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
                tab.set_font_size(size, cx);
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
        let colors = terminal_palette(cx);
        for set in self.shells.values() {
            for tab in &set.tabs {
                tab.set_palette(colors.clone(), cx);
            }
        }
    }

    /// Whether the active root already has a shell open, so a shortcut that
    /// reveals the panel can spawn the first one without spawning a second on
    /// every later press.
    pub fn has_shell(&self) -> bool {
        self.active_set().is_some_and(|set| !set.tabs.is_empty())
    }

    /// Focus the active shell, not the panel: the point of reaching for the
    /// terminal is to type into it.
    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        let handle = self
            .active_set()
            .and_then(|set| set.tabs.get(set.active))
            .map(|tab| tab.view().read(cx).focus_handle().clone());
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

    /// No content-only maximize, because there is no tab bar to put the button
    /// on: this panel is mounted bare, the way the conversation is. The app
    /// direction still has its key.
    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Terminal")
    }
}

impl EventEmitter<PanelEvent> for TerminalPanel {}

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
        let Some(set) = self.active_set().filter(|set| !set.tabs.is_empty()) else {
            if self.root.is_none() {
                return hint("No project root", cx).into_any_element();
            }
            return div()
                .size_full()
                .v_flex()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    crate::controls::action("new-shell")
                        .primary()
                        .icon(Icon::new(IconName::SquareTerminal))
                        .label("New terminal")
                        .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                            panel.open_shell(window, cx);
                        })),
                )
                .children(self.status.clone().map(|status| {
                    div()
                        .text_xs()
                        .text_color(crate::theme::status_ink(cx).danger)
                        .child(status)
                        .into_any_element()
                }))
                .into_any_element();
        };

        let active = set.active;
        let body = set.tabs.get(active).map(|tab| tab.view().clone());
        let labels: Vec<SharedString> = set.tabs.iter().map(PtyTab::label).collect();

        div()
            .size_full()
            .v_flex()
            .child(
                div()
                    .id("terminal-tabs")
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .children(labels.into_iter().enumerate().map(|(i, label)| {
                        div()
                            .id(("terminal-tab", i))
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .flex_none()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .text_xs()
                            .cursor_pointer()
                            .when(i == active, |tab| {
                                tab.bg(cx.theme().accent)
                                    .text_color(cx.theme().accent_foreground)
                            })
                            .child(div().max_w(px(120.)).truncate().child(label))
                            .on_click(cx.listener(move |panel: &mut Self, _, _, cx| {
                                panel.select_tab(i, cx);
                            }))
                            .child(
                                div()
                                    .id(("terminal-tab-close", i))
                                    .flex_none()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(Icon::new(IconName::Close).size_3())
                                    .on_click(cx.listener(move |panel: &mut Self, _, _, cx| {
                                        panel.close_tab(i, cx);
                                    })),
                            )
                    }))
                    .child(div().flex_1())
                    .child(
                        crate::controls::action("add-shell")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Plus))
                            .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                                panel.open_shell(window, cx);
                            })),
                    ),
            )
            .children(body.map(|view| {
                div()
                    .flex_1()
                    .min_h_0()
                    // The grid draws from its own top-left corner outward, so
                    // without this the first column sits against the panel edge
                    // and the last row against whatever is below it -- a hairline
                    // or the status bar. Matching the tab strip's inset above
                    // puts a shell's first character on the same column as the
                    // tab that names it.
                    //
                    // Costs the shell a column and a row rather than being
                    // painted over them: the view measures its own bounds and
                    // reports the cell count back through the PTY resize, so what
                    // it lays out and what the child believes stay in step.
                    .p_2()
                    .child(view)
                    .into_any_element()
            }))
            .into_any_element()
    }
}

fn hint(text: &'static str, cx: &App) -> impl IntoElement + use<> {
    div()
        .size_full()
        .v_flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
