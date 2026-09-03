//! Startup-only registry for built-in plugins.

use gpui::{App, FocusHandle};
use gpui::{ElementId, Styled as _};
use gpui_component::button::Button;
use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub trait WorkbenchModeView {
    fn set_root(&mut self, root: Option<PathBuf>);
    fn forget_root(&mut self, root: &Path);
    fn focus(&self) {}
    fn set_zoom(&mut self, _factor: f32) {}
    fn dirty_in(&self, _root: &Path) -> usize {
        0
    }
    fn shutdown(&mut self) {}
}

pub type WorkbenchModeFactory = fn() -> Box<dyn WorkbenchModeView>;
pub type RemoteChannelFactory = fn(String) -> Box<dyn onehand_core::remote::types::RemoteChannel>;

/// Build a plugin-owned button with Onehand's pointer affordance.
pub fn action(id: impl Into<ElementId>) -> Button {
    Button::new(id).cursor_pointer()
}

/// Per-window Workbench host. It owns the open contribution instances and
/// fans lifecycle changes only to those instances; UI rendering remains in
/// process and is selected by stable contribution ID.
pub struct WorkbenchHost {
    focus_handle: FocusHandle,
    active: PluginId,
    contributions: Vec<WorkbenchModeContribution>,
    views: HashMap<PluginId, Box<dyn WorkbenchModeView>>,
}

impl WorkbenchHost {
    pub fn new(contributions: Vec<WorkbenchModeContribution>, cx: &mut App) -> Self {
        let active = contributions
            .first()
            .expect("Workbench has no built-in modes")
            .id;
        let views = contributions
            .iter()
            .map(|item| {
                let factory = item
                    .factory
                    .expect("sealed Workbench contribution has no factory");
                (item.id, factory())
            })
            .collect();
        Self {
            focus_handle: cx.focus_handle(),
            active,
            contributions,
            views,
        }
    }

    pub fn active(&self) -> PluginId {
        self.active
    }
    pub fn contributions(&self) -> &[WorkbenchModeContribution] {
        &self.contributions
    }
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn select(&mut self, id: PluginId) -> bool {
        if self.views.contains_key(&id) {
            self.active = id;
            true
        } else {
            false
        }
    }

    pub fn set_root(&mut self, root: Option<PathBuf>) {
        for view in self.views.values_mut() {
            view.set_root(root.clone());
        }
    }

    pub fn forget_root(&mut self, root: &Path) {
        for view in self.views.values_mut() {
            view.forget_root(root);
        }
    }

    pub fn focus_active(&self) {
        if let Some(view) = self.views.get(&self.active) {
            view.focus();
        }
    }

    pub fn set_zoom(&mut self, factor: f32) {
        for view in self.views.values_mut() {
            view.set_zoom(factor);
        }
    }
}

impl Drop for WorkbenchHost {
    fn drop(&mut self) {
        for view in self.views.values_mut() {
            view.shutdown();
        }
    }
}

#[derive(Clone, Copy)]
pub struct WorkbenchModeContribution {
    pub plugin_id: PluginId,
    pub id: PluginId,
    pub label: &'static str,
    pub factory: Option<WorkbenchModeFactory>,
}

#[derive(Clone, Copy)]
pub struct RemoteChannelContribution {
    pub plugin_id: PluginId,
    pub id: PluginId,
    pub label: &'static str,
    pub factory: Option<RemoteChannelFactory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    DuplicatePlugin(PluginId),
    UnsupportedApi {
        plugin: PluginId,
        expected: u32,
        actual: u32,
    },
    MissingCapability {
        plugin: PluginId,
        contribution: PluginId,
        capability: Capability,
    },
    DuplicateContribution(PluginId),
    Registration {
        plugin: PluginId,
        message: String,
    },
    MissingFactory(PluginId),
    Sealed,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePlugin(id) => write!(f, "plugin `{id}` was registered twice"),
            Self::UnsupportedApi {
                plugin,
                expected,
                actual,
            } => write!(
                f,
                "plugin `{plugin}` targets API {actual}, but this host supports API {expected}"
            ),
            Self::MissingCapability {
                plugin,
                contribution,
                capability,
            } => write!(
                f,
                "plugin `{plugin}` registered contribution `{contribution}` without declaring capability {capability:?}"
            ),
            Self::DuplicateContribution(id) => {
                write!(f, "contribution ID `{id}` was registered twice")
            }
            Self::Registration { plugin, message } => {
                write!(f, "plugin `{plugin}` failed registration: {message}")
            }
            Self::MissingFactory(id) => write!(f, "contribution `{id}` has no factory"),
            Self::Sealed => f.write_str("plugin registry is sealed; registration is startup-only"),
        }
    }
}

