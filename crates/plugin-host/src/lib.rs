//! Startup-only registry for built-in plugins.

use gpui::{App, ElementId, FocusHandle, Styled as _};
use gpui_component::button::Button;
use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
    WorkbenchModeSpec,
};
use std::collections::HashSet;
use std::fmt;

pub type RemoteChannelFactory = fn(String) -> Box<dyn onehand_core::remote::types::RemoteChannel>;

/// A button that answers the pointer, which is every button Onehand draws.
///
/// The component library draws every button variant except `link` and `text`
/// with the **arrow** cursor. That is the platform convention this app is not
/// following: a session row, a completion candidate, a selector chip and an ask
/// choice are all hand-made `div`s that show a pointer, because that is the one
/// feedback a control gets *before* it is pressed. Half the actions on screen
/// answering the pointer and half not is worse than either rule applied whole —
/// the cursor stops meaning anything, and the only way left to find out whether
/// something is clickable is to click it.
///
/// The library re-applies the caller's own style refinement last, after its
/// `cursor_default`, so setting the cursor here wins — which is the whole reason
/// this can be a wrapper rather than a fork of the control.
///
/// **It lives here rather than in the app** because a built-in plugin draws
/// buttons too and cannot reach into the binary that hosts it. A second copy in
/// each half is two places for the library's default to be let through, and the
/// bypass is one line and looks exactly like ordinary code — which is why a
/// guard counts the call sites and why there must be only one of these to count
/// against.
pub fn action(id: impl Into<ElementId>) -> Button {
    Button::new(id).cursor_pointer()
}

/// Per-window Workbench host: which modes exist, which one is showing, and the
/// panel's focus handle.
///
/// It holds no per-mode state. The state a mode works on — open buffers, the
/// file tree, a live PTY — is owned by the panel that renders it, because
/// drawing it needs the window and the panel's own entity context. A parallel
/// set of mode objects here would be a second copy of facts the panel already
/// has, kept in step by hand and read by nobody.
pub struct WorkbenchHost {
    focus_handle: FocusHandle,
    active: PluginId,
    contributions: Vec<WorkbenchModeContribution>,
}

impl WorkbenchHost {
    pub fn new(contributions: Vec<WorkbenchModeContribution>, cx: &mut App) -> Self {
        let active = contributions
            .first()
            .expect("Workbench has no built-in modes")
            .id;
        Self {
            focus_handle: cx.focus_handle(),
            active,
            contributions,
        }
    }

    pub fn active(&self) -> PluginId {
        self.active
    }

    /// The showing mode's own declaration, which is where the panel reads the
    /// facts it would otherwise have to match on the ID for.
    pub fn active_contribution(&self) -> Option<&WorkbenchModeContribution> {
        self.contributions
            .iter()
            .find(|item| item.id == self.active)
    }

