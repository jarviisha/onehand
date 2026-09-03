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

pub trait PluginRegistrar {
    fn register_workbench_mode(&mut self, id: PluginId, label: &'static str) -> Result<(), String>;

    fn register_remote_channel(&mut self, id: PluginId, label: &'static str) -> Result<(), String>;
}

pub trait BuiltinPlugin {
    fn descriptor(&self) -> PluginDescriptor;
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String>;
}
