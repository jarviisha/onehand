//! The question the shell asks before it does something that cannot be taken
//! back.
//!
//! A modal rather than a press that arms itself and waits for a second one. An
//! armed control reads as one that did nothing: the press lands, a toast says
//! something and fades, and a user who has looked away comes back to a row that
//! is one stray click from gone with no warning left on screen. A modal names
//! what is about to go and has to be answered before anything else in the
//! window can be.

use super::Shell;
use gpui::{App, Context, Entity, ParentElement as _, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::{DialogClose, DialogFooter};
use std::rc::Rc;

/// What the modal says, and what its two buttons read.
pub(crate) struct Ask {
    /// Prefix of the two buttons' element ids, unique per question.
    pub(crate) id: &'static str,
    pub(crate) title: SharedString,
    pub(crate) description: SharedString,
    /// The danger-tinted word that goes ahead. The plain one always reads
    /// *Keep*, first.
    pub(crate) act: &'static str,
}

impl Shell {
    /// Ask `ask`, and run `then` on the shell only when it is answered yes.
    pub(super) fn ask(
        &self,
        ask: Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        ask_on(cx.entity(), ask, window, cx, then);
    }
}

/// Ask `ask`, and run `then` on `on` only when it is answered yes.
pub(crate) fn ask_on<T: 'static>(
    on: Entity<T>,
    ask: Ask,
    window: &mut Window,
    cx: &mut App,
    then: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) {
    let then = Rc::new(then);
    let ask = Rc::new(ask);
    window.open_alert_dialog(cx, move |alert, _, _| {
        // Cloned per build: a dialog's builder runs again on every frame it
        // is on screen, so nothing captured here can be consumed by one.
        let (on, then) = (on.clone(), then.clone());
        alert
            .title(ask.title.clone())
            .description(ask.description.clone())
            // Ours rather than the library's default pair: the library
            // draws its own with the arrow cursor.
            .footer(
                DialogFooter::new()
                    .child(
                        DialogClose::new().child(
                            crate::controls::action(SharedString::from(format!("{}-keep", ask.id)))
                                .ghost()
                                .label("Keep"),
                        ),
                    )
                    .child(
                        crate::controls::action(SharedString::from(format!("{}-confirm", ask.id)))
                            .danger()
                            .label(ask.act)
                            .on_click(move |_, window: &mut Window, cx: &mut App| {
                                window.close_dialog(cx);
                                let then = then.clone();
                                on.update(cx, |on: &mut T, cx| then(on, window, cx));
                            }),
                    ),
            )
    });
}
