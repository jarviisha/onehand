use super::items::ChatItem;
use super::Chat;
use crate::acp::AcpRequest;
use crate::chat::store::{ConfigPick, Prefs};

/// Session titles are compact task labels, not first-message previews.
///
/// **Sized for the widest place the name is shown, not the narrowest.** These
/// were cut for a sidebar row, and every surface took the loss: the
/// conversation's header is as wide as the agent pane and is the one place the
/// name is a heading rather than an entry in a list, so it had a paragraph's
/// worth of room and stopped mid-phrase for the sake of a column two hundred
/// pixels away. Every narrower reader already cuts what it cannot fit *at its
/// own edge* — the rail's rows, a menu row, the bridge's listing — with an
/// ellipsis and at the width it actually has, which is a cut that follows the
/// window instead of guessing at it.
///
/// What stays is that this is a **label**: a first prompt is a paragraph and a
/// conversation's name is not, so the sentence is still clipped at a word
/// boundary rather than left to run to whatever length somebody typed.
pub(super) const TITLE_MAX_CHARS: usize = 96;
const TITLE_MAX_WORDS: usize = 16;

/// Turn the first meaningful line of a prompt into a short, stable task label.
///
/// This deliberately stays local and deterministic: deriving a title must not
/// spend another model turn or delay sending the user's prompt. Request filler
/// is removed in the two languages commonly used in the app, then the label is
/// clipped at a word boundary. `None` is reserved for attachment-only turns so
/// callers can keep the agent-name fallback.
pub(crate) fn summarize_title(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut title = line
        .trim_start_matches(['#', '>', '-', '*', '•'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // A title should describe the task, not the way it was requested. Re-run
    // because prefixes are often stacked ("Could you please help me …").
    const PREFIXES: &[&str] = &[
        "i'd like you to ",
        "i would like you to ",
        "i want you to ",
        "would you please ",
        "could you please ",
        "can you please ",
        "would you ",
        "could you ",
        "can you ",
        "help me ",
        "please ",
        "bạn có thể ",
        "có thể ",
        "vui lòng ",
        "làm ơn ",
        "giúp tôi ",
        "giúp mình ",
        "tôi muốn ",
        "mình muốn ",
        "hãy ",
    ];
    loop {
        let lower = title.to_lowercase();
        let Some(prefix) = PREFIXES.iter().find(|p| lower.starts_with(**p)) else {
            break;
        };
        title = title[prefix.len()..].trim_start().to_string();
    }

    // Keep only the first request sentence. A punctuation mark counts as a
    // boundary only when followed by whitespace, avoiding splits in paths and
    // identifiers such as `src/app.rs`.
    if let Some(end) = title.char_indices().find_map(|(i, c)| {
        matches!(c, '.' | '?' | '!' | ';')
            .then(|| i + c.len_utf8())
            .filter(|end| title[*end..].starts_with(char::is_whitespace))
    }) {
        title.truncate(end);
    }

    const SUFFIXES: &[&str] = &[
        " được không nhỉ",
        " được không",
        " không nhỉ",
        " nhé",
        " nha",
        " nhỉ",
        " please",
    ];
    loop {
        title = title
            .trim_end_matches(|c: char| {
                c.is_whitespace() || matches!(c, '.' | '?' | '!' | ';' | ':')
            })
            .to_string();
        let lower = title.to_lowercase();
        let Some(suffix) = SUFFIXES.iter().find(|s| lower.ends_with(**s)) else {
            break;
        };
        title.truncate(title.len() - suffix.len());
    }

    // Common "make X better" phrasing becomes the task-shaped "Improve X".
    // Besides reading more naturally, this handles the most frequent case where
    // removing request filler alone would still leave a sentence fragment.
    let lower = title.to_lowercase();
    let rewrites = [
        (
            "làm cho ",
            [" đẹp hơn", " tốt hơn", " hợp lý hơn"].as_slice(),
            "Cải thiện ",
        ),
        (
            "make ",
            [" better", " prettier", " nicer"].as_slice(),
            "Improve ",
        ),
    ];
    for (prefix, suffixes, replacement) in rewrites {
        if lower.starts_with(prefix) {
            if let Some(suffix) = suffixes.iter().find(|s| lower.ends_with(**s)) {
                let subject = title[prefix.len()..title.len() - suffix.len()].trim();
                title = format!("{replacement}{subject}");
                break;
            }
        }
    }

    let title = title.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '`' | '\'' | '"' | '.' | '?' | '!' | ';' | ':')
    });
    if title.is_empty() {
        return None;
    }

    let mut clipped = String::new();
    let mut truncated = false;
    for (index, word) in title.split_whitespace().enumerate() {
        let separator = usize::from(!clipped.is_empty());
        if index == TITLE_MAX_WORDS
            || clipped.chars().count() + separator + word.chars().count() > TITLE_MAX_CHARS
        {
            truncated = true;
            break;
        }
        if separator == 1 {
            clipped.push(' ');
        }
        clipped.push_str(word);
    }
    // A single long token (usually a path) still needs a useful label.
    if clipped.is_empty() {
        clipped = title.chars().take(TITLE_MAX_CHARS).collect();
        truncated = title.chars().count() > TITLE_MAX_CHARS;
    }
    if truncated {
        clipped.push('…');
    }

    let mut chars = clipped.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// The id [`Chat::selectors`] gives the session mode.
