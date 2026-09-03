//! Composition root for every plugin compiled into the Onehand binary.

use onehand_plugin_host::PluginRegistry;

pub fn builtins() -> Result<PluginRegistry, onehand_plugin_host::RegistryError> {
    let mut registry = PluginRegistry::new();
    // This is the user-visible Workbench order. It is intentionally explicit,
    // rather than inherited from filesystem or linker order.
    registry.register(&onehand_workbench_editor::EditorPlugin)?;
    registry.register(&onehand_workbench_files::FilesPlugin)?;
    registry.register(&onehand_workbench_neovim::NeovimPlugin)?;
    registry.register(&onehand_remote_telegram::TelegramPlugin)?;
    registry.set_workbench_factory(
        onehand_workbench_editor::MODE_ID,
        onehand_workbench_editor::create_view,
    )?;
    registry.set_workbench_factory(
        onehand_workbench_files::MODE_ID,
        onehand_workbench_files::create_view,
    )?;
    registry.set_workbench_factory(
        onehand_workbench_neovim::MODE_ID,
        onehand_workbench_neovim::create_view,
    )?;
    registry.set_remote_factory(
        onehand_remote_telegram::CHANNEL_ID,
        onehand_remote_telegram::create_channel,
    )?;
    registry.seal()?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    #[test]
    fn builtins_have_explicit_workbench_order_and_telegram_factory() {
        let registry = super::builtins().unwrap();
        assert_eq!(
            registry
                .workbench_modes()
                .iter()
                .map(|mode| mode.label)
                .collect::<Vec<_>>(),
            ["Editor", "Files", "Neovim"]
        );
        assert_eq!(registry.remote_channels().len(), 1);
        assert_eq!(
            registry.remote_channels()[0].id,
            onehand_remote_telegram::CHANNEL_ID
        );
        assert!(registry.remote_channels()[0].factory.is_some());
    }
}
