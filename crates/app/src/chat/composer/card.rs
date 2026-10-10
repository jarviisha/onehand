use super::attachments::chip;
use super::presentation::{effort_action, fast_action, fast_toggle, mode_action, options_action};
use super::rows::OPTION_MAX_W;
use super::{CHIP_TEXT, Composer, ComposerEvent, Overlay};
use crate::chat::session::ChatSession;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Entity, InteractiveElement, IntoElement, ParentElement, Rems, SharedString,
    StatefulInteractiveElement, Styled, Window, div, relative, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Textarea;
use gpui_component::{ActiveTheme, Disableable as _, Icon, Sizable as _, StyledExt};
use onehand_core::attachment::AttachmentSource;

/// How big the `+` glyph is drawn: a step over the chips' glyphs, because it is
/// aimed at by its shape alone, and still under the chip's height so the row's
/// height stays the chips'.
const ACTION_ICON: Rems = rems(1.25);
/// The ring a file dragged over the card draws, in pixels as a hairline is. A
/// shadow rather than a border, so the card does not shift by its width while
/// the file is held over it.
const DROP_RING_PX: f32 = 1.;
/// Below this the strip under the card takes two lines, the branch over the
/// mode, rather than one squeezing the other. Measured on the stack, not the
/// window.
pub(in crate::chat) const COMPOSER_SPLIT: Rems = rems(36.);

impl Composer {
    /// The card: the field and the row of controls under it. What is staged
    /// to go with the text waits above the card, in the tray.
    ///
    /// **No edge.** Its raised fill and its own lift set it apart from the
    /// transcript it floats over, and the caret alone shows focus.
    pub fn card(
        &mut self,
        session: &Entity<ChatSession>,
        blocked: Option<onehand_core::chat::SubmitBlock>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (step_down, step_up, take_row) = (session.clone(), session.clone(), session.clone());
        let fast = self
            .fast_control(session, cx)
            .map(|fast| self.opens_menu(Overlay::Fast, fast));
        let options_open = self.overlay == Some(Overlay::Options);
        let effort_open = self.overlay == Some(Overlay::Effort);
        let model = options_action(session, cx).map(|label| {
            let chip = option_action(label, options_open, session, cx);
            self.opens_menu(Overlay::Options, chip)
        });
        let effort = effort_action(session, cx).map(|label| {
            let chip = effort_chip(label, effort_open, session, cx);
            self.opens_menu(Overlay::Effort, chip)
        });
        // The model and what it runs at sit side by side, each opening its own
        // short menu: one popup holding both outgrew any width a menu can take.
        let options_control = (model.is_some() || effort.is_some()).then(|| {
            div()
                .h_flex()
                .items_center()
                .min_w_0()
                .flex_shrink_1()
                .children(model)
                .children(effort)
                .into_any_element()
        });
        let add_open = self.overlay == Some(Overlay::Add);
        let add = {
            let session = session.clone();
            chip("add", add_open, cx)
                .child(Icon::new(crate::icons::Icon::PlusLight).size(ACTION_ICON))
                .when(!add_open, |chip| {
                    chip.tooltip("Attach a file, mention one, run a slash command or a workflow")
                })
                .on_click(cx.listener(move |composer: &mut Self, _, window, cx| {
                    composer.toggle_picker(Overlay::Add, &session, window, cx);
                }))
        };
        let add = self.opens_menu(Overlay::Add, add);
        let lift = crate::theme::composer_lift(cx);
        let drop_ring = cx.theme().ring;

        div()
            .v_flex()
            .gap_1()
            .w_full()
            .p_1p5()
            .bg(crate::theme::raised(cx))
            .shadow(lift.clone())
            .rounded(cx.theme().radius_lg)
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
            // the agent, and the whole card takes it.
            .on_drop(cx.listener(
                |composer: &mut Self, dropped: &gpui::ExternalPaths, _, cx| {
                    composer.stage(dropped.paths().to_vec(), AttachmentSource::Picker, cx);
                },
            ))
            // The one moment the card draws an outline: whether letting go now
            // puts the file here. Drawn as a shadow ring over its lift.
            .drag_over::<gpui::ExternalPaths>(move |card, _, _, _| {
                let mut shadows = lift.clone();
                shadows.push(gpui::BoxShadow {
                    color: drop_ring,
                    offset: gpui::point(gpui::px(0.), gpui::px(0.)),
                    blur_radius: gpui::px(0.),
                    spread_radius: gpui::px(DROP_RING_PX),
                    inset: false,
                });
                card.shadow(shadows)
            })
            .child(
                div()
                    .id("composer-field")
                    .min_h_10()
                    .cursor_text()
                    // The reading size and leading, as the transcript: what is typed
                    // reads as what it will be once sent.
                    .text_size(crate::chat::transcript::TEXT)
                    .line_height(relative(crate::chat::transcript::LEADING))
                    // The whole field's area takes the caret, not only its
                    // first line.
                    .on_click(cx.listener(|composer: &mut Self, _, window, cx| {
                        composer
                            .state
                            .update(cx, |state, cx| state.focus(window, cx));
                    }))
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
                    // Enter makes, so the two keys cannot drift into two
                    // answers about what is highlighted.
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
                    // What goes *into* the prompt, at the start of the row; the
                    // right-hand end is the turn itself.
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(add)
                            // Ahead of the model, because the model chip is the
                            // one thing here that truncates, and anything after
                            // it would be what is pushed off the end.
                            .children(fast)
                            .children(options_control),
                    )
                    .child(
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
            )
    }

