use super::{ChatPane, ChatPaneEvent, ProjectAction, ProjectFacts, rel_time};
use crate::chat::conversation::{Conversation, SessionPhase};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Rems, SharedString, Styled,
    Window, div, rems,
};
use gpui_component::WindowExt as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::chat::{Chat, ConvMeta};
use std::path::PathBuf;

/// The conversation header, which is the one row in the panel that never
/// scrolls and so the edge every other measurement here is taken from.
const HEADER_H: Rems = rems(2.75);
/// How little the header will settle for before it stops taking room from the
/// conversation's name.
///
/// **It bounds the name together with its menu mark**, which is the box the
/// two share: the mark never shrinks, so what the name itself is left with is
/// this figure less the mark and the gap before it. Stated because the number
/// is the one somebody tunes to get a given amount of name.
///
/// The controls at the other end are icon buttons at a fixed size and nothing
/// asks them to shrink, so before this floor existed the name was the only
/// thing in the row that could give way, and it gave way all of it: a panel
/// dragged narrow left six icons and an ellipsis where the conversation used to
/// be named. Below this the row is simply
/// narrower than its own furniture and the controls clip again, which is the
/// trade taken on purpose: a name cut to two characters names nothing, while a
/// panel this narrow has already stopped being a place a conversation is read.
const HEADER_NAME_MIN: Rems = rems(8.);

/// The conversations already had in the project on screen, for the header's
/// *Open a past conversation* menu.
///
/// **Held rather than read when the menu opens.** Building a menu happens inside
/// a render, and a render cannot wait on a directory of files; a menu that came
/// up empty and filled itself in afterwards would be one the user had already
/// closed and drawn their conclusion from.
pub(super) struct Archives {
    /// The project these belong to. An answer that lands after the pane has
    /// moved on names a project nobody is looking at, and adopting it would
    /// offer one project's conversations under another's name.
    pub(super) root: PathBuf,
    /// `None` while the read is out — which is a different thing from a project
    /// that has never been prompted, and the menu says the two differently.
    pub(super) found: Option<Vec<ConvMeta>>,
}

