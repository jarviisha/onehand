//! The question asked before something that cannot be taken back.
//!
//! A modal rather than a press that arms itself and waits for a second one. An
//! armed control reads as one that did nothing: the press lands, a toast says
//! something and fades, and a user who has looked away comes back to a row that
//! is one stray click from gone with no warning left on screen. A modal names
//! what is about to go and has to be answered before anything else in the
//! window can be.
//!
//! Here rather than in the app for the reason [`crate::action`] is: a feature
//! crate removes things too, and has to ask the way the window does.

use crate::Quiet as _;
use crate::Short as _;
use crate::{Refuses as _, action, status_ink};
use gpui::Styled as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, Context, Entity, ParentElement as _, SharedString, Window};
use gpui_component::button::ButtonVariants as _;
use gpui_component::dialog::DialogFooter;
use gpui_component::{ActiveTheme as _, StyledExt as _, WindowExt as _};
use std::rc::Rc;

/// What the modal says, and what its two buttons read.
#[derive(Default)]
pub struct Confirm {
    /// Prefix of the two buttons' element ids, unique per question.
    pub id: &'static str,
    pub title: SharedString,
    pub description: SharedString,
    /// The danger-tinted word that goes ahead. The plain one always reads
    /// *Keep*, first.
    pub act: &'static str,
    /// What was found, a line each, under the description: in the danger
    /// ink when it refuses.
    pub lines: Vec<String>,
    /// What was found refuses it: the danger-tinted word is spent.
    pub refused: bool,
    /// A second, lesser way to go ahead, between *Keep* and `act`: asked
    /// through [`confirm_either_on`], which says which was pressed. Enter
    /// then answers `act`.
    pub alt: Option<&'static str>,
}

/// Which way a question with two ways ahead was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The danger-tinted word.
    Act,
    /// The lesser one, [`Confirm::alt`].
    Alt,
}

/// Ask `ask`, and run `then` on `on` only when it is answered yes.
pub fn confirm_on<T: 'static>(
    on: Entity<T>,
    ask: Confirm,
    window: &mut Window,
    cx: &mut App,
    then: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) {
    confirm_either_on(on, ask, window, cx, move |on, _, window, cx| {
        then(on, window, cx)
    });
}

/// Ask `ask`, and run `then` on `on` with the way it was answered, unless
/// it was answered *Keep*.
pub fn confirm_either_on<T: 'static>(
    on: Entity<T>,
    ask: Confirm,
    window: &mut Window,
    cx: &mut App,
    then: impl Fn(&mut T, Answer, &mut Window, &mut Context<T>) + 'static,
) {
    let then = Rc::new(then);
    let ask = Rc::new(ask);
    window.open_alert_dialog(cx, move |alert, _, cx| {
        // Cloned per build: a dialog's builder runs again on every frame it
        // is on screen, so nothing captured here can be consumed by one.
        let (on, then) = (on.clone(), then.clone());
        let alt = ask.alt.map(|word| {
            let (on, then) = (on.clone(), then.clone());
            action(SharedString::from(format!("{}-alt", ask.id)))
                .short()
                .secondary()
                .label(word)
                .on_click(move |_, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    let then = then.clone();
                    on.update(cx, |on: &mut T, cx| then(on, Answer::Alt, window, cx));
                })
        });
        let ink = match ask.refused {
            true => status_ink(cx).danger,
            false => cx.theme().muted_foreground,
        };
        let go = action(SharedString::from(format!("{}-confirm", ask.id)))
            .short()
            .danger()
            .label(ask.act);
        // A question with a lesser way ahead takes Enter as its danger word,
        // the one it is asked for; one with only that word keeps Enter
        // closing it, so a key held too long removes nothing.
        let enter = (ask.alt.is_some() && !ask.refused).then(|| (on.clone(), then.clone()));
        let go = match ask.refused {
            true => go.refuses(Some("What was found above stops it")),
            false => go.on_click(move |_, window: &mut Window, cx: &mut App| {
                window.close_dialog(cx);
                let then = then.clone();
                on.update(cx, |on: &mut T, cx| then(on, Answer::Act, window, cx));
            }),
        };
        alert
            .when_some(enter, |alert, (on, then)| {
                alert.on_ok(move |_, window: &mut Window, cx: &mut App| {
                    let then = then.clone();
                    on.update(cx, |on: &mut T, cx| then(on, Answer::Act, window, cx));
                    true
                })
            })
            .title(ask.title.clone())
            .description(ask.description.clone())
            .when(!ask.lines.is_empty(), |alert| {
                alert.child(
                    gpui::div().v_flex().gap_1().w_full().children(
                        ask.lines
                            .iter()
                            .map(|line| gpui::div().text_xs().text_color(ink).child(line.clone())),
                    ),
                )
            })
            // Ours rather than the library's default pair: the library
            // draws its own with the arrow cursor. *Keep* closes the dialog
            // itself rather than through `DialogClose`, whose box takes the
            // whole footer and so stood it apart from the act beside it.
            .footer(
                DialogFooter::new()
                    .child(
                        action(SharedString::from(format!("{}-keep", ask.id)))
                            .short()
                            .quiet(cx)
                            .label("Keep")
                            .on_click(|_, window: &mut Window, cx: &mut App| {
                                window.close_dialog(cx);
                            }),
                    )
                    .children(alt)
                    .child(go),
            )
    });
}
