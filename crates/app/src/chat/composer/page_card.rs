//! The card a project's page writes in: the field, `+`, the agent the
//! session will run, and Send. No session stands behind it yet: `@` and `/`
//! complete against what the page keeps of its project and its agent.

use super::{Composer, ComposerEvent};
use crate::controls::Refuses as _;
use crate::controls::text_area;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, relative,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, Sizable as _, StyledExt};
use onehand_core::attachment::AttachmentSource;
use onehand_ui::ShortIcon as _;

impl Composer {
    /// The page's card: `agent` the chip picking who runs the session,
    /// `refuses` why Send does nothing yet, if it does not, and `locked`
    /// whether the words are fixed while what they were sent for is made.
    pub(in crate::chat) fn page_card(
        &mut self,
        agent: AnyElement,
        refuses: Option<SharedString>,
        locked: bool,
        lists: super::Lists,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (step_down, step_up, take_row) = (lists.clone(), lists.clone(), lists);
        let send = crate::controls::action("page-send")
            .primary()
            .short_icon()
            .icon(Icon::new(crate::icons::Icon::ArrowUpLight))
            .accessibility_label("Send");
        let send = match refuses {
            Some(why) => send.refuses(Some(why.to_string())),
            None => send
                .tooltip("Enter starts the session · Shift+Enter for a newline")
                .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                    cx.emit(ComposerEvent::SendPressed);
                })),
        };
        // What goes into the prompt from a control: files through the picker,
        // and `@` and `/` typed from code, which a keyboard whose input method
        // swallows the character still reaches.
        let me = cx.entity().downgrade();
        let add = onehand_ui::menu_below(
            "page-add",
            super::attachments::chip("page-add-trigger", false, cx)
                .child(Icon::new(crate::icons::Icon::PlusLight).small())
                .accessibility_label("Attach a file, mention one or run a slash command")
                .tooltip("Attach a file, mention one or run a slash command"),
            move |menu, _, _| {
                let act = |what: fn(&mut Composer, &mut gpui::Window, &mut Context<Composer>)| {
                    let me = me.clone();
                    move |_: &gpui::ClickEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
                        let _ = me.update(cx, |composer, cx| what(composer, window, cx));
                    }
                };
                menu.item(
                    crate::controls::menu_item("Attach files…")
                        .on_click(act(|composer, _, cx| composer.attach(cx))),
                )
                .item(crate::controls::menu_item("Mention a file").on_click(act(
                    |composer, window, cx| composer.insert_trigger('@', window, cx),
                )))
                .item(
                    crate::controls::menu_item("Run a slash command").on_click(act(
                        |composer, window, cx| composer.insert_trigger('/', window, cx),
                    )),
                )
            },
        );
        let lift = crate::theme::composer_lift(cx);
        let drop_ring = cx.theme().ring;
        let anchors = self.anchors.clone();
        div()
            .relative()
            .v_flex()
            .gap_1()
            .w_full()
            .p_1p5()
            // Where the card is, which the page's `@` and `/` list hangs from.
            .child(
                gpui::canvas(
                    move |bounds, _, _| {
                        let mut anchors = anchors.borrow_mut();
                        anchors.retain(|(o, _)| *o != super::Overlay::Completion);
                        anchors.push((super::Overlay::Completion, bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .bg(crate::theme::raised(cx))
            .border_1()
            .border_color(cx.theme().input)
            .shadow(lift.clone())
            .rounded(cx.theme().radius_lg)
            // A file dragged onto what is being written goes with it, as on a
            // session's card, ringed while it is held over the card.
            .when(!locked, |card| {
                card.on_drop(cx.listener(
                    |composer: &mut Self, dropped: &gpui::ExternalPaths, _, cx| {
                        composer.stage(dropped.paths().to_vec(), AttachmentSource::Picker, cx);
                    },
                ))
                .drag_over::<gpui::ExternalPaths>(move |card, _, _, _| {
                    let mut shadows = lift.clone();
                    shadows.push(gpui::BoxShadow {
                        color: drop_ring,
                        offset: gpui::point(gpui::px(0.), gpui::px(0.)),
                        blur_radius: gpui::px(0.),
                        spread_radius: gpui::px(super::card::DROP_RING_PX),
                        inset: false,
                    });
                    card.shadow(shadows)
                })
            })
            .child(
                div()
                    .id("page-composer-field")
                    .min_h_10()
                    .cursor_text()
                    .text_size(crate::chat::transcript::TEXT)
                    .line_height(relative(crate::tokens::LEADING))
                    .on_click(cx.listener(|composer: &mut Self, _, window, cx| {
                        composer
                            .state
                            .update(cx, |state, cx| state.focus(window, cx));
                    }))
                    // The `@` and `/` lists take the arrows and Tab while one
                    // is open, as on a session's card.
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
                    .on_action(cx.listener(
                        move |composer: &mut Self,
                              _: &crate::shell::CompletionAccept,
                              window,
                              cx| {
                            composer.commit(&take_row, window, cx);
                        },
                    ))
                    .child(
                        text_area(&self.state, cx)
                            .bg(cx.theme().transparent)
                            .border_0()
                            .disabled(locked)
                            .on_paste({
                                let composer = cx.entity().downgrade();
                                move |item, window, cx| {
                                    let handle = window.window_handle();
                                    composer
                                        .update(cx, |composer, cx| {
                                            composer.attach_pasted(item, handle, cx)
                                        })
                                        .unwrap_or(false)
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .when(!locked, |row| row.child(add))
                            .child(agent),
                    )
                    .child(div().flex_none().child(send)),
            )
    }
}
