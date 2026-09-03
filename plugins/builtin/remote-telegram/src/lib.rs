use onehand_plugin_api::{
    BuiltinPlugin, Capability, PLUGIN_API_VERSION, PluginDescriptor, PluginId, PluginRegistrar,
};

pub const CHANNEL_ID: PluginId = PluginId::new("remote.telegram");

pub struct TelegramPlugin;

impl BuiltinPlugin for TelegramPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: PluginId::new("builtin.remote-telegram"),
            name: "Remote Telegram",
            version: env!("CARGO_PKG_VERSION"),
            api_version: PLUGIN_API_VERSION,
            capabilities: &[Capability::RemoteChannel],
        }
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), String> {
        registrar.register_remote_channel(CHANNEL_ID, "Telegram")
    }
}

mod telegram;
pub use telegram::Telegram;

pub mod secret;

pub fn create_channel(token: String) -> Box<dyn onehand_core::remote::types::RemoteChannel> {
    Box::new(Telegram::new(token))
}
