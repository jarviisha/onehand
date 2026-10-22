//! What onehand reads into the config groups an agent offers: which one is
//! the model, the effort or the fast switch, and which way a switch stands.
//!
//! **Nothing in the data promises any of it.** A group is an id, a label and
//! a list of values, so the protocol's `category` is read first and a few
//! names adapters are known to use after it. A group none of them names gets
//! no role, and a front end lists it among the rest, which is correct for
//! anything; a role is an improvement guessed at, never a requirement.

use super::types::{ConfigChoice, ConfigOption};

/// What a config group is, where onehand can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Model,
    /// What the model runs at: how hard it thinks.
    Effort,
    /// A switch between the model's fast and its ordinary pace.
    Fast,
}

/// A group that is unmistakably a switch: which way it stands, and the value
/// a press sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switch {
    pub on: bool,
    pub flip_to: String,
}

/// The names adapters give each role: Claude's adapter calls its effort
/// `effort` and Codex's `reasoning_effort`; Claude's fast group is `fast`
/// and Codex's `fast-mode`.
const MODEL_NAMES: [&str; 1] = ["model"];
const EFFORT_NAMES: [&str; 2] = ["effort", "reasoning_effort"];
const FAST_NAMES: [&str; 2] = ["fast", "fast-mode"];

/// Words that name a choice as the *on* side of a switch, and as the off.
const ON_WORDS: [&str; 5] = ["on", "true", "enabled", "yes", "fast"];
const OFF_WORDS: [&str; 7] = [
    "off", "false", "disabled", "no", "normal", "standard", "default",
];

impl ConfigOption {
    /// What this group is: by the category the agent gave it when that is
    /// one the protocol defines, else by its id or its label.
    pub fn role(&self) -> Option<Role> {
        match self.category.as_deref() {
            Some("model") => return Some(Role::Model),
            Some("thought_level") => return Some(Role::Effort),
            _ => {}
        }
        let named = |names: &[&str]| {
            names.iter().any(|name| {
                self.id.eq_ignore_ascii_case(name) || self.name.eq_ignore_ascii_case(name)
            })
        };
        if named(&MODEL_NAMES) {
            Some(Role::Model)
        } else if named(&EFFORT_NAMES) {
            Some(Role::Effort)
        } else if named(&FAST_NAMES) {
            Some(Role::Fast)
        } else {
            None
        }
    }

    /// The choice in force, when the agent named one this list still holds.
    pub fn in_force(&self) -> Option<&ConfigChoice> {
        let value = self.current.as_ref()?;
        self.choices.iter().find(|choice| &choice.value == value)
    }

    /// This group as a switch, where it is unmistakably one.
    ///
    /// **Exactly two choices, and one of them named as on or off** by its
    /// value or its label. Anything else stays a picker: a switch that guessed
    /// which of two agent-chosen words meant *on* would show a position nobody
    /// can check, where a picker names both values and ticks the one in force.
    pub fn switch(&self) -> Option<Switch> {
        let [a, b] = self.choices.as_slice() else {
            return None;
        };
        let (on, off) = match (
            a.says(&ON_WORDS) || b.says(&OFF_WORDS),
            b.says(&ON_WORDS) || a.says(&OFF_WORDS),
        ) {
            (true, false) => (a, b),
            (false, true) => (b, a),
            _ => return None,
        };
        let is_on = self.current.as_deref() == Some(on.value.as_str());
        Some(Switch {
            on: is_on,
            flip_to: match is_on {
                true => off.value.clone(),
                false => on.value.clone(),
            },
        })
    }
}

impl ConfigChoice {
    /// Whether this choice is named as the off side of a switch.
    pub fn reads_off(&self) -> bool {
        self.says(&OFF_WORDS)
    }

    /// Whether this choice is named by one of `words`, by its value or its label.
    fn says(&self, words: &[&str]) -> bool {
        [&self.value, &self.name]
            .iter()
            .any(|word| words.contains(&word.trim().to_lowercase().as_str()))
    }
}

#[cfg(test)]
mod tests;
