//! The band over a worker's composer: which lead coordinates it and the way
//! back to that lead, or, once the person has written to it, that they hold
//! it and the way to give it back. It stands where a lead's strip stands, on
//! the same surface, and gives way to it on a worker that leads workers.

use super::over_composer::over_composer;
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

impl BandAct {
    fn label(self) -> &'static str {
        match self {
            BandAct::BackToLead => "Back to lead",
            BandAct::GiveBack => "Give back to the lead",
        }
    }
}

/// Who a worker answers to now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Standing<'a> {
    /// Its lead holds it: named when the lead is a session this window shows,
    /// else unnamed, as this pane knows no other's.
    Coordinated { lead: Option<&'a str> },
    /// The person holds it, `offered` whether giving it back is.
    Held { offered: bool },
}

/// The band's words and its button.
pub(super) fn band(standing: Standing) -> (String, Option<BandAct>) {
    match standing {
        Standing::Held { offered } => (
            "You hold this worker".into(),
            offered.then_some(BandAct::GiveBack),
        ),
        Standing::Coordinated { lead } => (
            format!("{} is coordinating this worker", lead.unwrap_or("Its lead")),
            lead.map(|_| BandAct::BackToLead),
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
            .zip(self.session_of(uid))
            .is_some_and(|(worker, session)| worker.held(&session.read(cx).chat));
        let name = self
            .conversations
            .contains_key(&lead)
            .then(|| self.worker_name(lead, cx));
        let standing = match held {
            true => Standing::Held {
                offered: crate::control::give_back_offered(uid, prompts(self, cx), cx),
            },
            false => Standing::Coordinated {
                lead: name.as_deref(),
            },
        };
        let (words, act) = band(standing);
        let button = act.map(|act| {
            crate::controls::action(("worker-band", uid))
                // Beside a sentence, so never a ghost.
                .secondary()
                .short()
                .label(act.label())
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