///
/// A name and not an index, so a caller can hold it across a change in what the
/// agent offers — the config groups come and go, and a position among them is
/// not a thing worth writing down anywhere.
pub const MODE_SELECTOR: &str = "mode";

/// One picker the agent offers, mode and config options alike.
///
/// Flattened out of the two shapes the protocol keeps apart, for callers that
/// have to describe the pickers rather than draw them. See [`Chat::selectors`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    /// `mode`, or the config group's own id.
    pub id: String,
    /// What to call it to a human.
    pub name: String,
    /// The value in force, matching one of `choices`.
    pub current: Option<String>,
    pub choices: Vec<SelectorChoice>,
}

/// One value a [`Selector`] can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorChoice {
    /// What goes on the wire.
    pub value: String,
    /// What to call it to a human.
    pub label: String,
}

impl Chat {
    /// The first thing the user said, capped — what names a conversation in the
    /// picker when nobody has renamed it.
    ///
    /// Kept in the metadata so listing never has to open a transcript to find
    /// it, which is what listing used to do to every conversation in the store.
    pub(super) fn first_prompt(&self) -> String {
        const PREVIEW_MAX: usize = 200;
        self.history
            .iter()
            .chain(self.items.iter())
            .find_map(|it| match it {
                ChatItem::User(u) => Some(u.text.clone()),
                _ => None,
            })
            .map(|text| match text.char_indices().nth(PREVIEW_MAX) {
                Some((cut, _)) => text[..cut].to_string(),
                None => text,
            })
            .unwrap_or_default()
    }

    /// The mode and config picks this conversation is currently on.
    pub(super) fn prefs(&self) -> Prefs {
        Prefs {
            mode: self.current_mode.clone(),
            config: self
                .config_options
                .iter()
                .filter_map(|o| {
                    o.current.clone().map(|value| ConfigPick {
                        id: o.id.clone(),
                        value,
                    })
                })
                .collect(),
        }
    }

    /// A title derived from the conversation's **first user prompt** — its first
    /// non-empty line, trimmed and length-capped — so each session reads
    /// distinctly in the rail row / pane header instead of all showing the
    /// agent name. Looks through resumed `history` then live `items`. `None`
    /// until a prompt exists (callers fall back to the agent name).
    pub(crate) fn derived_title(&self) -> Option<String> {
        self.history
            .iter()
            .chain(self.items.iter())
            .find_map(|item| match item {
                ChatItem::User(u) => Some(u.text.as_str()),
                _ => None,
            })
            .and_then(summarize_title)
    }

    /// The title shown throughout the UI: an explicit rename wins, otherwise
    /// use the compact task label derived from the first prompt.
    pub fn conversation_title(&self) -> Option<String> {
        self.custom_title.clone().or_else(|| self.derived_title())
    }