impl std::error::Error for RegistryError {}

#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<PluginDescriptor>,
    workbench_modes: Vec<WorkbenchModeContribution>,
    remote_channels: Vec<RemoteChannelContribution>,
    sealed: bool,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: &dyn BuiltinPlugin) -> Result<(), RegistryError> {
        if self.sealed {
            return Err(RegistryError::Sealed);
        }
        let descriptor = plugin.descriptor();
        if self.plugins.iter().any(|p| p.id == descriptor.id) {
            return Err(RegistryError::DuplicatePlugin(descriptor.id));
        }
        if descriptor.api_version != PLUGIN_API_VERSION {
            return Err(RegistryError::UnsupportedApi {
                plugin: descriptor.id,
                expected: PLUGIN_API_VERSION,
                actual: descriptor.api_version,
            });
        }

        let mut pending = Pending::new(descriptor);
        let registration = plugin.register(&mut pending);
        // A registrar error is sticky. A plugin cannot turn a rejected
        // contribution into a successful registration by ignoring its Result.
        if let Some(error) = pending.capability_error.take() {
            return Err(error);
        }
        if let Err(message) = registration {
            return Err(RegistryError::Registration {
                plugin: descriptor.id,
                message,
            });
        }
        pending.commit(self)?;
        self.plugins.push(descriptor);
        Ok(())
    }

    pub fn set_workbench_factory(
        &mut self,
        id: PluginId,
        factory: WorkbenchModeFactory,
    ) -> Result<(), RegistryError> {
        if self.sealed {
            return Err(RegistryError::Sealed);
        }
        let contribution = self
            .workbench_modes
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(RegistryError::MissingFactory(id))?;
        contribution.factory = Some(factory);
        Ok(())
    }

    pub fn set_remote_factory(
        &mut self,
        id: PluginId,
        factory: RemoteChannelFactory,
    ) -> Result<(), RegistryError> {
        if self.sealed {
            return Err(RegistryError::Sealed);
        }
        let contribution = self
            .remote_channels
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(RegistryError::MissingFactory(id))?;
        contribution.factory = Some(factory);
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), RegistryError> {
        if let Some(item) = self
            .workbench_modes
            .iter()
            .find(|item| item.factory.is_none())
        {
            return Err(RegistryError::MissingFactory(item.id));
        }
        if let Some(item) = self
            .remote_channels
            .iter()
            .find(|item| item.factory.is_none())
        {
            return Err(RegistryError::MissingFactory(item.id));
        }
        self.sealed = true;
        Ok(())
    }

    pub fn plugins(&self) -> &[PluginDescriptor] {
        &self.plugins
    }

    pub fn workbench_modes(&self) -> &[WorkbenchModeContribution] {
        &self.workbench_modes
    }

    pub fn remote_channels(&self) -> &[RemoteChannelContribution] {
        &self.remote_channels
    }
}

