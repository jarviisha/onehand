//! What a button on an outgoing message means, and how it survives the trip
//! there and back.
//!
//! **Everything a press has to carry rides in the button's own data**, because
//! there is nowhere else to put it: a channel hands back what it was given and
//! nothing more, and the app has no idea which of its windows the press is
//! about until it reads this. That data is small — a channel is free to cap it,
//! and Telegram caps it at 64 bytes — so what goes in it is a letter, a session
//! id and a position, and never a value the agent chose. The one exception is a
//! picker's *group*, which travels by name because the alternative is a place in
//! a list that is rebuilt while the message sits in somebody's chat; it is short
//! in practice and [`Press::fits`] is what refuses to draw a button when it is
//! not.

use super::types::Button;
use crate::acp::{ElicitField, ElicitKind, PermissionOption, PermissionWeight};
use crate::chat::{first_line_trunc, Selector};

/// The longest a button's label may be before it is clipped.
///
/// A permission option is short by convention and a question's choice is a
/// sentence, and a channel given a sentence lays it out as a button the width of
/// the screen with the important half missing. Clipped rather than wrapped: the
/// full text is in the message above the buttons either way.
const LABEL_MAX: usize = 32;

/// What pressing a button asks the app to do.
///
/// **Every variant names the exact card**, not "whatever that session is waiting
/// on". A message stays pressable in a chat for as long as it is scrollable, and
/// an agent can park a second permission while the first is still unanswered —
/// so a press that meant "the pending one" would, on the day that happens,
/// silently grant one thing while the reader believed they had granted another.
/// A card is named by where it sits in its transcript, which is a position a
/// live transcript only ever appends to.
///
/// Not `Copy`: the picker variant carries a group's name, and a name is the one
/// thing here that is not a number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Press {
    /// Answer the permission at `item` on session `uid` with the option at
    /// `option` in that card's own list.
    Permission {
        uid: u64,
        item: usize,
        option: usize,
    },
    /// Answer the question at `item` by picking the choice at `choice`.
    Question {
        uid: u64,
        item: usize,
        choice: usize,
    },
    /// Let the question at `item` go unanswered and let the turn carry on.
    Skip { uid: u64, item: usize },
    /// Set the picker `group` on session `uid` to its `choice`-th value.
    ///
    /// The group travels by **name** while everything else here travels by
    /// position, and the difference is what each one is. A card is frozen once
    /// it is raised, so a place in it cannot move; what the agent offers is live
    /// and its groups come and go, so a place among *them* is worth nothing by
    /// the time it comes back. A name survives that. The choice inside a group
    /// is still a place, because the alternative is an agent-chosen value of any
    /// length in a payload with a hard cap — and unlike a permission, a picker
    /// set to the wrong thing is one more press rather than something granted.
    Option {
        uid: u64,
        group: String,
        choice: usize,
    },
}

impl Press {
    /// The session this press is about. Every variant names one — a press that
    /// did not would have nowhere to land.
    pub fn uid(&self) -> u64 {
        match self {
            Self::Permission { uid, .. }
            | Self::Question { uid, .. }
            | Self::Skip { uid, .. }
            | Self::Option { uid, .. } => *uid,
        }
    }

    /// Which card in that session's transcript, for the presses that name one.
    pub fn item(&self) -> Option<usize> {
        match self {
            Self::Permission { item, .. }
            | Self::Question { item, .. }
            | Self::Skip { item, .. } => Some(*item),
            // A picker is not a card. It is not in the transcript at all, and
            // asking a live list what sits at a frozen position is exactly the
            // confusion this keeps apart.
            Self::Option { .. } => None,
        }
    }

    /// This press as the little string a channel will hand back.
    ///
    /// **Positions, not identifiers.** An option's id is the agent's to choose
    /// and can be anything, including something longer than the whole payload —
    /// so what is carried is where the option sits in the card's own list. That
    /// is safe for exactly the reason a parked card is worth answering at all:
    /// it is frozen from the moment it is raised until the moment it is
    /// answered, so the list cannot have been rebuilt underneath the button.
    pub fn encode(&self) -> String {
        match self {
            Self::Permission { uid, item, option } => format!("p:{uid}:{item}:{option}"),
            Self::Question { uid, item, choice } => format!("q:{uid}:{item}:{choice}"),
            Self::Skip { uid, item } => format!("s:{uid}:{item}"),
            Self::Option { uid, group, choice } => format!("o:{uid}:{group}:{choice}"),
        }
    }

