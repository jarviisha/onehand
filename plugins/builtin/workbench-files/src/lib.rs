use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
};
use onehand_plugin_host::WorkbenchModeView;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod view;
pub use view::*;

pub const MODE_ID: PluginId = PluginId::new("workbench.files");

pub struct FilesPlugin;

#[derive(Default)]
struct FilesView {
    roots: HashSet<PathBuf>,
    active: Option<PathBuf>,
    zoom: f32,
    focused: Cell<bool>,
}

impl WorkbenchModeView for FilesView {
    fn set_root(&mut self, root: Option<PathBuf>) {
        self.active = root.clone();
        if let Some(root) = root {
            self.roots.insert(root);
        }
    }
    fn forget_root(&mut self, root: &Path) {
        self.roots.remove(root);
        if self.active.as_deref() == Some(root) {
            self.active = None;
        }
    }
    fn focus(&self) {
        self.focused.set(true);
    }
    fn set_zoom(&mut self, factor: f32) {
        self.zoom = factor;
    }
    fn shutdown(&mut self) {
        self.roots.clear();
        self.active = None;
    }
}

pub fn create_view() -> Box<dyn WorkbenchModeView> {
    Box::new(FilesView::default())
}

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
        registrar.register_workbench_mode(MODE_ID, "Files")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_keeps_then_forgets_per_root_state() {
        let root = PathBuf::from("/tmp/files-root");
        let mut view = FilesView::default();
        view.set_root(Some(root.clone()));
        view.set_zoom(0.8);
        view.focus();
        assert!(view.roots.contains(&root) && view.focused.get());
        assert_eq!(view.zoom, 0.8);
        view.shutdown();
        assert!(view.roots.is_empty());
    }
}
