//! One step of the agent's activity as a row: the fixed mark at its head,
//! what it did, the object it did it to, and whether it opens.

use super::*;

/// What the fixed slot at the head of an activity row holds.
///
/// **Four drawings, one box.** The whole point of the slot is that a step going
/// from waiting to running to done swaps what is in it and moves nothing else
/// on the row — so a group of ten rows finishing one at a time is a column of
/// marks changing in place rather than ten lines of text nudging sideways.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowMark {
    Waiting,
    Running,
    Done,
    /// Something in the run failed and the run went on to succeed anyway.
    ///
    /// **Drawn as a settled step.** A run marked red because one of seven
    /// commands exited non-zero is an alarm for something already dealt with,
    /// and a reader who has learned that the red mark usually means nothing has
    /// learned to ignore the one that means something. The count of what went
    /// wrong is still said, in muted words, at the other end of the row.
    Recovered,
    Failed,
    /// A step somebody refused. Not a failure — nothing went wrong, a decision
    /// was taken — so it reads like any settled step and says the rest in
    /// words.
    Refused,
}

impl RowMark {
    /// How a whole run reads: by how it ended, not by its worst moment.
    pub fn of_run(run: onehand_core::chat::RunOutcome) -> Self {
        use onehand_core::chat::Outcome;
        match run.outcome {
            Outcome::Running => Self::Running,
            Outcome::Clean => Self::Done,
            Outcome::Recovered => Self::Recovered,
            Outcome::Failed => Self::Failed,
        }
    }

    pub(in crate::chat::transcript) fn of(status: ToolStatus) -> Self {
        match status {
            ToolStatus::Pending => Self::Waiting,
            ToolStatus::InProgress => Self::Running,
            ToolStatus::Completed => Self::Done,
            ToolStatus::Failed => Self::Failed,
        }
    }

    /// The glyph at the head of a row: the kind of work it was, or a spinner
    /// in the accent while it runs. How a settled step ended is said in words
    /// at the row's end, so the glyph only ever says what the work was.
    fn draw(self, kind: SharedString, cx: &App) -> gpui::AnyElement {
        match self {
            Self::Running => Spinner::new().xsmall().color(accent(cx)).into_any_element(),
            Self::Waiting | Self::Done | Self::Recovered | Self::Failed | Self::Refused => {
                Icon::empty()
                    .path(kind)
                    .xsmall()
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element()
            }
        }
    }
}

/// One row of an activity block, and the one shape every step takes.
///
/// **One anatomy on every row.** Left to right: the arrow's column, the kind
/// of work (a spinner while it runs), what was done, what it was done to, and
/// whatever is worth saying at the end. Adding a kind of activity must not add
/// a column; only what the row opens into differs.
pub(in crate::chat::transcript) struct ActivityRow {
    id: gpui::ElementId,
    mark: RowMark,
    /// The block's drawing for the sort of work.
    kind: SharedString,
    /// What was done, in the reading face: `Read`, `Ran`, `Edited`.
    verb: SharedString,
    /// What it was done to, in the machine face. A path arrives split so the
    /// directory can recede behind the name.
    object: Option<Object>,
    /// Whatever the row is worth saying at its end — a line count, a result
    /// count, an exit code.
    meta: Option<gpui::AnyElement>,
    /// A file that is gone, which is the one state that strikes its own name
    /// out rather than only marking the ends of the row.
    pub(in crate::chat::transcript) struck: bool,
    fold: Option<bool>,
}

/// What a row was pointed at.
///
/// **Split at the last separator, because the two halves are read
/// differently.** A reader scanning a column of paths is looking for the name;
/// the directory above it is only there for the times two names are the same,
/// and at one weight it takes the eye first every time it is long. So the
/// directory recedes a step and the name keeps the reading ink.
#[derive(Clone)]
pub(in crate::chat::transcript) struct Object {
    pub(in crate::chat::transcript) dir: Option<SharedString>,
    pub(in crate::chat::transcript) name: SharedString,
    /// The agent's own wording rather than a path or a command: the UI family
    /// in the middle ink, since mono would claim it is something to type.
    pub(in crate::chat::transcript) words: bool,
}

