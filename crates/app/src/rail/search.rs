//! The workspace control and the search over the list, drawn in the title bar, and the header that
//! names the list: how many sessions wait on the person, the way to the search, and the filter.

use super::RailState;
use super::model::{Filter, toggle_attention};
use super::row::{MAX_LABEL, ellipsize, labelled};
use crate::controls::Named as _;
use crate::controls::Quiet as _;
use crate::controls::Short as _;
use crate::controls::search_input;
use crate::shell::Shell;
use gpui::prelude::FluentBuilder as _;
use gpui::{Anchor, Context, InteractiveElement as _, IntoElement, ParentElement, Styled, div};
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{ActiveTheme, Icon, IconName, Selectable as _, StyledExt, h_flex};
use onehand_ui::ShortIcon as _;

/// The workspace's name and a chevron, a quiet button and a tab stop of its
/// own, which opens the workspace menu when pressed and on Enter. It stands at
/// the title bar's start; the name is cut at a character, as a menu row's is.
pub(crate) fn workspace_control(
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> impl IntoElement + use<> {
    let workspace = &shell.workspace_window().workspace;
    let name = ellipsize(&workspace.name, MAX_LABEL);
    let initial: String = workspace
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let theme = cx.theme();
    // A letter tile before the name, so the workspace reads as the window's
    // owner rather than one more control in the bar. It stands outside the
    // button: it marks the workspace, and only the name opens its menu.
    let tile = div()
        .size(onehand_ui::tokens::WORKSPACE_TILE)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.radius)
        .bg(theme.accent)
        .text_xs()
        .font_semibold()
        .text_color(theme.accent_foreground)
        .child(initial);
    let label = div()
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .font_medium()
        .text_color(theme.foreground)
        .child(name);
    let menu = super::workspace::workspace_menu(
        workspace.storage_dir.clone(),
        shell.recents(cx),
        cx.entity().downgrade(),
    );
    h_flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .child(tile)
        .child(onehand_ui::menu_below(
            "workspace-menu",
            crate::controls::action("workspace-segment")
                .quiet(cx)
                .short()
                .child(label)
                .dropdown_caret(true)
                .named("Open another workspace, or start one"),
            menu,
        ))
}

/// The search field, the library's edged field, with the key that reaches it
/// from anywhere. It stands in the title bar, whose press drags the window and
/// whose right press opens the window's menu, so either stops here before the
/// bar: a drag selects words, not the window.
pub(crate) fn search_field(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let state = shell.rail_state();
    div()
        .flex_1()
        .min_w_0()
        .max_w(crate::tokens::TITLE_SEARCH_W)
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .child(
            search_input(&state.search, cx)
                .short()
                // Its edge alone parts it from the bar: no fill of its own,
                // so it stands on the window's fill as the bar does.
                .bg(gpui::transparent_black())
                .when_some(
                    crate::keymap::first_key("focus_rail_search", cx),
                    |input, key| input.suffix(gpui_component::kbd::Kbd::new(key)),
                ),
        )
}

/// *Sessions*, how many wait on the person, the search and the filter: a heading over
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
    let warning = crate::theme::status_ink(cx).warning;
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
        .child(
            div()
                .text_color(cx.theme().foreground)
                .font_semibold()
                .child("Sessions"),
        )
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
        // The keys go to the title bar's search, as its own key puts them.
        .child(labelled(
            "rail-search-name",
            "Search sessions and projects",
            crate::controls::action("rail-search")
                .quiet_icon(false, cx)
                .short_icon()
                .icon(Icon::new(IconName::Search))
                .named_with_action(
                    "Search sessions and projects",
                    &super::FocusRailSearch,
                    None,
                )
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(super::FocusRailSearch), cx)
                }),
        ))
        .child(labelled(
            "rail-filter-name",
            "Filter sessions",
            // On while it holds anything but the list by project, so a
            // narrowed list says so where it is narrowed.
            crate::controls::action("rail-filter")
                .quiet_icon(filter != Filter::ByProject, cx)
                .short_icon()
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