    /// Fast mode on the card's row: a switch where the agent's two values say
    /// which is on, a picker otherwise, nothing where the agent offers none.
    fn fast_control(
        &self,
        session: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if let Some(toggle) = fast_toggle(session, cx) {
            let session = session.clone();
            let muted = cx.theme().muted_foreground;
            return Some(
                chip("fast-toggle", false, cx)
                    .child(
                        Icon::new(crate::icons::Icon::Zap)
                            .xsmall()
                            .text_color(muted),
                    )
                    .child(chip_text(if toggle.on { "On" } else { "Off" }, cx))
                    .tooltip(match toggle.on {
                        true => "Fast mode is on",
                        false => "Fast mode is off",
                    })
                    .on_click(cx.listener(move |composer: &mut Self, _, _, cx| {
                        composer.flip_fast(&session, cx);
                    }))
                    .into_any_element(),
            );
        }
        let open = self.overlay == Some(Overlay::Fast);
        fast_action(session, cx)
            .map(|label| status_action(Overlay::Fast, label, open, session, cx).into_any_element())
    }

    /// The bare strip under the card: the branch at the left, the permission
    /// mode at the right; below [`COMPOSER_SPLIT`] the branch over the mode.
    ///
    /// **Outside the card, and with no chrome of its own.** The card is the
    /// message being written; these two are standing state about the project
    /// and the session, which outlive the prompt in the field. Nothing at all
    /// where there is neither.
    ///
    /// `git` is the branch line the pane holds for the project on screen;
    /// `narrow` is whether the stack measured under the split last frame.
    pub fn status_row(
        &mut self,
        session: &Entity<ChatSession>,
        git: Option<SharedString>,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let mode_open = self.overlay == Some(Overlay::Mode);
        let mode = mode_action(session, cx).map(|label| {
            let chip = status_action(Overlay::Mode, label, mode_open, session, cx);
            self.opens_menu(Overlay::Mode, chip)
        });
        let branch_open = self.overlay == Some(Overlay::Branch);
        let branch = git.map(|line| {
            let session = session.clone();
            chip("branch", branch_open, cx)
                .flex_shrink_1()
                .min_w_0()
                .child(Icon::new(crate::icons::Icon::GitBranch).xsmall())
                .child(chip_text(line, cx))
                .when(!branch_open, |chip| {
                    chip.tooltip("The branch checked out here")
                })
                .on_click(cx.listener(move |composer: &mut Self, _, window, cx| {
                    composer.toggle_picker(Overlay::Branch, &session, window, cx);
                }))
        });
        let branch = branch.map(|chip| self.opens_menu(Overlay::Branch, chip));
        if branch.is_none() && mode.is_none() {
            return None;
        }
        let strip = match narrow {
            true => div().v_flex().items_start(),
            false => div().h_flex().items_center().justify_between().gap_2(),
        };
        Some(
            strip
                .w_full()
                // The card's own inset, so the branch lines up with the `+`
                // above it and the mode with Send.
                .px_1p5()
                .pt_1p5()
                .child(div().h_flex().min_w_0().max_w_full().children(branch))
                .children(mode),
        )
    }
}