    pub fn contributions(&self) -> &[WorkbenchModeContribution] {
        &self.contributions
    }
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    /// Show `id`, if it names a registered mode. A mode nobody contributed is
    /// refused rather than left as the active ID, or the panel would draw its
    /// unavailable state with no way back.
    pub fn select(&mut self, id: PluginId) -> bool {
        if self.contributions.iter().any(|item| item.id == id) {
            self.active = id;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy)]
pub struct WorkbenchModeContribution {
    pub plugin_id: PluginId,
    pub id: PluginId,
    pub label: &'static str,
    /// The key context the panel takes while this mode is showing.
    pub key_context: &'static str,
    /// Whether this mode's body is scaled by the panel's rem base.
    pub rem_zoom: bool,
}

impl WorkbenchModeContribution {
    fn new(plugin_id: PluginId, spec: WorkbenchModeSpec) -> Self {
        Self {
            plugin_id,
            id: spec.id,
            label: spec.label,
            key_context: spec.key_context,
            rem_zoom: spec.rem_zoom,
        }
    }
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
    /// A factory was offered for an ID nothing registered — the composition
    /// root naming a contribution that does not exist.
    ///
    /// Separate from [`Self::MissingFactory`] on purpose. The two are opposite
    /// mistakes with opposite fixes: this one means the ID is wrong, that one
    /// means the ID is right and nobody attached anything to it. Reported as one
    /// variant they are indistinguishable in the startup panic, which is the
    /// only place either is ever read.
    UnknownContribution(PluginId),
    /// A registered contribution reached [`PluginRegistry::seal`] with no
    /// factory attached to it.
    ///
    /// Names the plugin as well as the contribution, the way a capability
    /// failure does: the fix is a line in the composition root, and the reader
    /// of a startup panic needs to know which plugin's line is missing rather
    /// than only which ID went unserved.
    MissingFactory {
        plugin: PluginId,
        contribution: PluginId,
    },
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
            Self::UnknownContribution(id) => write!(
                f,
                "no contribution `{id}` is registered, so there is nothing to attach a factory to"
            ),
            Self::MissingFactory {
                plugin,
                contribution,
            } => write!(
                f,
                "plugin `{plugin}` registered contribution `{contribution}` but no factory was attached to it"
            ),
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
            .ok_or(RegistryError::UnknownContribution(id))?;
        contribution.factory = Some(factory);
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), RegistryError> {
        if let Some(item) = self
            .remote_channels
            .iter()
            .find(|item| item.factory.is_none())
        {
            return Err(RegistryError::MissingFactory {
                plugin: item.plugin_id,
                contribution: item.id,
            });
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
    workbench: Vec<WorkbenchModeSpec>,
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
        let contributed = self
            .workbench
            .iter()
            .map(|mode| mode.id)
            .chain(self.remote.iter().map(|(id, _)| *id));
        for id in contributed {
            if !ids.insert(id) {
                return Err(RegistryError::DuplicateContribution(id));
            }
        }
        registry.workbench_modes.extend(
            self.workbench
                .into_iter()
                .map(|mode| WorkbenchModeContribution::new(self.descriptor.id, mode)),
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
    fn register_workbench_mode(&mut self, mode: WorkbenchModeSpec) -> Result<(), String> {
        self.ensure(Capability::WorkbenchMode, mode.id)?;
        self.workbench.push(mode);
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
                Capability::WorkbenchMode => registrar
                    .register_workbench_mode(WorkbenchModeSpec::element(self.contribution, "Mode")),
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

    fn remote_plugin(id: &'static str, channel: &'static str) -> TestPlugin {
        TestPlugin {
            descriptor: PluginDescriptor {
                id: PluginId::new(id),
                name: id,
                version: "0.1.0",
                api_version: PLUGIN_API_VERSION,
                capabilities: &[Capability::RemoteChannel],
            },
            kind: Capability::RemoteChannel,
            contribution: PluginId::new(channel),
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
                let _ = registrar.register_workbench_mode(WorkbenchModeSpec::element(
                    PluginId::new("forbidden"),
                    "Bad",
                ));
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

    /// A channel that connects to nothing, so a factory can be attached in a
    /// test without a network or a token.
    fn channel_factory(_token: String) -> Box<dyn onehand_core::remote::types::RemoteChannel> {
        use onehand_core::remote::types::{RemoteChannel, RemoteEvent, ReqRx};
        struct Silent;
        impl RemoteChannel for Silent {
            fn name(&self) -> &'static str {
                "silent"
            }
            fn connect(
                self: Box<Self>,
                _requests: ReqRx,
            ) -> std::pin::Pin<Box<dyn futures::Stream<Item = RemoteEvent> + Send>> {
                Box::pin(futures::stream::empty())
            }
        }
        Box::new(Silent)
    }

    #[test]
    fn registry_cannot_seal_until_every_contribution_has_a_factory() {
        let mut registry = PluginRegistry::new();
        registry.register(&remote_plugin("a", "telegram")).unwrap();
        assert!(matches!(
            registry.seal(),
            Err(RegistryError::MissingFactory { .. })
        ));
        registry
            .set_remote_factory(PluginId::new("telegram"), channel_factory)
            .unwrap();
        registry.seal().unwrap();
        assert!(matches!(
            registry.register(&remote_plugin("b", "discord")),
            Err(RegistryError::Sealed)
        ));
    }

    /// A factory offered for an ID nobody registered is its own failure.
    ///
    /// Reported as `MissingFactory` it read as "this contribution has none
    /// attached", which is the opposite mistake and sends the reader looking at
    /// the wrong half of the composition root.
    #[test]
    fn an_unknown_id_is_not_reported_as_a_missing_factory() {
        let mut registry = PluginRegistry::new();
        registry.register(&remote_plugin("a", "telegram")).unwrap();
        assert_eq!(
            registry.set_remote_factory(PluginId::new("typo"), channel_factory),
            Err(RegistryError::UnknownContribution(PluginId::new("typo")))
        );
        assert!(matches!(
            registry.seal(),
            Err(RegistryError::MissingFactory { .. })
        ));
    }

    /// A mode's key context and rem-zoom behaviour survive registration.
    ///
    /// They are the two facts the panel used to work out by matching the mode's
    /// ID against a list it had to know by heart; carrying them here is what
    /// lets that list go.
    #[test]
    fn a_mode_carries_its_own_key_context_and_zoom_behaviour() {
        struct GridPlugin;
        impl BuiltinPlugin for GridPlugin {
            fn descriptor(&self) -> PluginDescriptor {
                PluginDescriptor {
                    id: PluginId::new("grid"),
                    name: "Grid",
                    version: "0.1.0",
                    api_version: PLUGIN_API_VERSION,
                    capabilities: &[Capability::WorkbenchMode],
                }
            }
            fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
                registrar.register_workbench_mode(WorkbenchModeSpec::terminal_grid(
                    PluginId::new("grid.mode"),
                    "Grid",
                ))
            }
        }

        let mut registry = PluginRegistry::new();
        registry.register(&plugin("a", "editor")).unwrap();
        registry.register(&GridPlugin).unwrap();
        let modes = registry.workbench_modes();
        assert_eq!(
            (modes[0].key_context, modes[0].rem_zoom),
            (onehand_plugin_api::WORKBENCH_KEY_CONTEXT, true)
        );
        assert_eq!(
            (modes[1].key_context, modes[1].rem_zoom),
            (onehand_plugin_api::TERMINAL_KEY_CONTEXT, false)
        );
    }
}
