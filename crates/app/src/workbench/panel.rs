//! The Workbench dock panel: Editor, Files, Markdown and Neovim, one at a time.
//!
//! **The panel owns no mode's state and draws no mode's body.** It keeps the
//! list, remembers which one is showing, draws the strip that switches between
//! them, and passes on the handful of things the shell asks of "the Workbench"
//! without knowing which mode answers. Everything a mode works on — open
//! buffers, the file tree, a document index, a live PTY — is inside that mode,
//! and every mode is per project root, so switching roots swaps the whole panel
//! rather than mixing one project's tabs with another's tree.
//!
//! Two facts about the showing mode reach the frame from here, and both are
//! read off that mode's own declaration rather than worked out from its ID. Its
//! **key context** is what makes `Ctrl+S` reach `:w` inside a PTY: that binding
//! is `Shell && !Terminal` precisely so a program in a terminal keeps it, and a
//! grid mounted with no such context would have the quick editor's save fire
//! over the top of it. And whether its body is **scaled by the rem base**: a
//! measured glyph grid is sized from a shaped glyph, so scaling the box around
//! it stretches the container while the cell stays put and every column lands
//! past its own character.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dock::{Panel, PanelControl, PanelEvent};
use gpui_component::{ActiveTheme, Sizable as _, StyledExt};
use onehand_core::gitstat::GitStatus;
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const EDITOR_MODE: PluginId = onehand_workbench_editor::SPEC.id;
pub const FILES_MODE: PluginId = onehand_workbench_files::SPEC.id;
pub const MARKDOWN_MODE: PluginId = onehand_workbench_markdown::SPEC.id;
pub const NEOVIM_MODE: PluginId = onehand_workbench_neovim::SPEC.id;

pub struct Workbench {
    focus_handle: FocusHandle,
    /// The modes, in the order they sit on the strip.
    ///
    /// Each is asked in this order, and a mode that has an answer to a request
    /// says so.
    modes: Vec<Box<dyn WorkbenchMode>>,
    /// Which mode is showing, as a place in `modes` rather than an ID.
    ///
    /// A place and not a name because a name can fail to resolve, and this one
    /// cannot: the list is built once and never added to, so every ID that ever
    /// reaches `active` was found in it first. Held as an ID, the panel had to
    /// draw an unavailable state for a mode nothing contributed — an arm no
    /// input could reach, which is the decoration the seam was meant to remove
    /// rather than keep.
    active: usize,
    /// The root every mode is pointed at. `None` before any root is active.
    root: Option<PathBuf>,
    /// Reading size for the body. Per panel, not per root: the dock's own
    /// sizes are per window here, and a zoom that reset itself on every root
    /// switch would be the odd one out.
    zoom: crate::zoom::Zoom,
}

