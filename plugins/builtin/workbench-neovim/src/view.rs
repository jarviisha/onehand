//! Neovim mode: the real editor, in a PTY, on the project root.
//!
//! Here and not in the terminal dock because this is the panel about files: a
//! tab called `nvim` sitting between two called `zsh` says the editor is a kind
//! of shell. One per root and never a second — several shells is what somebody
//! opens on purpose, while two editors on the same files are two views of one
//! buffer with no way to tell which holds the unsaved copy.

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, IntoElement, ParentElement, Pixels, Render,
    Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, StyledExt};
use onehand_plugin_host::{Ask, Request, action, status_ink};
use onehand_terminal_ui::{Program, PtyTab, TerminalThemeKey, spawn_pty, terminal_palette};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(crate) struct NeovimView {
    root: Option<PathBuf>,
    /// The Neovim running on each root, at most one apiece.
    ///
    /// Kept across a mode change and across the dock closing, the same way the
    /// quick editor's buffers are — closing the panel is putting work aside,
    /// not discarding it. It ends when the project root leaves the workspace,
    /// or when the window does.
    tabs: HashMap<PathBuf, PtyTab>,
    /// The reading size for the grid.
    ///
    /// A font size and not the panel's rem scale: the grid is *measured* from a
    /// shaped glyph, so scaling the box around it stretches the container while
    /// the cell stays put and every column lands past its own character.
    font_size: Pixels,
    /// What the palette was last built from, so a live grid is recoloured once
    /// per appearance change instead of on every frame.
    theme: TerminalThemeKey,
    /// A Neovim that would not start, shown as a line under the grid.
    ///
    /// The empty state is where a failure has to be readable: the button that
    /// tried is right above it, and there is no other surface saying why
    /// nothing appeared.
    status: Option<String>,
    /// How a child that has exited reaches the panel.
    ///
    /// It goes up rather than being collected here, because dropping a grid
    /// that holds the caret is the panel's problem: the window would be left
    /// pointing at an element no frame contains, and GPUI resolves a key along
    /// the path down to the focused node — so every shortcut stops working,
    /// including the one that would reopen this panel. Only the panel knows
    /// whether the caret was inside it.
    ask: Ask,
}

impl NeovimView {
    pub(crate) fn new(ask: Ask, font_size: Pixels, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            root: None,
            tabs: HashMap::new(),
            font_size,
            theme: TerminalThemeKey::current(cx),
            status: None,
            ask,
        })
    }

    pub(crate) fn set_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.root.as_deref() == Some(root) {
            return;
        }
        self.root = Some(root.to_path_buf());
        cx.notify();
    }

    pub(crate) fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        // Dropping the entry ends the child, the way dropping a terminal tab
        // does: a project removed from the workspace must not leave an editor
        // running on it with nothing on screen pointing at it.
        self.tabs.remove(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    /// Start Neovim on the active root, if it is not already running.
    ///
    /// Separate from becoming the active mode on purpose: switching to this
    /// mode is a view change and must not launch a process, or the mode strip
    /// becomes three buttons of which one spawns something. The key does this
    /// first and then switches; the empty state's own button is the other way
    /// in.
    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.tabs.contains_key(&root) {
            return;
        }
        let ask = self.ask.clone();
        let spawned = spawn_pty(
            &root,
            Program::Neovim,
            self.font_size,
            cx,
            move |window, cx| ask(&Request::Reap, window, cx),
        );
        match spawned {
            Ok(tab) => {
                self.tabs.insert(root, tab);
                self.status = None;
            }
            Err(e) => self.status = Some(e),
        }
        cx.notify();
    }

    /// Drop a Neovim that has exited, and say whether one went.
    ///
    /// Without this, `:q` leaves the grid drawing the last screen Neovim
    /// painted — which after quitting is an empty one with a cursor on it — and
    /// the mode becomes a panel that takes keystrokes nothing will ever read.
    /// The mode is *not* switched away from: the empty state offers to start
    /// another, and moving the user somewhere they did not ask to go is a worse
    /// answer than showing them what happened.
    ///
    /// A sweep over every tab rather than a removal of the one that spoke,
    /// because the question is asked of the process (`try_wait`) — so a child
    /// killed from somewhere else, or gone while its root was off screen, is
    /// collected too, and reaped rather than left a zombie.
    pub(crate) fn reap(&mut self, cx: &mut Context<Self>) -> bool {
        let before = self.tabs.len();
        self.tabs.retain(|_, tab| !tab.finished());
        let reaped = self.tabs.len() != before;
        if reaped {
            cx.notify();
        }
        reaped
    }

    /// Re-measure a live grid at a new reading size.
    pub(crate) fn set_font_size(&mut self, size: Pixels, cx: &mut Context<Self>) {
        self.font_size = size;
        for tab in self.tabs.values() {
            tab.set_font_size(size, cx);
        }
        cx.notify();
    }

    /// Where the caret goes, if there is a grid to put it in.
    ///
    /// Nothing else in this panel needs the caret as badly: a grid that is
    /// drawn but unfocused looks exactly like one that is running, and every
    /// keystroke aimed at it goes somewhere else. Handed back rather than
    /// focused here, since focusing needs the window and reading the handle
    /// needs this view — and holding both at once is one borrow too many.
    pub(crate) fn caret(&self, cx: &App) -> Option<FocusHandle> {
        let tab = self.root.as_ref().and_then(|root| self.tabs.get(root))?;
        Some(tab.view().read(cx).focus_handle().clone())
    }

    /// Recolour a live grid after an app appearance change.
    ///
    /// Guarded by comparison rather than run every frame: rebuilding the
    /// palette and pushing a whole config into the view is work, and this is
    /// called from render.
    fn sync_theme(&mut self, cx: &mut Context<Self>) {
        let current = TerminalThemeKey::current(cx);
        if self.theme == current {
            return;
        }
        self.theme = current;
        let colors = terminal_palette(cx);
        for tab in self.tabs.values() {
            tab.set_palette(colors.clone(), cx);
        }
    }
}

impl Render for NeovimView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_theme(cx);
        let body = self.body(cx);
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(body)
            .children(self.status.clone().map(|status| {
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(status_ink(cx).warning)
                    .child(status)
            }))
    }
}

impl NeovimView {
    fn body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(root) = self.root.clone() else {
            return hint("No project root", cx);
        };

        let Some(tab) = self.tabs.get(&root) else {
            // The button and not an automatic spawn, for the same reason
            // switching to this mode does not spawn: arriving at a tab should
            // not start a process. It also gives the failure somewhere to be
            // read — a Neovim that would not start leaves its reason in the
            // line below this.
            return div()
                .flex_1()
                .v_flex()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    action("start-neovim")
                        .primary()
                        .label("Start Neovim")
                        .on_click(cx.listener(|view: &mut Self, _, window, cx| {
                            view.start(cx);
                            if let Some(caret) = view.caret(cx) {
                                caret.focus(window, cx);
                            }
                        })),
                )
                .into_any_element();
        };

        div()
            .flex_1()
            .min_h_0()
            // The grid draws from its own top-left corner outward, so without
            // this the first column sits against the panel edge. Costs a column
            // rather than being painted over: the view measures its own bounds
            // and reports the cell count back through the PTY resize, so what it
            // lays out and what the child believes stay in step.
            .p_2()
            .child(tab.view().clone())
            .into_any_element()
    }
}

fn hint(text: &'static str, cx: &App) -> gpui::AnyElement {
    div()
        .flex_1()
        .v_flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
