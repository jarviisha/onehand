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

impl RootShells {
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
            maximized: false,
        })
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
            removed |= set.drop_where(|_, tab| tab.finished());
        }
        if !removed {
            return;
        }
        if held_focus {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Close one tab, ending its shell.
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
    pub fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let held_focus = self.focus_handle.contains_focused(window, cx);
        // Dropping the tab drops its PTY, which ends the child.
        let closed = self
            .root
            .as_ref()
            .and_then(|r| self.shells.get_mut(r))
            .is_some_and(|set| set.drop_where(|i, _| i == idx));
        if closed && held_focus {
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
            return hint("No project root", cx).into_any_element();
        }

        let set = self.active_set().filter(|set| !set.tabs.is_empty());
        let active = set.map_or(0, |set| set.active);
        let body = set
            .and_then(|set| set.tabs.get(active))
            .map(|tab| tab.view().clone());
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
        // terminal is on is already in the status bar, once.
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
                1 => tab.label(),
                _ => SharedString::from(format!("{} {}", tab.label(), i + 1)),
            })
            .collect();

        div()
            .size_full()
            .v_flex()
            .child(
                // **Drawn here rather than with the library's `TabBar`, after
                // trying it.** Every variant that component offers states more
                // than this strip wants to say: the default fills the bar,
                // raises a plate under the selected tab and rules a hairline
                // under the lot; `pill` makes the selected tab a white capsule;
                // `outline` rings it; `segmented` and `underline` each bring a
                // border of their own. A terminal panel is one surface with
                // rows of a shell on it, and the tabs are a label on that
                // surface -- so what is wanted is a small filled rectangle and
                // nothing else, and none of the five is that. The fill and the
                // radius are not reachable from outside either: the component
                // writes them into the same style refinement the call site
                // does, and later.
                //
                // What that costs is nothing it was still holding. The label,
                // the glyph, the ellipsis, the accessible name and the ✕ are
                // all written out below already, so the trade was the whole
                // component against its choice of selected fill.
                //
                // It is also, separately, not the *dock's* tab group: that one
                // wraps the whole panel in a strip carrying the panel's title,
                // which here would be one tab reading "Terminal" above the
                // strip that already names every shell.
                div()
                    .id("terminal-tabs")
                    .h_flex()
                    .items_center()
                    .gap_1p5()
                    .w_full()
                    // A hairline under the whole row, spanning the panel rather
                    // than stopping under the tabs. With no fill behind the
                    // strip and none behind the grid, the only thing that had
                    // been telling the chrome from the shell was the gap, and a
                    // gap reads as spacing rather than as an edge -- a line
                    // reads as an edge, which is what the top of a terminal is.
                    .border_b_1()
                    .border_color(cx.theme().border)
                    //
                    // **This is the room around the tab, not inside it.** A tab
                    // is the one thing on this row carrying a fill, so it is the
                    // one thing with an edge that can sit too near another: at
                    // 4px it was almost touching the panel's left border and the
                    // dock's top one, which reads as a chip wedged into a
                    // corner. The padding here is what holds it off them, and it
                    // is even on all four sides because all four are the same
                    // kind of neighbour -- a frame edge.
                    //
                    // **The horizontal number is not free**: with each tab's own
                    // 8px, a label starts 16px from the panel edge, and that is
                    // the inset the grid below has to take too, or a shell's
                    // first character stops sitting under the tab naming it.
                    // Moving one of the three means moving all three.
                    //
                    // Vertically 6px, and the number is bracketed rather than
                    // picked: 4px was tried and the chips sat against their own
                    // row's edges again -- which is the crowding this padding
                    // answers, and which the hairline below does not touch,
                    // because a rule separates the strip from the shell without
                    // holding the tabs off anything. 8px cleared that and left
                    // the strip taller than a row of 20px chips needs. This is
                    // the smaller of the two that still holds them clear.
                    .px_2()
                    .py_1p5()
                    // **The tabs live in a box of their own, and that box is
                    // what gives way.** Flat in the row with the controls, a
                    // fourth shell pushed `+` and the way out past the panel's
                    // right edge -- the two controls somebody wants precisely
                    // when there are too many tabs were the two the tabs took
                    // away. `flex_1` + `min_w_0` makes this the one part that
                    // shrinks and the controls the part that cannot, so they
                    // stay put at any count.
                    //
                    // **The box scrolls; the tabs inside it never narrow.** They
                    // did for a while, down to a floor, on the reasoning that a
                    // short tab can still be aimed at while a scrolled-out one
                    // cannot -- and what that bought was every tab getting worse
                    // the moment a fourth appeared, to spare the fourth a
                    // gesture. A tab that is the size it needs is readable at
                    // any count; the cost is a scroll, and it is paid by whoever
                    // opened the shells.
                    .child(
                        div()
                            .id("terminal-tab-list")
                            .h_flex()
                            .items_center()
                            .gap_1p5()
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .children(labels.into_iter().enumerate().map(|(i, label)| {
                                // The group the ✕ hovers off, one per tab: a single
                                // name shared by the strip would light every tab's
                                // close button the moment the pointer entered any of
                                // them.
                                let hovered = SharedString::from(format!("terminal-tab-{i}"));
                                div()
                            .id(("terminal-tab", i))
                            .group(hovered.clone())
                            .h_flex()
                            .items_center()
                            .gap_1p5()
                            // `flex_none` is the whole rule: a tab is the size
                            // its own name needs and gives nothing back to the
                            // row. What runs out of room is the box around them,
                            // and a box that has run out of room scrolls.
                            //
                            // The cap is on the name and not on the tab count:
                            // out of it come the padding at both ends, the
                            // glyph, and the width the ✕ holds whether or not it
                            // is drawn, so a project named at length ellipsizes
                            // here rather than making one tab as wide as three.
                            .flex_none()
                            .max_w(px(220.))
                            // 8px is what the grid's own inset is built on and
                            // cannot move on its own.
                            //
                            // **A tab is as tall as the tallest thing in it, and
                            // that is the ✕ below**, whose box the component
                            // sizes off `xsmall`. So this padding is not the
                            // height and taking it away does not shorten the
                            // chip -- it only stops the fill from clearing the
                            // text, which is the one thing a fill is for. Both
                            // were cut once, to 16px, and the tab stopped
                            // reading as something you press.
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .text_xs()
                            .cursor_pointer()
                            .when(i == active, |tab| {
                                tab.bg(cx.theme().accent)
                                    .text_color(cx.theme().accent_foreground)
                            })
                            .when(i != active, |tab| tab.hover(|tab| tab.bg(cx.theme().muted)))
                            .child(
                                Icon::new(IconName::SquareTerminal)
                                    .size_3()
                                    .flex_shrink_0()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            // `min_w_0` is what lets the text shrink far enough
                            // to ellipsize at all, since a flex child's floor is
                            // otherwise its own content.
                            .child(div().min_w_0().truncate().child(label))
                            .on_click(cx.listener(move |panel: &mut Self, _, _, cx| {
                                panel.select_tab(i, cx);
                            }))
                            // **Shown on hover alone.** Drawn always, a strip of
                            // three shells reads as three names and three
                            // crosses, and the crosses are the same size and
                            // weight as the one thing the strip is for. What it
                            // costs is that nobody learns the control is there
                            // by looking -- which is the trade every terminal
                            // makes, because a tab is closed by someone who
                            // already decided to close it. `invisible` rather
                            // than absent, so a tab does not change width under
                            // the pointer.
                            //
                            // Inside the tab, so `stop_propagation` is what
                            // keeps the press that closes a shell from also
                            // selecting the one it just closed.
                            .child(
                                crate::controls::action(("close-shell", i))
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::Close))
                                    .invisible()
                                    .group_hover(hovered, |style| style.visible())
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
                    // **A size up on the two controls, and it costs no height.**
                    // They are aimed at from across the panel while the ✕ is
                    // found by already being on the tab it belongs to, so the
                    // pair at the end are the ones that want a target. The row
                    // is as tall as the tabs, which are the ✕ plus their own
                    // padding, and that is the size these land on -- so the
                    // strip is the same height it was and the two buttons now
                    // line up with the chips rather than sitting inside them.
                    .child(
                        crate::controls::action("add-shell")
                            .ghost()
                            .small()
                            .flex_none()
                            .icon(Icon::new(IconName::Plus))
                            .tooltip("New shell in this project")
                            .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                                panel.open_shell(window, cx);
                            })),
                    )
                    // The way out, at the end of the row that is the only chrome
                    // this panel has. The dock is opened from four places and
                    // was closable from all four, every one of them outside the
                    // panel -- so the one place a user is certainly looking when
                    // they want it gone was the one place that could not do it.
                    //
                    // Between `+` and the way out, in the order a window's own
                    // chrome puts them: what it does is between making a shell
                    // and putting the panel away in how far it goes.
                    //
                    // **The icon is the state, not the action.** A toggle
                    // drawing one glyph in both states says the same thing
                    // about two opposite situations, and the one situation
                    // where it matters is the one where the panel fills the
                    // frame and the user is looking for the way back.
                    .child({
                        let full = self.maximized;
                        crate::controls::action("maximize-terminal")
                            .ghost()
                            .small()
                            .flex_none()
                            .icon(Icon::new(match full {
                                true => IconName::Minimize,
                                false => IconName::Maximize,
                            }))
                            .tooltip(match full {
                                true => "Back to the dock — Ctrl+Shift+K",
                                false => "Fill the window — Ctrl+Shift+K",
                            })
                            .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                                cx.emit(TerminalPanelEvent::ToggleMaximize);
                            }))
                    })
                    // A minus, the mark a window's own chrome uses for the
                    // thing that goes away and comes back, rather than a ✕: the
                    // shells are not being ended, and the ✕ an inch to its left
                    // on every tab is.
                    //
                    // `Minus` and never `Dash`, which is the same drawing under
                    // another name with `stroke` written into it as literal
                    // black -- so it ignores `text_color` and comes out
                    // invisible against a dark panel, with nothing in the build
                    // or the log to say why.
                    .child(
                        crate::controls::action("hide-terminal")
                            .ghost()
                            .small()
                            .flex_none()
                            .icon(Icon::new(IconName::Minus))
                            .tooltip("Hide the terminal — Ctrl+`")
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
                    // without this the first column sits against the panel edge
                    // and the last row against whatever is below it -- a hairline
                    // or the status bar.
                    //
                    // **The horizontal inset is the tab's, not a round number of
                    // our own.** A tab's label starts 16px in from the panel
                    // edge -- the strip's 8px and the tab's own 8px -- so that
                    // is where a shell's first character goes and a tab sits
                    // over the column it names. This number is downstream of the
                    // two above it and moves whenever either does.
                    //
                    // Vertically the reference is the frame rather than the
                    // strip, so it stays the smaller inset: this panel's job is
                    // to show rows of a shell, and every 4px spent above the
                    // first row is 4px not spent on one.
                    //
                    // Costs the shell a column and a row rather than being
                    // painted over them: the view measures its own bounds and
                    // reports the cell count back through the PTY resize, so what
                    // it lays out and what the child believes stay in step.
                    .px_4()
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
                        })),
                )
            })
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

#[cfg(test)]
mod tests {
    use super::selection_after;

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
}
