//! The band over a worker's composer: which lead coordinates it and the way
//! back to that lead, or, once the person has written to it, that they hold
//! it and the way to give it back. It stands where a lead's strip stands, on
//! the same surface; a worker leads none, so the two never meet.

use super::lead_strip::over_composer;
use super::{ChatPane, ChatPaneEvent};
use crate::controls::Short as _;
use gpui::{App, Context, IntoElement, ParentElement, Styled, div};
use gpui_component::ActiveTheme as _;
use gpui_component::button::ButtonVariants as _;

/// What the band's one button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BandAct {
    BackToLead,
    GiveBack,
}

/// The band's words and its button: `lead` the lead's name when this pane
/// knows it, `held` whether the person holds the worker, `offered` whether
/// giving it back is, and `lead_here` whether the lead is a session this
/// window can show.
pub(super) fn band(
    lead: Option<&str>,
    held: bool,
    offered: bool,
    lead_here: bool,
) -> (String, Option<BandAct>) {
    match held {
        true => (
            "You hold this worker".into(),
            offered.then_some(BandAct::GiveBack),
        ),
        false => (
            format!("{} is coordinating this worker", lead.unwrap_or("Its lead")),
            lead_here.then_some(BandAct::BackToLead),
        ),
    }
}

impl ChatPane {
    /// The band over the composer, while the session on screen is a worker.
    pub(super) fn worker_band(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let uid = self.active?;
        let lead = crate::control::lead_of(uid)?;
        let prompts = move |pane: &Self, cx: &App| {
            pane.session_of(uid)
                .map_or(0, |session| session.read(cx).chat.prompts_sent)
        };
        // Still opening, the lead has sent nothing yet the person could hold.
        let held = crate::control::workers_of(lead)
            .into_iter()
            .find(|worker| worker.uid == uid)
            .and_then(|worker| worker.sent)
            .zip(self.session_of(uid))
            .is_some_and(|(sent, session)| session.read(cx).chat.prompted_since(sent));
        let offered = crate::control::give_back_offered(uid, prompts(self, cx), cx);
        let lead_here = self.conversations.contains_key(&lead);
        let name = lead_here.then(|| self.worker_name(lead, cx));
        let (words, act) = band(name.as_deref(), held, offered, lead_here);
        let button = act.map(|act| {
            let label = match act {
                BandAct::BackToLead => "Back to lead",
                BandAct::GiveBack => "Give back to the lead",
            };
            crate::controls::action(("worker-band", uid))
                // Beside a sentence, so never a ghost.
                .secondary()
                .short()
                .label(label)
                .on_click(cx.listener(move |pane: &mut Self, _, _, cx| match act {
                    BandAct::BackToLead => cx.emit(ChatPaneEvent::ShowSession(lead)),
                    BandAct::GiveBack => {
                        // Read at the press: a prompt may have gone since the
                        // band was drawn.
                        crate::control::give_back(uid, prompts(pane, cx), cx);
                        cx.notify();
                    }
                }))
        });
        let line = over_composer("worker-band", cx)
            .text_color(cx.theme().muted_foreground)
            .child(div().flex_1().min_w_0().truncate().child(words))
            .children(button);
        Some(div().w_full().px_3().child(line).into_any_element())
    }
}

#[cfg(test)]
mod tests;
