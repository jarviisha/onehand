//! The rail's pages, Overview through Schedules, under *New session*: rows
//! that each open a page, filled while it shows. Settings is the title bar's.

use super::page_fill;
use crate::shell::Shell;
use crate::tokens::ROW_H;
use gpui::{
    App, ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_ui::{Tint, badge, mark_slot};

/// Overview, Tasks with how many tasks need a person, Issues, Workflows and
/// Schedules.
pub(super) fn pages(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement + use<> {
    let tasks = attention_pill(crate::task::attention(&shell.page_roots(), cx));
    div()
        .v_flex()
        .flex_none()
        .px_1p5()
        .pb_3()
        .gap_0p5()
        .child(page_row(
            ("rail-overview", "Overview"),
            Icon::new(IconName::LayoutDashboard),
            shell.workspace_shown(cx),
            None,
            (
                "What is waiting, working and open across every project",
                None,
            ),
            cx,
            |shell, window, cx| shell.show_workspace(window, cx),
        ))
        .child(page_row(
            ("rail-tasks", "Tasks"),
            Icon::new(IconName::Inbox),
            shell.tasks_shown(cx),
            tasks,
            (
                "Every task, and what each needs",
                Some(&crate::shell::ShowTasks),
            ),
            cx,
            |shell, window, cx| shell.show_tasks(None, window, cx),
        ))
        .child(page_row(
            ("rail-issues", "Issues"),
            Icon::new(crate::icons::Icon::CircleDot),
            shell.issues_shown(cx),
            None,
            (
                "Every issue of every project, and where its work stands",
                None,
            ),
            cx,
            |shell, window, cx| shell.show_issues(window, cx),
        ))
        .child(page_row(
            ("rail-workflows", "Workflows"),
            Icon::new(IconName::Play),
            shell.workflows_shown(cx),
            None,
            ("Every workflow, to run one or write one", None),
            cx,
            |shell, window, cx| shell.show_workflows(window, cx),
        ))
        .child(page_row(
            ("rail-schedules", "Schedules"),
            Icon::new(IconName::Calendar),
            shell.schedules_shown(cx),
            None,
            (
                "Every schedule, and when each runs next",
                Some(&crate::shell::ShowSchedules),
            ),
            cx,
            |shell, window, cx| shell.show_schedules(None, window, cx),
        ))
}

/// A page's row, on the selected fill in full ink while its page shows.
fn page_row<F: Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static>(
    (id, label): (&'static str, &'static str),
    icon: Icon,
    on: bool,
    count: Option<String>,
    (tip, key): (&'static str, Option<&'static dyn gpui::Action>),
    cx: &mut Context<Shell>,
    go: F,
) -> impl IntoElement + use<F> {
    #[expect(
        clippy::disallowed_methods,
        reason = "a rail row made by hand, which no shared builder draws"
    )]
    let row = div()
        .id(id)
        .h_flex()
        .items_center()
        .h(ROW_H)
        .px_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .cursor_pointer();
    page_fill(row, on, cx)
        .tooltip(onehand_ui::tip(move |window, cx| {
            let tip = Tooltip::new(tip);
            match key {
                Some(key) => tip.action(key, None),
                None => tip,
            }
            .build(window, cx)
        }))
        .on_click(
            cx.listener(move |shell: &mut Shell, _: &ClickEvent, window, cx| go(shell, window, cx)),
        )
        .child(mark_slot(icon.small()))
        .child(div().flex_1().child(label))
        .children(count.map(|n| badge(n, rail_badge(cx.theme().secondary_foreground, cx))))
}

/// A badge in the rail: sunk on the ramp's inset step, which stands off the
/// rail's fill, its hovered row and its chosen row alike, in `ink`.
pub(super) fn rail_badge(ink: gpui::Hsla, cx: &App) -> Tint {
    Tint {
        fill: crate::theme::inset(cx),
        ink,
    }
}

/// What the Tasks row's pill reads for `n` tasks needing a person: nothing
/// at zero, so a pill is always news.
pub(super) fn attention_pill(n: usize) -> Option<String> {
    (n > 0).then(|| n.to_string())
}
