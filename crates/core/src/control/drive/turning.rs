//! A lead's turn, from its `send_prompt` to its answer. A turn checked
//! by `turns::send` goes at once, or, when it names its worker's model,
//! once that model is in force. The model is picked as the composer picks
//! it, and the turn goes only once that model is in force:
//! an agent that offers no such model, or refuses the pick, refuses the
//! turn, and nothing is sent. The pick stays in force for the worker's next
//! turns, since it is the session's own setting.
//!
//! Asked only once `turns::send` let the turn go, so a turn already sent
//! picks nothing; and checked again as it goes, since the person may have
//! taken the worker, or a turn begun, while the model was picked.

use super::super::open::{self, Step};
use super::super::turns;
use super::{ControlDrive, Effect, Sending, OPEN_DEADLINE};
use crate::chat::Chat;

/// A turn waiting on its model's pick.
pub(super) struct Turning {
    turn: u64,
    text: String,
    model: String,
    /// The deadline's wait.
    token: u64,
    /// The groups already picked.
    picked: Vec<String>,
}

impl ControlDrive {
    /// The lead's `send_prompt` of turn `turn`, `text`, to worker `uid`,
    /// which the lead names `session`, on `model` when one is named, the
    /// worker's chat being `chat` (`None` while it has none yet, still
    /// connecting).
    #[must_use = "its effects are to be carried"]
    pub fn send_prompt(
        &mut self,
        uid: u64,
        session: &str,
        turn: u64,
        text: &str,
        model: Option<String>,
        chat: Option<&Chat>,
    ) -> (Sending, Vec<Effect>) {
        let said = |said| (Sending::Said(said), Vec::new());
        // A turn still waiting on its model goes first, or not at all.
        if let Some(waiting) = self.turn_waiting(uid, turn, text) {
            return said(waiting);
        }
        let Some(last) = self.last(uid) else {
            return said(Err(format!("{session} is closed")));
        };
        // A chat still connecting refuses the turn in the composer's words.
        let connecting = Chat::default();
        let send = match turns::send(chat.unwrap_or(&connecting), &last, turn, text) {
            Ok(send) => send,
            Err(why) => return said(Err(why)),
        };
        if chat.is_none() {
            return said(Err(format!("{session} is still connecting")));
        }
        match (send, model) {
            (turns::Send::AlreadySent(turn), _) => said(Ok(format!("turn {turn} already sent"))),
            (turns::Send::Go, None) => (
                Sending::Later,
                vec![Effect::Send {
                    uid,
                    turn,
                    text: text.to_string(),
                }],
            ),
            (turns::Send::Go, Some(model)) => (
                Sending::Later,
                self.turn_with_model(uid, turn, text.to_string(), model),
            ),
        }
    }

    /// Give worker `uid` turn `turn`, `text`, once `model` is in force.
    /// A turn waiting already for that worker is replaced.
    pub(super) fn turn_with_model(
        &mut self,
        uid: u64,
        turn: u64,
        text: String,
        model: String,
    ) -> Vec<Effect> {
        self.minted += 1;
        let token = self.minted;
        let turning = Turning {
            turn,
            text,
            model,
            token,
            picked: Vec::new(),
        };
        self.turning.insert(uid, turning);
        vec![
            Effect::Watch(uid),
            Effect::Arm {
                token,
                wait: OPEN_DEADLINE,
            },
        ]
    }

    /// Worker `uid`'s session changed, or is gone: what its turn waiting on
    /// a model does next, or `None` when no turn waits.
    pub(super) fn turn_woke(&mut self, uid: u64, chat: Option<&Chat>) -> Option<Vec<Effect>> {
        let turning = self.turning.get_mut(&uid)?;
        let Some(chat) = chat else {
            return Some(self.turn_over(uid, gone()));
        };
        Some(
            match open::step(chat, None, Some(&turning.model), &turning.picked) {
                Step::Wait => Vec::new(),
                Step::Pick(pick) => {
                    turning.picked.push(pick.group().to_string());
                    vec![Effect::Pick { uid, pick }]
                }
                Step::Refused(why) => self.turn_over(uid, Err(why)),
                Step::Ready => {
                    let turning = self.turning.remove(&uid)?;
                    let mut fx = vec![Effect::Unwatch(uid)];
                    fx.push(self.go(uid, chat, turning.turn, turning.text));
                    fx
                }
            },
        )
    }

    /// The wait armed with `token` ran out: the turn it timed is refused,
    /// or `None` when it timed none.
    pub(super) fn turn_elapsed(&mut self, token: u64) -> Option<Vec<Effect>> {
        let uid = self
            .turning
            .iter()
            .find(|(_, turning)| turning.token == token)
            .map(|(uid, _)| *uid)?;
        let why = "the agent did not answer the model pick in time".to_string();
        Some(self.turn_over(uid, Err(why)))
    }

    /// What a `send_prompt` of turn `turn`, `text`, to worker `uid` comes
    /// to while a turn of its waits on its model: the same turn again is
    /// told so and sends nothing, any other is refused. `None` when none
    /// waits.
    pub(super) fn turn_waiting(
        &self,
        uid: u64,
        turn: u64,
        text: &str,
    ) -> Option<Result<String, String>> {
        let waiting = self.turning.get(&uid)?;
        let said = format!("turn {} is waiting on its model pick", waiting.turn);
        Some(match waiting.turn == turn && waiting.text == text {
            true => Ok(said),
            false => Err(said),
        })
    }

    /// Every turn waiting on a model for `lead`'s workers is refused with
    /// `why`: its lead went, or its tools did.
    pub(super) fn turns_of_lead_ended(&mut self, lead: u64, why: &str) -> Vec<Effect> {
        let uids: Vec<u64> = self
            .turning
            .keys()
            .copied()
            .filter(|uid| {
                self.workers
                    .get(uid)
                    .is_some_and(|relation| relation.lead == lead)
            })
            .collect();
        uids.into_iter()
            .flat_map(|uid| self.turn_over(uid, Err(why.to_string())))
            .collect()
    }

    /// Worker `uid` closed: a turn of its waiting on a model is refused.
    pub(super) fn turn_forgotten(&mut self, uid: u64) -> Vec<Effect> {
        match self.turning.contains_key(&uid) {
            true => self.turn_over(uid, gone()),
            false => Vec::new(),
        }
    }

    /// Turn `turn`, `text`, its model now in force, checked again against
    /// worker `uid`'s `chat` as the call was: sent, or answered.
    fn go(&self, uid: u64, chat: &Chat, turn: u64, text: String) -> Effect {
        let answer = |outcome| Effect::TurnAnswer { uid, outcome };
        let Some(last) = self.last(uid) else {
            return answer(gone());
        };
        match turns::send(chat, &last, turn, &text) {
            Ok(turns::Send::Go) => Effect::Send { uid, turn, text },
            Ok(turns::Send::AlreadySent(turn)) => answer(Ok(format!("turn {turn} already sent"))),
            Err(why) => answer(Err(why)),
        }
    }

    fn turn_over(&mut self, uid: u64, outcome: Result<String, String>) -> Vec<Effect> {
        self.turning.remove(&uid);
        vec![Effect::Unwatch(uid), Effect::TurnAnswer { uid, outcome }]
    }
}

pub(super) fn gone() -> Result<String, String> {
    Err("the session was closed before the turn went".to_string())
}
