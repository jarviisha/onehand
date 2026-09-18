//! Derived, render-ready composer data.
//!
//! The protocol remains the source of truth. This module gives the view stable
//! selector keys and human labels without making the card renderer know how
//! modes and agent config groups are stored separately.

use super::super::session::ChatSession;
use gpui::{App, Entity, ParentElement, SharedString, Styled, div};
use gpui_component::{Icon, IconName, StyledExt};
use onehand_core::chat::SubmitBlock;

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
    pub(super) mark: Option<IconName>,
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

/// The one agent-advertised group this app draws as a rail of segments at the
/// foot of the list rather than as rows in it.
///
/// **Named, and only this one.** Effort is the setting whose values are a
/// *ladder* — less of a thing, then more of it — and three or four words on one
/// rail says that where a column of rows says only that there are four of them.
/// Nothing else the protocol carries is promised to be ordered, and a model list
/// laid out this way would be claiming an order the agent never stated.
///
/// The cost is stated plainly: an agent that names this group something else
/// gets rows, silently. That is the right way round — rows are correct for
/// anything, and the strip is an improvement this app is guessing at.
const SEGMENTED_GROUP: &str = "effort";

/// The group the trigger promises, and so the group that leads the list.
///
/// Named beside the other two rather than spelled at each call site: the chip
/// letters this group's value and the sort puts it first, and those two coming
/// to disagree about which group is meant is a chip naming one setting over a
/// list led by another.
const LEAD_GROUP: &str = "model";

/// Whether a config group is the one the caller means, by the agent's id or by
/// its label.
///
/// Neither is promised: one adapter's `effort` is another's `Effort`, and a
/// third may send the human name and an opaque id. Written once because several
/// callers ask it and several copies is several places for one of them to start
/// disagreeing about which group is which.
fn names(option: &onehand_core::acp::ConfigOption, wanted: &str) -> bool {
    option.id.eq_ignore_ascii_case(wanted) || option.name.eq_ignore_ascii_case(wanted)
}

/// A config group drawn as one rail of segments.
pub(super) struct Segments {
    pub(super) name: SharedString,
    pub(super) config_id: String,
    /// Each choice as it is shown and as it is sent.
    pub(super) choices: Vec<(SharedString, String)>,
    pub(super) current: Option<usize>,
}

/// The effort group, where the agent advertises one.
pub(super) fn segmented_group(session: &Entity<ChatSession>, cx: &App) -> Option<Segments> {
    session
        .read(cx)
        .chat
        .config_options
        .iter()
        .find(|option| names(option, SEGMENTED_GROUP))
        .and_then(segments_of)
}

/// The same decision, off the option alone.
///
/// Split out from the lookup above so the rule can be checked without a window:
/// what it decides is whether the rail is drawn *at all*, and the list filters
/// itself on the same answer — the two disagreeing is the setting appearing in
/// both controls or in neither.
fn segments_of(option: &onehand_core::acp::ConfigOption) -> Option<Segments> {
    // A group with one choice is not a ladder, and a rail with a single rung on
    // it is a label that happens to be pressable. Back to rows, where it reads
    // as the one value there is.
    if option.choices.len() < 2 {
        return None;
    }
    Some(Segments {
        name: SharedString::from(option.name.clone()),
        config_id: option.id.clone(),
        current: option
            .current
            .as_ref()
            .and_then(|value| option.choices.iter().position(|c| &c.value == value)),
        choices: option
            .choices
            .iter()
            .map(|choice| {
                (
                    SharedString::from(choice.name.clone()),
                    choice.value.clone(),
                )
            })
            .collect(),
    })
}

/// The one agent-advertised group this app gives a chip of its own on the strip
/// under the composer, rather than leaving it among the rows of the model list.
///
/// **Named, and only this one.** It is the setting most likely to be changed
/// between one prompt and the next and it has few enough values to be read off
/// a chip, so it earns a control the way the permission mode does: the value on
/// screen, the choices one press away. Nothing in the data says which group
/// that is -- every group is a name and a list of values -- so the name is
/// written here, and an agent that calls it something else simply leaves it in
/// the list, which is correct for anything.
const CHIP_GROUP: &str = "fast";