impl ChatPane {
    /// Go back to choosing which past conversation this session should run.
    ///
    /// The live one has its name and settings written on the way out: the
    /// transcript is written at the end of every turn, so an idle conversation
    /// is already on disk, but the title and the selector picks are metadata
    /// and leaving without writing them would lose them.
    ///
    /// The adapter stays up until a choice is made. Dropping it here would gain
    /// nothing -- the choice is what decides which conversation to connect to,
    /// and `connect` drops it before spawning the replacement anyway.
    fn show_history(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.active else {
            return;
        };
        let Some(conv) = self.conversations.get(&uid) else {
            return;
        };
        if let Some(session) = conv.session() {
            Self::archive_meta_detached(uid, &session.clone(), cx);
        }
        let (root, agent) = (conv.root.clone(), conv.spec.name.clone());
        cx.spawn(async move |pane, cx| {
            let past = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::chat::list_conversations(
                        &onehand_core::chat::conversations_dir(),
                        &root,
                        Some(&agent),
                    )
                })
                .await;
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // An empty list is not a picker with nothing in it -- it is a
                // session that has never been anywhere else, and putting up a
                // page whose only option is the one already on screen would be
                // a dead end.
                if !past.is_empty() {
                    pane.set_phase(uid, SessionPhase::ChoosingHistory(past));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Where the pipeline run driving this session stands: its template's
    /// name, then every step in order, the ones behind it checked off and the
    /// one it is at in full ink, and *Stop* at the far end. `None` for a
    /// session no run drives.
    ///
    /// The transcript says each step as it starts, but a line scrolled past is
    /// not an answer to "how far along is it", and the steps are few enough to
    /// be read in one glance. A run waiting for approval is approved here, in
    /// its own session, so *Revise…* and *Continue* come before *Stop*.
    pub(super) fn pipeline_strip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let uid = self.active?;
        let shown = crate::pipeline::shown(uid, cx)?;
        let muted = cx.theme().muted_foreground;
        let foreground = cx.theme().foreground;
        let at_step = shown.at;
        let steps = shown.steps.into_iter().enumerate().map(move |(i, label)| {
            div()
                .h_flex()
                .flex_none()
                .items_center()
                .gap_1()
                .when(i > 0, |row| {
                    row.child(Icon::new(IconName::ChevronRight).size_3().text_color(muted))
                })
                .when(i < at_step, |row| {
                    row.child(Icon::new(IconName::CircleCheck).size_3().text_color(muted))
                })
                .child(
                    div()
                        .child(label)
                        .text_color(if i == at_step { foreground } else { muted })
                        .when(i == at_step, |label| label.font_semibold()),
                )
        });
        Some(
            div()
                .h_flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_4()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child(div().flex_none().mr_1().child(shown.name))
                .child(
                    div()
                        .h_flex()
                        .min_w_0()
                        .overflow_hidden()
                        .gap_1()
                        .children(steps),
                )
                .child(
                    div()
                        .h_flex()
                        .flex_none()
                        .ml_auto()
                        .gap_1()
                        .when(shown.awaiting, |row| {
                            row.child(
                                crate::controls::action("pipeline-revise")
                                    .xsmall()
                                    .ghost()
                                    .label("Revise…")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        open_revise(uid, window, cx)
                                    })),
                            )
                            .child(
                                crate::controls::action("pipeline-continue")
                                    .xsmall()
                                    .primary()
                                    .icon(Icon::new(IconName::Check))
                                    .label("Continue")
                                    .tooltip("Approve it and go on to the next step")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(ChatPaneEvent::ContinuePipeline(uid))
                                    })),
                            )
                        })
                        .child(
                            crate::controls::action("pipeline-stop")
                                .xsmall()
                                .ghost()
                                .label("Stop")
                                .tooltip("Cancel the turn and end the pipeline here")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.emit(ChatPaneEvent::StopPipeline(uid))
                                })),
                        ),
                ),
        )
    }

    /// The session header: what conversation this is, what it is doing, and the
    /// things you do *to* it.
    ///
    /// Separate from the composer's row because the two answer different
    /// questions. The composer's controls are about the message being written —
    /// what to attach, which mode to send it in, whether to send it at all.
    /// Export, Restart and Close are about the conversation as a whole, and
    /// mixing them into one row of seven buttons made every one of them equally
    /// easy to hit by accident.
    ///
    /// It is also **the only chrome this panel has**. The dock draws the
    /// conversation as a bare panel with no tab bar, so the two ways back to
    /// something the window has put away — the rail and the Workbench — have
    /// nowhere else to be offered from, and a route that exists only as a
    /// keystroke is a route only someone who already knows it can take.
    ///
    /// **The name names the conversation and the vertical-dots mark beside it
    /// carries its menu**, and every other
    /// control sits on the side of what it acts on: the way back to a hidden
    /// rail at the row's left edge, the side the rail returns to, and the
    /// right-hand end reading outward from the name — the past conversations
    /// and closing the session, which act on the session the name names, then
    /// the terminal and last the Workbench, whose dock is the window's right
    /// edge, so the outermost control moves the outermost panel. The dots are
    /// the row's one menu mark, and everything behind them is something done
    /// to the conversation the name beside them is.
    pub(super) fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let chat = self.active_chat(cx);
        let title = chat.and_then(Chat::conversation_title).unwrap_or_else(|| {
            match (&self.workspace, &self.empty) {
                (Some(_), _) => "Workspace".to_string(),
                (None, Some(project)) => project.label.to_string(),
                (None, None) => String::new(),
            }
        });
        let busy = chat.is_some_and(|chat| chat.busy);
        // A conversation the agent has not named yet has no directory to remove:
        // nothing is written until the first turn ends. The menu says so by
        // refusing rather than by hiding the entry, which would make the whole
        // menu change shape between one turn and the next.
        let archive = chat
            .and_then(|chat| chat.session_id.as_deref())
            .map(|sid| onehand_core::chat::conv_dir(&onehand_core::chat::conversations_dir(), sid));
        // The title is a menu only where there *is* a conversation. Standing on
        // a project with no session the same line names the project, and every
        // entry behind it would be about something that does not exist yet.
        let live = chat.is_some();

        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            // **A fixed height, not one the tallest control happens to make.**
            // It is the one row that never scrolls, so it is the edge every
            // other measurement in the panel is taken from -- and sized by its
            // contents it moved whenever a badge appeared or a title wrapped,
            // taking the top of the conversation with it.
            .flex_none()
            .h(HEADER_H)
            .px_4()
            // **No rule under it.** A hairline is an edge between two surfaces,
            // and there are not two here: the header and the transcript are one
            // reading surface, and what separates them is that one is a row of
            // controls and the other is prose -- which the muted ink and the
            // spacing already say. The panels either side of this one draw
            // their own edges and nothing else does, so a line across the top of
            // the conversation was the last one left marking an inside.
            //
            // It was tried, on the reasoning that the list clips at exactly this
            // line and an unmarked cut reads as a rendering fault. What that
            // costs is a rule drawn permanently for a state the reader is only
            // in while scrolling -- and the fade at the other end of the list is
            // the answer that shape of problem actually takes.
            .text_color(cx.theme().muted_foreground)
            // Hiding the rail must not be a one-way door: with it gone there is
            // no workspace name, no project list and no session list, and the
            // way back would be a keystroke the user would have had to already
            // know. So the route rides in the header of the panel that took the
            // space -- and only while the rail is actually gone, because a
            // button that unhides what is already on screen does nothing. It
            // sits at the row's left edge because that is the side the rail
            // comes back on: filed among the window controls on the right it
            // had to be found rather than reached for, a control restoring the
            // left panel from the opposite edge of the row.
            .when(self.rail_hidden, |header| {
                header.child(
                    header_control("show-rail", IconName::PanelLeft, cx)
                        .tooltip("Show the navigation rail")
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::ShowRail);
                        })),
                )
            })
            // **The name gives way before the controls do, and it stops at a
            // floor.** What held the whole row open was the library drawing a
            // button's label in a `flex_none` box with nothing to ellipsize it:
            // the button could shrink and its label could not, so the name kept
            // its full width and what went over the right edge was every control
            // after it, the archive menu through *Close session*, clipped with nothing
            // on screen to say they were there. Drawing the name as a plain
            // truncating child is the whole of the fix, since a name half-read
            // still names the conversation while a button that is not drawn
            // cannot be pressed.
            //
            // The floor is the other half of that, and it was learnt the hard
            // way: with the name as the only thing in the row able to give, it
            // gave all of it, and a narrow panel came out as six icons over an
            // ellipsis. `HEADER_NAME_MIN` is where the taking stops -- and it
            // bounds this box rather than the name alone, so what the name
            // itself keeps is that figure less the menu mark and the gap
            // before it.
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .flex_initial()
                    .min_w(HEADER_NAME_MIN)
                    .child(self.title_control(title, busy, archive, cx)),
            )
            .child(div().flex_1())
            // Only while a session is showing, and for a reason worth stating:
            // this is the same list the project page draws, and that page is
            // exactly what the centre of the window shows when there is no
            // session — offering it there too would be saying one thing twice
            // within an inch of itself.
            .when(live, |header| header.child(self.history_control(cx)))
            // Only while there is a session to end, and *before* the docks
            // rather than last: the cluster reads outward from the name by
            // what each control is about -- this and the past conversations
            // act on the session the name names, the dock pair on the window
            // around it -- and the row's far edge, where a pointer drifts, is
            // the wrong seat for the one control that ends something. It keeps the
            // conversation -- the transcript is written at the end of every turn
            // and closing costs nothing that is not already on disk -- which is
            // why it can be a control on the row while deleting stays behind the
            // name, two presses and a warning away.
            .when(live, |header| {
                header.child(
                    // The exit-door arrow and not an ×: this ends a session
                    // while keeping every word, and every × in the row's
                    // reach says "dismiss this" -- beside two dock toggles it
                    // read as closing a panel, and a power mark read as
                    // quitting the whole app.
                    header_control("close-session", crate::icons::Icon::LogOut, cx)
                        .tooltip("Close this session and its agent")
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::CloseSession);
                        })),
                )
            })
            // Beside the Workbench button: both are docks this panel is
            // sitting between, and a closed one leaves nothing on screen at all
            // -- no edge, no strip, no name -- so the route to it belongs with
            // the panel that took the space. Both are a plain open-or-close and
            // not the three-state rule their keys follow, which is the shell's
            // to apply: a key has one binding and no other way to reach an open
            // panel, while a button can see the dock and is pressed with the
            // caret back in the composer.
            // Neither dock is offered on the workspace page: both hold one
            // project's things, and the page stands on none.
            .when(self.workspace.is_none(), |row| {
                row.child(self.terminal_control(cx))
            })
            // The Workbench closed leaves nothing on screen at all -- no strip,
            // no edge, no name -- so without this the file tree and the editor
            // exist only for someone who remembers two keystrokes. Offered from
            // here rather than done here: which mode it opens on, and closing it
            // rather than focusing it, are both the shell's rules, and the chat
            // has no business knowing a dock is where the Workbench lives.
            // Outermost on the row, always: its dock is the window's right
            // edge, so the control that moves it holds the row's right edge --
            // the same mapping that puts the rail's button at the left.
            .when(self.workspace.is_none(), |row| {
                row.child(
                    header_control("workbench", IconName::PanelRight, cx)
                        // **Both directions, because the button does both.** It
                        // said "Show the Workbench" while it was a three-state
                        // control that could only ever open from here, and kept
                        // saying it after it became a plain toggle -- so the one
                        // press a user most wants named, the one that puts the
                        // panel away, was the press the tooltip denied existed.
                        // Which way it will go this time is not said, since that
                        // needs a dock fact pushed down here and the panel on
                        // screen already answers it.
                        .tooltip("Show or hide the Workbench")
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::ToggleWorkbench);
                        })),
                )
            })
    }

    /// The way back to a conversation this project has already had — as a
    /// session of its own, beside the one on screen.
    ///
    /// **The gap it fills.** Every other route to an archive costs the session
    /// in front of the user. The project page lists them and mints a session on
    /// the one picked, but it is what the centre of the window shows *instead
    /// of* a conversation, so reaching it meant closing every session in the
    /// project first; and the title menu's own picker leaves nothing running —
    /// it takes the session on screen off its conversation to ask the question.
    /// So this is the one that opens an old conversation and keeps the current
    /// one where it is, as a second row in the rail.
    ///
    /// **A menu on a header button rather than a dialog**, because it is a
    /// short list of one project's own conversations and the header is already
    /// where the things about this pane are. A modal over the conversation to
    /// pick a conversation is a heavier gesture than the choice deserves.
    ///
    /// **Every agent's**, as the project page's list is and for the same reason:
    /// the question is which conversation, and which agent had it is a property
    /// of the answer rather than a filter on the question. The title menu's
    /// picker is the narrow one, because there the agent is already decided.
    ///
    /// **A conversation already open is listed and refused, not hidden.** Two
    /// sessions on one archive both believe the transcript so far is on disk, so
    /// the second one's first turn writes the file back holding only what came
    /// after it. Dropping the row instead would leave the one conversation the
    /// user is most likely to look for missing from the list with nothing said.
    fn history_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        // Owned rather than borrowed out of the sessions: the menu is built
        // later, from a closure that outlives this borrow of the pane.
        let open: Vec<String> = self
            .conversations
            .values()
            .filter_map(Conversation::session)
            .filter_map(|session| session.read(cx).chat.session_id.clone())
            .collect();
        let now = onehand_core::chat::now_secs();
        let held = self.archives.as_ref().and_then(|held| held.found.as_ref());
        let rows: Vec<HistoryRow> = held
            .into_iter()
            .flatten()
            .take(HISTORY_ROWS)
            .map(|meta| HistoryRow {
                title: SharedString::from(meta.title.clone()),
                // The age, and the agent only where it is not the one running
                // this session — on a machine with one agent configured, naming
                // it on every row is a column of the same word.
                aside: SharedString::from(format!(
                    "{} · {}",
                    rel_time(now, meta.updated),
                    meta.agent
                )),
                open: open.iter().any(|id| id == &meta.session_id),
                agent: SharedString::from(meta.agent.clone()),
                dir: meta.dir.clone(),
            })
            .collect();
        let hidden = held.map_or(0, |all| all.len().saturating_sub(HISTORY_ROWS));
        // `None` is the read still out and `Some([])` is a project that has
        // never been prompted. One is a wait and the other is an answer, and a
        // menu that gives the second while the first is true tells somebody with
        // a hundred conversations that they have none.
        let standing = match held {
            None => Some(SharedString::from("Reading conversations…")),
            Some(all) if all.is_empty() => {
                Some(SharedString::from("No conversations in this project yet"))
            }
            Some(_) => None,
        };
        let this = cx.entity();

        header_control("history", IconName::GalleryVerticalEnd, cx)
            .tooltip("Open a past conversation in a new session")
            // Anchored to its own right-hand corner: this button sits at the end
            // of the header, and a menu hanging rightwards from it opens off the
            // edge of the window.
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, cx| {
                let muted = cx.theme().muted_foreground;
                // A project is worked in for months and every conversation had
                // in it is a row here, so the list is longer than a menu's
                // height by design. Without this the rows past the bottom are
                // built and drawn with no way to reach them.
                let menu = menu.scrollable(true);
                let menu = match standing.clone() {
                    Some(line) => menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div().text_color(muted).child(line.clone())
                        })
                        .disabled(true),
                    ),
                    None => menu,
                };
                let menu = rows.iter().fold(menu, |menu, row| {
                    let (title, aside, open) = (row.title.clone(), row.aside.clone(), row.open);
                    let draw =
                        move |_: &mut Window, _: &mut App| {
                            div()
                                .h_flex()
                                .items_center()
                                .gap_3()
                                .w_full()
                                .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                                .child(div().flex_none().text_xs().text_color(muted).child(
                                    if open {
                                        SharedString::from("already open")
                                    } else {
                                        aside.clone()
                                    },
                                ))
                        };
                    match open {
                        // Already open keeps the library's own cursor with its
                        // refusal, so the pointer stays a promise a row can
                        // keep.
                        true => menu.item(PopupMenuItem::element(draw).disabled(true)),
                        false => {
                            let item = crate::controls::menu_row(draw);
                            let (start, agent, dir) =
                                (this.clone(), row.agent.clone(), row.dir.clone());
                            menu.item(item.on_click(move |_, _, cx: &mut App| {
                                start.update(cx, |_: &mut Self, cx| {
                                    cx.emit(ChatPaneEvent::StartSession {
                                        agent: Some(agent.clone()),
                                        resume: Some(dir.clone()),
                                    });
                                });
                            }))
                        }
                    }
                });
                // Said out loud rather than left as a list that simply stops: a
                // cut nobody is told about reads as archives that were lost.
                match hidden {
                    0 => menu,
                    n => menu.separator().item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{n} older, not shown"))
                        })
                        .disabled(true),
                    ),
                }
            })
    }

    /// The way to the terminal, and whether a shell is already running in it.
    ///
    /// **The dot is the whole reason this is not one more plain button.** A
    /// shell outliving a closed dock is the one fact the icon cannot carry: the
    /// child is still running, it is still holding whatever it was doing, and
    /// closing the window is what would end it. It rides at the corner rather
    /// than inside the button so the button keeps the square metrics its
    /// neighbours have — a child in the content row would make this one control
    /// wider than the three beside it, which reads as a mistake.
    ///
    /// Success ink, the same colour the app uses for a turn that finished
    /// unseen: both mean "something of yours is there and you are not looking at
    /// it".
    fn terminal_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let live = self.terminal_live;
        let success = crate::theme::status_ink(cx).success;

        div()
            .relative()
            .flex_none()
            .child(
                header_control("terminal", IconName::SquareTerminal, cx)
                    // What it says is about the *shell*, which is the fact
                    // this pane is pushed and the one the icon cannot carry.
                    // Which way the press will go is left out for the reason
                    // the Workbench's is: the panel on screen answers it, and
                    // saying it would need a second fact pushed down here to
                    // keep in step.
                    .tooltip(if live {
                        "A shell is running here — show or hide the terminal"
                    } else {
                        "Open a shell in this project"
                    })
                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                        cx.emit(ChatPaneEvent::ToggleTerminal);
                    })),
            )
            .when(live, |control| {
                control.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .size(rems(0.375))
                        .rounded_full()
                        .bg(success),
                )
            })
    }

    /// The name of the conversation on screen, and everything done *to* it.
    ///
    /// **The name is prose and the vertical-dots mark beside it is the
    /// control.** The name stays the loudest thing in the header --
    /// full-strength ink and semibold against a row that is otherwise muted --
    /// because it is the one thing there that answers "which conversation is
    /// this". The menu lives on the mark and not on the name, so the popup
    /// opens directly under the dots that were pressed rather than under
    /// whatever width the name happened to be that frame -- and the name is
    /// free to give way: it truncates while the mark is `flex_none`, so
    /// narrowing the panel shortens the name and never takes the control.
    /// The mark also carries a tooltip, which the name-as-button never could:
    /// the library builds a button's accessible name from `label` alone, and
    /// the name had to be a child to ellipsize at all.
    ///
    /// **The project page gets the same control**, naming the project instead
    /// and holding what is done to a project. Same shape on purpose: on that
    /// page this line is still "what you are looking at", and a name that is a
    /// menu in one state and inert in the other teaches the user it is neither.
    /// Where there is no project at all it *is* inert — there is nothing behind
    /// it to act on, and an empty menu is worse than none.
    ///
    /// **Closing the session is not in here.** It is a control at the right-hand
    /// end of the header, with the rest of what is about the window — it keeps
    /// every word of the conversation, so it does not belong beside the entry
    /// that throws the conversation away.
    fn title_control(
        &self,
        title: String,
        busy: bool,
        archive: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let live = self.active_chat(cx).is_some();
        let project = (!live).then_some(self.empty.as_ref()).flatten();
        let name = div()
            .min_w_0()
            .truncate()
            .text_color(cx.theme().foreground)
            .font_semibold()
            .child(title);
        if !live && project.is_none() {
            return div().min_w_0().child(name).into_any_element();
        }
        // **What the menu's state is keyed by, and why it is not a constant.**
        // The popover holds its open flag and the menu it built under this
        // key, and the rows are frozen at the press -- the archive path a
        // *Delete conversation* carries is the one captured when the menu
        // opened. A fixed key is a menu that survives what it was opened on:
        // `Ctrl+2` is bound with no context, so it switches session with the
        // menu up and focused, and the rows stay while the transcript under
        // them changes -- then Delete asks about the conversation now on
        // screen and removes the directory of the one that is gone. Keying by
        // the session means the key stops being reached for, the window
        // collects the state, and the menu is gone by the time the new
        // transcript is drawn. The project page takes the same treatment
        // against a project switch, where the stale rows would mislabel a pin
        // and drop the wrong root from the workspace.
        let key = match &project {
            Some(project) => {
                use std::hash::{Hash as _, Hasher as _};
                // In-process only, so the standard hasher is fine here --
                // unlike a digest that names a directory on disk, nothing
                // survives the run for a toolchain change to move.
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                project.path.hash(&mut hasher);
                hasher.finish()
            }
            // `active` is `Some` wherever a conversation is showing, which is
            // every case this branch is reached in.
            None => self.active.unwrap_or_default(),
        };
        let project = project.map(|project| project.facts);
        let this = cx.entity();

        // The same small ghost button as the rest of the header's controls,
        // so one row keeps one kind of control. The menu goes through the
        // below-anchored builder rather than the library's dropdown, because
        // the dropdown's every anchor opens the menu *over* its trigger and
        // the ask here is that the popup land under the dots that were
        // pressed.
        let trigger = header_control("conversation-menu", IconName::EllipsisVertical, cx).tooltip(
            match project.is_some() {
                true => "Everything done to this project",
                false => "Everything done to this conversation",
            },
        );
        let row = div().h_flex().items_center().gap_1().min_w_0().child(name);

        // Two names as well as two keys: the project page's menu and a
        // conversation's are different menus, and one name for both would key
        // them together across the one switch the page itself makes.
        let menu = match project {
            Some(facts) => crate::controls::menu_below(
                ("project-menu-popup", key),
                trigger,
                project_menu(facts, this),
            ),
            None => crate::controls::menu_below(
                ("conversation-menu-popup", key),
                trigger,
                move |menu, _, cx| {
                    let danger = crate::theme::status_ink(cx).danger;
                    let (rename, export, history) = (this.clone(), this.clone(), this.clone());
                    let (restart, remove) = (this.clone(), this.clone());
                    let archive = archive.clone();
                    menu.item(
                        crate::controls::menu_item("Rename…")
                            .icon(Icon::new(crate::icons::Icon::SquarePen))
                            .on_click(move |_, _, cx: &mut App| {
                                rename
                                    .update(cx, |_: &mut Self, cx| cx.emit(ChatPaneEvent::Rename));
                            }),
                    )
                    .item(
                        crate::controls::menu_item("Export as Markdown…")
                            .icon(Icon::new(IconName::ExternalLink))
                            .on_click(move |_, _, cx: &mut App| {
                                export.update(cx, |pane: &mut Self, cx| pane.export(cx));
                            }),
                    )
                    // Named and refusing rather than absent. The transcript is held
                    // in a shape JSON can carry and this is the format another tool
                    // reads; leaving it out entirely would say the opposite.
                    .item(
                        PopupMenuItem::new("Export as JSON… (not yet)")
                            .icon(Icon::new(IconName::File))
                            .disabled(true),
                    )
                    .separator()
                    .item(
                        // Named for what it does *to this session*, because the
                        // header now carries a control that reaches the same
                        // archives and leaves the session alone: this one swaps
                        // what the conversation on screen is, and the difference
                        // between the two is the whole question.
                        //
                        // Disabled mid-turn rather than guarded by a second click:
                        // going back to the picker throws the running turn away
                        // exactly as a restart does, and a menu that has to be
                        // opened twice to be believed is a worse warning than an
                        // item that will not go.
                        match busy {
                            true => PopupMenuItem::new("Resume in this session…"),
                            false => crate::controls::menu_item("Resume in this session…"),
                        }
                        .icon(Icon::new(IconName::Undo))
                        .disabled(busy)
                        .on_click(move |_, _, cx: &mut App| {
                            history.update(cx, |pane: &mut Self, cx| pane.show_history(cx));
                        }),
                    )
                    .item(
                        crate::controls::menu_item("Restart the agent")
                            .icon(Icon::new(IconName::Redo))
                            .on_click(move |_, _, cx: &mut App| {
                                restart
                                    .update(cx, |_: &mut Self, cx| cx.emit(ChatPaneEvent::Restart));
                            }),
                    )
                    .separator()
                    .item(
                        // The only entry here that ends something for good.
                        // Closing the session -- which keeps every word of this on
                        // disk -- is a control of its own at the other end of the
                        // header, so the two are never one press apart.
                        {
                            let row = move |_: &mut Window, _: &mut App| {
                                div().text_color(danger).child("Delete conversation")
                            };
                            // Nothing on disk to remove until the first turn has
                            // ended, so until then this refuses -- and a refusal
                            // keeps the library's own cursor, since a pointer over
                            // it would promise a press that does nothing.
                            match archive.is_none() {
                                true => PopupMenuItem::element(row),
                                false => crate::controls::menu_row(row),
                            }
                        }
                        .icon(Icon::new(IconName::Delete).text_color(danger))
                        // An entry that can only report that it has nothing to do is
                        // one the eye has to learn to skip, so it is refused rather
                        // than hidden -- the menu keeps its shape between one turn
                        // and the next.
                        .disabled(archive.is_none())
                        .on_click(move |_, _, cx: &mut App| {
                            let Some(dir) = archive.clone() else {
                                return;
                            };
                            remove.update(cx, |_: &mut Self, cx| {
                                cx.emit(ChatPaneEvent::DeleteConversation(dir))
                            });
                        }),
                    )
                },
            ),
        };

        row.child(div().flex_none().child(menu)).into_any_element()
    }
}

