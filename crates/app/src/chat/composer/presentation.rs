//! Derived, render-ready composer data.
//!
//! The protocol remains the source of truth. This module gives the view stable
//! selector keys and human labels without making the card renderer know how
//! modes and agent config groups are stored separately.

use super::super::session::ChatSession;
use gpui::{App, Entity, SharedString};
use gpui_component::{Icon, IconName};
use onehand_core::acp::Role;

#[derive(Default)]
pub(super) struct Row {
    pub(super) label: SharedString,
    pub(super) detail: Option<SharedString>,
    pub(super) checked: bool,
    pub(super) pick: Pick,
    /// What kind of thing this row offers, where the words alone do not say.
    ///
    /// Only the `@` list sets it: a file, a folder and something the session
    /// already touched are three different offers that read as one column of
    /// paths, and the folder is the one that must not be mistaken — accepting
    /// it inserts a listing where the reader was expecting a file. A settings
    /// choice leaves it empty, because there the rows are all the same kind of
    /// thing and an icon per row would be a second alphabet for a column of
    /// four words.
    pub(super) mark: Option<Icon>,
    /// Where the query matched the label, and the detail.
    ///
    /// **The whole of how a row says why it is in the list**, drawn as the two
    /// ends of the ink ramp and nothing else — no second hue, no weight, no
    /// rule under the letters. The affordance has to survive a reader who does
    /// not separate colours, and it has to not compete with the one fill in
    /// this popup that means something, which is the row about to be taken.
    ///
    /// Byte ranges into the string as drawn, computed in core against the same
    /// text. At most one of the two is ever set.
    pub(super) label_span: Option<std::ops::Range<usize>>,
    pub(super) detail_span: Option<std::ops::Range<usize>>,
    /// The heading this row opens, where it is the first of its group.
    ///
    /// **Carried by the row rather than being a row of its own.** One flat list
    /// holds every agent-advertised group, so it needs the headings — but the
    /// index into this list is what the arrow keys walk and what Enter takes,
    /// and a heading sitting in it as an entry is a stop on that walk that
    /// cannot be committed to anything. Hung off the row that follows it, the
    /// list stays entirely made of choices and the drawing is the only place
    /// that has to know a heading exists.
    pub(super) group: Option<SharedString>,
}

#[derive(Clone)]
pub(super) enum Pick {
    /// Take this completion: the string that replaces the trigger and its
    /// query. Carried on the row rather than looked up again by index, because
    /// what a mention row *says* and what it *inserts* are no longer the same
    /// text — a row reading `composer.rs` inserts a whole path, and a folder
    /// row inserts a trailing slash that appears nowhere in its own label.
    Complete(SharedString),
    Mode(String),
    Config {
        config_id: String,
        value: String,
    },
    /// Something done to the branch, carried out by the pane.
    Branch(super::BranchAct),
    /// Open one of the composer's own controls rather than putting anything in
    /// the buffer. What the row named is a way *to* a control, so taking it has
    /// to leave the field as it found it.
    Act(Act),
}

/// A composer control reachable by name from the `/` list.
///
/// **The same controls the chips already offer, under the names people would
/// guess.** Nothing here is new behaviour — every one is a `toggle_picker` or a
/// trigger the `+` menu already inserts. What it adds is that they can be
/// *typed*, which is what somebody with their hands on the keyboard wants, and
/// which is the only route that does not start with knowing which chip a
/// setting lives behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    /// The agent's config list: model, and whatever else it advertises.
    Options,
    /// The effort the model runs at, where it has a chip of its own.
    Effort,
    /// The session mode.
    Mode,
    /// The one config group promoted to a chip of its own.
    Fast,
    /// The system file picker, for staging an attachment.
    Attach,
    /// Type an `@` into the field, so the mention list opens on it.
    Mention,
    /// Put a `/` at the front of the field, so the command list opens on it.
    Command,
    /// Open the workflow launcher, which the window owns.
    Workflow,
}

impl Act {
    /// The overlay this opens, where it opens one.
    ///
    /// **One mapping and not two.** It was written out at both ends — once to
    /// ask whether the control had anything in it, once to open it — and two
    /// copies of a three-arm table is two places for an arm to be added to one
    /// and not the other, which draws a row that refuses when pressed.
    pub(super) fn opens(self) -> Option<super::Overlay> {
        match self {
            Self::Options => Some(super::Overlay::Options),
            Self::Effort => Some(super::Overlay::Effort),
            Self::Mode => Some(super::Overlay::Mode),
            Self::Fast => Some(super::Overlay::Fast),
            Self::Attach | Self::Mention | Self::Command | Self::Workflow => None,
        }
    }
}