struct Pending {
    descriptor: PluginDescriptor,
    workbench: Vec<(PluginId, &'static str)>,
    remote: Vec<(PluginId, &'static str)>,
    capability_error: Option<RegistryError>,
}

impl Pending {
    fn new(descriptor: PluginDescriptor) -> Self {
        Self {
            descriptor,
            workbench: Vec::new(),
            remote: Vec::new(),
            capability_error: None,
        }
    }

    fn ensure(&mut self, capability: Capability, id: PluginId) -> Result<(), String> {
        if !self.descriptor.has(capability) {
            self.capability_error = Some(RegistryError::MissingCapability {
                plugin: self.descriptor.id,
                contribution: id,
                capability,
            });
            return Err(format!(
                "contribution `{id}` requires undeclared capability {capability:?}"
            ));
        }
        Ok(())
    }

    fn commit(self, registry: &mut PluginRegistry) -> Result<(), RegistryError> {
        let mut ids: HashSet<PluginId> = registry
            .workbench_modes
            .iter()
            .map(|c| c.id)
            .chain(registry.remote_channels.iter().map(|c| c.id))
            .collect();
        for (id, _) in self.workbench.iter().chain(self.remote.iter()) {
            if !ids.insert(*id) {
                return Err(RegistryError::DuplicateContribution(*id));
            }
        }
        registry
            .workbench_modes
            .extend(
                self.workbench
                    .into_iter()
                    .map(|(id, label)| WorkbenchModeContribution {
                        plugin_id: self.descriptor.id,
                        id,
                        label,
                        factory: None,
                    }),
            );
        registry
            .remote_channels
            .extend(
                self.remote
                    .into_iter()
                    .map(|(id, label)| RemoteChannelContribution {
                        plugin_id: self.descriptor.id,
                        id,
                        label,
                        factory: None,
                    }),
            );
        Ok(())
    }
}

impl PluginRegistrar for Pending {
    fn register_workbench_mode(&mut self, id: PluginId, label: &'static str) -> Result<(), String> {
        self.ensure(Capability::WorkbenchMode, id)?;
        self.workbench.push((id, label));
        Ok(())
    }

    fn register_remote_channel(&mut self, id: PluginId, label: &'static str) -> Result<(), String> {
        self.ensure(Capability::RemoteChannel, id)?;
        self.remote.push((id, label));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        descriptor: PluginDescriptor,
        kind: Capability,
        contribution: PluginId,
    }

    impl BuiltinPlugin for TestPlugin {
        fn descriptor(&self) -> PluginDescriptor {
            self.descriptor
        }
        fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
            match self.kind {
                Capability::WorkbenchMode => {
                    registrar.register_workbench_mode(self.contribution, "Mode")
                }
                Capability::RemoteChannel => {
                    registrar.register_remote_channel(self.contribution, "Remote")
                }
            }
        }
    }

    fn plugin(id: &'static str, mode: &'static str) -> TestPlugin {
        TestPlugin {
            descriptor: PluginDescriptor {
                id: PluginId::new(id),
                name: id,
                version: "0.1.0",
                api_version: PLUGIN_API_VERSION,
                capabilities: &[Capability::WorkbenchMode],
            },
            kind: Capability::WorkbenchMode,
            contribution: PluginId::new(mode),
        }
    }

    #[test]
    fn preserves_registration_order() {
        let mut registry = PluginRegistry::new();
        registry.register(&plugin("a", "editor")).unwrap();
        registry.register(&plugin("b", "files")).unwrap();
        assert_eq!(
            registry
                .workbench_modes()
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["editor", "files"]
        );
    }

    #[test]
    fn rejects_duplicate_plugin_and_contribution_ids() {
        let mut registry = PluginRegistry::new();
        registry.register(&plugin("a", "editor")).unwrap();
        assert!(matches!(
            registry.register(&plugin("a", "files")),
            Err(RegistryError::DuplicatePlugin(_))
        ));
        assert!(matches!(
            registry.register(&plugin("b", "editor")),
            Err(RegistryError::DuplicateContribution(_))
        ));
    }

    #[test]
    fn rejects_wrong_api_and_missing_capability() {
        let mut wrong = plugin("wrong", "mode");
        wrong.descriptor.api_version += 1;
        assert!(matches!(
            PluginRegistry::new().register(&wrong),
            Err(RegistryError::UnsupportedApi { .. })
        ));

        let mut missing = plugin("missing", "mode");
        missing.descriptor.capabilities = &[];
        assert!(matches!(
            PluginRegistry::new().register(&missing),
            Err(RegistryError::MissingCapability { .. })
        ));
    }

    #[test]
    fn capability_mismatch_stays_rejected_when_the_plugin_ignores_it() {
        struct SwallowsRegistrarError;
        impl BuiltinPlugin for SwallowsRegistrarError {
            fn descriptor(&self) -> PluginDescriptor {
                PluginDescriptor {
                    id: PluginId::new("swallows-error"),
                    name: "Swallows error",
                    version: "0.1.0",
                    api_version: PLUGIN_API_VERSION,
                    capabilities: &[],
                }
            }

            fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
                let _ = registrar.register_workbench_mode(PluginId::new("forbidden"), "Bad");
                Ok(())
            }
        }

        let mut registry = PluginRegistry::new();
        assert!(matches!(
            registry.register(&SwallowsRegistrarError),
            Err(RegistryError::MissingCapability { .. })
        ));
        assert!(registry.plugins().is_empty());
        assert!(registry.workbench_modes().is_empty());
    }

    #[test]
    fn registry_cannot_seal_until_every_contribution_has_a_factory() {
        fn factory() -> Box<dyn WorkbenchModeView> {
            struct View;
            impl WorkbenchModeView for View {
                fn set_root(&mut self, _: Option<PathBuf>) {}
                fn forget_root(&mut self, _: &Path) {}
            }
            Box::new(View)
        }

        let mut registry = PluginRegistry::new();
        registry.register(&plugin("a", "editor")).unwrap();
        assert!(matches!(
            registry.seal(),
            Err(RegistryError::MissingFactory(_))
        ));
        registry
            .set_workbench_factory(PluginId::new("editor"), factory)
            .unwrap();
        registry.seal().unwrap();
        assert!(matches!(
            registry.register(&plugin("b", "files")),
            Err(RegistryError::Sealed)
        ));
    }
}
