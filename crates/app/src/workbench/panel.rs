//! The Workbench dock panel: Editor, Files and Neovim, one at a time.
//!
//! State is **per project root** — `RootBuffers`, `FileTree` and the Neovim PTY
//! are all keyed by root path, so switching roots swaps the whole panel rather
//! than mixing one project's tabs with another's tree.
//!
//! **Neovim is a mode here rather than a tab of the terminal**, because this is
//! the panel about files: a tab called `nvim` sitting between two called `zsh`
//! says the editor is a kind of shell. Two consequences the other two modes do
//! not have, both because this one is a live PTY rather than an element tree.
//! Its zoom is a font size and not the rem scale wrapped around the body below,
//! since the grid is *measured* from a shaped glyph — scaling the box around it
//! leaves every column landing past its own character. And the panel takes the
//! terminal's key context while it is showing, which is what gives `Ctrl+S` back
//! to `:w`: that binding is `Shell && !Terminal` precisely so a program in a PTY
//! keeps it, and a grid mounted somewhere with no such context would have the
//! save of the quick editor fire over the top of it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::input::InputEvent;
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
use onehand_core::editor::SaveOutcome;
use onehand_core::gitstat::GitStatus;
use onehand_core::tree::{self, FileTree};
use onehand_plugin_api::PluginId;
use onehand_plugin_host::WorkbenchHost;
use onehand_terminal_ui::{Program, TerminalThemeKey, spawn_pty, terminal_palette};
use onehand_workbench_editor::{self as editor, RootBuffers};
use onehand_workbench_files as files;
use onehand_workbench_neovim::NeovimSessions;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type WorkbenchMode = PluginId;
pub const EDITOR_MODE: WorkbenchMode = onehand_workbench_editor::MODE_ID;
pub const FILES_MODE: WorkbenchMode = onehand_workbench_files::MODE_ID;
pub const NEOVIM_MODE: WorkbenchMode = onehand_workbench_neovim::MODE_ID;

pub struct Workbench {
    host: WorkbenchHost,
    /// The root everything below is keyed by. `None` before any root is active.
    root: Option<PathBuf>,
    editors: HashMap<PathBuf, RootBuffers>,
    trees: HashMap<PathBuf, FileTree>,
    git: HashMap<PathBuf, GitStatus>,
    /// The Neovim running on each root, at most one apiece.
    ///
    /// One and not a set, unlike the terminal's shells: several shells is what
    /// somebody opens on purpose, one per thing they are watching, while a
    /// second editor on the same files is two views of one buffer with no way to
    /// tell which holds the unsaved copy.
    ///
    /// Kept across a mode change and across the dock closing, the same way the
    /// quick editor's buffers are — closing this panel is putting work aside,
    /// not discarding it. It ends when the project root leaves the workspace, or
    /// when the window does.
    neovim: NeovimSessions,
    /// What the terminal palette was last built from, so a live grid is
    /// recoloured once per appearance change instead of on every frame.
    terminal_theme: TerminalThemeKey,
    /// A save conflict, a read failure, or a Neovim that would not start, shown
    /// as a line under the body.
    ///
    /// **Deliberately not a notification**, unlike the rest of the app's
    /// transient status. "Changed on disk — nothing was written" is not news
    /// that may be missed: it is a standing condition, and the next Ctrl+S is
    /// what resolves it. A toast that fades leaves the user believing the save
    /// went through. Cleared by whatever answers it — a successful save, or a
    /// successful open.
    pub status: Option<String>,
    /// Tabs with a write in flight, and whether another save was asked for
    /// while it ran.
    ///
    /// Two saves of one tab must not overlap: each captures `disk_mtime` at
    /// spawn, so the second carries the time from *before* the first wrote and
    /// comes back as a conflict against our own write. The
    /// second press is not dropped either -- it is remembered here and re-run
    /// once the first lands, which is what makes it pick up the keystrokes that
    /// prompted it.
    saving: HashMap<u64, bool>,
    /// A close that would discard unsaved edits, armed for its second click.
    ///
    /// Same shape as the shell's guarded root removal, and for the same reason:
    /// the destructive half of a one-touch control needs a touch of its own.
    /// Any other action disarms it, so the confirmation always belongs to the
    /// control just clicked.
    pending_close: Option<PendingClose>,
    /// Reading size for the body. Per panel, not per root: the dock's own
    /// sizes are per window here, and a zoom that reset itself on every root
    /// switch would be the odd one out.
    zoom: crate::zoom::Zoom,
}

