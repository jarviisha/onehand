//! What a button on an outgoing message means, and how it survives the trip
//! there and back.
//!
//! **Everything a press has to carry rides in the button's own data**, because
//! there is nowhere else to put it: a channel hands back what it was given and
//! nothing more, and the app has no idea which of its windows the press is
//! about until it reads this. That data is small — a channel is free to cap it,
//! and Telegram caps it at 64 bytes — so what goes in it is a letter, a session
//! id and an index, and never an identifier the agent chose.

use super::types::Button;
use crate::acp::{ElicitField, ElicitKind, PermissionOption, PermissionWeight};
use crate::chat::first_line_trunc;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Press {
    /// The session this press is about. Every variant names one — a press that
    /// did not would have nowhere to land.
    pub fn uid(self) -> u64 {
        match self {
            Self::Permission { uid, .. } | Self::Question { uid, .. } | Self::Skip { uid, .. } => {
                uid
            }
        }
    }

    /// Which card in that session's transcript.
    pub fn item(self) -> usize {
        match self {
            Self::Permission { item, .. }
            | Self::Question { item, .. }
            | Self::Skip { item, .. } => item,
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
    pub fn encode(self) -> String {
        match self {
            Self::Permission { uid, item, option } => format!("p:{uid}:{item}:{option}"),
            Self::Question { uid, item, choice } => format!("q:{uid}:{item}:{choice}"),
            Self::Skip { uid, item } => format!("s:{uid}:{item}"),
        }
    }

    /// Read one back. `None` for anything this build does not recognise, which
    /// includes a button drawn by an older version of the app that is still
    /// sitting in somebody's chat history.
    pub fn decode(data: &str) -> Option<Self> {
        let mut parts = data.split(':');
        let kind = parts.next()?;
        let uid: u64 = parts.next()?.parse().ok()?;
        let item: usize = parts.next()?.parse().ok()?;
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
                item,
                option: i.parse().ok()?,
            }),
            ("q", Some(i)) => Some(Self::Question {
                uid,
                item,
                choice: i.parse().ok()?,
            }),
            ("s", None) => Some(Self::Skip { uid, item }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::ElicitChoice;

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
        ] {
            assert_eq!(Press::decode(&press.encode()), Some(press));
            assert_eq!(Press::decode(&press.encode()).unwrap().uid(), press.uid());
            assert_eq!(Press::decode(&press.encode()).unwrap().item(), press.item());
        }
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
