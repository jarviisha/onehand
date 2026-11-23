//! The card a project's page writes in: the field, the agent the session
//! will run, and Send. No session stands behind it yet, so nothing here reads
//! one: what the agent offers, `@` and `/`, and what is staged come once the
//! session is made.

use super::{Composer, ComposerEvent};
use crate::controls::Refuses as _;
use crate::controls::text_area;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, relative,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, StyledExt};
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
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
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
        div()
            .v_flex()
            .gap_1()
            .w_full()
            .p_1p5()
            .bg(crate::theme::raised(cx))
            .border_1()
            .border_color(cx.theme().input)
            .shadow(crate::theme::composer_lift(cx))
            .rounded(cx.theme().radius_lg)
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
                    .child(
                        text_area(&self.state, cx)
                            .bg(cx.theme().transparent)
                            .border_0()
                            .disabled(locked),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(div().h_flex().flex_1().min_w_0().child(agent))
                    .child(div().flex_none().child(send)),
            )
    }
}