/// Written out rather than derived: `#[default]` only reaches unit variants,
/// and the completion arm carries the string it would insert.
impl Default for Pick {
    fn default() -> Self {
        Self::Complete(SharedString::default())
    }
}

/// The mode in force, as the chip says it.
///
/// The value alone, without the setting's own name in front of it: the row has
/// four controls competing for a narrow panel's last inch, and `Mode · ` is
/// five characters of a word the chip's tooltip already carries. What is left
/// is the only part that ever changes.
pub(super) fn mode_action(session: &Entity<ChatSession>, cx: &App) -> Option<SharedString> {
    let chat = &session.read(cx).chat;
    (!chat.modes.is_empty()).then(|| {
        chat.current_mode
            .as_ref()
            .and_then(|id| chat.modes.iter().find(|mode| &mode.id == id))
            .map(|mode| SharedString::from(mode.name.clone()))
            // Nothing picked yet, so the setting names itself rather than
            // leaving a chip with no word in it at all.
            .unwrap_or_else(|| SharedString::from("Mode"))
    })
}

/// The one agent-advertised group this app gives a chip and a menu of its own,
/// beside the model's: what the model runs at, read as a qualifier of it.
///
/// Which group that is, is core's to say ([`Role::Effort`]). An agent whose
/// group core cannot place gets rows in the model's list, silently, which is
/// the right way round: rows are correct for anything, and the chip is an
/// improvement guessed at. Two choices at least, since a group with one is a
/// value and not a choice, and goes back to being a row.
fn has_own_chip(option: &onehand_core::acp::ConfigOption) -> bool {
    option.role() == Some(Role::Effort) && option.choices.len() >= 2
}

/// The effort menu's rows, where the group has a chip of its own.
pub(super) fn effort_rows(session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    session
        .read(cx)
        .chat
        .config_options
        .iter()
        .find(|option| has_own_chip(option))
        .map(rows_of)
        .unwrap_or_default()
}

/// The one agent-advertised group this app gives a chip of its own on the strip
/// under the composer, rather than leaving it among the rows of the model list.
///
/// **Only this one.** It is the setting most likely to be changed between one
/// prompt and the next and it has few enough values to be read off a chip, so
/// it earns a control the way the permission mode does: the value on screen,
/// the choices one press away. Which group it is, is core's to say
/// ([`Role::Fast`]); one core cannot place stays in the list, which is correct
/// for anything.
fn fast_group(
    options: &[onehand_core::acp::ConfigOption],
) -> Option<&onehand_core::acp::ConfigOption> {
    options
        .iter()
        .find(|option| option.role() == Some(Role::Fast))
        // A group with nothing to pick is a chip that opens an empty list.
        .filter(|option| !option.choices.is_empty())
}

/// Fast mode as a picker draws it: whether its bolt is lit, and the words its
/// tooltip names the value in force by, since the bolt alone names none.
pub(super) struct FastPick {
    pub(super) lit: bool,
    pub(super) label: SharedString,
}

/// What that picker shows: the value in force after the setting's name, or the
/// setting's own name until one is; lit once a value in force is not one of
/// the words that name a fast group's off side.
pub(super) fn fast_action(session: &Entity<ChatSession>, cx: &App) -> Option<FastPick> {
    let option = fast_group(&session.read(cx).chat.config_options)?;
    let current = option.in_force();
    Some(FastPick {
        lit: current.is_some_and(|choice| !choice.reads_off()),
        label: SharedString::from(
            current
                .map(|choice| format!("Fast mode: {}", choice.name))
                .unwrap_or_else(|| option.name.clone()),
        ),
    })
}

/// The choices behind it.
pub(super) fn fast_rows(session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    let options = &session.read(cx).chat.config_options;
    fast_group(options).map(rows_of).unwrap_or_default()
}

/// One config group as popup rows, headed by the group's own name.
///
/// Written once because two lists build rows this way -- the model list, which
/// concatenates every group it holds, and the chip above, which holds one. Two
/// copies is two places for a heading or a tick to start being drawn
/// differently in lists that open a finger's width apart.
fn rows_of(option: &onehand_core::acp::ConfigOption) -> Vec<Row> {
    option
        .choices
        .iter()
        .enumerate()
        .map(|(i, choice)| Row {
            // The choice's own name, with the group said **once** in the
            // heading above it rather than again on every row. Repeated per row
            // it was the widest thing in the list and the only part of it that
            // never varied, so a reader scanning for a model name read
            // `Model · ` five times to find the five words that differed.
            label: SharedString::from(choice.name.clone()),
            // The agent's own sentence about the choice, where it sent one.
            // This is what tells two model names apart for a reader who has not
            // read the vendor's notes -- and, on a setting that refuses to stay
            // where it is put, where the reason is.
            detail: choice.description.clone().map(SharedString::from),
            checked: option.current.as_ref() == Some(&choice.value),
            pick: Pick::Config {
                config_id: option.id.clone(),
                value: choice.value.clone(),
            },
            group: (i == 0).then(|| SharedString::from(option.name.clone())),
            ..Row::default()
        })
        .collect()
}

