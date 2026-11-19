//! A worker's standing, in the pane's header: which lead coordinates it and
//! the way back to that lead, or, once the person has written to it, that
//! they hold it and the way to give it back.

use super::{ChatPane, ChatPaneEvent};
use crate::controls::Short as _;
use gpui::{App, Context, IntoElement, ParentElement, Styled, div};
use gpui_component::StyledExt as _;
use gpui_component::button::ButtonVariants as _;

/// What the standing's one button does.
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

/// The standing's words and its button.
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
    /// The worker's standing in the header, while the session on screen is
    /// a worker: its words in the header's ink, giving way before the name
    /// does, then its button.
    pub(super) fn worker_standing(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let uid = self.active?;
        let lead = crate::control::lead_of(uid)?;
        let prompts = move |pane: &Self, cx: &App| {
            pane.session_of(uid)
                .map_or(0, |session| session.read(cx).chat().prompts_sent)
        };
        // Still opening, the lead has sent nothing yet the person could hold.
        let held = crate::control::workers_of(lead)
            .into_iter()
            .find(|worker| worker.uid == uid)
            .zip(self.session_of(uid))
            .is_some_and(|(worker, session)| worker.held(session.read(cx).chat()));
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
            crate::controls::action(("worker-standing", uid))
                // Beside a sentence, so never a ghost.
                .secondary()
                .short()
                .label(act.label())
                .on_click(cx.listener(move |pane: &mut Self, _, _, cx| match act {
                    BandAct::BackToLead => cx.emit(ChatPaneEvent::ShowSession(lead)),
                    BandAct::GiveBack => {
                        // Read at the press: a prompt may have gone since the
                        // header was drawn.
                        crate::control::give_back(uid, prompts(pane, cx), cx);
                        cx.notify();
                    }
                }))
        });
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(div().min_w_0().truncate().child(words))
                .children(button.map(|button| button.flex_none()))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;
