use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};

mod view;
pub use view::*;

pub const MODE_ID: PluginId = PluginId::new("workbench.editor");

pub struct EditorPlugin;

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
        registrar.register_workbench_mode(WorkbenchModeSpec::element(MODE_ID, "Editor"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn dirty_editor_state_is_reported() {
        let mut buffers = RootBuffers::default();
        buffers.tabs.open(PathBuf::from("file.rs"), None, 7);
        buffers.tabs.files[0].dirty = true;
        assert!(buffers.any_dirty());
    }
}
