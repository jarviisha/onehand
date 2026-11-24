//! The Telegram group on the Connections page: the switch, what the bridge
//! is doing, where its token comes from, the chats it allows and the ones it
//! ignored. What each means is the bridge's drive's; this draws it.

use super::{list_row, list_rows, section};
use crate::controls::text_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement,
    SharedString, Styled, Subscription, Window, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{ActiveTheme, StyledExt};
use onehand_core::remote::{BridgeState, TokenSource, TokenWrite};
use onehand_ui::{Refuses as _, Short as _, Tall as _};

/// The fields the group is typed into.
pub struct TelegramDraft {
    /// The bot token, masked as it is typed and emptied once saved.
    pub token: Entity<InputState>,
    /// A chat id to allow.
    pub chat: Entity<InputState>,
    /// *Replace…* was pressed: the field shows in place of the saved token.
    pub replacing: bool,
    _typing: [Subscription; 2],
}

impl TelegramDraft {
    pub fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let token = cx.new(|cx| InputState::new(window, cx).masked(true));
        let chat = cx.new(|cx| InputState::new(window, cx));
        // *Save* and *Add* refuse by what their fields hold, so each key
        // draws the page again.
        let redraw = |_: &mut Shell,
                      _: &Entity<InputState>,
                      event: &InputEvent,
                      _: &mut Window,
                      cx: &mut Context<Shell>| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        };
        let typing = [
            cx.subscribe_in(&token, window, redraw),
            cx.subscribe_in(&chat, window, redraw),
        ];
        Self {
            token,
            chat,
            replacing: false,
            _typing: typing,
        }
    }
}

/// The Telegram group.
pub(super) fn telegram_section(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let drive = &crate::state::Shared::global(cx).remote.drive;
    let on = drive.on();
    let switch = handle.clone();
    section(
        Some("Telegram"),
        Some(SharedString::from(
            "A bot that tells you when an agent needs you, and takes your answer back",
        )),
        cx,
    )
    .gap_3()
    // Held to its own width, as the outside control's switch is.
    .child(
        div().h_flex().child(
            div().flex_none().cursor_pointer().child(
                onehand_ui::toggle("telegram-on", on)
                    .label("Let a Telegram bot reach onehand")
                    .on_click(move |on: &bool, window: &mut Window, cx: &mut App| {
                        switch.update(cx, |shell, cx| shell.set_telegram_on(*on, window, cx));
                    }),
            ),
        ),
    )
    .child(state_line(drive.state(), cx))
    .child(token_row(handle, cx))
    .child(chats(handle, cx))
    .children(ignored(handle, cx))
    .into_any_element()
}

/// What the bridge is doing, in the muted ink at rest and the warning ink
/// when it needs the person; *Try again* once it stopped.
fn state_line(state: &BridgeState, cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let warning = crate::theme::status_ink(cx).warning;
    let (line, ink, stopped) = match state {
        BridgeState::Off => ("Off".to_string(), muted, false),
        BridgeState::NoToken => ("No bot token".to_string(), warning, false),
        BridgeState::Connecting { retrying: None } => ("Connecting…".to_string(), muted, false),
        BridgeState::Connecting {
            retrying: Some(why),
        } => (format!("Connecting… {why}"), warning, false),
        BridgeState::Connected { name } => (format!("Connected as {name}"), muted, false),
        BridgeState::Stopped { why } => (format!("Stopped: {why}"), warning, true),
    };
    div()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(crate::tokens::CHROME_TEXT)
                .text_color(ink)
                .child(line),
        )
        .when(stopped, |row| {
            row.child(
                crate::controls::action("telegram-retry")
                    .secondary()
                    .short()
                    .label("Try again")
                    .on_click(|_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        crate::remote::retry(cx)
                    }),
            )
        })
        .into_any_element()
}

