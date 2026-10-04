use super::attachments::chip;
use super::presentation::{composer_status, fast_action, mode_action, options_action};
use super::rows::OPTION_MAX_W;
use super::{CHIP_H, CHIP_TEXT, Composer, ComposerEvent, Overlay};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, InteractiveElement, IntoElement, ParentElement, Rems, SharedString,
    Styled, Window, div, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Textarea;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{ActiveTheme, Disableable as _, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::attachment::AttachmentSource;

/// How big the glyph in an icon-only composer action is drawn.
///
/// A step up from the 0.75rem the row's *text* is set at, and deliberately not
/// the same number. A word is read, so it can be small and still legible; a
/// glyph is aimed at and recognised by its shape, and at the text's own size
/// the `+` was a faint mark the eye had to hunt for beside a chip carrying a
/// whole model name.
///
/// It stays **under the chip's height** with room either side, so this is the
/// glyph growing inside a target that does not move: the row's height is set by
/// `CHIP_H` and every control in it stands at that whether it holds a word or a
/// drawing. The last step this can take before the two numbers meet and the
/// icon starts deciding the row.
const ACTION_ICON: Rems = rems(1.25);

impl Composer {
    /// The card: the attachment tray, the field, and the row of controls under
    /// it. One card holding the text and everything done to it — a rule across
    /// the pane instead would say the composer is the bottom of the window; it
    /// is the message being written, and a message has edges.
    ///
    /// Drawn here rather than by the pane that mounts it. What the composer is
    /// made of is the composer's own business, and split across two files it
    /// was: the popup and the tray here, the field and the controls they belong
    /// to over there. The pane still decides *where* the card goes and how much
    /// room it takes out of the conversation.
    ///
    /// `typing_here` is passed rather than measured, because focus is a
    /// question about a window and the caller is holding one.
    pub fn card(
        &mut self,
        session: &Entity<ChatSession>,
        blocked: Option<onehand_core::chat::SubmitBlock>,
        typing_here: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (step_down, step_up, take_row) = (session.clone(), session.clone(), session.clone());
        let tray = self.tray(cx).map(IntoElement::into_any_element);
        let options = options_action(session, cx);
        let options_open = self.overlay == Some(Overlay::Options);
        let fast_open = self.overlay == Some(Overlay::Fast);
        let fast = fast_action(session, cx)
            .map(|label| status_action(Overlay::Fast, label, fast_open, session, cx));
        let options_control = options.map(|label| {
            option_action("model-selector", label, options_open, session, cx).into_any_element()
        });

        div()
            .v_flex()
            // The card's own inset, and the gap between the prompt and the row
            // of controls under it. Both a step tighter than the app's ordinary
            // panel inset, because this card is not a panel: it holds exactly
            // two things, and every rem of air around them is taken off the
            // conversation above rather than out of unused space. What the
            // inset still has to do is keep the caret and the controls clear of
            // the border the focus ring is drawn on, which one step does.
            .gap_1()
            .w_full()
            .p_1p5()
            // A floating input needs its own opaque elevation: using the
            // reading surface here makes transcript content behind it visually
            // bleed into the card.
            .bg(cx.theme().popover.alpha(1.))
            .shadow_lg()
            // `radius_lg`: the theme's named step for a card, which is what the
            // Workbench and the terminal round their own dock cards at. This is
            // a card in the same sense — a bordered surface floating on the
            // reading one — so it takes the value rather than a second answer
            // arithmetic on `radius` happened to land near. The doubled step it
            // had is the transcript's, for a block the width of the reading
            // column, and one window drawing its floating surfaces at two
            // different corners is a difference nobody chose.
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(if typing_here {
                cx.theme().ring
            } else {
                cx.theme().border
            })
            // Held always, so `Ctrl+V` can be taken from the input wherever the
            // caret is in the prompt. The list context below is the one that
            // comes and goes.
            .key_context("ChatComposerCard")
            .on_action(cx.listener(
                |composer: &mut Self, _: &crate::shell::PasteHere, window, cx| {
                    composer.paste(window, cx);
                },
            ))
            .on_action({
                let session = session.clone();
                move |_: &crate::shell::CycleMode, _, cx| {
                    session.update(cx, |session, cx| {
                        if session.chat.cycle_mode().is_some() {
                            cx.notify();
                        }
                    });
                }
            })
            // A file dragged onto the message being written is being offered to
            // the agent, and the card is what the user aims at. The whole card
            // takes it rather than the tray, which is not there yet the first
            // time and is exactly where a first attachment cannot be dropped.
            .on_drop(cx.listener(
                |composer: &mut Self, dropped: &gpui::ExternalPaths, _, cx| {
                    composer.stage(dropped.paths().to_vec(), AttachmentSource::Picker, cx);
                },
            ))
            .drag_over::<gpui::ExternalPaths>(move |card, _, _, cx| {
                // The same edge the caret lights, because it answers the same
                // question -- whether letting go now puts the file here.
                card.border_color(cx.theme().ring)
            })
            .children(tray)
            // The card is the border, so the field inside it draws none: two
            // rings around one input read as two inputs.
            .child(
                div()
                    // Claimed only while a list is open, because this is what
                    // takes the arrow keys away from the caret: with nothing to
                    // walk, the context is gone and the input's own bindings
                    // win again.
                    .when(self.overlay_open(), |field| {
                        field.key_context("ChatComposer")
                    })
                    .on_action(cx.listener(
                        move |composer: &mut Self, _: &crate::shell::CompletionNext, _, cx| {
                            composer.step(1, &step_down, cx);
                        },
                    ))
                    .on_action(cx.listener(
                        move |composer: &mut Self, _: &crate::shell::CompletionPrev, _, cx| {
                            composer.step(-1, &step_up, cx);
                        },
                    ))
                    // Tab takes the highlighted row, through the same call
                    // Enter makes. Two keys meaning one thing is the point:
                    // what they must not become is two answers to "what is
                    // highlighted for", which is what a second accept path
                    // written out here would drift into. The key is claimed
                    // only while a list is open, so there is always a row for
                    // it to be about.
                    .on_action(cx.listener(
                        move |composer: &mut Self,
                              _: &crate::shell::CompletionAccept,
                              window,
                              cx| {
                            composer.commit(&take_row, window, cx);
                        },
                    ))
                    .child(Textarea::new(&self.state).appearance(false)),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    // What goes *into* the prompt, at the start of the row: the
                    // two ways a file joins it, the slash commands, and what
                    // will answer it. The right-hand end is the turn itself.
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(add_menu(cx))
                            // Ahead of the model rather than after it, because
                            // it is read as a qualifier of the name beside it --
                            // and because the model chip is the one thing on
                            // this row that truncates, so anything standing
                            // after it would be the thing pushed off the end.
                            .children(fast)
                            .children(options_control),
                    )
                    .child(
                        div().h_flex().items_center().gap_2().child(
                            send_controls(
                                blocked.clone(),
                                !self.text(cx).trim().is_empty() || !self.attachments.is_empty(),
                                cx.listener(|_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::SendPressed);
                                }),
                                cx.listener(|_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::StopPressed);
                                }),
                            )
                            .flex_none(),
                        ),
                    ),
            )
            .children(composer_status(blocked, cx).map(IntoElement::into_any_element))
            .children(self.feedback.clone().map(|message| {
                div()
                    .h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(crate::theme::status_ink(cx).danger)
                    .child(Icon::new(IconName::Info).size_3())
                    .child(message)
                    .into_any_element()
            }))
    }

    /// The bare strip under the card: what the project is, and what the turn
    /// will be taken under.
    ///
    /// **Outside the card, and with no chrome of its own.** The card is the
    /// message being written and everything inside it acts on that message;
    /// these two facts do not. The branch is about the project and holds across
    /// every session in it, and the two settings here outlive the prompt in the
    /// field — so a strip resting under the card says "this is the standing
    /// state" where a fourth control inside the row would have said they were
    /// part of what is being typed.
    ///
    /// **Left is the project, right is the turn.** The branch is about the
    /// repository the whole window is on; the permission mode is about the
    /// prompt about to be sent. Which side a thing is on is the whole of what
    /// says which kind it is.
    ///
    /// Both sides are pressable. The branch is a control and not a label,
    /// because everything a reader might want to do about the branch they are
    /// reading — switch it, rename it, take it to a worktree — is a thing they
    /// have to leave for the rail to reach otherwise, and a word that answers
    /// "which branch" while refusing "and now what" is the one place in this
    /// row that stops short.
    ///
    /// Nothing at all where there is neither: on a project that is not a
    /// repository, with an agent advertising no modes, an empty rule of blank
    /// space under the composer would be chrome reporting that it has nothing
    /// to report.
    pub fn status_row(
        &mut self,
        session: &Entity<ChatSession>,
        git: Option<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let mode_open = self.overlay == Some(Overlay::Mode);
        let mode = mode_action(session, cx).map(|label| {
            status_action(Overlay::Mode, label, mode_open, session, cx).into_any_element()
        });
        if git.is_none() && mode.is_none() {
            return None;
        }

        Some(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .w_full()
                // The card's own inset, so the branch lines up with the `+`
                // above it and the controls with Send -- the strip is under the
                // card rather than in it, and a column that did not line up
                // would read as a second panel that had slipped.
                .px_1p5()
                .pt_1p5()
                // Built by the pane and dropped in here: git is the project's,
                // and the panel that talks to the shell about the project is
                // the one that can offer to do anything about it. The composer
                // decides only where on the row it goes.
                .child(div().h_flex().items_center().min_w_0().children(git))
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .flex_none()
                        .children(mode),
                ),
        )
    }
}

