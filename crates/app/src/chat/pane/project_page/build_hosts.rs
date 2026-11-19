//! The project page's *Build hosts*: where the project builds, and what
//! comes back from each.

use super::{ChatPane, ChatPaneEvent};
use crate::controls::Named as _;
use crate::controls::Short as _;
use crate::settings::{list_row, list_rows};
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, Icon, StyledExt};
use onehand_core::workspace::BuildHost;
use onehand_ui::ShortIcon as _;

/// How many build hosts the page lists before it says how many it left out.
const BUILD_HOSTS_SHOWN: usize = 20;

/// What a build host's row says under its name: the paths a build brings
/// back, and the folder they land under when it names one.
pub(crate) fn build_host_line(entry: &BuildHost) -> String {
    if entry.artifacts.is_empty() {
        return "Brings nothing back".into();
    }
    let paths: Vec<String> = entry
        .artifacts
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    match &entry.into {
        Some(into) => format!("{}, into {}", paths.join(", "), into.display()),
        None => paths.join(", "),
    }
}

/// The group under *Check command*: one row per host the project builds on,
/// then the way to add one.
pub(super) fn build_hosts_group(builds: &[BuildHost], cx: &mut Context<ChatPane>) -> AnyElement {
    let ink = crate::theme::status_ink(cx);
    let muted = cx.theme().muted_foreground;
    let more = builds.len().saturating_sub(BUILD_HOSTS_SHOWN);
    let rows: Vec<AnyElement> = builds
        .iter()
        .take(BUILD_HOSTS_SHOWN)
        .enumerate()
        .map(|(idx, entry)| {
            list_row(
                entry.host.to_string(),
                Some(build_host_line(entry).into_any_element()),
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        crate::controls::action(("edit-build-host", idx))
                            .short_icon()
                            .secondary()
                            .icon(Icon::new(crate::icons::Icon::SquarePen))
                            .named("Edit")
                            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                                cx.emit(ChatPaneEvent::EditBuildHost(idx));
                            })),
                    )
                    .child(
                        crate::controls::action(("remove-build-host", idx))
                            .short()
                            .secondary()
                            .text_color(ink.danger)
                            .label("Remove…")
                            .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                                cx.emit(ChatPaneEvent::RemoveBuildHost(idx));
                            })),
                    ),
                cx,
            )
            .into_any_element()
        })
        .collect();
    let said = |line: String| div().text_xs().text_color(muted).child(line);
    onehand_ui::field(
        "Build hosts",
        Some("Where this project builds, and what comes back".into_any_element()),
        div()
            .v_flex()
            .gap_2()
            .children((!rows.is_empty()).then(|| list_rows(rows, cx)))
            .children((more > 0).then(|| said(format!("{more} more not shown"))))
            .children(builds.is_empty().then(|| said("No build hosts".into())))
            .child(
                div().h_flex().child(
                    crate::controls::action("add-build-host")
                        .short()
                        .secondary()
                        .label("Add build host…")
                        .on_click(cx.listener(|_: &mut ChatPane, _, _, cx| {
                            cx.emit(ChatPaneEvent::AddBuildHost);
                        })),
                ),
            ),
        cx,
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::build_host_line;
    use onehand_core::workspace::{BuildHost, HostName};

    fn entry(artifacts: &[&str], into: Option<&str>) -> BuildHost {
        BuildHost {
            host: HostName("build01".into()),
            artifacts: artifacts.iter().map(Into::into).collect(),
            into: into.map(Into::into),
        }
    }

    #[test]
    fn a_row_says_what_comes_back_and_where_it_lands() {
        assert_eq!(
            build_host_line(&entry(&["target/release/onehand", "dist"], None)),
            "target/release/onehand, dist"
        );
        assert_eq!(
            build_host_line(&entry(&["bin/Release"], Some("out/windows"))),
            "bin/Release, into out/windows"
        );
        assert_eq!(build_host_line(&entry(&[], None)), "Brings nothing back");
        assert_eq!(
            build_host_line(&entry(&[], Some("out"))),
            "Brings nothing back"
        );
    }
}
