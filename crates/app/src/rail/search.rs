//! The search over the list, and the header that names the list: how many
//! sessions wait on the person, and the filter.

use super::RailState;
use super::model::{Filter, toggle_attention};
use super::row::labelled;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use crate::controls::search_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{Anchor, Context, IntoElement, ParentElement, Styled, div};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{ActiveTheme, Icon, Selectable as _, Sizable as _, StyledExt};

/// The search field, the library's edged field, with the key that reaches it
/// from anywhere.
pub(super) fn search_field(state: &RailState, cx: &Context<Shell>) -> impl IntoElement + use<> {
    div()
        .flex_1()
        .min_w_0()
        .child(search_input(&state.search, cx).short().when_some(
            crate::keymap::first_key("focus_rail_search", cx),
            |input, key| input.suffix(gpui_component::kbd::Kbd::new(key)),
        ))
}

/// *Sessions*, how many wait on the person, and the filter: a label line over
/// the list, its words on the edge the rows' marks start at, its controls at
/// a row's size.
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
    let (warning, muted) = (
        crate::theme::status_ink(cx).warning,
        cx.theme().muted_foreground,
    );
    div()
        .h_flex()
        .items_center()
        .flex_none()
        .h(crate::tokens::ROW_H)
        .mx_1p5()
        .px_2()
        .gap_2()
        // Under the header, so it is also the edge the list scrolls under.
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .child(div().text_color(muted).font_medium().child("Sessions"))
        .when(waiting > 0 || attention, |d| {
            let verb = if waiting == 1 { "needs" } else { "need" };
            d.child(
                crate::controls::action("rail-attention")
                    .quiet(cx)
                    .short()
                    .label(format!("{waiting} {verb} attention"))
                    .text_color(warning)
                    .selected(attention)
                    .tooltip(match attention {
                        true => "Show every session again",
                        false => "Show only the sessions waiting on you",
                    })
                    .on_click(cx.listener(|shell: &mut Shell, _, _, cx| {
                        let rail = shell.rail_state_mut();
                        rail.set_filter(toggle_attention(rail.filter, rail.filter_before));
                        cx.notify();
                    })),
            )
        })
        .child(div().flex_1())
        .child(labelled(
            "rail-filter-name",
            "Filter sessions",
            // On while it holds anything but the list by project, so a
            // narrowed list says so where it is narrowed.
            crate::controls::action("rail-filter")
                .quiet_icon(filter != Filter::ByProject, cx)
                .small()
                .icon(Icon::new(crate::icons::Icon::ListFilter))
                // Named for what it is; the tooltip says what it holds now.
                .accessibility_label("Filter sessions")
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
                                            shell.rail_state_mut().set_filter(f);
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                        )
                    })
                }),
        ))
}
