//! What a run does for each action its engine asks for: the step said in
//! the transcript, the prompt sent once the agent is up, the command run,
//! the forge called, the wait begun, the run finished.

use super::{settings, Asked, Effect, Lane, RunDrive};
use crate::chat::{Chat, Link};
use crate::preflight::{found_late, mode_refused, About};
use crate::workflow::{Action, Outcome, Pick, StepKind, Stop};
use crate::Now;

/// The longest answer kept from a turn. An answer the length of a book is cut
/// rather than carried whole into the run's file and the next prompt.
const ANSWER_MAX: usize = 20_000;

impl RunDrive {
    /// Do what the engine said, now.
    pub(super) fn carry_out(
        &mut self,
        action: Action,
        chat: Option<&Chat>,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        let Some(chat) = chat else {
            return self.carry_out_without_session(action, now, fx);
        };
        if !ends_run(&action) {
            self.announce_step();
        }
        // The clock counts the agent's work on a step and nothing else: it
        // stands still for whatever onehand does next, and goes on when a
        // prompt is handed over.
        if !matches!(action, Action::Prompt(_)) {
            self.budget.pause(now.at);
        }
        let dir = self.run.setup.dir.clone();
        match action {
            Action::Measure => fx.push(Effect::Measure { dir }),
            Action::Prompt(text) => {
                // A step in another lane is prompted once its session is.
                if let Some(text) = self.enter_lane(text, fx) {
                    self.send(text, chat, now, fx);
                }
            }
            Action::RunCommand(command) => {
                self.command = true;
                self.asked.push(Asked::Note(format!("Running: {command}")));
                fx.push(Effect::RunCommand { dir, command });
            }
            Action::AwaitApproval => {
                self.asked.push(Asked::Note(
                    "Waiting for approval: Continue or Revise… under the header".to_string(),
                ));
            }
            Action::AwaitRoom => self.await_room(true, now, fx),
            Action::Push(commit) => {
                self.asked.push(Asked::Note(format!(
                    "Pushing {}, which the check passed on",
                    short(&commit)
                )));
                self.push(commit, fx);
            }
            Action::OpenPullRequest => {
                self.asked
                    .push(Asked::Note("Opening the pull request".to_string()));
                self.open_pull_request(fx);
            }
            Action::AwaitStatusChecks { wait, pushed } => {
                self.asked.push(Asked::Note(
                    "Waiting for the pull request's status checks".to_string(),
                ));
                self.watch_checks(pushed, wait, now, fx);
            }
            Action::Finish(outcome) => return self.finish(Some(chat), outcome, now, fx),
            Action::Idle => return,
        }
        self.run.spent_secs = self.spent_secs(now);
        fx.push(Effect::Save);
        fx.push(Effect::Refresh);
    }

    /// Do what the engine said with no session to do it in: a run that never
    /// had one runs its command and measures its work; one whose session went
    /// is only ended.
    fn carry_out_without_session(&mut self, action: Action, now: Now, fx: &mut Vec<Effect>) {
        let dir = self.run.setup.dir.clone();
        match action {
            Action::Finish(outcome) => return self.finish(None, outcome, now, fx),
            Action::RunCommand(command) if self.sessionless => {
                self.command = true;
                fx.push(Effect::RunCommand { dir, command });
            }
            Action::Measure if self.sessionless => fx.push(Effect::Measure { dir }),
            Action::AwaitRoom => self.await_room(false, now, fx),
            Action::Measure
            | Action::Prompt(_)
            | Action::RunCommand(_)
            | Action::AwaitApproval
            | Action::Push(_)
            | Action::OpenPullRequest
            | Action::AwaitStatusChecks { .. }
            | Action::Idle => return,
        }
        self.run.spent_secs = self.spent_secs(now);
        fx.push(Effect::Save);
        fx.push(Effect::Refresh);
    }

    /// Tell the transcript the step the run is now in, once per step entered.
    /// A prompt's run has no steps to tell apart, so it says none.
    /// A step whose prompt goes in another lane is said there, once its
    /// session is open, not in the lane the run is leaving.
    fn announce_step(&mut self) {
        let fresh = self.noted != Some(self.run.step);
        self.noted = Some(self.run.step);
        let elsewhere = matches!(
            self.run.current().map(|step| &step.kind),
            Some(StepKind::Agent { .. } | StepKind::Review { .. })
        ) && Lane::at(&self.run) != self.lane;
        if fresh && !elsewhere {
            self.asked.extend(self.step_said().map(Asked::Note));
        }
    }

    /// The step the run is in, as the transcript says it; nothing for a
    /// prompt's run, which has no steps to tell apart.
    pub(super) fn step_said(&self) -> Option<String> {
        let step = self.run.current()?;
        (!self.run.template.is_prompt()).then(|| {
            format!(
                "Step {} of {}: {}",
                self.run.step + 1,
                self.run.template.steps.len(),
                step.label
            )
        })
    }