/// What the project page's own name opens.
///
/// The same set the rail offers on a project row, minus the two this page
/// already answers with a control of its own, and reached the same way the
/// conversation's menu is — so the header's leftmost thing is always "what you
/// are looking at, and what can be done to it", whichever of the two it is.
///
/// Every entry is announced, not done: a project is a row in the workspace tree,
/// and the pane holds conversations. The builder rather than the element, so the
/// row it hangs off stays the header's to draw.
fn project_menu(
    ProjectFacts {
        pinned,
        is_repo,
        unattended,
    }: ProjectFacts,
    pane: Entity<ChatPane>,
) -> impl Fn(
    gpui_component::menu::PopupMenu,
    &mut Window,
    &mut Context<gpui_component::menu::PopupMenu>,
) -> gpui_component::menu::PopupMenu
+ 'static {
    move |menu, _, cx| {
        let danger = crate::theme::status_ink(cx).danger;
        let act = |action: ProjectAction, pane: Entity<ChatPane>| {
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                pane.update(cx, |_: &mut ChatPane, cx| {
                    cx.emit(ChatPaneEvent::Project(action))
                });
            }
        };
        menu.item(
            // The label is the state readout as well as the action: with no pin
            // marker anywhere on this page, a project would otherwise only say
            // it is pinned by where it sits in a rail that may be hidden.
            crate::controls::menu_item(if pinned { "Unpin" } else { "Pin to top" })
                .icon(Icon::new(IconName::Star))
                .on_click(act(ProjectAction::TogglePin, pane.clone())),
        )
        .when_some(unattended, |menu, on| {
            menu.item(crate::rail::unattended_item(
                on,
                act(ProjectAction::ToggleUnattended, pane.clone()),
            ))
        })
        .when(is_repo && unattended.is_some(), |menu| {
            menu.item(crate::rail::pick_item(act(
                ProjectAction::PickIssue,
                pane.clone(),
            )))
        })
        // Only where there is a repository to split. On a plain folder this
        // could do nothing but report that git said no, and an entry whose whole
        // job is to fail is one the eye has to learn to skip.
        .when(is_repo, |menu| {
            menu.item(
                crate::controls::menu_item("New worktree…")
                    .icon(Icon::new(crate::icons::Icon::GitBranch))
                    .on_click(act(ProjectAction::Worktree, pane.clone())),
            )
        })
        .item(
            crate::controls::menu_item("Copy project path")
                .icon(Icon::new(IconName::Copy))
                .on_click(act(ProjectAction::CopyPath, pane.clone())),
        )
        .item(
            crate::controls::menu_item("Refresh Git status")
                .icon(Icon::new(IconName::Redo))
                .on_click(act(ProjectAction::RefreshGit, pane.clone())),
        )
        .separator()
        .item(
            crate::controls::menu_row(move |_, _| {
                div().text_color(danger).child("Remove from workspace")
            })
            .icon(Icon::new(IconName::Delete).text_color(danger))
            .on_click(act(ProjectAction::Remove, pane.clone())),
        )
    }
}

