//! Markdown mode: the project's `.md` files, and the one being read, rendered.

use gpui::{AnyView, App, Entity};
use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod index;
mod mode;
mod view;
pub use mode::MarkdownView;

pub const MODE_ID: PluginId = PluginId::new("workbench.markdown");

/// The Markdown mode: a view and nothing else.
pub struct Mode {
    view: Entity<MarkdownView>,
}

impl Mode {
    pub fn new(ask: Ask, cx: &mut App) -> Self {
        Self {
            view: MarkdownView::new(ask, cx),
        }
    }
}

impl WorkbenchMode for Mode {
    fn spec(&self) -> WorkbenchModeSpec {
        WorkbenchModeSpec::element(MODE_ID, "Markdown")
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

    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        match request {
            // The walk is the whole project, so what these two do is note that
            // it is out of date. It is run when the mode is next drawn, which
            // is the moment that is known not to be a project nobody has
            // opened this mode on.
            Request::Rescan | Request::Shown => {
                self.view.update(cx, |view, cx| view.mark_stale(cx));
                true
            }
            _ => false,
        }
    }
}

pub struct MarkdownPlugin;

impl BuiltinPlugin for MarkdownPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: PluginId::new("builtin.workbench-markdown"),
            name: "Workbench Markdown",
            version: env!("CARGO_PKG_VERSION"),
            api_version: PLUGIN_API_VERSION,
            capabilities: &[Capability::WorkbenchMode],
        }
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
        registrar.register_workbench_mode(WorkbenchModeSpec::element(MODE_ID, "Markdown"))
    }
}
