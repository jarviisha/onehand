//! What happens next for a setting: shown at once, written after, put back
//! when its write fails; and what each agent offers, learned when one comes
//! up or when a person asks to check it.
//!
//! **Decided here, carried out elsewhere.** A [`SettingsDrive`] owns the
//! agents a session can be opened with, the appearance, what each agent was
//! last seen to offer, and the checks under way or failed. A change is put in
//! force at once ([`Effect::Show`]) and written after every write before it
//! ([`Effect::Write`]); a write that fails puts back each setting no later
//! write carries, to its last value known saved, and is said once
//! ([`Signal::SaveFailed`]). The ordering and the put-back are
//! [`crate::saving::Saves`]'.
//!
//! **The agent list is never put back.** Its form already waited on the
//! press, so a write that fails is said and the list on screen stands; the
//! next edit's write carries it.
//!
//! **A check is never run unasked**: it costs an agent start. It is brought up
//! as far as its session, whose answer carries the modes and config options
//! ([`fold_probe`]), and dropped, within [`CHECK_WAIT`].

use super::{AgentEdit, AgentSpec, AppConfig, Appearance};
use crate::acp::AcpEvent;
use crate::saving::{Pending, Saves};
use crate::workflow::Offer;
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Duration;

/// How long an agent is given to come up before its check gives up on it.
pub const CHECK_WAIT: Duration = Duration::from_secs(90);

/// One setting, put back on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Setting {
    Appearance,
    Agents,
    /// Whether a program outside onehand may use the tools.
    Outside,
}

/// A setting's value, as it is put in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Appearance(Appearance),
    Outside(bool),
    /// The agent list, which is never put back.
    Agents,
}

/// What one write edits in the file, in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Appearance(Appearance),
    Agents(Box<AgentEdit>),
    Outside(bool),
}

impl Edit {
    /// Make the edit on `cfg`, as it is read from the file at the write.
    pub fn apply(&self, cfg: &mut AppConfig) {
        match self {
            Self::Appearance(choice) => cfg.appearance = *choice,
            Self::Agents(edit) => edit.apply(&mut cfg.agents),
            Self::Outside(on) => cfg.control.outside = *on,
        }
    }

    /// What the write is, in a person's words, for the line saying it failed.
    fn doing(&self) -> &'static str {
        match self {
            Self::Appearance(_) => "Saving the appearance",
            Self::Agents(_) => "Saving the agents",
            Self::Outside(_) => "Saving outside control",
        }
    }
}

/// A check of an agent under way, or the last one that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checking {
    Running,
    Failed(String),
}

/// What the settings are, read through the drive.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub agents: Vec<AgentSpec>,
    pub appearance: Appearance,
    /// What each agent offered when it last came up, by the spec it was
    /// started with. For this process only: a restart forgets, so an adapter
    /// upgraded meanwhile is looked at afresh.
    pub offers_seen: Vec<(AgentSpec, Offer)>,
    /// The checks under way, or the last one that failed, per spec.
    pub checks: Vec<(AgentSpec, Checking)>,
}

impl Settings {
    /// The check of `spec` under way, or its last failure.
    pub fn checking(&self, spec: &AgentSpec) -> Option<&Checking> {
        self.checks
            .iter()
            .find(|(seen, _)| seen == spec)
            .map(|(_, checking)| checking)
    }
}

/// What a person is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A write failed, `doing` saying what it was: why. Said once, at the
    /// edge of a run of failures.
    SaveFailed { doing: &'static str, why: String },
}

/// What the front end carries out.
pub enum Effect {
    /// Put `value` in force: the appearance drawn, outside control held.
    Show(Value),
    /// Write `edit` into the config file after every write before it, and
    /// report [`Input::Written`] with `pending` and `edit`. A late answer is
    /// **recorded**: the write landed or it did not, whatever changed since.
    Write {
        pending: Pending<Setting, Value>,
        edit: Edit,
    },
    /// Bring `spec` up in `cwd` as far as its session, fold its events with
    /// [`fold_probe`], and report [`Input::Probed`] with `token`; and report
    /// [`Input::Elapsed`] with `token` after [`CHECK_WAIT`]. A late answer of
    /// either is **dropped** by its token: the check it belongs to is over.
    Probe {
        spec: AgentSpec,
        cwd: PathBuf,
        token: u64,
    },
    /// Call off the check `token`: drop its agent.
    CallOff {
        token: u64,
    },
    /// What the settings say changed: whatever draws them draws them again.
    Changed,
    Signal(Signal),
}

