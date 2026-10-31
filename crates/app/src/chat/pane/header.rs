use super::{ChatPane, ChatPaneEvent, ProjectAction, ProjectFacts};
use crate::controls::{Named as _, Quiet as _, Short as _};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Rems,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_core::chat::{Chat, ConvMeta};
use onehand_ui::strip_control;
use std::path::PathBuf;

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
/// The dot on a toggle saying something is alive behind a closed region:
/// small enough to sit on the glyph's corner without covering it.
pub(crate) const LIVE_DOT: Rems = rems(0.375);

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
    /// The header, kept at its own size under the reading zoom: it is drawn
    /// inside the zoomed part of every view the pane shows, and a header that
    /// grew with the conversation would spend the room the zoom asked for and
    /// stop lining up with the Workbench's strip beside it.
    pub(super) fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        self.zoom.unscale(self.header_row(cx))
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
    /// The docks' toggles are the status bar's, which holds them for every
    /// page; the header keeps the way back to a hidden rail, which sits on the
    /// side the rail returns to.
    ///
    /// **The name names the conversation and the vertical-dots mark beside it
    /// carries its menu**, and every other
    /// control sits on the side of what it acts on: the way back to a hidden
    /// rail at the row's left edge, the side the rail returns to, and the
    /// right-hand end reading outward from the name — the past conversations
    /// and closing the session, which act on the session the name names, then
    /// the workers' toggle for the region beside the conversation. The dots are
    /// the row's one menu mark, and everything behind them is something done
    /// to the conversation the name beside them is.
    fn header_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let chat = self.active_chat(cx);
        // Unnamed until its first prompt, a conversation is called by the
        // agent that runs it, as its row on the rail is.
        let agent = chat
            .and(self.active_conversation())
            .map(|conv| conv.spec.name.clone());
        let title = chat
            .and_then(Chat::conversation_title)
            .or(agent)
            // A page, and a project's page, names itself in its own head at
            // its title size.
            .unwrap_or_default();
        let busy = chat.is_some_and(|chat| chat.busy);
        // A conversation the agent has not named yet has no directory to remove:
        // nothing is written until the first turn ends. The menu says so by
        // refusing rather than by hiding the entry, which would make the whole
        // menu change shape between one turn and the next.
        let archive = chat
            .and_then(|chat| chat.session_id.as_deref())
            .map(|sid| onehand_core::chat::conv_dir(&onehand_core::chat::conversations_dir(), sid));
        // The title is a menu only where there *is* a conversation. Standing on
        // a project with no session the line carries no name, the page naming
        // the project, and every entry behind it would be about something that
        // does not exist yet.
        let live = chat.is_some();

        // **A strip, at a fixed height rather than one the tallest control
        // happens to make.** It is the one row that never scrolls, so it is
        // the edge every other measurement in the panel is taken from -- and
        // sized by its contents it moved whenever a badge appeared or a title
        // wrapped, taking the top of the conversation with it.
        onehand_ui::strip(cx)
            // On a page, and on the project page that stands where no
            // conversation shows, the name stands as far off the pane's edge as
            // the page's body does, so the two start on one line.
            .when(self.page.is_some() || !live, |header| header.px_8())
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
            .when(self.rail_hidden, |header| header.child(show_rail(cx)))
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
            // Absent while the agent runs nothing on its own, so a quiet
            // session carries no extra chrome; it reads outward with history,
            // both being about this session.
            .when_some(self.background_control(cx), |header, control| {
                header.child(control)
            })
            // Only while a session is showing, and for a reason worth stating:
            // this is the same list the project page draws, and that page is
            // exactly what the centre of the window shows when there is no
            // session — offering it there too would be saying one thing twice
            // within an inch of itself.
            .when(live, |header| header.child(self.history_control(cx)))
            // Only while there is a session to end, and *before* the docks
            // rather than last: the cluster reads outward from the name by
            // what each control is about -- this and the past conversations
            // act on the session the name names, the workers' toggle on the
            // region beside it -- and the row's far edge, where a pointer drifts, is
            // the wrong seat for the one control that ends something. It keeps the
            // conversation -- the transcript is written at the end of every turn
            // and closing costs nothing that is not already on disk -- which is
            // why it can be a control on the row while deleting stays behind the
            // name, two presses and a warning away.
            .when(live, |header| {
                header.child(
                    // The exit-door arrow and not an ×: this ends a session
                    // while keeping every word, and every × in the row's
                    // reach says "dismiss this" -- beside a panel toggle it
                    // read as closing a panel, and a power mark read as
                    // quitting the whole app.
                    strip_control("close-session", crate::icons::Icon::LogOut, false, cx)
                        .named_with_action(
                            "Close this session and its agent",
                            &crate::shell::CloseSession,
                            None,
                        )
                        .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                            cx.emit(ChatPaneEvent::CloseSession);
                        })),
                )
            })
            // The workers' region, which takes the Workbench's place. The
            // docks' own toggles are the status bar's.
            .when(self.page.is_none(), |row| {
                row.children(self.workers_control(cx))
            })
    }

    /// What the agent left running in the background, and a way to stop each.
    ///
    /// `None` without a session or with nothing running, so an agent that
    /// never reports background work shows no button at all.
    ///
    /// **Each row *is* its Stop.** A menu row is one press target and the
    /// library's menu walks rows, not controls inside them, so a Stop button
    /// nested in a row would be out of the keyboard's reach. The row says
    /// *Stop* at its end and is named `Stop <name>` for assistive technology.
    /// It asks the adapter to stop that one item and never cancels the turn;
    /// the list follows the adapter's reports, and a refusal comes back as a
    /// toast.
    ///
    /// The rows close over the session entity rather than the pane, and the
    /// menu is keyed by the session, so a menu left open across a session
    /// switch neither survives it nor stops another session's work.
    fn background_control(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let session = self.session()?;
        let key = self.active.unwrap_or_default();
        // A menu open on another session went with that session's frame.
        if self.background_menu.get().is_some_and(|open| open != key) {
            self.background_menu.set(None);
        }
        let items = session.read(cx).chat.background();
        // Kept while its menu is open: the menu holds focus, and dropping its
        // trigger mid-open would leave focus on nothing. Closing the menu
        // hands focus back to where it was, and the button goes after.
        if items.is_empty() && self.background_menu.get() != Some(key) {
            return None;
        }
        let count = items.len();
        let rows: Vec<(String, String, String)> = items
            .iter()
            .take(BACKGROUND_MENU_CAP)
            .map(|item| {
                let aside = format!("{} · {}", item.kind, ran_for(item.started.elapsed()));
                (item.id.clone(), item.name.clone(), aside)
            })
            .collect();
        let hidden = count.saturating_sub(BACKGROUND_MENU_CAP);
        let pane = self.handle.clone();

        // Worded, so a ghost at a strip's worded size rather than the
        // icon-only `strip_control`: the count is the whole message.
        let trigger = crate::controls::action(("background", key))
            .quiet(cx)
            .short()
            .label(format!("{count} in background"))
            .tooltip("Work the agent left running in the background");
        let menu = crate::controls::menu_below(
            ("background-menu-popup", key),
            trigger,
            move |menu, window, cx| {
                // A menu sizes to the least its rows can shrink to, and a
                // name that clips shrinks to nothing: without a floor the
                // menu read `sleep…` with the window's width to spare.
                let menu = menu.min_w(BACKGROUND_MENU_MIN_W.to_pixels(window.rem_size()));
                let muted = cx.theme().muted_foreground;
                let menu = rows.iter().fold(menu, |menu, (id, name, aside)| {
                    let (row_id, label, name, aside) = (
                        gpui::SharedString::from(format!("background-stop-{id}")),
                        gpui::SharedString::from(format!("Stop {name}")),
                        name.clone(),
                        aside.clone(),
                    );
                    let (session, id) = (session.clone(), id.clone());
                    menu.item(
                        crate::controls::menu_row(move |_, _| {
                            div()
                                .id(gpui::ElementId::Name(row_id.clone()))
                                .aria_label(label.clone())
                                .h_flex()
                                .items_center()
                                .gap_3()
                                .w_full()
                                .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(aside.clone()),
                                )
                                .child(div().flex_none().child("Stop"))
                        })
                        .on_click(move |_, _, cx: &mut App| {
                            session.update(cx, |session, _| session.chat.stop_background(&id));
                        }),
                    )
                });
                // Said rather than left as a list that simply stops.
                match hidden {
                    0 => menu,
                    n => menu
                        .separator()
                        .item(crate::controls::menu_label(format!("{n} more"))),
                }
            },
        )
        // Right-aligned: the button sits near the row's right end, and a menu
        // hanging rightwards from it would open off the window's edge.
        .anchor(gpui::Anchor::TopRight)
        .on_open_change(move |open, _, cx| {
            _ = pane.update(cx, |pane, cx| {
                pane.background_menu.set(open.then_some(key));
                cx.notify();
            });
        });
        Some(div().flex_none().child(menu).into_any_element())
    }

    /// The name of the conversation on screen, and everything done *to* it.
    ///
    /// **The name is prose and the vertical-dots mark beside it is the
    /// control.** The name stays the loudest thing in the header --
    /// full-strength ink and medium weight against a row that is otherwise muted --
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
            .font_medium()
            .child(title);
        if !live && project.is_none() {
            // A page that draws no head of its own (Settings) is named here at
            // the size a page's title takes.
            return div()
                .min_w_0()
                .when(self.page.is_some(), |title| title.text_lg())
                .child(name)
                .into_any_element();
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
        let trigger = strip_control("conversation-menu", IconName::EllipsisVertical, false, cx)
            .named(match project.is_some() {
                true => "Everything done to this project",
                false => "Everything done to this conversation",
            });
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
                    menu.item(crate::controls::menu_item("Rename…").on_click(
                        move |_, _, cx: &mut App| {
                            rename.update(cx, |_: &mut Self, cx| cx.emit(ChatPaneEvent::Rename));
                        },
                    ))
                    .item(crate::controls::menu_item("Export as Markdown…").on_click(
                        move |_, window, cx: &mut App| {
                            export.update(cx, |pane: &mut Self, cx| pane.export(window, cx));
                        },
                    ))
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
                            true => crate::controls::menu_still(|_, _| {
                                div().child("Resume in this session…")
                            }),
                            false => crate::controls::menu_item("Resume in this session…"),
                        }
                        .disabled(busy)
                        .on_click(move |_, _, cx: &mut App| {
                            history.update(cx, |pane: &mut Self, cx| pane.show_history(cx));
                        }),
                    )
                    .item(
                        crate::controls::menu_command(
                            "Restart the agent",
                            Box::new(crate::shell::RestartSession),
                        )
                        .on_click(move |_, _, cx: &mut App| {
                            restart.update(cx, |_: &mut Self, cx| cx.emit(ChatPaneEvent::Restart));
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
                                div().text_color(danger).child("Delete conversation…")
                            };
                            // Nothing on disk to remove until the first turn has
                            // ended, so until then this refuses -- and a refusal
                            // keeps the library's own cursor, since a pointer over
                            // it would promise a press that does nothing.
                            match archive.is_none() {
                                true => crate::controls::menu_still(row),
                                false => crate::controls::menu_row(row),
                            }
                        }
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
        check: _,
        asks,
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
                    .on_click(act(ProjectAction::Worktree, pane.clone())),
            )
        })
        .item(
            crate::controls::menu_item("Copy project path")
                .on_click(act(ProjectAction::CopyPath, pane.clone())),
        )
        .item(
            crate::controls::menu_item("Refresh Git status")
                .on_click(act(ProjectAction::RefreshGit, pane.clone())),
        )
        .separator()
        .item(
            crate::controls::menu_row(move |_, _| {
                div().text_color(danger).child(match asks {
                    true => "Remove from workspace…",
                    false => "Remove from workspace",
                })
            })
            .on_click(act(ProjectAction::Remove, pane.clone())),
        )
    }
}