/// The `+` menu: everything that can be put into the prompt from a control.
///
/// Attaching a file, mentioning one and starting a slash command are three
/// answers to one question — *put something in front of the agent* — and as
/// three icons they took the row's whole left-hand end to say it three times.
/// Behind one control they are what the `+` means, which is also the only thing
/// a plus sign can mean here.
///
/// **Each row still draws the mark it stands for, and that is load-bearing.**
/// With a Vietnamese input method on Linux a typed `@` or `/` can be swallowed
/// before it reaches the composer, so these rows are the only route to either
/// trigger — and for somebody who cannot type the character, the row is the
/// only thing on screen naming it. A stand-in glyph would leave them with a
/// menu entry about a character they have no way to see.
///
/// Anchored at the trigger's bottom, because the composer is at the foot of the
/// window: a menu hung below it opens off the bottom of the frame.
fn add_menu(cx: &mut Context<Composer>) -> impl IntoElement + use<> {
    let composer = cx.entity();
    chip("add", false, cx)
        .child(Icon::new(crate::icons::Icon::PlusLight).size(ACTION_ICON))
        .tooltip("Attach a file, mention one, run a slash command or a workflow")
        .dropdown_menu_with_anchor(gpui::Anchor::BottomLeft, move |menu, _, _| {
            let (attach, mention, command) = (composer.clone(), composer.clone(), composer.clone());
            menu.item(
                crate::controls::menu_item("Attach files…")
                    .icon(Icon::new(crate::icons::Icon::Paperclip))
                    .on_click(move |_, _, cx: &mut gpui::App| {
                        attach.update(cx, |composer: &mut Composer, cx| composer.attach(cx));
                    }),
            )
            .item(
                crate::controls::menu_item("Mention a file")
                    .icon(Icon::new(crate::icons::Icon::AtSign))
                    .on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
                        mention.update(cx, |composer: &mut Composer, cx| {
                            composer.insert_trigger('@', window, cx);
                        });
                    }),
            )
            .item(
                crate::controls::menu_item("Run a slash command")
                    .icon(Icon::new(crate::icons::Icon::SquareSlash))
                    .on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
                        command.update(cx, |composer: &mut Composer, cx| {
                            composer.insert_trigger('/', window, cx);
                        });
                    }),
            )
            // Not something put into this prompt but a run of prompts, so it
            // goes up to the window, which owns runs, as its key does.
            .separator()
            .item(
                crate::controls::menu_item("Run a workflow…")
                    .icon(Icon::new(IconName::Play))
                    // Once the menu has gone, so the action starts from the
                    // focus it hands back, inside the window's shell.
                    .on_click(move |_, window: &mut Window, cx: &mut gpui::App| {
                        window.defer(cx, |window, cx| {
                            window.dispatch_action(Box::new(crate::shell::RunWorkflow), cx)
                        });
                    }),
            )
        })
}

