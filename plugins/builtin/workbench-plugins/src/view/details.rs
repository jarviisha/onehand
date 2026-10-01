use crate::cli::Plugin;
use crate::inventory::Inventory;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div, rems,
};
use gpui_component::{ActiveTheme, StyledExt, WindowExt as _};
use onehand_plugin_host::status_ink;

/// "4 skills", "1 command" … for each counted kind a plugin has any of, in one
/// order wherever counts are said. Hooks are left to the caller: a chip says
/// only that there are some, and a sentence counts them.
pub(super) fn tally(inventory: &Inventory, mcp_one: &str, mcp_many: &str) -> Vec<String> {
    [
        (inventory.skills.len(), "skill", "skills"),
        (inventory.commands.len(), "command", "commands"),
        (inventory.agents.len(), "agent", "agents"),
        (inventory.mcp.len(), mcp_one, mcp_many),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| counted(n, one, many))
    .collect()
}

/// `n` of a thing, named in the singular or the plural as `n` needs.
pub(super) fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What a plugin carries, one chip per kind it has any of — none for a kind
/// it has none of, and no row at all for a plugin whose folder said nothing.
/// *hooks* is in the warning ink and carries no count: a hook is code that
/// runs on its own, and that there is any is the fact worth reading.
pub(super) fn component_chips(plugin: &Plugin, cx: &App) -> Option<AnyElement> {
    let inventory = &plugin.inventory;
    let mut chips: Vec<AnyElement> = tally(inventory, "MCP", "MCP")
        .into_iter()
        .map(|text| chip(text, false, cx))
        .collect();
    if !inventory.hooks.is_empty() {
        chips.push(chip("hooks".to_string(), true, cx));
    }
    // The walk stopped at its bound, so the counts above are not the whole.
    if inventory.cut {
        chips.push(chip("not all read".to_string(), false, cx));
    }
    (!chips.is_empty()).then(|| {
        div()
            .h_flex()
            .flex_wrap()
            .gap_1()
            .pt_0p5()
            .children(chips)
            .into_any_element()
    })
}

fn chip(text: String, warn: bool, cx: &App) -> AnyElement {
    let ink = if warn {
        status_ink(cx).warning
    } else {
        cx.theme().muted_foreground
    };
    div()
        .px_1p5()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(if warn { ink } else { cx.theme().border })
        .text_xs()
        .text_color(ink)
        .child(text)
        .into_any_element()
}

/// A plugin's details, in a drawer over the window: what it says it is, and
/// every skill, command, agent, MCP server and hook it carries — a hook with
/// the command it runs, which is what somebody opens this to check.
pub(super) fn open_details(plugin: &Plugin, window: &mut Window, cx: &mut App) {
    let plugin = plugin.clone();
    window.open_sheet(cx, move |sheet, _, cx| {
        sheet
            .title(plugin.name().to_string())
            // In rems, so the drawer follows the zoom the rest of the panel
            // is read at.
            .size(rems(26.25))
            .child(details(&plugin, cx))
    });
}

fn details(plugin: &Plugin, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mono = theme.mono_font_family.clone();
    let inventory = &plugin.inventory;
    let section = |title: &str, names: &[String]| -> Option<AnyElement> {
        (!names.is_empty()).then(|| {
            div()
                .v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .text_color(muted)
                        .child(format!("{title} · {}", names.len())),
                )
                .children(names.iter().map(|name| div().text_sm().child(name.clone())))
                .into_any_element()
        })
    };
    let hooks = (!inventory.hooks.is_empty()).then(|| {
        div()
            .v_flex()
            .gap_1p5()
            .child(
                div()
                    .text_xs()
                    .font_medium()
                    .text_color(status_ink(cx).warning)
                    .child(format!(
                        "Hooks · {} — run on their own",
                        inventory.hooks.len()
                    )),
            )
            .children(inventory.hooks.iter().map(|hook| {
                let when = match &hook.matcher {
                    Some(matcher) => format!("{} · {matcher}", hook.event),
                    None => hook.event.clone(),
                };
                div()
                    .v_flex()
                    .gap_0p5()
                    .child(div().text_xs().text_color(muted).child(when))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(theme.radius)
                            .bg(theme.secondary)
                            .text_xs()
                            .font_family(mono.clone())
                            .child(hook.command.clone()),
                    )
            }))
            .into_any_element()
    });
    let origin = format!(
        "{} · {}",
        plugin.marketplace(),
        plugin
            .installed
            .iter()
            .map(|scope| scope.label())
            .collect::<Vec<_>>()
            .join(", ")
    );
    div()
        .id("plugin-details")
        .size_full()
        .v_flex()
        .gap_4()
        .overflow_y_scroll()
        .children(
            inventory
                .description
                .clone()
                .map(|text| div().text_sm().child(text)),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child(match &plugin.version {
                    Some(version) => format!("{origin} · {version}"),
                    None => origin,
                }),
        )
        .when(inventory.cut, |body| {
            body.child(div().text_xs().text_color(muted).child(
                "Not every file was read — the folder is larger than the walk allows, so \
                 what is listed here is not all of it.",
            ))
        })
        .children(section("Skills", &inventory.skills))
        .children(section("Commands", &inventory.commands))
        .children(section("Agents", &inventory.agents))
        .children(section("MCP servers", &inventory.mcp))
        .children(hooks)
        .into_any_element()
}

/// One muted line inside the list — an empty section, a cut, a plugin that is
/// already installed where an install would land.
pub(super) fn note(text: impl Into<gpui::SharedString>, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// A count as a person reads one at a glance: `3327` as `3.3k`. The exact
/// number says nothing more about which plugin to pick, and four digits on
/// every row are four digits of noise.
pub(super) fn compact(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{:.1}k", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
    .replace(".0", "")
}
