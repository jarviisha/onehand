//! The session's *Cards* chip, in the strip under its composer: what this
//! session answers of its cards by itself, and the menu that changes it for
//! this session alone. Not on a host's session, whose host answers per folder.

use super::Composer;
use super::attachments::chip;
use crate::chat::session::ChatSession;
use gpui::{App, Context, Entity, IntoElement, ParentElement, SharedString, Styled};
use gpui_component::{ActiveTheme, Icon, Sizable as _};
use onehand_core::chat::Cards;

/// The chip for `session`, or none on a host's.
pub(super) fn cards_chip(
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> Option<gpui::AnyElement> {
    let read = session.read(cx);
    if read.on_host {
        return None;
    }
    let level = read.chat().cards();
    let ink = match level {
        Cards::Manual => cx.theme().muted_foreground,
        Cards::AllowEdits | Cards::Bypass => crate::theme::status_ink(cx).warning,
    };
    let tip = SharedString::from(match level.cost() {
        Some(cost) => format!("Cards: {}. {cost}", level.label()),
        None => format!("Cards: {}. {}", level.label(), level.about()),
    });
    let session = session.clone();
    Some(
        onehand_ui::menu_below(
            "session-cards",
            chip("session-cards-trigger", false, cx)
                .child(
                    Icon::new(crate::icons::Icon::Shield)
                        .small()
                        .text_color(ink),
                )
                .child(super::card::chip_text(level.label()))
                .tooltip(tip),
            move |mut menu, _, _| {
                menu = menu.item(crate::controls::menu_label("Cards, this session only"));
                for choice in Cards::ALL {
                    let session = session.clone();
                    menu = menu.item(
                        crate::controls::menu_item(format!(
                            "{} · {}",
                            choice.label(),
                            choice.about()
                        ))
                        .checked(choice == level)
                        .on_click(move |_, _, cx: &mut App| {
                            session.update(cx, |session, cx| {
                                session.drive(cx, |chat, _| ((), chat.set_cards(choice)));
                            });
                        }),
                    );
                }
                menu
            },
        )
        .into_any_element(),
    )
}
