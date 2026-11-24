//! What Settings asks of the bridge: switched on or off, the chats it allows,
//! a token saved or removed, a stopped bridge tried again. Each is the
//! bridge's drive's, carried out like everything else it answers.

use super::{BridgeState, Effect, Ignored, RemoteDrive};
use crate::config::drive::Value;
use crate::remote::types::ChatId;

/// The token's write, while it runs or after it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenWrite {
    Writing,
    /// In the file system's words.
    Failed(String),
}

impl RemoteDrive {
    /// Put `value`, shown by the settings' drive, in force on the bridge.
    pub fn shown(&mut self, value: &Value) -> Vec<Effect> {
        match value {
            Value::TelegramOn(on) => self.switch(*on),
            Value::TelegramChats(chats) => {
                self.set_allowed(chats.clone());
                Vec::new()
            }
            Value::Appearance(_) | Value::Outside(_) | Value::Agents | Value::Hosts => Vec::new(),
        }
    }

    /// Whether the bridge is switched on.
    pub fn on(&self) -> bool {
        self.on
    }

    /// The chats allowed to reach the app.
    pub fn allowed(&self) -> &[ChatId] {
        self.chats.allowed()
    }

    /// The allowed chats with `row` let in: its chat, and in a group its
    /// sender too, since a group admits only members listed themselves.
    pub fn allowing(&self, row: &Ignored) -> Vec<ChatId> {
        let mut out = self.allowed().to_vec();
        for id in [&row.chat, &row.from] {
            let id = id.trim();
            if !id.is_empty() && !out.iter().any(|seen| seen == id) {
                out.push(id.to_string());
            }
        }
        out
    }

    /// Start a stopped bridge again, as it was set up.
    pub fn retry(&mut self) -> Vec<Effect> {
        self.start()
    }

    /// Save `token` as the bridge's own; it starts again once written.
    pub fn save_token(&mut self, token: String) -> Vec<Effect> {
        self.write_token(Some(token))
    }

    /// Remove the bridge's own token; it starts again, and finds none.
    pub fn remove_token(&mut self) -> Vec<Effect> {
        self.write_token(None)
    }

    /// The token's write, while it runs or once it failed.
    pub fn token_write(&self) -> Option<&TokenWrite> {
        self.token_write.as_ref()
    }

    /// One write at a time: a second press while one runs is the same press.
    fn write_token(&mut self, token: Option<String>) -> Vec<Effect> {
        if self.token_write == Some(TokenWrite::Writing) {
            return Vec::new();
        }
        self.token_write = Some(TokenWrite::Writing);
        vec![Effect::WriteToken(token)]
    }

    /// What the token's write came to. Written, the bridge starts again: a
    /// new token may be another bot.
    pub(super) fn token_written(&mut self, result: Result<(), String>) -> Vec<Effect> {
        match result {
            Ok(()) => {
                self.token_write = None;
                self.start()
            }
            Err(why) => {
                self.token_write = Some(TokenWrite::Failed(why));
                Vec::new()
            }
        }
    }

    /// Switch the bridge on, which starts it, or off, which lets its channel
    /// go. Away goes with it: the only way back, `/here`, would arrive over
    /// the channel being let go.
    fn switch(&mut self, on: bool) -> Vec<Effect> {
        self.on = on;
        if on {
            return self.start();
        }
        let live = self.live();
        self.looking = false;
        self.away = false;
        self.state = BridgeState::Off;
        self.ignored.clear();
        if live {
            vec![Effect::ShutDown]
        } else {
            Vec::new()
        }
    }

    /// Allow exactly `allowed`. A chat taken off forgets what it pointed at
    /// and followed; a refused chat now allowed leaves the ignored list.
    fn set_allowed(&mut self, allowed: Vec<ChatId>) {
        self.chats.set_allowed(allowed);
        let chats = &self.chats;
        self.ignored
            .retain(|row| !chats.allows(&row.chat, &row.from));
    }
}
