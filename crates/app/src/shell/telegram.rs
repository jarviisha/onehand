//! What the Telegram group's controls ask: each goes to the settings' drive
//! or the bridge's, which say what happens next.

use super::Shell;
use super::confirm::Confirm;
use crate::settings::TelegramDraft;
use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Context, Window};
use onehand_core::remote::Ignored;

impl Shell {
    pub(crate) fn telegram_draft(&self) -> &TelegramDraft {
        &self.telegram_draft
    }

    /// Switch the bridge on or off, at once, then write it.
    pub(crate) fn set_telegram_on(
        &mut self,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = Shared::global(cx).remote.drive.on();
        let fx =
            cx.update_global::<Shared, _>(|shared, _| shared.settings.set_telegram_on(before, on));
        crate::saves::carry_settings(fx, window, cx);
    }

    /// Allow the chat typed in the field; the field empties.
    pub(crate) fn add_telegram_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.telegram_draft.chat.read(cx).value().trim().to_string();
        let before = allowed(cx);
        if onehand_core::config::chat_refusal(&typed, &before).is_some() {
            return;
        }
        let mut after = before.clone();
        after.push(typed);
        set_chats(before, after, window, cx);
        self.telegram_draft
            .chat
            .update(cx, |field, cx| field.set_value("", window, cx));
    }

    /// Stop allowing `id`. Asks nothing: adding it back undoes it.
    pub(crate) fn remove_telegram_chat(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = allowed(cx);
        let after = before.iter().filter(|seen| **seen != id).cloned().collect();
        set_chats(before, after, window, cx);
    }

    /// Allow the chat `row` was refused from, and in a group its sender.
    pub(crate) fn allow_telegram(
        &mut self,
        row: &Ignored,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = allowed(cx);
        let after = Shared::global(cx).remote.drive.allowing(row);
        set_chats(before, after, window, cx);
    }

    /// Save the token typed in the field; the field empties.
    pub(crate) fn save_telegram_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self
            .telegram_draft
            .token
            .read(cx)
            .value()
            .trim()
            .to_string();
        if token.is_empty() {
            return;
        }
        self.telegram_draft
            .token
            .update(cx, |field, cx| field.set_value("", window, cx));
        self.telegram_draft.replacing = false;
        crate::remote::save_token(token, cx);
        cx.notify();
    }

    /// Show the field in place of the saved token.
    pub(crate) fn replace_telegram_token(&mut self, cx: &mut Context<Self>) {
        self.telegram_draft.replacing = true;
        cx.notify();
    }

    /// Ask, then remove the saved token.
    pub(crate) fn confirm_remove_telegram_token(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = Confirm {
            id: "remove-telegram-token",
            title: "Remove the bot token?".into(),
            description: "The bridge stops until another is saved.".into(),
            act: "Remove",
            ..Default::default()
        };
        self.confirm(ask, window, cx, |_, _, cx| crate::remote::remove_token(cx));
    }
}

fn allowed(cx: &App) -> Vec<String> {
    Shared::global(cx).remote.drive.allowed().to_vec()
}

fn set_chats(before: Vec<String>, after: Vec<String>, window: &mut Window, cx: &mut App) {
    let fx = cx
        .update_global::<Shared, _>(|shared, _| shared.settings.set_telegram_chats(before, after));
    crate::saves::carry_settings(fx, window, cx);
}