/// A picker for a setting a turn is run under: a mark, the value in force, and
/// its list.
///
/// Two of these, and they sit apart because the two questions do. Fast mode is
/// in the card's own row, ahead of the model, because it is read as a qualifier
/// of the name beside it -- the two were briefly one chip for that reason. The
/// permission mode is on the strip below, with the branch, because what the
/// agent may do without asking is standing state about the project and the
/// session rather than about this message.
///
/// **Three things separate both from the Model chip**, which is the card's other
/// picker.
///
/// They letter their value at **full strength, like every other chip**. The
/// chip's own muted ink is what the *mark* takes, and the caret and the chrome
/// with it -- what a chip is for is the value in it, and a chip standing beside
/// another with its word a shade fainter reads as one that is somehow less
/// settled rather than as one about a different question.
///
/// They draw **no caret**. Between the two chips and the branch's own mark that
/// would be three small glyphs in a row an inch long, and the caret is the
/// least of the three: a chip is the only thing near it with a hover fill and a
/// pointer, so what can be pressed is already said twice.
///
/// They lead with the **mark of the setting**, which is what makes a value
/// legible without reading it -- and it is the setting and not the value,
/// because a mark per value would mean the app deciding what an agent-chosen
/// word means. What the agent picked is the word right beside it.
fn status_action(
    target: Overlay,
    label: SharedString,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    // The id, the mark and the sentence are all per chip, so they are decided
    // where the chip is decided. Handed in at the call site they were three
    // things that had to agree with a fourth, in two places.
    let (id, icon, hint) = match target {
        Overlay::Fast => (
            "fast-selector",
            crate::icons::Icon::Zap,
            "Choose how fast the agent answers",
        ),
        _ => (
            "mode-selector",
            crate::icons::Icon::Shield,
            "Choose what the agent may do without asking",
        ),
    };
    let session = session.clone();
    chip(id, open, cx)
        .max_w(OPTION_MAX_W)
        .flex_shrink_1()
        .min_w_0()
        .child(Icon::new(icon).size_3())
        // A child and not the button's `label`, for the reason the card's chip
        // does the same: a label is wrapped in a box the library builds
        // `flex_none`, so against the cap above it takes its whole natural
        // width and the end of the word is clipped with nothing saying it was.
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(CHIP_TEXT)
                .text_color(cx.theme().foreground)
                .child(label),
        )
        .tooltip(hint)
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(target.clone(), &session, window, cx);
        }))
}