/// What the front end reports back.
pub enum Input {
    /// The write [`Effect::Write`] asked for landed, or failed.
    Written {
        pending: Pending<Setting, Value>,
        edit: Edit,
        result: Result<(), String>,
    },
    /// The check `token` found what `spec` offers, or why not.
    Probed {
        token: u64,
        found: Result<Offer, String>,
    },
    /// The check `token` ran out of time.
    Elapsed { token: u64 },
}

/// The settings' drive. See the module header.
pub struct SettingsDrive {
    settings: Settings,
    saves: Saves<Setting, Value>,
    /// The checks under way, by token.
    probing: Vec<(u64, AgentSpec)>,
    minted: u64,
}

impl Deref for SettingsDrive {
    type Target = Settings;
    fn deref(&self) -> &Settings {
        &self.settings
    }
}

impl SettingsDrive {
    /// The settings as the config file was read at start.
    pub fn new(agents: Vec<AgentSpec>, appearance: Appearance) -> Self {
        Self {
            settings: Settings {
                agents,
                appearance,
                ..Settings::default()
            },
            saves: Saves::default(),
            probing: Vec::new(),
            minted: 0,
        }
    }

    /// Draw everything in `choice`, then write it.
    pub fn set_appearance(&mut self, choice: Appearance) -> Vec<Effect> {
        let before = self.settings.appearance;
        if before == choice {
            return Vec::new();
        }
        self.settings.appearance = choice;
        let mut fx = vec![Effect::Show(Value::Appearance(choice))];
        fx.push(self.write(
            Setting::Appearance,
            Value::Appearance(before),
            Value::Appearance(choice),
            Edit::Appearance(choice),
        ));
        fx
    }

    /// Hold outside control `on` at once, from `before`, then write it.
    /// Whether it is held is the tools' drive's to keep; this keeps its save.
    pub fn set_outside(&mut self, before: bool, on: bool) -> Vec<Effect> {
        if before == on {
            return Vec::new();
        }
        vec![
            Effect::Show(Value::Outside(on)),
            self.write(
                Setting::Outside,
                Value::Outside(before),
                Value::Outside(on),
                Edit::Outside(on),
            ),
        ]
    }

    /// Make `edit` to the agent list on screen, then to the file's.
    pub fn edit_agents(&mut self, edit: AgentEdit) -> Vec<Effect> {
        edit.apply(&mut self.settings.agents);
        vec![
            Effect::Changed,
            self.write(
                Setting::Agents,
                Value::Agents,
                Value::Agents,
                Edit::Agents(Box::new(edit)),
            ),
        ]
    }

    /// `spec` came up offering `offer`: what was learned of it before is
    /// replaced.
    pub fn saw_offer(&mut self, spec: AgentSpec, offer: Offer) {
        self.settings.offers_seen.retain(|(seen, _)| *seen != spec);
        self.settings.offers_seen.push((spec, offer));
    }

    /// Check `spec` in `cwd`: start it, read what it offers, and close it. A
    /// second press while one runs starts nothing.
    pub fn check(&mut self, spec: AgentSpec, cwd: PathBuf) -> Vec<Effect> {
        if self.settings.checking(&spec) == Some(&Checking::Running) {
            return Vec::new();
        }
        self.minted += 1;
        let token = self.minted;
        self.set_check(&spec, Some(Checking::Running));
        self.probing.push((token, spec.clone()));
        vec![Effect::Changed, Effect::Probe { spec, cwd, token }]
    }

