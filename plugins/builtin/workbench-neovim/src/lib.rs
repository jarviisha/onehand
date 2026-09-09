//! Neovim mode: the real editor, in a PTY, on the project root.

use gpui::{AnyView, App, Entity, Pixels, Window};
use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod view;
pub use view::NeovimView;

pub const MODE_ID: PluginId = PluginId::new("workbench.neovim");

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
        WorkbenchModeSpec::terminal_grid(MODE_ID, "Neovim")
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

pub struct NeovimPlugin;

impl BuiltinPlugin for NeovimPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: PluginId::new("builtin.workbench-neovim"),
            name: "Workbench Neovim",
            version: env!("CARGO_PKG_VERSION"),
            api_version: PLUGIN_API_VERSION,
            capabilities: &[Capability::WorkbenchMode],
        }
    }

    /// A live PTY, so it declares the terminal's key context and takes its
    /// reading size as a font size rather than from the panel's rem base.
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
        registrar.register_workbench_mode(WorkbenchModeSpec::terminal_grid(MODE_ID, "Neovim"))
    }
}