    /// Whether a channel will carry this at all.
    ///
    /// **The only press whose size is not known in advance** is the one carrying
    /// a group name the agent chose, and a payload over the cap is not truncated
    /// by the far side — the whole message is refused, so a notification that
    /// grew one button too long simply never arrives. Asked before a button is
    /// built rather than discovered when nothing shows up.
    pub fn fits(&self) -> bool {
        // What Telegram allows, and the smallest cap of anything likely to
        // follow. One number for every channel: a payload sized to the most
        // generous one is a payload the next channel silently drops.
        const CALLBACK_DATA_MAX: usize = 64;
        // A separator inside the group would come back as a field boundary and
        // decode as something else, or as nothing.
        let clean = !matches!(self, Self::Option { group, .. } if group.contains(':'));
        clean && self.encode().len() <= CALLBACK_DATA_MAX
    }

    /// Read one back. `None` for anything this build does not recognise, which
    /// includes a button drawn by an older version of the app that is still
    /// sitting in somebody's chat history.
    pub fn decode(data: &str) -> Option<Self> {
        let mut parts = data.split(':');
        let kind = parts.next()?;
        let uid: u64 = parts.next()?.parse().ok()?;
        // The third field is a number for every press but the picker's, whose
        // is the group's own name.
        let third = parts.next()?;
        let last = parts.next();
        // Anything after the fourth field means this was not written by this
        // build, and guessing at the rest is how a press lands on the wrong
        // answer.
        if parts.next().is_some() {
            return None;
        }
        match (kind, last) {
            ("p", Some(i)) => Some(Self::Permission {
                uid,
                item: third.parse().ok()?,
                option: i.parse().ok()?,
            }),
            ("q", Some(i)) => Some(Self::Question {
                uid,
                item: third.parse().ok()?,
                choice: i.parse().ok()?,
            }),
            ("s", None) => Some(Self::Skip {
                uid,
                item: third.parse().ok()?,
            }),
            ("o", Some(i)) if !third.is_empty() => Some(Self::Option {
                uid,
                group: third.to_string(),
                choice: i.parse().ok()?,
            }),
            _ => None,
        }
    }
}

/// Which option a press at `index` actually means.
///
/// **Out of range falls to a refusal rather than to nothing.** A press aimed at
/// an option that is not there is a press whose meaning this build cannot
/// establish — an older message, a card rebuilt, an index that never existed —
/// and the safe reading of an answer nobody can read is "no". Falling through to
/// nothing would look safe and is not: it leaves the agent parked with the user
/// believing they have answered.
///
/// The refusal is found by [`PermissionOption::weight`] and not by reading the
/// option's text, so an option kind this build has never met is a refusal too
/// rather than something that merely fails to match "allow".
pub fn option_at(options: &[PermissionOption], index: usize) -> Option<&PermissionOption> {
    options.get(index).or_else(|| {
        options
            .iter()
            .find(|option| option.weight() == PermissionWeight::Deny)
    })
}

/// The buttons a parked permission offers.
///
/// **Grants on one row, refusals on the next**, and the split is read from
/// [`PermissionOption::weight`] rather than from what an option is called. Which
/// of these is the safe answer is a fact about the protocol; a layout that
/// decided it from the text would put "reject" next to "allow always" the day an
/// adapter spells one of them differently, and on a phone those two are a
/// thumb-width apart.
pub fn permission_buttons(uid: u64, item: usize, options: &[PermissionOption]) -> Vec<Vec<Button>> {
    let row = |keep: fn(PermissionWeight) -> bool| -> Vec<Button> {
        options
            .iter()
            .enumerate()
            .filter(|(_, option)| keep(option.weight()))
            .map(|(option_index, option)| Button {
                label: first_line_trunc(&option.name, LABEL_MAX),
                data: Press::Permission {
                    uid,
                    item,
                    option: option_index,
                }
                .encode(),
            })
            .collect()
    };
    [
        row(|w| w != PermissionWeight::Deny),
        row(|w| w == PermissionWeight::Deny),
    ]
    .into_iter()
    .filter(|row| !row.is_empty())
    .collect()
}