/// A close waiting on its confirming click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingClose {
    Tab(usize),
    All,
}

impl Workbench {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let modes = crate::state::Shared::global(cx)
            .plugins
            .workbench_modes()
            .to_vec();
        let host = WorkbenchHost::new(modes, cx);
        cx.new(|cx| Self {
            host,
            zoom: crate::zoom::Zoom::default(),
            root: None,
            editors: HashMap::new(),
            trees: HashMap::new(),
            git: HashMap::new(),
            neovim: NeovimSessions::default(),
            terminal_theme: TerminalThemeKey::current(cx),
            status: None,
            saving: HashMap::new(),
            pending_close: None,
        })
    }

    pub fn mode(&self) -> WorkbenchMode {
        self.host.active()
    }

    /// Drop everything this panel holds for `root`.
    ///
    /// Called when a project root leaves the workspace. Buffers are dropped
    /// with it: an unsaved edit in a project the user just removed has nowhere
    /// to be saved *to* -- the tab strip it belonged to is gone.
    /// How many of `root`'s open files have edits a removal would discard.
    ///
    /// Asked by the shell *before* it removes a root, because that is the one
    /// place with a control to guard. Until `dirty` was wired up to typing,
    /// this question had no answer worth asking.
    pub fn unsaved_in(&self, root: &Path) -> usize {
        self.editors
            .get(root)
            .map(|buffers| buffers.tabs.files.iter().filter(|f| f.dirty).count())
            .unwrap_or(0)
    }

    pub fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        self.editors.remove(root);
        self.trees.remove(root);
        self.git.remove(root);
        // Dropping the entry ends the child, the way dropping a terminal tab
        // does: a project removed from the workspace must not leave an editor
        // running on it with nothing on screen pointing at it.
        self.neovim.forget_root(root);
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    /// Start Neovim on the active root, if it is not already running.
    ///
    /// Separate from [`Self::set_mode`] on purpose: switching to this mode is a
    /// view change and must not launch a process, or the mode strip becomes
    /// three buttons of which one spawns something. The key does this first and
    /// then switches; the empty state's own button is the other way in.
    pub fn open_neovim(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.neovim.contains(&root) {
            return;
        }
        let panel = cx.entity().downgrade();
        let spawned = spawn_pty(
            &root,
            Program::Neovim,
            crate::zoom::term_font_size(self.zoom),
            cx,
            move |window, cx| {
                let _ = panel.update(cx, |panel: &mut Self, cx| panel.reap_neovim(window, cx));
            },
        );
        match spawned {
            Ok(tab) => {
                self.neovim.insert(root, tab);
                self.status = None;
            }
            Err(e) => self.status = Some(e),
        }
        cx.notify();
    }

    /// Drop a Neovim that has exited.
    ///
    /// Without this, `:q` leaves the grid drawing the last screen Neovim
    /// painted — which after quitting is an empty one with a cursor on it — and
    /// the mode becomes a panel that takes keystrokes nothing will ever read.
    /// The mode is *not* switched away from: the empty state offers to start
    /// another, and moving the user somewhere they did not ask to go is a worse
    /// answer than showing them what happened.
    ///
    /// Focus is only moved when it was inside this panel already. A grid dropped
    /// while holding the caret leaves the window pointing at an element no frame
    /// contains, and GPUI resolves a key along the path down to the focused
    /// node, so every shortcut stops working — including the ones that would get
    /// out of here.
    pub fn reap_neovim(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let held_focus = self.host.focus_handle().contains_focused(window, cx);
        if !self.neovim.reap_finished() {
            return;
        }
        if held_focus {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Step this panel's zoom.
    ///
    /// Handed the whole value rather than a `&mut` to the field, because a step
    /// here is not only a number: a live grid has to be re-measured at the new
    /// font size, and a caller holding the field directly would set it and leave
    /// Neovim drawn at the old one.
    pub fn set_zoom(&mut self, zoom: crate::zoom::Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        let size = crate::zoom::term_font_size(zoom);
        self.neovim.set_font_size(size, cx);
        cx.notify();
    }

    /// This panel's zoom, for the status bar to report.
    pub fn zoom(&self) -> crate::zoom::Zoom {
        self.zoom
    }

    /// Put focus where this mode's work happens: the open buffer in Editor
    /// mode, the grid in Neovim, the panel itself in Files, which is clicked
    /// rather than typed into. A panel shortcut that opens a dock without moving
    /// focus makes the user reach for the mouse to use what they just opened.
    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        if self.mode() == NEOVIM_MODE {
            // Nothing else in this panel needs the caret as badly: a grid that
            // is drawn but unfocused looks exactly like one that is running,
            // and every keystroke aimed at it goes somewhere else.
            if let Some(tab) = self.root.as_ref().and_then(|root| self.neovim.get(root)) {
                tab.view().read(cx).focus_handle().clone().focus(window, cx);
                return;
            }
        }
        if self.mode() == EDITOR_MODE {
            let buffer = self
                .root
                .as_ref()
                .and_then(|root| self.editors.get(root))
                .and_then(|buffers| {
                    let uid = buffers.tabs.active_file()?.uid;
                    buffers.buffer(uid)
                });
            if let Some(state) = buffer {
                state.focus_handle(cx).focus(window, cx);
                return;
            }
        }
        self.host.focus_handle().focus(window, cx);
    }

    pub fn set_mode(&mut self, mode: WorkbenchMode, cx: &mut Context<Self>) {
        if self.host.select(mode) {
            cx.notify();
        }
    }

    /// Point the panel at a project root, seeding its tree on first sight.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.root.as_ref() == Some(&root) {
            return;
        }
        self.root = Some(root.clone());
        if self.trees.contains_key(&root) {
            // A root seen before: its cached listings are as old as the last
            // time it was on screen, and the agent has been working since.
            self.rescan(cx);
        } else {
            self.trees.insert(root.clone(), FileTree::default());
            self.scan(root, cx);
        }
        cx.notify();
    }

    pub fn set_git(&mut self, git: HashMap<PathBuf, GitStatus>, cx: &mut Context<Self>) {
        self.git = git;
        cx.notify();
    }

    /// Read one directory off the UI loop and cache its listing.
    fn scan(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        cx.spawn(async move |panel, cx| {
            let listing = cx
                .background_executor()
                .spawn({
                    let dir = dir.clone();
                    async move { tree::read_dir_sorted(&dir) }
                })
                .await;
            let _ = panel.update(cx, |panel: &mut Self, cx| {
                if let Some(tree) = panel.trees.get_mut(&root) {
                    tree.listings.insert(dir, listing);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-read every directory currently on screen for the active root.
    ///
    /// The tree was seeded once per root and never refreshed, so a file the
    /// agent had just created did not appear until the user folded and unfolded
    /// its directory by hand. Called when a turn ends and when
    /// the window is activated — the two moments the tree is most likely to
    /// have moved under it.
    ///
    /// Bounded by what is *visible*: the root plus its expanded directories,
    /// which is exactly what `visible_rows` draws from. A collapsed subtree is
    /// rescanned when it is opened, as it always was.
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(tree) = self.trees.get(&root) else {
            return;
        };
        let dirs: Vec<PathBuf> = std::iter::once(root)
            .chain(tree.expanded.iter().cloned())
            .collect();
        for dir in dirs {
            self.scan(dir, cx);
        }
    }

    /// Fold or unfold a directory. Expanding rescans — a cached listing can be
    /// minutes old, and the agent has been writing to this tree the whole time.
    pub fn toggle_dir(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let expanding = self
            .trees
            .get_mut(&root)
            .map(|tree| {
                if tree.expanded.remove(&dir) {
                    false
                } else {
                    tree.expanded.insert(dir.clone());
                    true
                }
            })
            .unwrap_or(false);
        if expanding {
            self.scan(dir, cx);
        }
        cx.notify();
    }

    /// Open `path` in the editor.
    ///
    /// An already-open file is focused, never reloaded: the second click on a
    /// path in a tool card must not discard unsaved edits (core's
    /// `RootEditors::open` decides that, not this).
    ///
    /// **No line number, and nothing to take one from.** ACP's diff payload is
    /// `{ path, old_text, new_text }` with no hunk offsets, so the tool card
    /// that links here has no line to send. The other half — `path:line:col`
    /// tokens written in an agent's prose — needs the transcript to *detect*
    /// them first, which it does not do. Core's
    /// `parse::parse_path_line` is the parser that feature would use and
    /// currently has no caller.
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.host.select(EDITOR_MODE);

        if let Some(buffers) = self.editors.get(&root)
            && buffers.tabs.index_of(&path).is_some()
        {
            self.editors
                .entry(root)
                .or_default()
                .tabs
                .open(path, None, 0);
            cx.notify();
            return;
        }

        // `spawn_in`, not `spawn`: building an `EditorState` needs a `&mut
        // Window`, and the read has to happen off the UI loop first.
        cx.spawn_in(window, async move |panel, cx| {
            let read = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { onehand_core::editor::read_blocking(&path) }
                })
                .await;

            let _ = panel.update_in(cx, |panel: &mut Self, window, cx| match read {
                Ok((text, mtime)) => {
                    // A read that worked answers whatever the last failure said.
                    // Left set, the message outlived its subject and read as a
                    // complaint about the file now on screen.
                    panel.status = None;
                    let uid = next_buffer_uid();
                    let state = editor::new_buffer(&path, &text, window, cx);
                    // Typing is what makes a buffer dirty, and nothing was
                    // listening for it. Subscribed *after*
                    // `new_buffer`, whose `set_value` seeds the text --
                    // gpui-component's `set_value` does not emit `Change`, but
                    // ordering it this way means a tab is never born dirty even
                    // if that changes upstream.
                    let watch = cx.subscribe(&state, {
                        let root = root.clone();
                        move |panel: &mut Self, _, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Change) {
                                panel.mark_dirty(&root, uid, cx);
                            }
                        }
                    });
                    let buffers = panel.editors.entry(root).or_default();
                    // Core decides whether this is a new tab; a buffer is only
                    // adopted when it is, so a re-open keeps its edits.
                    let (_, is_new) = buffers.tabs.open(path.clone(), mtime, uid);
                    if is_new {
                        buffers.insert(uid, state, watch);
                    }
                    cx.notify();
                }
                Err(e) => {
                    panel.status = Some(format!("{} — {e}", path.display()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Note that a buffer has been typed into.
    ///
    /// The one place `dirty` is ever set true; before this the amber tab dot
    /// was unreachable and every close discarded edits in silence.
    fn mark_dirty(&mut self, root: &Path, uid: u64, cx: &mut Context<Self>) {
        if let Some(buffers) = self.editors.get_mut(root)
            && let Some(file) = buffers.tabs.files.iter_mut().find(|f| f.uid == uid)
            && !file.dirty
        {
            file.dirty = true;
            cx.notify();
        }
    }

    /// Save the active tab, refusing to clobber a concurrent writer.
    pub fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(uid) = self
            .editors
            .get(&root)
            .and_then(|buffers| buffers.tabs.active_file())
            .map(|file| file.uid)
        else {
            return;
        };
        self.save_tab(root, uid, cx);
    }

    /// Write one tab, keyed by `uid` rather than by tab position so a re-run
    /// after an in-flight save still targets the buffer that asked for it.
    fn save_tab(&mut self, root: PathBuf, uid: u64, cx: &mut Context<Self>) {
        self.pending_close = None;
        // Already writing this tab: remember that another save was asked for
        // and let the one in flight land first (see `saving`).
        if let Some(again) = self.saving.get_mut(&uid) {
            *again = true;
            return;
        }
        let Some(buffers) = self.editors.get(&root) else {
            return;
        };
        let Some(file) = buffers.tabs.files.iter().find(|f| f.uid == uid) else {
            return;
        };
        let Some(state) = buffers.buffer(uid) else {
            return;
        };

        let (path, label, mtime) = (file.path.clone(), file.label.clone(), file.disk_mtime);
        let text = state.read(cx).text().to_string();
        self.saving.insert(uid, false);

        cx.spawn(async move |panel, cx| {
            let outcome = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { onehand_core::editor::save_blocking(&path, text, mtime) }
                })
                .await;

            let _ = panel.update(cx, |panel: &mut Self, cx| {
                panel.status = editor::save_status(&outcome, &label);
                let again = panel.saving.remove(&uid).unwrap_or(false);
                // Read the live buffer before taking the tab set mutably: what
                // the dot should say depends on whether the user kept typing
                // while the write was in flight.
                let current = panel
                    .editors
                    .get(&root)
                    .and_then(|buffers| buffers.buffer(uid))
                    .map(|state| state.read(cx).text().to_string());

                if let Some(buffers) = panel.editors.get_mut(&root)
                    && let Some(file) = buffers.tabs.files.iter_mut().find(|f| f.uid == uid)
                {
                    match &outcome {
                        SaveOutcome::Saved { mtime, text } => {
                            file.disk_mtime = *mtime;
                            // The snapshot core hands back is the whole point of
                            // it carrying one: keystrokes that arrived during
                            // the write are still unsaved, and clearing the dot
                            // for them would be a lie the next close acts on.
                            file.dirty = current.is_some_and(|live| live != **text);
                        }
                        // Record the *current* on-disk time so the next
                        // save is an explicit, informed overwrite.
                        SaveOutcome::Conflict { disk_mtime } => file.disk_mtime = *disk_mtime,
                        SaveOutcome::Failed(_) => {}
                    }
                }
                if again {
                    panel.save_tab(root, uid, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn select_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.pending_close = None;
        if let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get_mut(r))
            && idx < buffers.tabs.files.len()
        {
            buffers.tabs.active = idx;
        }
        cx.notify();
    }

    /// Close tab `idx`, asking twice when that would throw away edits.
    pub fn close_tab(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get_mut(r)) else {
            return;
        };
        let Some(file) = buffers.tabs.files.get(idx) else {
            return;
        };
        if file.dirty && self.pending_close != Some(PendingClose::Tab(idx)) {
            let label = file.label.clone();
            self.pending_close = Some(PendingClose::Tab(idx));
            self.status = Some(format!(
                "{label} has unsaved edits — Ctrl+S to save, or click ✕ again to discard them."
            ));
            cx.notify();
            return;
        }

        let uid = file.uid;
        buffers.tabs.close(idx);
        buffers.forget(uid);
        self.pending_close = None;
        self.status = None;
        cx.notify();
    }

    /// Close every tab, asking twice when any of them has unsaved edits.
    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get_mut(r)) else {
            return;
        };
        if buffers.any_dirty() && self.pending_close != Some(PendingClose::All) {
            let n = buffers.tabs.files.iter().filter(|f| f.dirty).count();
            let s = if n == 1 { "file has" } else { "files have" };
            self.pending_close = Some(PendingClose::All);
            self.status = Some(format!(
                "{n} {s} unsaved edits — click ✕ again to discard them."
            ));
            cx.notify();
            return;
        }

        for file in std::mem::take(&mut buffers.tabs.files) {
            buffers.forget(file.uid);
        }
        buffers.tabs.close_all();
        self.pending_close = None;
        self.status = None;
        cx.notify();
    }
}

/// Process-wide buffer id salt, for the same reason core hands one out per tab.
fn next_buffer_uid() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Panel for Workbench {
    fn panel_name(&self) -> &'static str {
        "Workbench"
    }

    /// The content-only maximize: the panel fills the frame right of the rail,
    /// which stays. Put in the
    /// toolbar rather than the overflow menu -- it is the direction reached
    /// for most often, and the app direction already has a key.
    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        Some(PanelControl::Toolbar)
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Workbench")
    }
}

impl EventEmitter<PanelEvent> for Workbench {}

impl Focusable for Workbench {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.host.focus_handle()
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_terminal_theme(cx);
        let mode = self.mode();
        let contributions = self.host.contributions().to_vec();
        let modes = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(
                contributions
                    .into_iter()
                    .map(|item| mode_tab(item.label, item.id, mode, cx)),
            );

        // The two facts below are read off the showing mode's own declaration
        // rather than worked out from its ID. A mode nothing registered has
        // neither, and falls back to the panel's own context and scale so the
        // strip that would switch away from it still answers the keyboard.
        let showing = self.host.active_contribution().copied();
        let key_context = showing
            .map(|item| item.key_context)
            .unwrap_or(onehand_plugin_api::WORKBENCH_KEY_CONTEXT);
        let rem_zoom = showing.is_none_or(|item| item.rem_zoom);

        // The mode strip is chrome and keeps its size; only the work below it
        // scales. A zoomed-in editor whose own tab bar grew with it wastes the
        // room the zoom was asking for.
        let zoom = self.zoom;
        let body = if mode == EDITOR_MODE {
            self.editor_body(cx)
        } else if mode == FILES_MODE {
            self.files_body(cx)
        } else if mode == NEOVIM_MODE {
            self.neovim_body(cx)
        } else {
            hint("This Workbench contribution is unavailable", cx)
        };
        // A measured glyph grid is sized by the font it was configured with and
        // not by the rem base around it, so a mode that says so is left alone:
        // wrapping it in the scale would stretch the box while the cell stayed
        // put, leaving every column landing past its own character. Such a mode
        // takes its reading size as a font size instead, through `set_zoom`.
        let body = if rem_zoom {
            zoom.scale(window, body).into_any_element()
        } else {
            body
        };
        div()
            .size_full()
            .v_flex()
            // A mode hosting a PTY takes the *terminal's* context while it is
            // showing, and it has to be that name and not one of its own:
            // `Ctrl+S` is bound `Shell && !Terminal` so that a program in a PTY
            // keeps it, and a predicate that had to learn a second name for the
            // same fact is one that gets updated in one place and not the other.
            // Under any other mode this is the Workbench, which is what the save
            // is *for*.
            .key_context(key_context)
            .child(modes)
            .child(body)
            .when_some(self.status.clone(), |panel, status| {
                panel.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(crate::theme::status_ink(cx).warning)
                        .child(status),
                )
            })
    }
}

impl Workbench {
    /// Recolour a live grid after an app appearance change.
    ///
    /// Guarded by comparison rather than run every frame: rebuilding the palette
    /// and pushing a whole config into the view is work, and this is called from
    /// render.
    fn sync_terminal_theme(&mut self, cx: &mut Context<Self>) {
        let current = TerminalThemeKey::current(cx);
        if self.terminal_theme == current {
            return;
        }
        self.terminal_theme = current;
        let colors = terminal_palette(cx);
        self.neovim.set_palette(colors, cx);
    }

    fn neovim_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(root) = self.root.clone() else {
            return hint("No project root", cx);
        };

        let Some(tab) = self.neovim.get(&root) else {
            // The button and not an automatic spawn, for the same reason
            // switching to this mode does not spawn: arriving at a tab should
            // not start a process. It also gives the failure somewhere to be
            // read — a Neovim that would not start leaves its reason in
            // `status`, one line below this.
            return div()
                .flex_1()
                .v_flex()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    crate::controls::action("start-neovim")
                        .primary()
                        .label("Start Neovim")
                        .on_click(cx.listener(|panel: &mut Self, _, window, cx| {
                            panel.open_neovim(cx);
                            panel.focus_active(window, cx);
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

    fn editor_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(buffers) = self.root.as_ref().and_then(|r| self.editors.get(r)) else {
            return hint("Open a file from a tool card or the Files tab", cx);
        };
        if buffers.tabs.files.is_empty() {
            return hint("Open a file from a tool card or the Files tab", cx);
        }
        let active = buffers.tabs.active_file().map(|f| f.uid);
        let body = active.and_then(|uid| buffers.buffer(uid)).cloned();

        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(editor::tab_strip(
                buffers,
                cx.listener(|panel: &mut Self, idx: &usize, _, cx| panel.select_tab(*idx, cx)),
                cx.listener(|panel: &mut Self, idx: &usize, _, cx| panel.close_tab(*idx, cx)),
                cx.listener(|panel: &mut Self, _, _, cx| panel.close_all_tabs(cx)),
                cx,
            ))
            .children(body.map(|state| {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(editor::body(&state))
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn files_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(root) = self.root.clone() else {
            return hint("No project root", cx);
        };
        let Some(tree) = self.trees.get(&root) else {
            return hint("No project root", cx);
        };
        let ink = crate::theme::status_ink(cx);
        div()
            .flex_1()
            .min_h_0()
            .child(files::tree(
                &root,
                tree,
                self.git.get(&root),
                cx.listener(|panel: &mut Self, dir: &PathBuf, _, cx| {
                    panel.toggle_dir(dir.clone(), cx)
                }),
                cx.listener(|panel: &mut Self, path: &PathBuf, window, cx| {
                    panel.open_file(path.clone(), window, cx)
                }),
                files::ChangeColors {
                    warning: ink.warning,
                    success: ink.success,
                    danger: ink.danger,
                },
                cx,
            ))
            .into_any_element()
    }
}

fn mode_tab(
    label: &'static str,
    which: WorkbenchMode,
    active: WorkbenchMode,
    cx: &mut Context<Workbench>,
) -> impl IntoElement + use<> {
    crate::controls::action(label)
        .xsmall()
        .map(|b| {
            if which == active {
                b.primary()
            } else {
                b.ghost()
            }
        })
        .label(label)
        .on_click(cx.listener(move |panel: &mut Workbench, _, _, cx| {
            panel.set_mode(which, cx);
        }))
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