/// A chip's words: the composer's metadata size, in full ink, giving way with
/// an ellipsis rather than pushing the chip's caret out of its box.
fn chip_text(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .min_w_0()
        .truncate()
        .text_size(CHIP_TEXT)
        .text_color(cx.theme().foreground)
        .child(text.into())
}

/// A picker for a setting a turn is run under: the setting's mark in muted ink,
/// and the value in force in full. No caret: the hover fill and the pointer
/// already say it can be pressed.
fn status_action(
    target: Overlay,
    label: SharedString,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
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
        .child(Icon::new(icon).xsmall())
        .child(chip_text(label, cx))
        .when(!open, |chip| chip.tooltip(hint))
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(target.clone(), &session, window, cx);
        }))
}

/// The card's own picker: the model in force, then the caret.
fn option_action(
    label: SharedString,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    let session = session.clone();
    chip("model-selector", open, cx)
        .max_w(OPTION_MAX_W)
        .flex_shrink_1()
        .min_w_0()
        // Children and not the button's `label`, which the library boxes
        // `flex_none`: against the cap it would push the caret past the clip.
        .child(chip_text(label, cx))
        .dropdown_caret(true)
        .when(!open, |chip| {
            chip.tooltip("Choose the model and other options")
        })
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(Overlay::Options, &session, window, cx);
        }))
}

/// The effort the model runs at, in muted ink beside the model's chip so the
/// two read as a name and its qualifier, opening a menu of its own.
fn effort_chip(
    label: SharedString,
    open: bool,
    session: &Entity<ChatSession>,
    cx: &mut Context<Composer>,
) -> impl IntoElement + use<> {
    let session = session.clone();
    chip("effort-selector", open, cx)
        .flex_none()
        .child(
            div()
                .text_size(CHIP_TEXT)
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .dropdown_caret(true)
        .when(!open, |chip| {
            chip.tooltip("Choose how hard the model thinks")
        })
        .on_click(cx.listener(move |composer: &mut Composer, _, window, cx| {
            composer.toggle_picker(Overlay::Effort, &session, window, cx);
        }))
}

/// Send, or Stop and Queue while a turn is in flight.
///
/// Every state is read off the conversation's own answer to "may this be sent",
/// the running turn included, and a refusal disables the control and puts the
/// reason on it.
///
/// **While a turn runs, Stop is always there and Queue joins it once there is a
/// draft.** Stop is the solid danger fill with its word, the one press here
/// that ends something; Queue is an outline beside it, so the region keeps one
/// primary and the two are told apart by more than colour.
fn send_controls(
    blocked: Option<onehand_core::chat::SubmitBlock>,
    has_draft: bool,
    on_send: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_stop: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    use onehand_core::chat::SubmitBlock;

    fn control(id: &'static str) -> Button {
        crate::controls::action(id).small()
    }

    if blocked == Some(SubmitBlock::Busy) {
        return div()
            .h_flex()
            .gap_1()
            .children(has_draft.then(|| {
                control("queue")
                    .outline()
                    .label("Queue")
                    .tooltip("Send this prompt when the current turn finishes")
                    .on_click(on_send)
            }))
            .child(
                control("stop")
                    .danger()
                    .label("Stop")
                    .tooltip("Stop the current turn")
                    .on_click(on_stop),
            );
    }
    // The arrow alone: the one control here whose meaning never changes, in a
    // row that runs out of width before anything else in the card does.
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
        None => div().child(
            send.tooltip("Enter sends · Shift+Enter for a newline")
                .on_click(on_send),
        ),
    }
}