/// The buttons a parked question offers, and the reason there may be none.
///
/// Only a form that is **one single-select field** becomes buttons. A form with
/// several questions is answered in an order a row of buttons cannot express, a
/// multi-select needs a press per choice and then a press to say "done", and a
/// free-text field is not a button at all — so each of those is left to the
/// window, where the card that can actually take the answer is.
///
/// One choice per row, because a question's choices are sentences rather than
/// words and two of them side by side are two clipped halves.
///
/// **Skip is always offered**, including when nothing else is. It is the one
/// answer that is always available and always safe: the model is told the user
/// passed and the turn carries on, so a question nobody wants to answer from a
/// phone still stops being a session standing still.
pub fn question_buttons(uid: u64, item: usize, fields: &[ElicitField]) -> Vec<Vec<Button>> {
    let mut rows: Vec<Vec<Button>> = Vec::new();
    if let [ElicitField {
        kind: ElicitKind::Select(choices),
        ..
    }] = fields
    {
        rows.extend(choices.iter().enumerate().map(|(choice, c)| {
            vec![Button {
                label: first_line_trunc(&c.label, LABEL_MAX),
                data: Press::Question { uid, item, choice }.encode(),
            }]
        }));
    }
    rows.push(vec![Button {
        label: "Skip".to_string(),
        data: Press::Skip { uid, item }.encode(),
    }]);
    rows
}