impl Object {
    /// A path, split; anything else whole.
    pub(in crate::chat::transcript) fn path(value: impl Into<SharedString>) -> Self {
        let value: SharedString = value.into();
        match value.rfind('/') {
            // A command is not a path and must not be cut at its last slash.
            Some(_) if value.contains(char::is_whitespace) => Self::plain(value),
            Some(at) => Self {
                dir: Some(value[..=at].to_string().into()),
                name: value[at + 1..].to_string().into(),
                words: false,
            },
            None => Self::plain(value),
        }
    }

    pub(in crate::chat::transcript) fn plain(value: impl Into<SharedString>) -> Self {
        Self {
            dir: None,
            name: value.into(),
            words: false,
        }
    }

    /// What the agent said, rather than something to type.
    pub(in crate::chat::transcript) fn words(value: impl Into<SharedString>) -> Self {
        Self {
            words: true,
            ..Self::plain(value)
        }
    }
}

impl ActivityRow {
    pub(in crate::chat::transcript) fn new(
        id: gpui::ElementId,
        mark: RowMark,
        kind: SharedString,
        verb: impl Into<SharedString>,
    ) -> Self {
        Self {
            id,
            mark,
            kind,
            verb: verb.into(),
            object: None,
            meta: None,
            struck: false,
            fold: None,
        }
    }

    pub(in crate::chat::transcript) fn object(mut self, object: Option<Object>) -> Self {
        self.object = object;
        self
    }

    pub(in crate::chat::transcript) fn meta(mut self, meta: Option<gpui::AnyElement>) -> Self {
        self.meta = meta;
        self
    }

    pub(in crate::chat::transcript) fn fold(mut self, fold: Option<bool>) -> Self {
        self.fold = fold;
        self
    }
}

pub(in crate::chat::transcript) fn activity_row(
    row: ActivityRow,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let interactive = row.fold.is_some();
    let id = row.id.clone();
    // **The layout and the ink sit on the row itself, not on a box inside it.**
    // A hover styles the element whose hitbox the pointer is over, and text
    // colour cascades *down* -- so a wrapper that hovers over a child which has
    // already set its own colour changes nothing. Everything that should lift
    // therefore inherits, and the one thing that should not says so.
    fn dress<E>(row_div: E, row: ActivityRow, cx: &App) -> E
    where
        E: Styled + ParentElement,
    {
        let muted = cx.theme().muted_foreground;
        row_div
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .text_size(TEXT_SM)
            .text_color(muted)
            .child(chevron_slot(row.fold))
            .child(row.mark.draw(row.kind, cx))
            .child(div().flex_none().whitespace_nowrap().child(row.verb))
            .children(row.object.map(|object| {
                div()
                    .min_w_0()
                    .h_flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .when(!object.words, |o| {
                        o.font_family(cx.theme().mono_font_family.clone())
                    })
                    .text_color(match (row.struck, object.words) {
                        (true, _) => muted,
                        (false, true) => crate::theme::meta_ink(cx),
                        (false, false) => cx.theme().foreground,
                    })
                    .when(row.struck, |o| o.line_through())
                    .children(object.dir.map(|dir| {
                        div()
                            .flex_none()
                            .text_color(crate::theme::meta_ink(cx))
                            .child(dir)
                    }))
                    .child(div().min_w_0().truncate().child(object.name))
            }))
            .children(row.meta)
    }

    match interactive {
        // **Ink, and no plate**: a fill behind a row is the row answering as a
        // surface, and these are lines of a list rather than things on it.
        true => {
            let lit = cx.theme().foreground;
            div()
                .id(id)
                .cursor_pointer()
                .hover(move |row| row.text_color(lit))
                .on_click(on_click)
                .map(|row_div| dress(row_div, row, cx))
                .into_any_element()
        }
        // Nothing to open, so nothing to press: a pointer on a row that does
        // not answer is a promise the row cannot keep.
        false => dress(div(), row, cx).into_any_element(),
    }
}
