use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
};
use onehand_plugin_host::WorkbenchModeView;
use onehand_terminal_ui::PtyTab;
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const MODE_ID: PluginId = PluginId::new("workbench.neovim");

pub struct NeovimPlugin;

#[derive(Default)]
struct NeovimView {
    roots: HashSet<PathBuf>,
    active: Option<PathBuf>,
    zoom: f32,
    focused: Cell<bool>,
}

impl WorkbenchModeView for NeovimView {
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
    Box::new(NeovimView::default())
}

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

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
        registrar.register_workbench_mode(MODE_ID, "Neovim")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_keeps_zoom_and_stops_all_roots() {
        let root = PathBuf::from("/tmp/neovim-root");
        let mut view = NeovimView::default();
        view.set_root(Some(root.clone()));
        view.set_zoom(1.5);
        view.focus();
        assert!(view.roots.contains(&root) && view.focused.get());
        assert_eq!(view.zoom, 1.5);
        view.shutdown();
        assert!(view.roots.is_empty());
    }
}
