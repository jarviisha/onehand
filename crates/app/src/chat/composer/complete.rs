use super::popup::Runs;
use super::presentation::{
    Act, Pick, Row, add_rows, branch_rows, effort_action, effort_rows, fast_action, fast_rows,
    fast_toggle, mode_action, mode_rows, options_action, options_rows,
};
use super::{Composer, Overlay, highlight};
use crate::chat::session::ChatSession;
use gpui::{App, Context, Entity, SharedString, Window};
use gpui_component::IconName;
use onehand_core::completion::{self, TriggerKind};

/// How far back through the transcript the `@` list looks for paths this
/// session has touched.
///
/// **A bound on the walk and not on the list.** How many of them are *drawn* is
/// decided after the query has narrowed them, where the count of what was held
/// back can be reported; cut to the display size here instead, a query aimed at
/// something older found nothing in that group and nothing anywhere said a
/// bound had bitten. This number only stops the walk growing with a
/// conversation that has run all day.
const MAX_ARTIFACT_SCAN: usize = 200;
/// Rows drawn in the completion popup. The list scrolls past this; the cap is
/// what keeps a 10 000-file repo from building 10 000 elements (bounded rendering).
const MAX_COMPLETION_ROWS: usize = 50;

/// The composer's own controls, as rows of the `/` list.
///
/// **Offered only where they would do something.** A settings list an agent
/// never advertised opens onto nothing, and a row that opens nothing is worse
/// than a missing row: it is a name the user now believes in. The three
/// pickers are gated on the same answer their chips are — `picker_rows` — so a
/// control that is not on the row is not in the list either.
///
/// **A group of their own, after the agent's.** `/` has meant "a command the
/// agent offers" everywhere a user has met it before, and it still leads;
/// these are underneath, named for what they act on. Two rows can share a name
/// — an agent is free to advertise `model` too — and that is answered by the
/// heading over each rather than by hiding one, because which of them somebody
/// means is a thing only they know.
fn act_rows(query: &str, session: &Entity<ChatSession>, cx: &App) -> Vec<Row> {
    // **The question the chips ask, and not the one that looks like it.** This
    // asked `picker_rows(..).is_some()`, which is `Some` for all three pickers
    // unconditionally — the `None` arms are the completion list and the
    // attachment tray. So every row was offered whatever the agent advertised,
    // and taking one against a setting that does not exist deleted the typed
    // text, set an overlay with nothing to draw, and left the composer's key
    // context claimed: a popup that is not on screen still holding the arrows
    // until Esc.
    //
    // `toggle_picker` is guarded the same way and so was no help. These three
    // are the answers the chips themselves are drawn on.
    let offered = |act: Act| match act {
        Act::Options => options_action(session, cx).is_some(),
        Act::Effort => effort_action(session, cx).is_some(),
        Act::Mode => mode_action(session, cx).is_some(),
        Act::Fast => fast_action(session, cx).is_some(),
        Act::Attach | Act::Mention | Act::Command | Act::Workflow => true,
    };
    // Hoisted, for the reason the folder run hoists its own: inside the filter
    // it is an allocation per row, on a list rebuilt at every keystroke.
    let q = query.to_lowercase();
    [
        (
            "model",
            "Choose the model and the agent's other settings",
            Act::Options,
        ),
        ("effort", "Choose how hard the model thinks", Act::Effort),
        ("mode", "Switch the session mode", Act::Mode),
        ("fast", "Turn fast mode on or off", Act::Fast),
        ("attach", "Pick files to send with the prompt", Act::Attach),
        (
            "mention",
            "Put an @ in the prompt to name a file",
            Act::Mention,
        ),
    ]
    .into_iter()
    .filter(|(name, _, act)| offered(*act) && (q.is_empty() || name.to_lowercase().contains(&q)))
    .map(|(name, about, act)| Row {
        label: SharedString::from(name),
        detail: Some(SharedString::from(about)),
        pick: Pick::Act(act),
        label_span: completion::span(name, query),
        ..Row::default()
    })
    .collect()
}