    /// Commit a user-entered title. Whitespace is normalized so a value pasted
    /// from multiple lines still behaves like a single-line header label.
    /// Blank input is ignored; Reset is an explicit separate action.
    pub fn rename(&mut self, title: &str) -> bool {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return false;
        }
        self.custom_title = Some(title);
        true
    }

    /// Return to the automatically derived title.
    pub fn reset_title(&mut self) {
        self.custom_title = None;
    }
    /// Every picker the agent offers, mode and config options alike, as one
    /// list.
    ///
    /// The two are separate in the protocol and separate on the composer, which
    /// is right there — mode is a first-class field of `session/new` while the
    /// rest arrive as a config-option group, and the composer draws them as the
    /// distinct controls they are. **Anything that has to name one from outside
    /// the app wants them flattened**, because from there they are one question:
    /// what can I change, and to what. Written once here rather than at each
    /// such caller, so a second one cannot come to a different answer about
    /// which pickers exist.
    ///
    /// Mode leads, and takes the id `mode`. That cannot collide with a config
    /// group of the same name because the parser drops the agent's `mode` group
    /// on the way in — the session's own `modes` field is the one that is
    /// honoured.
    pub fn selectors(&self) -> Vec<Selector> {
        let mode = (!self.modes.is_empty()).then(|| Selector {
            id: MODE_SELECTOR.to_string(),
            name: "Mode".to_string(),
            current: self.current_mode.clone(),
            choices: self
                .modes
                .iter()
                .map(|mode| SelectorChoice {
                    value: mode.id.clone(),
                    label: mode.name.clone(),
                })
                .collect(),
        });
        mode.into_iter()
            .chain(self.config_options.iter().map(|option| {
                Selector {
                    id: option.id.clone(),
                    name: option.name.clone(),
                    current: option.current.clone(),
                    choices: option
                        .choices
                        .iter()
                        .map(|choice| SelectorChoice {
                            value: choice.value.clone(),
                            label: choice.name.clone(),
                        })
                        .collect(),
                }
            }))
            .collect()
    }

    /// Pick the `choice`-th value of the picker `group`, and say what was set.
    ///
    /// `None` for a group or a choice that is not there. **Which is a real
    /// answer and not a slip to paper over**: what the agent offers is live and
    /// can be re-advertised mid-session, so a caller holding a place in a list —
    /// a button in a chat, a number somebody typed — can be pointing at
    /// something that has moved. Guessing at the nearest one would change a
    /// model or a mode nobody asked for, quietly.
    ///
    /// The sentence it returns is read from the picker itself rather than from
    /// what the caller thought it was choosing, so a reply built from it is
    /// always true about what actually happened.
    pub fn choose(&mut self, group: &str, choice: usize) -> Option<String> {
        let selector = self.selectors().into_iter().find(|s| s.id == group)?;
        let picked = selector.choices.get(choice)?.clone();
        if group == MODE_SELECTOR {
            self.set_mode(&picked.value);
        } else {
            self.set_config_option(group, &picked.value);
        }
        Some(format!("{} → {}", selector.name, picked.label))
    }

    /// Switch the session mode: tell the adapter, then reflect it locally.
    ///
    /// Optimistic on purpose — the adapter confirms a mode change in its reply
    /// rather than as a `session/update`, so waiting for an echo would leave
    /// the picker showing the old value until the next turn.
    pub fn set_mode(&mut self, mode_id: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::SetMode(mode_id.to_string()));
        }
        self.current_mode = Some(mode_id.to_string());
    }

    /// Step to the mode after the one in force, wrapping at the end.
    ///
    /// A mode the list no longer holds — or none at all — steps to the first,
    /// so the key always lands somewhere the picker can show. `None` when the
    /// agent advertises fewer than two modes, since there is nowhere to go.
    pub fn cycle_mode(&mut self) -> Option<&str> {
        if self.modes.len() < 2 {
            return None;
        }
        let at = self
            .current_mode
            .as_ref()
            .and_then(|id| self.modes.iter().position(|mode| &mode.id == id))
            .map_or(0, |at| (at + 1) % self.modes.len());
        let id = self.modes[at].id.clone();
        self.set_mode(&id);
        self.current_mode.as_deref()
    }

    /// Pick a config option (model / effort / sub-agent), same contract as
    /// [`Self::set_mode`].
    pub fn set_config_option(&mut self, config_id: &str, value: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(AcpRequest::SetConfigOption {
                config_id: config_id.to_string(),
                value: value.to_string(),
            });
        }
        self.set_config_current(config_id, value);
    }

    /// Optimistically reflect a config-option choice (the adapter answers the
    /// `set_config_option` in its reply, not as a `session/update`).
    pub(crate) fn set_config_current(&mut self, config_id: &str, value: &str) {
        if let Some(opt) = self.config_options.iter_mut().find(|o| o.id == config_id) {
            opt.current = Some(value.to_string());
        }
    }

    /// Arm the mode + config picks to replay once this resumed session
    /// reconnects (`Connected { resumed: true }` → [`Self::reapply_prefs`]).
    /// Set right after [`Self::load_history`] from the archive's `prefs`.
    pub(crate) fn arm_prefs(&mut self, mode: Option<String>, config: Vec<(String, String)>) {
        self.pending_mode = mode;
        self.pending_config = config;
    }

    /// Replay the armed selector state onto a freshly-reconnected resumed
    /// session. Re-sends `set_mode` / `set_config_option` only where the
    /// adapter came up on a *different* value than the archive recorded, and
    /// only for options/modes the adapter still offers. **Model is skipped**:
    /// the SDK re-reads it from the transcript on resume, and re-pushing a
    /// picker alias (e.g. `opus` vs the live `opus[1m]`) can switch the context
    /// lane rather than describe it (see the adapter's `getAvailableModels`).
    pub(super) fn reapply_prefs(&mut self) {
        let Some(tx) = self.tx.clone() else { return };
        if let Some(mode) = self.pending_mode.take() {
            if self.current_mode.as_deref() != Some(mode.as_str())
                && self.modes.iter().any(|m| m.id == mode)
            {
                let _ = tx.send(AcpRequest::SetMode(mode.clone()));
                self.current_mode = Some(mode);
            }
        }
        for (id, value) in std::mem::take(&mut self.pending_config) {
            if id.eq_ignore_ascii_case("model") {
                continue;
            }
            let restorable = self
                .config_options
                .iter()
                .find(|o| o.id == id)
                .is_some_and(|o| {
                    o.current.as_deref() != Some(value.as_str())
                        && o.choices.iter().any(|c| c.value == value)
                });
            if restorable {
                let _ = tx.send(AcpRequest::SetConfigOption {
                    config_id: id.clone(),
                    value: value.clone(),
                });
                self.set_config_current(&id, &value);
            }
        }
    }
}
