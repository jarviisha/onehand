use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};
use onehand_terminal_ui::PtyTab;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const MODE_ID: PluginId = PluginId::new("workbench.neovim");

pub struct NeovimPlugin;

/// The live Neovim processes owned by one Workbench window, at most one per
/// project root. Dropping or forgetting an entry terminates and reaps its PTY.
#[derive(Default)]
pub struct NeovimSessions {
    tabs: HashMap<PathBuf, PtyTab>,
}

impl NeovimSessions {
    pub fn contains(&self, root: &Path) -> bool {
        self.tabs.contains_key(root)
    }
    pub fn insert(&mut self, root: PathBuf, tab: PtyTab) {
        self.tabs.insert(root, tab);
    }
    pub fn forget_root(&mut self, root: &Path) {
        self.tabs.remove(root);
    }
    pub fn get(&self, root: &Path) -> Option<&PtyTab> {
        self.tabs.get(root)
    }

    pub fn reap_finished(&mut self) -> bool {
        let before = self.tabs.len();
        self.tabs.retain(|_, tab| !tab.finished());
        self.tabs.len() != before
    }

    pub fn set_font_size(&self, size: gpui::Pixels, cx: &mut gpui::App) {
        for tab in self.tabs.values() {
            tab.set_font_size(size, cx);
        }
    }

    pub fn set_palette(&self, palette: gpui_terminal::ColorPalette, cx: &mut gpui::App) {
        for tab in self.tabs.values() {
            tab.set_palette(palette.clone(), cx);
        }
    }
}

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