    /// Send `text`, or keep it until the agent is up.
    pub(super) fn send(&mut self, text: String, chat: &Chat, now: Now, fx: &mut Vec<Effect>) {
        if chat.link != Link::Connected {
            self.here.pending = Some(text);
            return;
        }
        if chat.prompted_beyond(self.here.sent) {
            return self.end(Stop::TakenOver, Some(chat), now, fx);
        }
        if !self.came_up(chat, now, fx) {
            return;
        }
        let Some(text) = settings::settle(self, text, chat, now, fx) else {
            return;
        };
        self.here.sent += 1;
        self.answer_from = chat.items.len();
        self.sent_at = Some(now.at);
        self.budget.resume(now.at);
        self.asked.push(Asked::Prompt { text, at: fx.len() });
    }

    /// The agent is up, the first time in this session: in the run's own
    /// lane, put it in the run's mode before its first prompt.
    /// Whether the prompt may go: an agent that does not offer the mode fails
    /// the run, since one left in the mode that asks before every edit would
    /// park at the first.
    fn came_up(&mut self, chat: &Chat, now: Now, fx: &mut Vec<Effect>) -> bool {
        if std::mem::replace(&mut self.here.up, true) {
            return true;
        }
        // A step's lane runs in the step's own mode, which its settings put
        // in force, else in its agent's.
        if self.lane != Lane::Run {
            self.here.baseline = Some(chat.in_force());
            return true;
        }
        let Some(mode) = self.run.setup.mode.clone() else {
            self.start_settings(chat, None);
            return true;
        };
        let offered: Vec<String> = chat.modes.iter().map(|m| m.id.clone()).collect();
        if let Some(why) = mode_refused(&mode, &offered) {
            self.advance(Some(chat), now, fx, move |run| {
                run.failed(why, found_late(About::Mode), now.secs)
            });
            return false;
        }
        self.picks.push(Pick::Mode(mode.clone()));
        self.here.mode_out = Some(mode.clone());
        self.asked.push(Asked::Note(format!("Mode {mode} set")));
        self.start_settings(chat, Some(mode));
        true
    }

    /// Keep what the agent came up running as the run's starting settings,
    /// in the run's `mode` when it has one.
    fn start_settings(&mut self, chat: &Chat, mode: Option<String>) {
        let mut start = chat.in_force();
        if mode.is_some() {
            start.mode = mode;
        }
        self.run.start_settings(start);
    }

    /// A turn the run sent ended: read the work, then let the engine judge it.
    pub(super) fn after_turn(&mut self, chat: &Chat, now: Now, fx: &mut Vec<Effect>) {
        // The agent is done with the step: what follows is onehand's.
        self.budget.pause(now.at);
        let Some(from) = self.run.marks.step_from.clone() else {
            return;
        };
        self.answer = Some(turn_answer(chat, self.answer_from));
        fx.push(Effect::ReadFacts {
            dir: self.run.setup.dir.clone(),
            from,
        });
    }

    /// End the run: say how in its transcript, then let the front end keep it
    /// and let its place go. Nothing is done after this.
    pub(super) fn finish(
        &mut self,
        chat: Option<&Chat>,
        outcome: Outcome,
        now: Now,
        fx: &mut Vec<Effect>,
    ) {
        self.over = true;
        if chat.is_some() {
            let said = outcome.said_in(&self.run.template);
            let said = match outcome.resumable() {
                true => format!("{said}; resume it from the Tasks page"),
                false => said,
            };
            self.asked.push(Asked::Note(said));
        }
        self.run.spent_secs = self.spent_secs(now);
        fx.push(Effect::Finish { outcome });
    }
}

/// Whether doing `action` ends the run, which is the one action after which
/// there is no step to announce and nothing to save.
pub(super) fn ends_run(action: &Action) -> bool {
    match action {
        Action::Finish(_) => true,
        Action::Measure
        | Action::Prompt(_)
        | Action::RunCommand(_)
        | Action::AwaitApproval
        | Action::AwaitRoom
        | Action::Push(_)
        | Action::OpenPullRequest
        | Action::AwaitStatusChecks { .. }
        | Action::Idle => false,
    }
}

/// A commit as a person reads it.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(10)]
}

/// The whole of the answer to the prompt sent at `from` items, cut at
/// [`ANSWER_MAX`] characters.
fn turn_answer(chat: &Chat, from: usize) -> String {
    let prose = chat.prose_since(from);
    match prose.char_indices().nth(ANSWER_MAX) {
        Some((cut, _)) => format!("{}\n\n(cut at {ANSWER_MAX} characters)", &prose[..cut]),
        None => prose,
    }
}