    /// What the front end reports back.
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Written {
                pending,
                edit,
                result,
            } => {
                let Some((why, back)) = self.saves.landed(pending, result) else {
                    return Vec::new();
                };
                let doing = edit.doing();
                let mut fx = Vec::new();
                for value in back {
                    match &value {
                        Value::Appearance(choice) => self.settings.appearance = *choice,
                        Value::Outside(_) => {}
                        Value::Agents => continue,
                    }
                    fx.push(Effect::Show(value));
                }
                fx.push(Effect::Signal(Signal::SaveFailed { doing, why }));
                fx
            }
            Input::Probed { token, found } => {
                let Some(spec) = self.take_probe(token) else {
                    return Vec::new();
                };
                match found {
                    Ok(offer) => {
                        self.set_check(&spec, None);
                        self.saw_offer(spec, offer);
                    }
                    Err(why) => self.set_check(&spec, Some(Checking::Failed(why))),
                }
                vec![Effect::Changed]
            }
            Input::Elapsed { token } => {
                let Some(spec) = self.take_probe(token) else {
                    return Vec::new();
                };
                let why = format!("the agent did not come up within {}s", CHECK_WAIT.as_secs());
                self.set_check(&spec, Some(Checking::Failed(why)));
                vec![Effect::CallOff { token }, Effect::Changed]
            }
        }
    }

    fn write(&mut self, key: Setting, before: Value, after: Value, edit: Edit) -> Effect {
        self.saves.change(key, before, after);
        Effect::Write {
            pending: self.saves.write_of([key]),
            edit,
        }
    }

    fn take_probe(&mut self, token: u64) -> Option<AgentSpec> {
        let at = self.probing.iter().position(|(held, _)| *held == token)?;
        Some(self.probing.remove(at).1)
    }

    fn set_check(&mut self, spec: &AgentSpec, to: Option<Checking>) {
        self.settings.checks.retain(|(seen, _)| seen != spec);
        if let Some(to) = to {
            self.settings.checks.push((spec.clone(), to));
        }
    }
}

/// Fold one event of an agent brought up for a check into `offer`: `Some`
/// once the check is over, found or failed. Nothing is asked of an agent that
/// is never prompted, so whatever else it says before it comes up is let go.
pub fn fold_probe(offer: &mut Offer, event: AcpEvent) -> Option<Result<(), String>> {
    match event {
        AcpEvent::Modes { available, .. } => {
            offer.modes = available.into_iter().map(|m| m.id).collect();
            None
        }
        AcpEvent::ConfigOptions(opts) => {
            offer.options = opts
                .into_iter()
                .map(|opt| (opt.id, opt.choices.into_iter().map(|c| c.value).collect()))
                .collect();
            None
        }
        AcpEvent::Connected { .. } => Some(Ok(())),
        AcpEvent::Error { text: why, .. } | AcpEvent::Disconnected(why) => Some(Err(why)),
        AcpEvent::SessionId(_)
        | AcpEvent::AgentChunk { .. }
        | AcpEvent::ThoughtChunk(_)
        | AcpEvent::UserChunk(_)
        | AcpEvent::Interrupted
        | AcpEvent::ToolCall(_)
        | AcpEvent::ToolUpdate(_)
        | AcpEvent::Plan(_)
        | AcpEvent::Usage(_)
        | AcpEvent::Background(_)
        | AcpEvent::SubagentSpawned { .. }
        | AcpEvent::SubagentEnded { .. }
        | AcpEvent::InChild { .. }
        | AcpEvent::StopRefused { .. }
        | AcpEvent::Permission(_)
        | AcpEvent::Elicitation(_)
        | AcpEvent::AvailableCommands(_)
        | AcpEvent::ModeChanged(_)
        | AcpEvent::PickConfirmed { .. }
        | AcpEvent::PickRefused { .. }
        | AcpEvent::TerminalOutput { .. }
        | AcpEvent::TerminalExit { .. }
        | AcpEvent::TurnEnded { .. }
        | AcpEvent::Notice(_)
        | AcpEvent::Echo(_)
        | AcpEvent::At(_)
        // The refusal it comes ahead of ends the check, in words.
        | AcpEvent::LoginRequired { .. } => None,
    }
}

/// What a check whose agent closed before it came up says.
pub const CLOSED_EARLY: &str = "the agent closed before it came up";
#[cfg(test)]
mod tests;