/// The rows of a settings list, and `None` for an overlay that is not one.
///
/// **One answer, because three places ask it.** Opening a list seeds the
/// highlight from its rows, walking one is bounded by how many it has, and
/// drawing it needs the rows themselves — written out per site, the three had
/// already begun to differ in which overlays they recognised, and a list whose
/// count comes from one rule and whose rows come from another highlights a row
/// that is not there. The match is exhaustive on purpose: a fourth overlay is a
/// decision about all three at once, so it should not compile until it is made.
pub(super) fn picker_rows(
    overlay: &Overlay,
    session: &Entity<ChatSession>,
    cx: &App,
) -> Option<Vec<Row>> {
    match overlay {
        Overlay::Mode => Some(mode_rows(session, cx)),
        Overlay::Options => Some(options_rows(session, cx)),
        Overlay::Effort => Some(effort_rows(session, cx)),
        Overlay::Fast => Some(fast_rows(session, cx)),
        Overlay::Add => Some(add_rows()),
        Overlay::Branch => Some(branch_rows()),
        Overlay::Completion | Overlay::Attachments => None,
    }
}

/// Where a trigger typed from the toolbar belongs in the buffer.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum TriggerSpot {
    /// Put the character in at this byte offset.
    Insert(usize),
    /// One is already there; only the caret moves, to this offset.
    Reuse(usize),
}

/// Decide where the toolbar's `@` or `/` goes.
///
/// A **mention** is positional: it names a file at the point in the sentence
/// where it is written, so it goes at the caret.
///
/// A **command** is not. A prompt is one message and the adapter reads a
/// command off the front of it, which is why a `/` anywhere else is not a
/// trigger at all -- `src/main.rs` and `and/or` must stay prose. So the
/// toolbar's slash goes to the **front of the buffer** wherever the caret was:
/// pressed with a sentence already typed, it used to drop a slash in the middle
/// of it, open nothing, and leave the user with a stray character and no way to
/// tell what had gone wrong. At the front it opens the list, and whatever was
/// already written stays put as the command's argument.
///
/// A buffer already beginning with `/` gets no second one -- the caret just
/// moves in behind it, which is where the query is typed.
pub(super) fn trigger_spot(ch: char, text: &str, caret: usize) -> TriggerSpot {
    if ch != '/' {
        return TriggerSpot::Insert(caret.min(text.len()));
    }
    match text.starts_with('/') {
        true => TriggerSpot::Reuse(1),
        false => TriggerSpot::Insert(0),
    }
}

