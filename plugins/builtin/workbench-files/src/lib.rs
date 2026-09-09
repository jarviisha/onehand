use gpui::{AnyView, App, Entity};
use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod view;
pub use view::*;

pub const MODE_ID: PluginId = PluginId::new("workbench.files");

/// The Files mode: a view and nothing else.
///
/// Everything this mode works on lives in the view, so what is left here is the
/// declaration and the forwarding — which is the whole point of the shape.
pub struct Mode {
    view: Entity<FilesView>,
}

impl Mode {
    pub fn new(ask: Ask, cx: &mut App) -> Self {
        Self {
            view: FilesView::new(ask, cx),
        }
    }
}

impl WorkbenchMode for Mode {
    fn spec(&self) -> WorkbenchModeSpec {
        WorkbenchModeSpec::element(MODE_ID, "Files")
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
            Request::SetGit(git) => {
                self.view.update(cx, |view, cx| view.set_git(git, cx));
                true
            }
            Request::Rescan => {
                self.view.update(cx, |view, cx| view.rescan(cx));
                true
            }
            _ => false,
        }
    }
}

pub struct FilesPlugin;

impl BuiltinPlugin for FilesPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: PluginId::new("builtin.workbench-files"),
            name: "Workbench Files",
            version: env!("CARGO_PKG_VERSION"),
            api_version: PLUGIN_API_VERSION,
            capabilities: &[Capability::WorkbenchMode],
        }
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
        registrar.register_workbench_mode(WorkbenchModeSpec::element(MODE_ID, "Files"))
    }
}