/// How many background items the header's menu lists before it says how many
/// it left out. An agent rarely runs more than a few; this bounds a runaway.
const BACKGROUND_MENU_CAP: usize = 12;

/// The header's background menu at its narrowest: room for a command line's
/// worth of name beside its kind, age and Stop. A longer one still clips.
const BACKGROUND_MENU_MIN_W: Rems = rems(28.);

/// How long a background item has run, in whole minutes: `now` under one.
fn ran_for(elapsed: std::time::Duration) -> String {
    match elapsed.as_secs() / 60 {
        0 => "now".to_string(),
        m => format!("{m}m"),
    }
}

/// The way back to a hidden rail.
pub(super) fn show_rail(cx: &mut Context<ChatPane>) -> gpui_component::button::Button {
    strip_control("show-rail", IconName::PanelLeftOpen, false, cx)
        .named_with_action("Show the navigation rail", &crate::shell::ToggleRail, None)
        .on_click(cx.listener(|_: &mut ChatPane, _, _, cx| {
            cx.emit(ChatPaneEvent::ShowRail);
        }))
}

#[cfg(test)]
mod tests {
    use super::ran_for;
    use std::time::Duration;

    #[test]
    fn ran_for_counts_whole_minutes_and_says_now_under_one() {
        assert_eq!(ran_for(Duration::from_secs(59)), "now");
        assert_eq!(ran_for(Duration::from_secs(60)), "1m");
        assert_eq!(ran_for(Duration::from_secs(4 * 60 + 59)), "4m");
    }
}