/// The buttons one picker offers, and a mark on the value in force.
///
/// **The current value is marked rather than left out.** A row of models with
/// nothing saying which one is running is a row that has to be pressed to be
/// read, and pressing to find out is how somebody away from the machine changes
/// a setting they only meant to check. It stays pressable, because re-setting
/// what is already set costs nothing and removing it would leave a gap where the
/// eye expects an option.
///
/// A choice whose press would not survive the trip is dropped rather than drawn:
/// a payload over a channel's cap is not truncated, it makes the far side refuse
/// the whole message, so one over-long group would cost the entire listing. What
/// went missing is the caller's to report — this returns the rows and the count
/// it could not offer.
pub fn option_buttons(uid: u64, selector: &Selector) -> (Vec<Vec<Button>>, usize) {
    let mut rows = Vec::new();
    let mut dropped = 0;
    for (choice, value) in selector.choices.iter().enumerate() {
        let press = Press::Option {
            uid,
            group: selector.id.clone(),
            choice,
        };
        if !press.fits() {
            dropped += 1;
            continue;
        }
        let here = selector.current.as_deref() == Some(value.value.as_str());
        rows.push(vec![Button {
            label: first_line_trunc(
                &if here {
                    format!("• {}", value.label)
                } else {
                    value.label.clone()
                },
                LABEL_MAX,
            ),
            data: press.encode(),
        }]);
    }
    (rows, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::ElicitChoice;
    use crate::chat::SelectorChoice;

    /// The cap a channel puts on what it will carry back. Anything longer is
    /// simply refused when the message is sent, so a button that outgrew it
    /// would be a notification that never arrives.
    const CALLBACK_DATA_MAX: usize = 64;

    fn option(name: &str, kind: &str) -> PermissionOption {
        PermissionOption {
            id: format!("id-for-{name}"),
            name: name.into(),
            kind: kind.into(),
        }
    }

    fn select(labels: &[&str]) -> Vec<ElicitField> {
        vec![ElicitField {
            key: "answer".into(),
            title: None,
            description: None,
            kind: ElicitKind::Select(
                labels
                    .iter()
                    .map(|l| ElicitChoice {
                        value: l.to_string(),
                        label: l.to_string(),
                        description: None,
                    })
                    .collect(),
            ),
            custom_key: None,
        }]
    }

    #[test]
    fn a_press_survives_the_round_trip() {
        for press in [
            Press::Permission {
                uid: 1,
                item: 4,
                option: 0,
            },
            Press::Question {
                uid: 42,
                item: 0,
                choice: 7,
            },
            Press::Skip { uid: 3, item: 12 },
            Press::Option {
                uid: 5,
                group: "model".into(),
                choice: 2,
            },
        ] {
            let back = Press::decode(&press.encode());
            assert_eq!(back.as_ref(), Some(&press));
            assert_eq!(back.as_ref().unwrap().uid(), press.uid());
            assert_eq!(back.as_ref().unwrap().item(), press.item());
        }
    }

    /// A picker's group is the one thing here that travels by name, so it is the
    /// one press whose size is not known in advance -- and a payload over the cap
    /// makes the far side refuse the *whole* message, so one over-long group
    /// would cost the entire listing rather than one button.
    #[test]
    fn a_press_that_would_not_survive_the_trip_says_so() {
        let ok = Press::Option {
            uid: 1,
            group: "model".into(),
            choice: 0,
        };
        assert!(ok.fits());

        let long = Press::Option {
            uid: u64::MAX,
            group: "g".repeat(64),
            choice: usize::MAX,
        };
        assert!(!long.fits());
        // And it must not merely be refused at the cap: a separator inside the
        // name comes back as a field boundary and decodes as something else.
        let split = Press::Option {
            uid: 1,
            group: "mo:del".into(),
            choice: 0,
        };
        assert!(!split.fits());
        assert_eq!(Press::decode(&split.encode()), None);

        // Every other press is bounded by its own types, so `fits` is a
        // formality for them and must stay true at the extremes.
        assert!(Press::Permission {
            uid: u64::MAX,
            item: usize::MAX,
            option: usize::MAX
        }
        .fits());
    }

    /// The payload has to fit in what the channel will carry, at the largest
    /// values the types allow rather than at the ones a test happens to pick.
    #[test]
    fn even_the_largest_press_fits_the_payload() {
        for press in [
            Press::Permission {
                uid: u64::MAX,
                item: usize::MAX,
                option: usize::MAX,
            },
            Press::Question {
                uid: u64::MAX,
                item: usize::MAX,
                choice: usize::MAX,
            },
            Press::Skip {
                uid: u64::MAX,
                item: usize::MAX,
            },
        ] {
            assert!(press.encode().len() <= CALLBACK_DATA_MAX, "{press:?}");
        }
    }

    /// A button drawn by another build, or by nothing at all, must not be
    /// guessed at: the guess would land on a real card of a real session.
    #[test]
    fn nonsense_decodes_to_nothing() {
        for data in [
            "",
            ":",
            "p",
            "p:",
            "p:1",
            "p:1:2",
            "s:1",
            "s:1:2:3",
            "p:1:2:3:4",
            "z:1:2:3",
            "p:x:2:3",
            "p:1:x:3",
            "p:1:2:x",
            "p:-1:2:3",
        ] {
            assert_eq!(Press::decode(data), None, "{data:?} must not decode");
        }
    }

    /// The whole point of the fallback: an index that names nothing has to
    /// resolve to a refusal, because a press nobody can read must not be able to
    /// grant anything.
    #[test]
    fn a_press_that_names_nothing_lands_on_a_refusal() {
        let options = [
            option("Allow", "allow_once"),
            option("Always allow", "allow_always"),
            option("Reject", "reject_once"),
        ];
        assert_eq!(option_at(&options, 0).unwrap().name, "Allow");
        assert_eq!(option_at(&options, 1).unwrap().name, "Always allow");
        assert_eq!(option_at(&options, 9).unwrap().name, "Reject");

        // An option kind this build has never met is a refusal too, so it is
        // both a valid fallback and never dressed as a grant.
        let strange = [option("Allow", "allow_once"), option("Ask again", "later")];
        assert_eq!(option_at(&strange, 5).unwrap().name, "Ask again");

        // Nothing to fall back to is the one case with no answer, and saying so
        // is better than picking a grant.
        assert!(option_at(&[option("Allow", "allow_once")], 9).is_none());
        assert!(option_at(&[], 0).is_none());
    }

    /// The layout rule, and the reason it is read from the classification: a
    /// refusal on the same row as `allow always` is a thumb-width from the
    /// widest grant on the card.
    #[test]
    fn grants_and_refusals_are_on_separate_rows() {
        let options = [
            option("Allow", "allow_once"),
            option("Reject", "reject_once"),
            option("Always allow", "allow_always"),
            option("Never", "reject_always"),
        ];
        let rows = permission_buttons(4, 2, &options);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].iter().map(|b| &b.label).collect::<Vec<_>>(),
            ["Allow", "Always allow"]
        );
        assert_eq!(
            rows[1].iter().map(|b| &b.label).collect::<Vec<_>>(),
            ["Reject", "Never"]
        );
    }

    /// A button carries where its option sits in the card's own list, not where
    /// it sits in the row it was laid out on -- the two differ the moment a
    /// refusal comes before a grant. And it carries the card, so a second
    /// permission parked before it is pressed cannot take the answer.
    #[test]
    fn a_button_points_back_at_the_option_it_was_built_from() {
        let options = [
            option("Reject", "reject_once"),
            option("Allow", "allow_once"),
        ];
        let rows = permission_buttons(4, 7, &options);
        let grant = &rows[0][0];
        assert_eq!(grant.label, "Allow");
        assert_eq!(
            Press::decode(&grant.data),
            Some(Press::Permission {
                uid: 4,
                item: 7,
                option: 1,
            })
        );
        assert_eq!(option_at(&options, 1).unwrap().name, "Allow");
    }

    /// A card with only refusals still draws them, and a card with only grants
    /// does not draw an empty second row.
    #[test]
    fn an_empty_row_is_not_drawn() {
        assert_eq!(
            permission_buttons(1, 0, &[option("Allow", "allow_once")]).len(),
            1
        );
        assert_eq!(
            permission_buttons(1, 0, &[option("Reject", "reject_once")]).len(),
            1
        );
        assert!(permission_buttons(1, 0, &[]).is_empty());
    }

    #[test]
    fn a_single_choice_question_becomes_one_button_per_choice_plus_skip() {
        let rows = question_buttons(2, 5, &select(&["Rewrite it", "Leave it"]));
        assert_eq!(rows.len(), 3);
        assert_eq!(
            Press::decode(&rows[0][0].data),
            Some(Press::Question {
                uid: 2,
                item: 5,
                choice: 0,
            })
        );
        assert_eq!(
            Press::decode(&rows[1][0].data),
            Some(Press::Question {
                uid: 2,
                item: 5,
                choice: 1,
            })
        );
        assert_eq!(
            Press::decode(&rows[2][0].data),
            Some(Press::Skip { uid: 2, item: 5 })
        );
    }

    /// Forms a row of buttons cannot express are left to the window -- but skip
    /// is still offered, because an unanswered question is a session standing
    /// still and skipping always unblocks it.
    #[test]
    fn a_form_that_taps_cannot_answer_offers_only_skip() {
        let mut two = select(&["a", "b"]);
        two.push(two[0].clone());
        assert_eq!(question_buttons(2, 0, &two).len(), 1);

        let text = vec![ElicitField {
            key: "why".into(),
            title: None,
            description: None,
            kind: ElicitKind::Text,
            custom_key: None,
        }];
        assert_eq!(question_buttons(2, 0, &text).len(), 1);

        assert_eq!(question_buttons(2, 0, &[]).len(), 1);
        assert_eq!(
            Press::decode(&question_buttons(2, 3, &[])[0][0].data),
            Some(Press::Skip { uid: 2, item: 3 })
        );
    }

    fn selector(current: Option<&str>, labels: &[&str]) -> Selector {
        Selector {
            id: "model".into(),
            name: "Model".into(),
            current: current.map(str::to_string),
            choices: labels
                .iter()
                .map(|l| SelectorChoice {
                    value: l.to_lowercase(),
                    label: l.to_string(),
                })
                .collect(),
        }
    }

    /// A row of models with nothing saying which is running has to be pressed
    /// to be read -- and pressing to find out is how somebody away from the
    /// machine changes a setting they only meant to check.
    #[test]
    fn the_value_in_force_is_marked_and_still_pressable() {
        let (rows, dropped) = option_buttons(7, &selector(Some("sonnet"), &["Opus", "Sonnet"]));
        assert_eq!(dropped, 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].label, "Opus");
        assert_eq!(rows[1][0].label, "• Sonnet");
        // Marked, not removed: a gap where the eye expects an option reads as a
        // missing choice, and re-setting what is set costs nothing.
        assert_eq!(
            Press::decode(&rows[1][0].data),
            Some(Press::Option {
                uid: 7,
                group: "model".into(),
                choice: 1,
            })
        );
    }

    /// Nothing in force yet marks nothing, rather than marking the first.
    #[test]
    fn an_unset_picker_marks_none_of_its_choices() {
        let (rows, _) = option_buttons(7, &selector(None, &["Opus", "Sonnet"]));
        assert!(rows.iter().all(|row| !row[0].label.starts_with('•')));
    }

    /// One over-long group would make the far side refuse the whole message, so
    /// what cannot be carried is dropped here and counted for the caller to
    /// report -- silence would read as a picker with fewer choices than it has.
    #[test]
    fn a_choice_that_cannot_be_carried_is_dropped_and_counted() {
        let mut wide = selector(None, &["Opus", "Sonnet"]);
        wide.id = "g".repeat(64);
        let (rows, dropped) = option_buttons(7, &wide);
        assert!(rows.is_empty());
        assert_eq!(dropped, 2);
    }

    /// A choice is a sentence, and a channel handed one lays out a button the
    /// width of the screen with its important half missing.
    #[test]
    fn a_long_label_is_clipped() {
        let long = "Rewrite the whole module and then run the full test suite again";
        let rows = question_buttons(1, 0, &select(&[long]));
        assert!(rows[0][0].label.chars().count() <= LABEL_MAX + 1);
        assert!(rows[0][0].label.ends_with('…'));
    }
}