/// The card's own picker: the model in force, and behind it every config group
/// the strip below has not taken a chip of.
///
/// `detail` is the value that qualifies the one in the label — the effort a
/// model is being run at — and is drawn in the chip's own muted ink while the
/// label takes full strength. Two words at one weight read as one name.
fn option_action(
    id: &'static str,
    label: SharedString,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    let session = session.clone();
    let fg = cx.theme().foreground;
    chip(id, open, cx)
        .max_w(OPTION_MAX_W)
        .flex_shrink_1()
        .min_w_0()
        // The value is a **child and not the button's `label`**. A label is
        // wrapped in a box the library builds `flex_none`, so it can neither
        // shrink nor truncate: against the cap above it took its whole natural
        // width, pushed the caret out past the clip, and left the end of the
        // word and the mark that says the button opens both cut off, with
        // nothing on screen saying either had been. As a child it gives way and
        // ends in an ellipsis, and the caret keeps its place.
        //
        // What it costs is the accessible name, which the library derives from
        // that same `label` and offers no other way to set -- `aria_label` is a
        // stateful-element method and this button is not one. So the name is
        // left to be read off the button's own text, which is the value; the
        // tooltip still says which setting the value belongs to.
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(CHIP_TEXT)
                .text_color(fg)
                .child(label),
        )
        .dropdown_caret(true)
        .tooltip("Choose model, effort and other options")
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(Overlay::Options, &session, window, cx);
        }))
}

