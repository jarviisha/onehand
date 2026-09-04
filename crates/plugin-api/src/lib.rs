//! The small, GUI-free part of Onehand's built-in plugin contract.
//!
//! Version 1 is intentionally a `0.x` API. The IDs and descriptors are designed
//! to survive a future out-of-process protocol; the Rust traits are only an
//! in-process composition seam and are not a third-party ABI promise.

use std::fmt;

pub const PLUGIN_API_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PluginId(&'static str);

impl PluginId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    WorkbenchMode,
    RemoteChannel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub id: PluginId,
    pub name: &'static str,
    pub version: &'static str,
    pub api_version: u32,
    pub capabilities: &'static [Capability],
}

impl PluginDescriptor {
    pub fn has(self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// The key context a Workbench mode takes while it is on screen, unless it says
/// otherwise. It is the app's own name for the panel, and it is what `Ctrl+S`
/// is bound against.
pub const WORKBENCH_KEY_CONTEXT: &str = "Workbench";

/// The key context a mode hosting a live PTY takes instead.
///
/// It has to be the terminal's own name and not one of the mode's: `Ctrl+S` is
/// bound so that a program inside a PTY keeps it, and that binding names this
/// context. A mode that invented a second name for the same fact would have the
/// quick editor's save fire over the top of the program's own.
pub const TERMINAL_KEY_CONTEXT: &str = "Terminal";

/// What a plugin declares when it contributes a Workbench mode.
///
/// The two facts past the name are stated by the mode rather than worked out by
/// the panel from the mode's ID. A panel branching on IDs it has to know by
/// heart is a panel that cannot host a mode it was not compiled against, and
/// every such branch is one more place a fourth mode has to be remembered in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkbenchModeSpec {
    pub id: PluginId,
    /// The word on the mode strip.
    pub label: &'static str,
    /// The key context the panel takes while this mode shows.
    pub key_context: &'static str,
    /// Whether this mode's body is scaled by the panel's rem base.
    ///
    /// False for a measured glyph grid: it is sized from a shaped glyph rather
    /// than from the rem base around it, so scaling the box stretches the
    /// container while the cell stays put and every column lands past its own
    /// character. Such a mode takes its reading size some other way.
    pub rem_zoom: bool,
}

impl WorkbenchModeSpec {
    /// A mode drawn as an ordinary element tree: it scales with the rem base
    /// and leaves the panel's own key context alone.
    pub const fn element(id: PluginId, label: &'static str) -> Self {
        Self {
            id,
            label,
            key_context: WORKBENCH_KEY_CONTEXT,
            rem_zoom: true,
        }
    }

    /// A mode hosting a live terminal grid.
    pub const fn terminal_grid(id: PluginId, label: &'static str) -> Self {
        Self {
            id,
            label,
            key_context: TERMINAL_KEY_CONTEXT,
            rem_zoom: false,
        }
    }
}

pub trait PluginRegistrar {
    fn register_workbench_mode(&mut self, mode: WorkbenchModeSpec) -> Result<(), String>;

    fn register_remote_channel(&mut self, id: PluginId, label: &'static str) -> Result<(), String>;
}

pub trait BuiltinPlugin {
    fn descriptor(&self) -> PluginDescriptor;
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String>;
}
