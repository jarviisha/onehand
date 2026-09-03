use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
};
use onehand_plugin_host::WorkbenchModeView;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod view;
pub use view::*;

pub const MODE_ID: PluginId = PluginId::new("workbench.editor");

pub struct EditorPlugin;

#[derive(Default)]
struct EditorView {
    roots: HashSet<PathBuf>,
    active: Option<PathBuf>,
    zoom: f32,
    focused: Cell<bool>,
}

impl WorkbenchModeView for EditorView {
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
    Box::new(EditorView::default())
}

impl BuiltinPlugin for EditorPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: PluginId::new("builtin.workbench-editor"),
            name: "Workbench Editor",
            version: env!("CARGO_PKG_VERSION"),
            api_version: PLUGIN_API_VERSION,
            capabilities: &[Capability::WorkbenchMode],
        }
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
        registrar.register_workbench_mode(MODE_ID, "Editor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_keeps_then_forgets_per_root_state() {
        let root = PathBuf::from("/tmp/editor-root");
        let mut view = EditorView::default();
        view.set_root(Some(root.clone()));
        view.set_zoom(1.3);
        view.focus();
        assert!(view.roots.contains(&root) && view.focused.get());
        assert_eq!(view.zoom, 1.3);
        view.forget_root(&root);
        assert!(!view.roots.contains(&root));
    }

    #[test]
    fn dirty_editor_state_is_reported() {
        let mut buffers = RootBuffers::default();
        buffers.tabs.open(PathBuf::from("file.rs"), None, 7);
        buffers.tabs.files[0].dirty = true;
        assert!(buffers.any_dirty());
    }
}
