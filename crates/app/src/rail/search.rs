//! The search over the list, and the header that names the list: how many
//! sessions wait on the person, and the filter.

use super::RailState;
use super::model::{Filter, toggle_attention};
use super::row::labelled;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::Input;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

/// The search field: borderless, with the key that reaches it from anywhere.
pub(super) fn search_field(state: &RailState, cx: &Context<Shell>) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    div().flex_1().min_w_0().child(
        Input::new(&state.search)
            .small()
            .bordered(false)
            .prefix(Icon::new(IconName::Search).small().text_color(muted))
            .when_some(
                crate::keymap::first_key("focus_rail_search", cx),
                |input, key| input.suffix(gpui_component::kbd::Kbd::new(key)),
            ),
    )
}

/// *Sessions*, how many wait on the person, and the filter.
///
/// The count is also the switch for its filter, and remembers the filter it
/// replaced; while that filter is on the count stays, even at none, so it can
/// be switched off where it was switched on.
pub(super) fn sessions_header(
    state: &RailState,
    waiting: usize,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let filter = state.filter;
    let attention = filter == Filter::NeedsAttention;
    let shell = cx.entity().downgrade();
    let (warning, muted, ink) = (
        crate::theme::status_ink(cx).warning,
        cx.theme().muted_foreground,
        cx.theme().foreground,
    );
    let hover = super::row::hover_fill(cx);
    div()
        .h_flex()
        .items_center()
        .flex_none()
        .px_3()
        .pt_1()
        .pb_1p5()
        .gap_2()
        // Under the header, so it is also the edge the list scrolls under.
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .child(div().font_medium().text_color(muted).child("Sessions"))
        .when(waiting > 0 || attention, |d| {
            let verb = if waiting == 1 { "needs" } else { "need" };
            d.child(
                div()
                    .id("rail-attention")
                    .px_1()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .text_color(warning)
                    .hover(move |d| d.bg(hover))
                    .when(attention, |d| d.bg(cx.theme().accent).font_medium())
                    .tooltip(move |window, cx| {
                        Tooltip::new(match attention {
                            true => "Show every session again",
                            false => "Show only the sessions waiting on you",
                        })
                        .build(window, cx)
                    })
                    .on_click(cx.listener(|shell: &mut Shell, _, _, cx| {
                        let rail = shell.rail_state_mut();
                        if rail.filter != Filter::NeedsAttention {
                            rail.filter_before = rail.filter;
                        }
                        rail.filter = toggle_attention(rail.filter, rail.filter_before);
                        cx.notify();
                    }))
                    .child(format!("{waiting} {verb} attention")),
            )
        })
        .child(div().flex_1())
        .child(labelled(
            "rail-filter-name",
            "Filter sessions",
            crate::controls::action("rail-filter")
                .ghost()
                .xsmall()
                .icon(
                    Icon::new(crate::icons::Icon::ListFilter).text_color(match filter {
                        Filter::ByProject => muted,
                        Filter::All | Filter::NeedsAttention | Filter::Recent => ink,
                    }),
                )
                .tooltip(format!("Showing: {}", filter.label()))
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    Filter::ALL.iter().fold(menu, |menu, f| {
                        let (f, shell) = (*f, shell.clone());
                        menu.item(
                            crate::controls::menu_item(f.label())
                                .checked(f == filter)
                                .on_click(move |_, _, cx| {
                                    shell
                                        .update(cx, |shell: &mut Shell, cx| {
                                            let rail = shell.rail_state_mut();
                                            rail.filter = f;
                                            if f != Filter::NeedsAttention {
                                                rail.filter_before = f;
                                            }
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                        )
                    })
                }),
        ))
}