/// Send, or Stop while a turn is in flight — the same button, because they are
/// the same affordance at two moments of one turn.
///
/// Every state is read off the conversation's own answer to "may this be sent",
/// the running turn included. A button that refuses without saying why does
/// nothing when pressed and gives no reason, which is the shape of a broken app
/// rather than of a rule — so a refusal disables the control and puts the reason
/// on it. One argument and not two, because "a turn is running" and "Send would
/// refuse" are the same fact, and two of them can be made to disagree.
fn send_controls(
    blocked: Option<onehand_core::chat::SubmitBlock>,
    has_draft: bool,
    on_send: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_stop: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    use onehand_core::chat::SubmitBlock;

    // Whichever of these is showing, it is the row's size and not the
    // library's default.
    //
    // **The size and the height both, and the size is the half that was
    // missing.** With no size named these took the library's `Medium`, and the
    // explicit height below only answered half of it: an icon-only button at
    // that size is a *square* of 2rem, so pinning the height left a 2rem-wide
    // box 1.5rem tall -- a lozenge beside a row of controls that are all as
    // wide as what is in them. `Small` is the row's own height in both
    // directions, so Send comes out a square the size of every chip beside it,
    // and it takes the padding, the text and the glyph size down with it.
    //
    // Nothing else changes: the primary fill is what says this is the row's
    // one important action, and it never needed the extra quarter-inch to.
    fn control(id: &'static str) -> Button {
        crate::controls::action(id).small().h(CHIP_H)
    }

    // **Stop is always there while a turn runs, and Queue joins it once there
    // is something to queue.** Stop is the one control in this app that throws
    // running work away, and it is reachable at every moment the work is
    // running -- a Stop that disappeared the instant somebody started typing
    // would be gone at exactly the moment they had most to say about the turn.
    //
    // They are never one button wearing two faces, and never on screen
    // together: Send is drawn only with no turn running and Stop only while one
    // is. They do share a place, though -- the trailing end of the row is the
    // turn's own control whichever of them is in it -- so what tells them apart
    // is the tint, the glyph, and Queue arriving *labelled* beside Stop the
    // moment there is a draft. The cost is real and is taken knowingly: press
    // that same spot twice quickly and the second press stops the turn the
    // first one started. A slot reserved so neither ever moves spends a fixed
    // inch of a row that runs out of width before anything else in the card
    // does, and spends it on the state that is idle most of the time.
    if blocked == Some(SubmitBlock::Busy) {
        return div()
            .h_flex()
            .gap_2()
            .children(has_draft.then(|| {
                control("queue")
                    .primary()
                    .icon(Icon::new(crate::icons::Icon::ArrowUpLight))
                    .label("Queue")
                    .tooltip("Send this prompt when the current turn finishes")
                    .on_click(on_send)
            }))
            .child(
                // The word is gone and the tint carries it: this is the one
                // press here that ends something rather than starting it. It
                // keeps the word's *place* though -- Queue is labelled, so the
                // two are still told apart by more than colour.
                control("stop")
                    .danger()
                    .icon(Icon::new(IconName::Pause))
                    .tooltip("Stop the current turn")
                    .on_click(on_stop),
            );
    }
    // The arrow alone. The word sat beside four other controls in a row that
    // runs out of width before anything else in the card does, and it is the
    // one control here whose meaning never changes -- so it is the one that can
    // afford to be a glyph. Queue and Stop keep their words: they appear only
    // mid-turn, they appear together, and which of the two a press lands on is
    // exactly what a reader has to be sure of.
    let send = control("send")
        .primary()
        .icon(Icon::new(crate::icons::Icon::ArrowUpLight));
    match blocked {
        // No pointer over a button that refuses: the cursor is a promise a
        // press will do something, and this one is here to say it will not.
        Some(reason) => div().child(
            crate::controls::resting(send)
                .disabled(true)
                .tooltip(SharedString::from(reason.hint())),
        ),
        // What Enter does, on the control it does it to. Standing in the row as
        // its own line instead, it was a fixed width competing with the
        // selector chips for a narrow panel's last inch -- and the chips say
        // something that changes, where this says the same thing forever.
        None => div().child(
            send.tooltip("Enter sends · Shift+Enter for a newline")
                .on_click(on_send),
        ),
    }
}