impl Composer {
    /// Do what a row was for, whether it was clicked or committed with Enter.
    pub(super) fn apply_pick(
        &mut self,
        pick: &Pick,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Whatever the pick was, the caret comes back to the prompt: a click
        // lands on a plain div, which lets the pane take focus, and the next
        // thing the user does is keep writing the message.
        self.state.update(cx, |state, cx| state.focus(window, cx));
        match pick {
            Pick::Complete(_) => self.accept(session, window, cx),
            Pick::Act(act) => {
                let act = *act;
                // **The words that reached the control come back out first.**
                // `/model` is a way to a picker and not a message, so leaving
                // it in the field would make it the opening of whatever the
                // user types next — and they would have to notice and delete
                // it, having never meant to write it.
                //
                // Done here rather than left to the change event, for the
                // reason `accept` does the same: the trigger and the overlay
                // are settled outright instead of being trusted to arrive in
                // an order this depends on.
                // Only a completion holds words that reached the control: the
                // `+` menu opens over a field that may hold a trigger the user
                // is still typing.
                let completing = self.overlay == Some(Overlay::Completion);
                if let Some(trigger) = self.trigger.clone().filter(|_| completing) {
                    let text = self.text(cx);
                    let caret = self.state.read(cx).cursor();
                    let (next, at) = completion::remove(&text, caret, &trigger);
                    self.state.update(cx, |state, cx| {
                        state.set_value(next, window, cx);
                        state.set_selected_range(at..at, cx);
                    });
                }
                self.trigger = None;
                self.set_overlay(None);
                self.selected = 0;
                match (act.opens(), act) {
                    // A fast group that reads as a switch is flipped by its
                    // name, as its chip flips it, rather than opened.
                    (_, Act::Fast) if self.flip_fast(session, cx) => {}
                    (Some(overlay), _) => self.toggle_picker(overlay, session, window, cx),
                    (None, Act::Attach) => self.attach(cx),
                    // Typed from code rather than inserted as text, so it goes
                    // through the same path a keyboard that cannot produce the
                    // character needs.
                    (None, Act::Command) => self.insert_trigger('/', window, cx),
                    // Once the popup has gone, so the action starts from the
                    // focus it hands back, inside the window's shell.
                    (None, Act::Workflow) => window.defer(cx, |window, cx| {
                        window.dispatch_action(Box::new(crate::shell::RunWorkflow), cx)
                    }),
                    (None, Act::Mention) => self.insert_trigger('@', window, cx),
                    // Every picker opens an overlay, so these never land here.
                    (None, Act::Options | Act::Effort | Act::Mode | Act::Fast) => {}
                }
                true
            }
            Pick::Branch(act) => {
                cx.emit(super::ComposerEvent::Branch(*act));
                self.close_overlay(cx);
                true
            }
            Pick::Mode(id) => {
                let id = id.clone();
                session.update(cx, |session, cx| {
                    session.chat.set_mode(&id);
                    cx.notify();
                });
                self.close_overlay(cx);
                true
            }
            Pick::Config { config_id, value } => {
                let (config_id, value) = (config_id.clone(), value.clone());
                session.update(cx, |session, cx| {
                    session.chat.set_config_option(&config_id, &value);
                    cx.notify();
                });
                self.close_overlay(cx);
                true
            }
        }
    }