/// One of the header's controls.
///
/// **Bigger and quieter than the library's default.** Two changes that pull in
/// opposite directions and are one decision: at the smallest size these were
/// three glyphs the pointer had to be aimed at, and at full-strength ink four
/// icons in a row out-shouted the conversation's own name two inches to their
/// left. A step up in size makes them easy to hit; a step down in tone puts them
/// behind the name, which is what the header is for. What brings the ink back is
/// hovering one — the fill arrives and says which is about to be pressed.
///
/// Built in one place because the alternative is four call sites that each have
/// to remember two things, and the one that forgets is the one that looks wrong.
fn header_control(
    id: &'static str,
    icon: impl Into<Icon>,
    cx: &App,
) -> gpui_component::button::Button {
    crate::controls::action(id)
        .ghost()
        .small()
        .icon(icon.into())
        .text_color(cx.theme().muted_foreground)
}

/// How many past conversations the header's menu offers before it stops and
/// says so.
///
/// **High enough that it is not the thing deciding what the list shows.** The
/// menu scrolls, so what a reader can reach is not bounded by what fits; this
/// bounds the *work*, because a menu builds every row it holds the moment it
/// opens. It sits far past what anybody scrolls a menu for — the menu's own
/// height shows on the order of a dozen rows at a time — so it is a backstop
/// against a store nothing ever prunes rather than an editorial cut, and the one
/// time it bites it says so.
const HISTORY_ROWS: usize = 200;