/// The model in force, as the chip says it.
///
/// `None` where the model's list would hold nothing.
pub(super) fn options_action(session: &Entity<ChatSession>, cx: &App) -> Option<SharedString> {
    let options = &session.read(cx).chat.config_options;
    if !options.iter().any(|option| listed(&option)) {
        return None;
    }
    let model = options
        .iter()
        .find(|option| option.role() == Some(Role::Model))
        .and_then(|option| option.in_force())
        .map(|choice| SharedString::from(choice.name.clone()));
    Some(model.unwrap_or_else(|| SharedString::from("Model")))
}

/// The effort in force, as its chip beside the model's says it: the qualifier
/// in the middle ink, so the two read as a name and what it is run at. `None`
/// where the group has no chip of its own.
pub(super) fn effort_action(session: &Entity<ChatSession>, cx: &App) -> Option<SharedString> {
    let options = &session.read(cx).chat.config_options;
    options
        .iter()
        .find(|option| has_own_chip(option))
        .map(|option| {
            option
                .in_force()
                .map(|choice| SharedString::from(choice.name.clone()))
                .unwrap_or_else(|| SharedString::from("Effort"))
        })
}

/// Fast mode as a switch: which way it stands, and the value a press sends.
pub(super) struct FastToggle {
    pub(super) on: bool,
    pub(super) config_id: String,
    pub(super) flip_to: String,
}

/// The fast group as a switch, where the agent offers one that reads as one.
pub(super) fn fast_toggle(session: &Entity<ChatSession>, cx: &App) -> Option<FastToggle> {
    let option = fast_group(&session.read(cx).chat.config_options)?;
    let switch = option.switch()?;
    Some(FastToggle {
        on: switch.on,
        config_id: option.id.clone(),
        flip_to: switch.flip_to,
    })
}

/// The `+` menu's rows.
///
/// **Each row names the mark it stands for**, in its glyph and its detail. With
/// a Vietnamese input method on Linux a typed `@` or `/` can be swallowed before
/// it reaches the composer, so these rows are the only route to either trigger.
pub(super) fn add_rows() -> Vec<Row> {
    [
        (
            "Attach files…",
            None,
            Icon::new(crate::icons::Icon::Paperclip),
            Act::Attach,
        ),
        (
            "Mention a file",
            Some("@"),
            Icon::new(crate::icons::Icon::AtSign),
            Act::Mention,
        ),
        (
            "Run a slash command",
            Some("/"),
            Icon::new(crate::icons::Icon::SquareSlash),
            Act::Command,
        ),
        // Not something put into this prompt but a run of prompts, so it goes
        // up to the window, which owns runs, as its key does.
        (
            "Run a workflow…",
            None,
            Icon::new(IconName::Play),
            Act::Workflow,
        ),
    ]
    .into_iter()
    .map(|(label, detail, mark, act)| Row {
        label: label.into(),
        detail: detail.map(SharedString::from),
        mark: Some(mark),
        pick: Pick::Act(act),
        ..Row::default()
    })
    .collect()
}

/// The branch chip's rows: everything the shell already does about the branch
/// a reader is looking at, without leaving for the rail.
pub(super) fn branch_rows() -> Vec<Row> {
    use super::BranchAct;
    [
        (
            "Rename branch…",
            Icon::new(crate::icons::Icon::SquarePen),
            BranchAct::Rename,
        ),
        (
            "New worktree…",
            Icon::new(crate::icons::Icon::GitBranch),
            BranchAct::Worktree,
        ),
        (
            "Refresh Git status",
            Icon::new(IconName::Redo),
            BranchAct::Refresh,
        ),
    ]
    .into_iter()
    .map(|(label, mark, act)| Row {
        label: label.into(),
        mark: Some(mark),
        pick: Pick::Branch(act),
        ..Row::default()
    })
    .collect()
}

pub(super) fn mode_rows(session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    let chat = &session.read(cx).chat;
    chat.modes
        .iter()
        .enumerate()
        .map(|(i, mode)| Row {
            label: SharedString::from(mode.name.clone()),
            detail: None,
            checked: chat.current_mode.as_ref() == Some(&mode.id),
            pick: Pick::Mode(mode.id.clone()),
            // Headed like the other list even though there is only ever one
            // group here. The two open from chips a finger-width apart and are
            // read as one pair of controls; a heading on one of them and not
            // the other is a difference that says something, and there is
            // nothing here for it to say.
            group: (i == 0).then(|| SharedString::from("Mode")),
            ..Row::default()
        })
        .collect()
}