/// Where the token comes from, or a field for one.
fn token_row(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let drive = &crate::state::Shared::global(cx).remote.drive;
    let draft = handle.read(cx).telegram_draft();
    let muted = cx.theme().muted_foreground;
    let ink = crate::theme::status_ink(cx);
    let writing = drive.token_write() == Some(&TokenWrite::Writing);
    let failed = match drive.token_write() {
        Some(TokenWrite::Failed(why)) => Some(format!("The token was not saved: {why}")),
        Some(TokenWrite::Writing) | None => None,
    };
    let mono = cx.theme().mono_font_family.clone();
    let body = match drive.source() {
        Some(TokenSource::Env(name)) => div()
            .text_size(crate::tokens::CHROME_TEXT)
            .text_color(muted)
            .child(format!("Read from ${name}"))
            .into_any_element(),
        Some(TokenSource::File(path)) if !draft.replacing => {
            let (replace, remove) = (handle.clone(), handle.clone());
            div()
                .h_flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .h_flex()
                        .flex_wrap()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .child("Saved in")
                        .child(div().font_family(mono).child(path.display().to_string())),
                )
                .child(
                    crate::controls::action("telegram-token-replace")
                        .secondary()
                        .short()
                        .label("Replace…")
                        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            replace.update(cx, |shell, cx| shell.replace_telegram_token(cx));
                        }),
                )
                .child(
                    crate::controls::action("telegram-token-remove")
                        .secondary()
                        .short()
                        .text_color(ink.danger)
                        .label("Remove…")
                        .loading(writing)
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            remove.update(cx, |shell, cx| {
                                shell.confirm_remove_telegram_token(window, cx)
                            });
                        }),
                )
                .into_any_element()
        }
        // Not looked for yet, missing, or being replaced: saving one is how
        // the bridge is set up, switched on or not.
        Some(TokenSource::File(_) | TokenSource::Missing) | None => {
            let empty = draft.token.read(cx).value().trim().is_empty();
            let save = handle.clone();
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(text_input(&draft.token, cx).tall()),
                )
                .child(
                    crate::controls::action("telegram-token-save")
                        .tall()
                        .secondary()
                        .label("Save")
                        .loading(writing)
                        .refuses(empty.then_some("Paste the bot token first"))
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            save.update(cx, |shell, cx| shell.save_telegram_token(window, cx));
                        }),
                )
                .into_any_element()
        }
    };
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .text_size(crate::tokens::CHROME_TEXT)
                .font_semibold()
                .child("Token"),
        )
        .child(body)
        .children(failed.map(|why| div().text_xs().text_color(ink.danger).child(why)))
        .into_any_element()
}

/// The chats allowed, and a field to allow another.
fn chats(handle: &Entity<Shell>, cx: &App) -> AnyElement {
    let drive = &crate::state::Shared::global(cx).remote.drive;
    let allowed = drive.allowed().to_vec();
    let draft = handle.read(cx).telegram_draft();
    let muted = cx.theme().muted_foreground;
    let danger = crate::theme::status_ink(cx).danger;
    let more = allowed.len().saturating_sub(super::pages::LIST_SHOWN);
    let rows = allowed
        .iter()
        .take(super::pages::LIST_SHOWN)
        .enumerate()
        .map(|(idx, id)| {
            let (remove, id) = (handle.clone(), id.clone());
            // Telegram numbers groups below zero, and a group admits only the
            // members listed too: saying which is which explains the list.
            let title = match id.starts_with('-') {
                true => format!("{id} (group)"),
                false => id.clone(),
            };
            list_row(
                title,
                None,
                crate::controls::action(("telegram-chat-remove", idx))
                    .secondary()
                    .short()
                    .text_color(danger)
                    .label("Remove")
                    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let id = id.clone();
                        remove.update(cx, |shell, cx| shell.remove_telegram_chat(id, window, cx));
                    }),
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();
    let refusal = onehand_core::config::chat_refusal(&draft.chat.read(cx).value(), &allowed);
    let add = handle.clone();
    div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .text_size(crate::tokens::CHROME_TEXT)
                .font_semibold()
                .child("Allowed chats"),
        )
        .when(rows.is_empty(), |list| {
            list.child(
                div()
                    .text_size(crate::tokens::CHROME_TEXT)
                    .text_color(muted)
                    .child("No chat is allowed yet; the bot answers nobody"),
            )
        })
        .when(!rows.is_empty(), |list| list.child(list_rows(rows, cx)))
        .when(more > 0, |list| {
            list.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{more} more not shown")),
            )
        })
        .child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(text_input(&draft.chat, cx).tall()),
                )
                .child(
                    crate::controls::action("telegram-chat-add")
                        .tall()
                        .secondary()
                        .label("Add")
                        .refuses(refusal)
                        .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            add.update(cx, |shell, cx| shell.add_telegram_chat(window, cx));
                        }),
                ),
        )
        .into_any_element()
}

/// The chats that wrote and got no answer, each with *Allow*; nothing when
/// none did.
fn ignored(handle: &Entity<Shell>, cx: &App) -> Option<AnyElement> {
    let rows = crate::state::Shared::global(cx).remote.drive.ignored();
    if rows.is_empty() {
        return None;
    }
    let rows = rows
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let (allow, row) = (handle.clone(), row.clone());
            let title = match row.who.is_empty() {
                true => row.chat.clone(),
                false => format!("{} · {}", row.who, row.chat),
            };
            list_row(
                title,
                None,
                crate::controls::action(("telegram-allow", idx))
                    .secondary()
                    .short()
                    .label("Allow")
                    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        let row = row.clone();
                        allow.update(cx, |shell, cx| shell.allow_telegram(&row, window, cx));
                    }),
                cx,
            )
            .into_any_element()
        })
        .collect::<Vec<_>>();
    Some(
        div()
            .v_flex()
            .gap_2()
            .child(
                div()
                    .text_size(crate::tokens::CHROME_TEXT)
                    .font_semibold()
                    .child("Ignored: messaged the bot and got no answer"),
            )
            .child(list_rows(rows, cx))
            .into_any_element(),
    )
}