/// One row of that menu, prepared before the menu builder runs.
///
/// The builder is an `Fn` that outlives the borrow of the pane these came from,
/// so everything it needs is copied out here rather than read through a handle
/// at the moment it draws.
struct HistoryRow {
    title: SharedString,
    /// How long ago, and which agent had it.
    aside: SharedString,
    /// Whether a session in this window is already on this conversation.
    open: bool,
    agent: SharedString,
    dir: PathBuf,
}

/// Put up the window that sends what session `uid`'s run waits on back to
/// be done again, with what to change.
fn open_revise(uid: u64, window: &mut Window, cx: &mut Context<ChatPane>) {
    let pane = cx.entity().downgrade();
    let note =
        cx.new(|cx| TextareaState::new(window, cx).placeholder("What should be done differently?"));
    note.update(cx, |input, cx| input.focus(window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let send = {
            let (note, pane) = (note.clone(), pane.clone());
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let text = note.read(cx).value().trim().to_string();
                if text.is_empty() {
                    window.push_notification("Say what to change", cx);
                    return;
                }
                window.close_dialog(cx);
                let _ = pane.update(cx, |_, cx| {
                    cx.emit(ChatPaneEvent::RevisePipeline { uid, note: text })
                });
            }
        };
        dialog
            .title("Send it back")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "The step it approves runs again, with this note and the \
                                 answer it gave before.",
                            ),
                    )
                    .child(Textarea::new(&note).h(rems(10.))),
            )
            .footer(
                div()
                    .h_flex()
                    .gap_2()
                    .w_full()
                    .justify_end()
                    .child(
                        crate::controls::action("pipeline-revise-cancel")
                            .small()
                            .ghost()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        crate::controls::action("pipeline-revise-send")
                            .small()
                            .primary()
                            .label("Send back")
                            .on_click(send),
                    ),
            )
    });
}