/// Whether a group belongs in the model's list at all.
///
/// Two of them do not: effort, which has a chip and a menu of its own, and the
/// one drawn as a switch in the composer's row. Both are dropped on **exactly
/// the condition their own control is drawn on**, never on the name alone -- a
/// group named `effort` that offers one choice, or `fast` that offers two
/// values neither of which reads as *on*, gets no control of its own and has
/// to come back here or it is reachable from nowhere.
fn listed(option: &&onehand_core::acp::ConfigOption) -> bool {
    let promoted =
        has_own_chip(option) || (option.role() == Some(Role::Fast) && !option.choices.is_empty());
    !promoted
}

pub(super) fn options_rows(session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    let chat = &session.read(cx).chat;
    let mut options: Vec<_> = chat
        .config_options
        .iter()
        .enumerate()
        .filter(|(_, option)| listed(option))
        .collect();
    // The trigger promises Model, so Model leads. Preserve the agent's own
    // order within everything after it.
    options.sort_by_key(|(index, option)| config_rank(option, *index));
    options
        .into_iter()
        .flat_map(|(_, option)| rows_of(option))
        .collect()
}

/// Where a group sits in the list, and its own place within its rank.
///
/// Asked through the group's role rather than by matching names here: a rank
/// that recognised a group the rest of this file does not is a list whose
/// order disagrees with what it drew.
fn config_rank(option: &onehand_core::acp::ConfigOption, index: usize) -> (u8, usize) {
    let rank = match option.role() {
        Some(Role::Model) => 0,
        Some(Role::Effort) => 1,
        Some(Role::Fast) | None => 2,
    };
    (rank, index)
}

#[cfg(test)]
mod tests {
    use super::{config_rank, has_own_chip, listed};
    use onehand_core::acp::Role;
    use onehand_core::acp::{ConfigChoice, ConfigOption};

    fn choice(value: &str) -> ConfigChoice {
        ConfigChoice {
            value: value.into(),
            name: value.into(),
            description: None,
        }
    }

    fn effort(id: &str, name: &str, values: &[&str], current: Option<&str>) -> ConfigOption {
        ConfigOption {
            id: id.into(),
            name: name.into(),
            category: None,
            current: current.map(str::to_string),
            choices: values.iter().copied().map(choice).collect(),
        }
    }

    #[test]
    fn effort_needs_two_choices_for_a_chip_of_its_own() {
        let one = effort("effort", "Effort", &["high"], Some("high"));
        assert!(
            !has_own_chip(&one),
            "one choice falls back to a row, so the list must keep it"
        );
        assert!(has_own_chip(&effort(
            "effort",
            "Effort",
            &["low", "high"],
            Some("high")
        )));
    }

    #[test]
    fn a_promoted_group_that_cannot_be_drawn_stays_in_the_list() {
        let chip = effort("fast", "Fast mode", &["on", "off"], Some("on"));
        assert_eq!(chip.role(), Some(Role::Fast));
        assert!(!listed(&&chip), "the chip has it, so the list must not");
        assert!(
            listed(&&effort("fast", "Fast mode", &[], None)),
            "a group with nothing to pick gets no chip, so dropping it would hide it entirely"
        );

        let own = effort("effort", "Effort", &["low", "high"], Some("high"));
        assert!(!listed(&&own), "its own chip has it");
        assert!(
            listed(&&effort("effort", "Effort", &["high"], Some("high"))),
            "one choice gets no chip, so it is a row"
        );
    }

    #[test]
    fn effort_and_fast_each_have_a_control_and_leave_no_row() {
        let promoted = [
            effort("effort", "Effort", &["low", "high"], Some("low")),
            effort("fast", "Fast mode", &["on", "off"], Some("off")),
        ];
        assert!(
            !promoted.iter().any(|option| listed(&option)),
            "both are drawn by controls of their own, so neither is a row"
        );
        assert!(has_own_chip(&promoted[0]), "effort is still reachable");
    }

    #[test]
    fn model_then_effort_lead_the_combined_popup() {
        let mut groups = [
            config_rank(&effort("agent", "Sub-agent", &[], None), 0),
            config_rank(&effort("effort", "Effort", &[], None), 1),
            config_rank(&effort("model", "Model", &[], None), 2),
        ];
        groups.sort();
        assert_eq!(groups, [(0, 2), (1, 1), (2, 0)]);
    }
}