    /// Open one of the two option lists, or close it if it is already open.
    ///
    /// **Focus goes to the prompt field, not to the chip.** The chip is a plain
    /// div, so the click that opened the list travels up to the pane, which
    /// takes focus -- and the keys that walk the list only reach it while focus
    /// is in an input inside the composer. Without this the arrows did nothing
    /// at all on a list opened with the mouse, which is every list of the
    /// agent's own options.
    ///
    /// The list opens on the first value already in force. The Options popup is
    /// intentionally flat: model, effort and remaining agent options are all
    /// visible and directly selectable without entering another screen.
    pub(super) fn toggle_picker(
        &mut self,
        target: Overlay,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Not `is_some`: `picker_rows` answers `Some` for every picker, so the
        // guard that was meant to refuse an overlay with nothing in it refused
        // only the two overlays that are not pickers at all.
        let Some(rows) = picker_rows(&target, session, cx).filter(|rows| !rows.is_empty()) else {
            return;
        };
        self.selected = rows.iter().position(|row| row.checked).unwrap_or(0);
        self.set_overlay((self.overlay.as_ref() != Some(&target)).then_some(target));
        self.reveal_selected();
        self.state.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// Flip fast mode, where the agent's group reads as a switch. Whether it did.
    pub(super) fn flip_fast(
        &mut self,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(toggle) = fast_toggle(session, cx) else {
            return false;
        };
        session.update(cx, |session, cx| {
            session
                .chat
                .set_config_option(&toggle.config_id, &toggle.flip_to);
            cx.notify();
        });
        cx.notify();
        true
    }

    /// Recompute the active trigger after an edit.
    pub(super) fn retrigger(&mut self, text: &str, caret: usize, cx: &mut Context<Self>) {
        let next = completion::detect(text, caret);
        // The selection belongs to a candidate list, so it only survives while
        // the trigger it was made against does -- *including* the query, since
        // one more typed letter refilters the list and leaves the same index
        // pointing at a different file.
        if next != self.trigger {
            self.selected = 0;
            // A list opening on top of another one's scroll offset shows its
            // middle, with the highlighted first row above the fold.
            self.reveal_selected();
        }
        self.trigger = next;
        // Typing dismisses a selector: the user has moved on to the prompt.
        self.set_overlay(self.trigger.is_some().then_some(Overlay::Completion));
        cx.notify();
    }

    /// How tall this list stands while it is open, in rows and in headings.
    ///
    /// **Measured against an empty query, not against what is typed.** The
    /// height belongs to the *list* — which pool it is drawn from, how many
    /// groups it has — and not to the query, which changes on every keystroke.
    /// Taken from what is on screen instead, the popup was stable in one
    /// direction and not the other: narrowing held, because the floor had
    /// already been set higher, while deleting a character broadened the list
    /// and it grew. Growing is the common one, and it moves every row under the
    /// hand that is aiming at one.
    ///
    /// Against the full list the floor can never be exceeded, so the floor is
    /// the height: filtered rows are a subset of unfiltered ones. A short list
    /// keeps its own height rather than being padded to the cap — a three
    /// command agent gets a three row popup, and it is the same three rows
    /// high whatever is typed into it.
    pub(super) fn shape(&self, session: &Entity<ChatSession>, cx: &App) -> (usize, usize) {
        let (all, _) = self.matches_for("", session, cx);
        (
            all.len(),
            all.iter().filter(|row| row.group.is_some()).count(),
        )
    }

    /// The rows the `@` or `/` list draws, and how many matches were held back.
    ///
    /// **One builder, read by three callers** — the drawing, the arrow keys'
    /// bound, and Enter. They have to agree exactly: a row count taken from a
    /// second filter is a walk that can run past the end of the list on screen,
    /// and an insert value looked up by index in a third is the wrong file
    /// accepted. What a row *says* and what it *inserts* stopped being the same
    /// string once a mention led with its filename, so the insert rides on the
    /// row rather than being derivable from it.
    ///
    /// The held-back count is returned because a capped list has to say so. Cut
    /// with nothing admitting it, a query that matched four hundred files reads
    /// as one that matched fifty, and the file the user is looking for is
    /// missing for no visible reason.
    pub(super) fn matches(&self, session: &Entity<ChatSession>, cx: &App) -> (Vec<Row>, usize) {
        let query = self
            .trigger
            .as_ref()
            .map(|t| t.query.as_str())
            .unwrap_or("");
        self.matches_for(query, session, cx)
    }

    /// The same list against a query that is not necessarily the one typed.
    ///
    /// The override exists for one caller: the popup asks what this list holds
    /// with **nothing** typed, because that is the height it should stand at
    /// for as long as it is open. See [`Self::shape`].
    fn matches_for(
        &self,
        query: &str,
        session: &Entity<ChatSession>,
        cx: &App,
    ) -> (Vec<Row>, usize) {
        let Some(trigger) = &self.trigger else {
            return (Vec::new(), 0);
        };
        let chat = &session.read(cx).chat;
        match trigger.kind {
            TriggerKind::File => {
                let artifacts = chat.artifacts(MAX_ARTIFACT_SCAN);
                let (found, held) = completion::mentions(
                    &chat.files,
                    &chat.folders,
                    &artifacts,
                    query,
                    MAX_COMPLETION_ROWS,
                );
                // The heading is carried by the first row of each run rather
                // than by a row of its own, so the index the arrows walk stays
                // made entirely of things that can be taken.
                let mut runs = Runs::default();
                let rows = found
                    .into_iter()
                    .map(|m| {
                        let heading = runs.opening(match m.kind {
                            completion::MentionKind::File => "Files",
                            completion::MentionKind::Folder => "Folders",
                            completion::MentionKind::Artifact => "This session",
                        });
                        Row {
                            label: SharedString::from(m.name),
                            // The folder's weight rides with its parent in the
                            // one trailing column, because they answer the same
                            // question — which of the several folders with this
                            // name, and how much am I about to hand over.
                            // Read before the two are consumed below: whether
                            // a folder's weight went in front of the parent is
                            // what decides if a span into that parent still
                            // points at the characters it was found in.
                            detail_span: m.parent_span.clone().filter(|_| m.note.is_none()),
                            detail: match (m.parent, m.note) {
                                (Some(parent), Some(note)) => {
                                    Some(SharedString::from(format!("{note} · {parent}")))
                                }
                                (parent, note) => parent.or(note).map(SharedString::from),
                            },
                            pick: Pick::Complete(SharedString::from(m.insert)),
                            // Two glyphs for three groups, and deliberately:
                            // an artifact *is* a file, and the one distinction
                            // the icon has to carry is the folder — accepting
                            // one inserts a listing where the reader was
                            // expecting a file to be read. That this file is
                            // also a recent one is said by the heading over it,
                            // which is where a fact about a whole run of rows
                            // belongs. A third shape invented for it would be
                            // one chosen for a category rather than for a thing.
                            mark: Some(gpui_component::Icon::new(match m.kind {
                                completion::MentionKind::Folder => IconName::Folder,
                                _ => IconName::File,
                            })),
                            label_span: m.name_span,
                            group: heading,
                            checked: false,
                        }
                    })
                    .collect();
                (rows, held)
            }
            TriggerKind::Command => {
                let (found, held) =
                    completion::commands(&chat.commands, query, MAX_COMPLETION_ROWS);
                // The heading opens each run, as it does in the mention list,
                // and for the same reason: hung off the first row of the run,
                // the index the arrows walk stays made entirely of commands.
                //
                // The agent's own commands are the run with no namespace, and
                // they lead. Their heading names the agent rather than saying
                // "Commands", which every row under it is — two runs both
                // labelled by what they contain would be one label repeated.
                // **The composer's own controls open the list.** They were at
                // the foot of it, on the reasoning that `/` has meant "a
                // command the agent offers" everywhere a user has met it
                // before. What that reasoning missed is who is reading: the
                // agent's list is long, arrives over the wire and changes
                // between agents, while this one is five rows that are always
                // the same and are the only way to reach a control without
                // knowing which chip it sits behind. A fixed short run is
                // something a hand learns the position of; put under a list of
                // unknown length it is somewhere different every time.
                let mut rows: Vec<Row> = Vec::new();
                let mut own = act_rows(query, session, cx).into_iter();
                if let Some(first) = own.next() {
                    rows.push(Row {
                        group: Some("Composer".into()),
                        ..first
                    });
                    rows.extend(own);
                }
                // The heading opens each run, as it does in the mention list,
                // and for the same reason: hung off the first row of the run,
                // the index the arrows walk stays made entirely of commands.
                //
                // The agent's own commands are the run with no namespace, and
                // they lead the rest. Their heading names the agent rather than
                // saying "Commands", which every row under it is — two runs
                // both labelled by what they contain would be one label
                // repeated.
                let mut runs = Runs::default();
                rows.extend(found.into_iter().map(|c| {
                    let heading = runs.opening(c.namespace.as_deref().unwrap_or("Built in"));
                    Row {
                        label: SharedString::from(c.name),
                        detail: c.summary.map(SharedString::from),
                        pick: Pick::Complete(SharedString::from(c.insert)),
                        label_span: c.name_span,
                        group: heading,
                        ..Row::default()
                    }
                }));
                (rows, held)
            }
        }
    }

    /// How many rows the open list has, which is what walking it is bounded by.
    fn row_count(&self, session: &Entity<ChatSession>, cx: &App) -> usize {
        match &self.overlay {
            None => 0,
            Some(Overlay::Completion) => self.matches(session, cx).0.len(),
            // The tray draws its own list and the arrows do not walk it.
            Some(Overlay::Attachments) => 0,
            Some(picker) => picker_rows(picker, session, cx).map_or(0, |rows| rows.len()),
        }
    }

    /// Move the highlight by one row, wrapping at both ends.
    ///
    /// Wrapping because the list is short and the keys are held: stopping dead
    /// at the last row makes the user let go and reach for the other arrow to
    /// get back to a match they passed.
    pub(super) fn step(
        &mut self,
        delta: isize,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) {
        let rows = self.row_count(session, cx);
        let Some(from) = highlight(self.selected, rows) else {
            return;
        };
        let rows = rows as isize;
        self.selected = ((from as isize + delta).rem_euclid(rows)) as usize;
        self.reveal_selected();
        cx.notify();
    }

    /// Accept the highlighted candidate, rewriting the buffer around the
    /// trigger. Returns whether anything was accepted.
    pub fn accept(
        &mut self,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let rows = self.matches(session, cx).0;
        // The insert comes off the row rather than from its label: a mention
        // row reads as a filename and inserts a whole path, and a folder row
        // inserts a trailing slash that appears nowhere in what it says.
        let Some(Pick::Complete(choice)) = highlight(self.selected, rows.len())
            .and_then(|row| rows.into_iter().nth(row))
            .map(|row| row.pick)
        else {
            return false;
        };
        let Some(trigger) = self.trigger.clone() else {
            return false;
        };

        let text = self.text(cx);
        let caret = self.state.read(cx).cursor();
        let (next, next_caret) = completion::apply(&text, caret, &trigger, &choice);

        self.state.update(cx, |state, cx| {
            state.set_value(next, window, cx);
            // An empty range is a caret: `completion::apply` returns where the
            // caret belongs after the rewrite, which is past the inserted
            // value, not at the end of the buffer.
            state.set_selected_range(next_caret..next_caret, cx);
        });
        self.trigger = None;
        self.set_overlay(None);
        self.selected = 0;
        cx.notify();
        true
    }

    /// Type a trigger character into the buffer from code.
    ///
    /// This exists because of an input-method bug, not as a convenience. With a
    /// Vietnamese IME active on Linux, a typed `/` can be swallowed before it
    /// ever reaches the composer — so the slash-command popup cannot be opened
    /// by typing at all, and `@` is unreliable for the same reason. Inserting
    /// the character here bypasses the IME entirely.
    ///
    /// `retrigger` is called explicitly rather than trusting the `Change` event
    /// to arrive: the whole point is to open the popup, and a button that
    /// inserts an `@` without opening anything is the bug wearing a different
    /// hat.
    pub(super) fn insert_trigger(&mut self, ch: char, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let caret = self.state.read(cx).cursor().min(text.len());
        let (next, after) = match trigger_spot(ch, &text, caret) {
            TriggerSpot::Reuse(after) => (text.clone(), after),
            TriggerSpot::Insert(at) => {
                let mut next = String::with_capacity(text.len() + ch.len_utf8());
                next.push_str(&text[..at]);
                next.push(ch);
                next.push_str(&text[at..]);
                (next, at + ch.len_utf8())
            }
        };

        self.state.update(cx, |state, cx| {
            state.set_value(next.clone(), window, cx);
            state.set_selected_range(after..after, cx);
            // Focus goes back to the buffer: the click that inserted the
            // trigger took it, and the next thing the user does is type the
            // query after it.
            state.focus(window, cx);
        });
        self.retrigger(&next, after, cx);
    }

    /// Put the highlight on a row.
    ///
    /// Guarded, because the pointer moving across a row calls this on every
    /// mouse event it generates and an unguarded notify would redraw the panel
    /// for each one.
    pub(super) fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.selected == row {
            return;
        }
        self.selected = row;
        cx.notify();
    }
}