impl Workbench {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            // Handed to the modes as they are built rather than set afterwards,
            // so none of them is ever on screen holding nothing to ask through.
            let panel = cx.weak_entity();
            let ask: Ask = Rc::new(move |request, window, cx| {
                let _ = panel.update(cx, |panel: &mut Self, cx| {
                    panel.answer(request, window, cx);
                });
            });
            let zoom = crate::zoom::Zoom::default();
            let modes = crate::plugins::workbench_modes(ask, crate::zoom::term_font_size(zoom), cx);
            assert!(!modes.is_empty(), "the Workbench has no built-in modes");
            Self {
                active: 0,
                modes,
                focus_handle: cx.focus_handle(),
                root: None,
                zoom,
            }
        })
    }

    /// Answer a request a mode raised, which is the panel's half of
    /// [`onehand_plugin_host::Ask`].
    ///
    /// The vocabulary is the same in both directions, so what arrives here is
    /// either something one mode is asking of another — opening a file is the
    /// quick editor's business and the file tree cannot reach it — or something
    /// only the panel can decide.
    fn answer(&mut self, request: &Request<'_>, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            Request::OpenFile(path) => self.open_file(path, window, cx),
            // The caret is the panel's half of reaping: a view dropped while it
            // holds focus leaves the window pointing at an element no frame
            // contains, and GPUI resolves a key along the path down to the
            // focused node — so every shortcut stops working, including the one
            // that would reopen this panel. Asked *before* the drop, since a
            // handle no longer drawn cannot answer. Only moved when focus was
            // inside this panel already: a child exiting in the background must
            // not take the caret from what the user is doing.
            Request::Reap => {
                let held = self.focus_handle.contains_focused(window, cx);
                if self.broadcast(&Request::Reap, cx) && held {
                    self.focus_active(window, cx);
                }
            }
            other => {
                self.broadcast(other, cx);
            }
        }
    }

    /// Put a request to every mode in the panel's order, and say whether one of
    /// them took it.
    fn broadcast(&mut self, request: &Request<'_>, cx: &mut Context<Self>) -> bool {
        let mut taken = false;
        for mode in &mut self.modes {
            taken |= mode.handle(request, cx);
        }
        taken
    }

    /// The showing mode's own declaration, which is where the two facts the
    /// frame needs are read from.
    fn showing(&self) -> WorkbenchModeSpec {
        self.modes[self.active].spec()
    }

    /// Where `id` sits on the strip, if it names a mode this panel holds.
    fn place_of(&self, id: PluginId) -> Option<usize> {
        self.modes.iter().position(|item| item.spec().id == id)
    }

    pub fn mode(&self) -> PluginId {
        self.showing().id
    }

    /// How many of `root`'s open files have edits a removal would discard.
    ///
    /// Asked by the shell *before* it removes a root, because that is the one
    /// place with a control to guard.
    pub fn unsaved_in(&self, root: &Path, cx: &App) -> usize {
        self.modes.iter().map(|mode| mode.unsaved(root, cx)).sum()
    }

    /// Drop everything every mode holds for `root`.
    ///
    /// Called when a project root leaves the workspace.
    pub fn forget_root(&mut self, root: &Path, cx: &mut Context<Self>) {
        for mode in &mut self.modes {
            mode.forget_root(root, cx);
        }
        if self.root.as_deref() == Some(root) {
            self.root = None;
        }
        cx.notify();
    }

    /// Start whatever child process a mode is a front end for.
    ///
    /// Separate from switching to that mode on purpose: a mode change is a view
    /// change and must not launch a process, or the strip becomes a row of
    /// buttons one of which spawns something. The key does this first and then
    /// switches; the mode's own empty state is the other way in.
    pub fn start_child(&mut self, cx: &mut Context<Self>) {
        self.broadcast(&Request::Start, cx);
    }

    /// Write whatever is open, which is `Ctrl+S`.
    pub fn save_active(&mut self, cx: &mut Context<Self>) {
        self.broadcast(&Request::Save, cx);
    }

    /// Step this panel's zoom.
    ///
    /// Handed the whole value rather than a `&mut` to the field, because a step
    /// here is not only a number: a measured glyph grid has to be re-measured
    /// at the new font size, and a caller holding the field directly would set
    /// it and leave that grid drawn at the old one.
    pub fn set_zoom(&mut self, zoom: crate::zoom::Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        self.broadcast(&Request::SetFontSize(crate::zoom::term_font_size(zoom)), cx);
        cx.notify();
    }

    /// This panel's zoom, for the status bar to report.
    pub fn zoom(&self) -> crate::zoom::Zoom {
        self.zoom
    }

    /// Put focus where the showing mode's work happens.
    ///
    /// A panel shortcut that opens a dock without moving focus makes the user
    /// reach for the mouse to use what they just opened. A mode that is clicked
    /// rather than typed into refuses, and the caret lands on the panel itself.
    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        if self.modes[self.active].focus(window, cx) {
            return;
        }
        self.focus_handle.focus(window, cx);
    }

    /// Show `mode`, if it names one this panel holds.
    ///
    /// A mode nobody contributed is refused rather than left as the active
    /// place, which is what keeps that place always in range.
    ///
    /// **Showing the mode already showing is not a no-op**, deliberately: the
    /// keys are three-state and pressing one on the mode in front of you is how
    /// a listing gets asked for again. Returning early here is what silently
    /// took the document walk away from a second `Ctrl+Shift+M`.
    pub fn set_mode(&mut self, mode: PluginId, cx: &mut Context<Self>) {
        let Some(place) = self.place_of(mode) else {
            return;
        };
        self.active = place;
        // To the arriving mode alone, and not broadcast: it is the one request
        // whose answer depends on being the mode about to be seen. A listing
        // that costs a walk of the whole project is refreshed here rather than
        // at boot or on a project switch behind another mode's back.
        self.modes[place].handle(&Request::Shown, cx);
        cx.notify();
    }

    /// Point every mode at a project root.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.root.as_ref() == Some(&root) {
            return;
        }
        self.root = Some(root.clone());
        for mode in &mut self.modes {
            mode.set_root(&root, cx);
        }
        cx.notify();
    }

    pub fn set_git(&mut self, git: HashMap<PathBuf, GitStatus>, cx: &mut Context<Self>) {
        self.broadcast(&Request::SetGit(&git), cx);
        cx.notify();
    }

    /// Re-read whatever the modes listed off the disk.
    ///
    /// Called when a turn ends and when the window is activated — the two
    /// moments a project is most likely to have moved under it.
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        self.broadcast(&Request::Rescan, cx);
    }

    /// Open `path` in whichever mode edits files, and switch to it.
    ///
    /// Switching is what the answer buys: the mode that took the file is the
    /// one worth looking at, and the panel does not have to know which that is.
    pub fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let opened = self
            .modes
            .iter_mut()
            .position(|mode| mode.open_file(path, window, cx));
        if let Some(place) = opened {
            self.active = place;
            self.modes[place].handle(&Request::Shown, cx);
        }
        cx.notify();
    }
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
        self.focus_handle.clone()
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let showing = self.showing();
        let specs: Vec<WorkbenchModeSpec> = self.modes.iter().map(|item| item.spec()).collect();
        let strip = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(
                specs
                    .into_iter()
                    .map(|spec| mode_tab(spec.label, spec.id, showing.id, cx)),
            );

        // The mode strip is chrome and keeps its size; only the work below it
        // scales. A zoomed-in editor whose own tab bar grew with it wastes the
        // room the zoom was asking for.
        let body = self.modes[self.active].view().into_any_element();
        // A measured glyph grid is sized by the font it was configured with and
        // not by the rem base around it, so a mode that says so is left alone:
        // wrapping it in the scale would stretch the box while the cell stayed
        // put, leaving every column landing past its own character. Such a mode
        // takes its reading size as a font size instead, through `set_zoom`.
        let body = if showing.rem_zoom {
            self.zoom.scale(window, body).into_any_element()
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
            .key_context(showing.key_context)
            .child(strip)
            .child(body)
    }
}

fn mode_tab(
    label: &'static str,
    which: PluginId,
    active: PluginId,
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