/// The group behind that chip, where the agent advertises one.
fn chip_group<'a>(
    options: &'a [onehand_core::acp::ConfigOption],
    name: &str,
) -> Option<&'a onehand_core::acp::ConfigOption> {
    options
        .iter()
        .find(|option| names(option, name))
        // A group with nothing to pick is a chip that opens an empty list.
        .filter(|option| !option.choices.is_empty())
}

/// What that chip says: the value in force, or the setting's own name until one
/// is.
pub(super) fn fast_action(session: &Entity<ChatSession>, cx: &App) -> Option<SharedString> {
    let option = chip_group(&session.read(cx).chat.config_options, CHIP_GROUP)?;
    Some(SharedString::from(
        in_force(option)
            .map(|choice| choice.name.clone())
            .unwrap_or_else(|| option.name.clone()),
    ))
}

/// The choices behind it.
pub(super) fn fast_rows(session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    let options = &session.read(cx).chat.config_options;
    chip_group(options, CHIP_GROUP)
        .map(rows_of)
        .unwrap_or_default()
}

/// The choice a group is currently set to, if the agent named one this list
/// still holds.
fn in_force(option: &onehand_core::acp::ConfigOption) -> Option<&onehand_core::acp::ConfigChoice> {
    let value = option.current.as_ref()?;
    option.choices.iter().find(|choice| &choice.value == value)
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
/// **The model alone.** Effort used to ride here in the quieter ink, and it has
/// its own control in the row now — the same setting said twice an inch apart
/// is two places to read one fact and one of them will be a frame behind.
///
/// `None` where there is nothing for the chip's popup to hold at all.
///
/// **The rail counts as something to open.** Effort is drawn at the foot of
/// that popup and nowhere else, so a chip withheld because the *list* above the
/// rail is empty is effort made unreachable — which is exactly the shape an
/// agent advertising effort and fast mode and nothing else has, since both of
/// those are promoted out of the list and neither leaves a row behind. The
/// popup itself already draws a rail with no rows above it; this is the control
/// that opens it agreeing about when there is something to see.
pub(super) fn options_action(session: &Entity<ChatSession>, cx: &App) -> Option<SharedString> {
    let options = &session.read(cx).chat.config_options;
    if !opens_onto_something(options) {
        return None;
    }
    let model = options
        .iter()
        .find(|option| names(option, LEAD_GROUP))
        .and_then(in_force)
        .map(|choice| SharedString::from(choice.name.clone()));
    Some(model.unwrap_or_else(|| SharedString::from("Model")))
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

/// Whether a group belongs in the list at all.
///
/// Two of them do not: the one drawn as a rail under the list and the one drawn
/// as a switch in the composer's row. Both are dropped on **exactly the
/// condition their own control is drawn on**, never on the name alone -- a
/// group named `effort` that offers one choice, or `fast` that offers two
/// values neither of which reads as *on*, gets no promoted control and has to
/// come back here or it is reachable from nowhere.
///
/// One function because the list and the chip that opens it both ask, and the
/// two disagreeing is a chip whose popup is empty.
/// Whether the Options popup has anything in it at all.
///
/// **Rows or the rail, because the rail is inside that popup.** Effort is drawn
/// at its foot and nowhere else, so a chip withheld on the strength of the list
/// alone takes effort off the screen entirely -- which is precisely the shape of
/// an agent advertising effort and fast mode and nothing more, both of them
/// promoted out of the list and neither leaving a row behind.
///
/// One function because the chip and the popup both ask, and the two
/// disagreeing is either a chip opening onto nothing or a setting with no way
/// in.
fn opens_onto_something(options: &[onehand_core::acp::ConfigOption]) -> bool {
    options.iter().any(|option| listed(&option))
        || options
            .iter()
            .any(|option| names(option, SEGMENTED_GROUP) && segments_of(option).is_some())
}

fn listed(option: &&onehand_core::acp::ConfigOption) -> bool {
    let promoted = (names(option, SEGMENTED_GROUP) && segments_of(option).is_some())
        || (names(option, CHIP_GROUP) && !option.choices.is_empty());
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
/// Asked through `names` and the two group constants rather than by matching
/// the strings here: the id and the label are both unpromised, and a rank that
/// recognised a group the rest of this file does not is a list whose order
/// disagrees with what it drew.
fn config_rank(option: &onehand_core::acp::ConfigOption, index: usize) -> (u8, usize) {
    let rank = if names(option, LEAD_GROUP) {
        0
    } else if names(option, SEGMENTED_GROUP) {
        1
    } else {
        2
    };
    (rank, index)
}

pub(super) fn composer_status(blocked: Option<SubmitBlock>, cx: &App) -> Option<gpui::Div> {
    let reason = match blocked? {
        SubmitBlock::UnreadableAttachment(name) => {
            format!("{name} could not be read — remove it before sending")
        }
        SubmitBlock::NotConnected => "Agent disconnected — waiting to reconnect".to_string(),
        SubmitBlock::Empty | SubmitBlock::Busy => return None,
    };
    Some(
        div()
            .h_flex()
            .gap_1()
            .text_xs()
            .text_color(crate::theme::status_ink(cx).danger)
            .child(Icon::new(IconName::Info).size_3())
            .child(reason),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CHIP_GROUP, SEGMENTED_GROUP, config_rank, listed, names, opens_onto_something, segments_of,
    };
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
            current: current.map(str::to_string),
            choices: values.iter().copied().map(choice).collect(),
        }
    }

    #[test]
    fn a_group_is_found_by_id_or_by_label() {
        assert!(names(
            &effort("effort", "Reasoning", &[], None),
            SEGMENTED_GROUP
        ));
        assert!(names(&effort("x-9", "Effort", &[], None), SEGMENTED_GROUP));
        assert!(names(&effort("x-9", "EFFORT", &[], None), SEGMENTED_GROUP));
        assert!(!names(
            &effort("model", "Model", &[], None),
            SEGMENTED_GROUP
        ));
    }

    #[test]
    fn a_ladder_needs_two_rungs_to_be_a_strip() {
        let one = effort("effort", "Effort", &["high"], Some("high"));
        assert!(
            segments_of(&one).is_none(),
            "one choice falls back to a row, so the list must keep it"
        );
        let two = effort("effort", "Effort", &["low", "high"], Some("high"));
        let strip = segments_of(&two).expect("two choices is a strip");
        assert_eq!(strip.current, Some(1));
        assert_eq!(strip.choices.len(), 2);
    }

    #[test]
    fn a_promoted_group_that_cannot_be_drawn_stays_in_the_list() {
        let chip = effort("fast", "Fast mode", &["on", "off"], Some("on"));
        assert!(names(&chip, CHIP_GROUP));
        assert!(!listed(&&chip), "the chip has it, so the list must not");
        assert!(
            listed(&&effort("fast", "Fast mode", &[], None)),
            "a group with nothing to pick gets no chip, so dropping it would hide it entirely"
        );

        let rail = effort("effort", "Effort", &["low", "high"], Some("high"));
        assert!(!listed(&&rail), "the rail has it");
        assert!(
            listed(&&effort("effort", "Effort", &["high"], Some("high"))),
            "one rung is no rail, so it is a row"
        );
    }

    #[test]
    fn a_value_the_agent_never_offered_lights_nothing() {
        let stale = effort("effort", "Effort", &["low", "high"], Some("medium"));
        assert_eq!(
            segments_of(&stale).expect("still a strip").current,
            None,
            "guessing at the nearest rung would report a setting nobody chose"
        );
    }

    #[test]
    fn a_rail_alone_is_still_worth_opening() {
        let promoted = [
            effort("effort", "Effort", &["low", "high"], Some("low")),
            effort("fast", "Fast mode", &["on", "off"], Some("off")),
        ];
        assert!(
            !promoted.iter().any(|option| listed(&option)),
            "both are drawn by controls of their own, so neither is a row"
        );
        assert!(
            opens_onto_something(&promoted),
            "the rail lives in that popup, so withholding the chip hides effort entirely"
        );
        assert!(
            !opens_onto_something(&[effort("fast", "Fast mode", &["on", "off"], Some("on"))]),
            "the chip draws fast mode itself, so its popup would open onto nothing"
        );
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
