//! Markdown mode: the project's `.md` files, and the one being read, rendered.

use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};

mod index;
mod view;
pub use index::*;
pub use view::*;

pub const MODE_ID: PluginId = PluginId::new("workbench.markdown");

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
