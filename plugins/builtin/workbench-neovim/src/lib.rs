//! Neovim mode: the real editor, in a PTY, on the project root.

// Nothing here is `pub` unless the binary names it: a `pub` item in a
// library is reachable from outside the crate as far as rustc is concerned,
// so `dead_code` stops at one and a contribution that lost its last caller
// looks exactly like a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity, Pixels, Window};
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod view;
pub(crate) use view::NeovimView;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::terminal_grid(PluginId::new("workbench.neovim"), "Neovim");

/// The Neovim mode: a view and nothing else.
pub struct Mode {
    view: Entity<NeovimView>,
}

impl Mode {
    pub fn new(ask: Ask, font_size: Pixels, cx: &mut App) -> Self {
        Self {
            view: NeovimView::new(ask, font_size, cx),
        }
    }
}

impl WorkbenchMode for Mode {
    /// A live PTY, so it declares the terminal's key context and takes its
    /// reading size as a font size rather than from the panel's rem base.
    fn spec(&self) -> WorkbenchModeSpec {
        SPEC
    }

    fn view(&self) -> AnyView {
        self.view.clone().into()
    }

    fn set_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.set_root(root, cx));
    }

    fn forget_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.forget_root(root, cx));
    }

    fn focus(&self, window: &mut Window, cx: &mut App) -> bool {
        let Some(caret) = self.view.read(cx).caret(cx) else {
            return false;
        };
        caret.focus(window, cx);
        true
    }

    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        match request {
            Request::Start => {
                self.view.update(cx, |view, cx| view.start(cx));
                true
            }
            Request::Reap => self.view.update(cx, |view, cx| view.reap(cx)),
            Request::SetFontSize(size) => {
                self.view
                    .update(cx, |view, cx| view.set_font_size(*size, cx));
                true
            }
            _ => false,
        }
    }
}
