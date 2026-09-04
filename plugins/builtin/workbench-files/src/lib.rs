use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};

mod view;
pub use view::*;

pub const MODE_ID: PluginId = PluginId::new("workbench.files");

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
