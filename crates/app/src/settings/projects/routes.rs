//! The Projects page's *Routed commands*: the commands of the project an
//! agent's build runs on its build host in place of here, and the way to add
//! one.

use super::super::{list_row, list_rows};
use crate::controls::Named as _;
use crate::shell::Shell;
use gpui::{AnyElement, App, Entity, IntoElement, ParentElement, Styled, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, StyledExt};
use onehand_core::workspace::RoutedCommand;
use onehand_ui::{Short as _, ShortIcon as _};
use std::path::Path;

/// How many routed commands the page lists before it says how many it left
/// out: as many as a project routes, so it bites only if that grows.
const ROUTES_SHOWN: usize = 20;

/// What a routed command's row says under its words: what it brings back.
pub(in crate::settings) fn route_line(route: &RoutedCommand) -> String {
    if route.artifacts.is_empty() {
        return "Brings nothing back".into();
    }
    route
        .artifacts
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One row per command the project at `root` routes, then the way to add one.
pub(super) fn routed_commands(
    handle: &Entity<Shell>,
    root: &Path,
    routes: &[RoutedCommand],
    cx: &App,
) -> AnyElement {
    let ink = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    let more = routes.len().saturating_sub(ROUTES_SHOWN);
    let rows: Vec<AnyElement> = routes
        .iter()
        .take(ROUTES_SHOWN)
        .enumerate()
        .map(|(idx, route)| {
            let (edit, remove) = (handle.clone(), handle.clone());
            let (edit_root, remove_root) = (root.to_path_buf(), root.to_path_buf());
            list_row(
                route.words.clone(),
                Some(route_line(route).into_any_element()),
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        crate::controls::action(("edit-route", idx))
                            .short_icon()
                            .secondary()
                            .icon(Icon::new(crate::icons::Icon::SquarePen))
                            .named("Edit")
                            .on_click(move |_, window, cx| {
                                edit.update(cx, |shell, cx| {
                                    shell.open_route_form(edit_root.clone(), Some(idx), window, cx);
                                });
                            }),
                    )
                    .child(
                        crate::controls::action(("remove-route", idx))
                            .short()
                            .secondary()
                            .text_color(ink.danger)
                            .label("Remove…")
                            .on_click(move |_, window, cx| {
                                remove.update(cx, |shell, cx| {
                                    shell.confirm_remove_route(
                                        remove_root.clone(),
                                        idx,
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    ),
                cx,
            )
            .into_any_element()
        })
        .collect();
    let said = |line: String| div().text_xs().text_color(muted).child(line);
    let (add, add_root) = (handle.clone(), root.to_path_buf());
    onehand_ui::field(
        "Routed commands",
        Some(
            "What an agent's build runs on the build host, and what comes back".into_any_element(),
        ),
        div()
            .v_flex()
            .gap_2()
            .children((!rows.is_empty()).then(|| list_rows(rows, cx)))
            .children((more > 0).then(|| said(format!("{more} more not shown"))))
            .children(routes.is_empty().then(|| said("No routed commands".into())))
            .child(
                div().h_flex().child(
                    crate::controls::action("add-route")
                        .short()
                        .secondary()
                        .label("Add routed command…")
                        .on_click(move |_, window, cx| {
                            add.update(cx, |shell, cx| {
                                shell.open_route_form(add_root.clone(), None, window, cx);
                            });
                        }),
                ),
            ),
        cx,
    )
    .into_any_element()
}
