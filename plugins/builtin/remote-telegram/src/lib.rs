// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use onehand_plugin_api::PluginId;

pub const CHANNEL_ID: PluginId = PluginId::new("remote.telegram");

mod telegram;
pub(crate) use telegram::Telegram;

pub mod secret;

pub fn create_channel(token: String) -> Box<dyn onehand_core::remote::types::RemoteChannel> {
    Box::new(Telegram::new(token))
}
