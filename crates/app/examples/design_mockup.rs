//! The Issues page mocked up on the app's own fonts and palette, to look the UI
//! contract over by: the rail, the page with its head, filter bar and table,
//! and one issue floating over the list. Static data; hover is the only
//! behaviour.
//!
//! `cargo run -p onehand --example design_mockup [light]`

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, Entity, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Rems, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, WindowBounds, WindowOptions, div, px, relative, rems, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::tag::Tag;
use gpui_component::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Root, Sizable as _, StyledExt as _, Theme,
    ThemeMode,
};
use onehand::{assets::Assets, shell};
use onehand_plugin_host::tokens::{
    BAR_H, CODE_TEXT, CONTROL_H, LEADING, LEADING_TIGHT, PAGE_WIDE, ROW_H, lift,
};
use onehand_plugin_host::{
    AppIcon, Quiet as _, Tall as _, action, search_input, status_ink, tab_chip, tint, well,
};

/// The rail at its narrowest drag stop, near enough at the default rem.
const RAIL_W: Rems = rems(15.);
/// The floating issue's width, before anyone drags it.
const ISSUE_W: Rems = rems(28.);
/// The search's floor in the filter bar, where the bar wraps instead.
const SEARCH_W: Rems = rems(14.);
/// Fixed columns, so the title is the only thing that gives way and a longer
/// number or age does not push its neighbours.
const NUMBER_W: Rems = rems(2.75);
const LABELS_W: Rems = rems(11.);
const UPDATED_W: Rems = rems(4.5);
/// How many issues the table draws before it says how many more there are.
const ISSUE_CAP: usize = 8;

#[derive(Clone, Copy)]
enum State {
    Open,
    Running,
    Waiting,
    Done,
}

#[derive(Clone, Copy)]
enum Hue {
    Blue,
    Green,
    Yellow,
    Red,
    Magenta,
}

struct Issue {
    number: u32,
    title: &'static str,
    state: State,
    labels: &'static [(&'static str, Hue)],
    updated: &'static str,
}

const ISSUES: &[Issue] = &[
    Issue {
        number: 152,
        title: "Composer loses the draft when a session reconnects mid-turn",
        state: State::Waiting,
        labels: &[("bug", Hue::Red), ("chat", Hue::Blue)],
        updated: "4m",
    },
    Issue {
        number: 149,
        title: "Workbench strip: keep the action slots while the mode label truncates",
        state: State::Running,
        labels: &[("ui", Hue::Magenta)],
        updated: "18m",
    },
    Issue {
        number: 147,
        title: "Settings ▸ Agents shows the wrong command after a rename",
        state: State::Open,
        labels: &[("bug", Hue::Red)],
        updated: "1h",
    },
    Issue {
        number: 144,
        title: "Issues page: the picked issue floats over the list",
        state: State::Done,
        labels: &[("ui", Hue::Magenta), ("issues", Hue::Green)],
        updated: "3h",
    },
    Issue {
        number: 141,
        title: "Rail search should match project names as well as session titles",
        state: State::Open,
        labels: &[("rail", Hue::Yellow)],
        updated: "5h",
    },
    Issue {
        number: 138,
        title: "Bundle the UI and mono fonts so text centres the same everywhere",
        state: State::Done,
        labels: &[("ui", Hue::Magenta)],
        updated: "1d",
    },
    Issue {
        number: 133,
        title: "Move the launch rules into core so the remote bridge can share them",
        state: State::Open,
        labels: &[("core", Hue::Blue)],
        updated: "2d",
    },
    Issue {
        number: 129,
        title: "Unattended run: a step that fails goes back to the one its margin names",
        state: State::Open,
        labels: &[("workflows", Hue::Green)],
        updated: "3d",
    },
    Issue {
        number: 121,
        title: "Remote bridge drops a reply longer than one message",
        state: State::Open,
        labels: &[("remote", Hue::Yellow)],
        updated: "6d",
    },
    Issue {
        number: 118,
        title: "Terminal dock keeps its dragged height across projects",
        state: State::Open,
        labels: &[("terminal", Hue::Blue)],
        updated: "1w",
    },
    Issue {
        number: 112,
        title: "Progress ticket on the issue for every step a run takes",
        state: State::Done,
        labels: &[("issues", Hue::Green)],
        updated: "2w",
    },
    Issue {
        number: 104,
        title: "Workflow library: a built-in workflow is read-only",
        state: State::Done,
        labels: &[("workflows", Hue::Green)],
        updated: "3w",
    },
];

/// The row picked, and so the issue floating over the list.
const PICKED: usize = 1;

struct Mockup {
    rail_search: Entity<InputState>,
    page_search: Entity<InputState>,
}

impl Mockup {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            rail_search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            page_search: cx.new(|cx| InputState::new(window, cx).placeholder("Search issues")),
        }
    }
}

impl Render for Mockup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .size_full()
            .h_flex()
            .font_family(theme.font_family.clone())
            .text_color(theme.foreground)
            .bg(theme.background)
            .child(rail(&self.rail_search, cx))
            .child(pane(&self.page_search, cx))
    }
}

/// The middle ink: a detail beside a name, mixed from the full ink it sits
/// under toward the muted one. The app mixes it in its theme module, out of an
/// example's reach, so it is mixed here by the same rule; on a fill, `full` is
/// that fill's paired ink, so the step stays solid there too.
fn middle_ink(full: Hsla, cx: &App) -> Hsla {
    full.mix_oklab(cx.theme().muted_foreground, 0.5)
}

/// A mark's slot: every icon and dot down a column sits in one of these, so
/// the marks share a centre line and the labels after them start on one edge.
fn slot(mark: impl IntoElement) -> Div {
    div()
        .w_4()
        .flex_none()
        .h_flex()
        .justify_center()
        .child(mark)
}

// ── Rail ──────────────────────────────────────────────────────────────────

fn rail(search: &Entity<InputState>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .w(RAIL_W)
        .h_full()
        .flex_none()
        .v_flex()
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .text_sm()
        .child(workspace_bar(cx))
        .child(
            div()
                .h_flex()
                .gap_2()
                .px_2()
                .pb_3()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(search_input(search, cx).small()),
                )
                .child(
                    action("new")
                        .quiet(cx)
                        .small()
                        .icon(IconName::Plus)
                        .label("New")
                        .dropdown_caret(true),
                ),
        )
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .px_2()
                .pb_3()
                .child(nav_row(
                    "overview",
                    IconName::LayoutDashboard.into(),
                    "Overview",
                    None,
                    false,
                    cx,
                ))
                .child(nav_row(
                    "tasks",
                    IconName::Inbox.into(),
                    "Tasks",
                    Some(count_pill(2, cx)),
                    false,
                    cx,
                ))
                .child(nav_row(
                    "issues",
                    Icon::new(AppIcon::CircleDot),
                    "Issues",
                    None,
                    true,
                    cx,
                ))
                .child(nav_row(
                    "workflows",
                    IconName::Play.into(),
                    "Workflows",
                    None,
                    false,
                    cx,
                )),
        )
        .child(
            div()
                .h(ROW_H)
                .mx_2()
                .px_1p5()
                .h_flex()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .text_color(theme.muted_foreground)
                        .child("Sessions"),
                )
                .child(div().flex_1())
                .child(
                    Tag::custom(theme.transparent, status_ink(cx).warning, theme.transparent)
                        .small()
                        .rounded(theme.radius_lg)
                        .child("2 need attention"),
                ),
        )
        .child(
            div()
                .id("rail-list")
                .flex_1()
                .overflow_y_scroll()
                .v_flex()
                .gap_0p5()
                .p_2()
                .child(project_row("onehand-gpui", "main", true, cx))
                .child(session_row(
                    "s1",
                    State::Waiting,
                    "Fix the composer draft on reconnect",
                    "waiting 4m · Mock UI",
                    false,
                    cx,
                ))
                .child(session_row(
                    "s2",
                    State::Running,
                    "Workbench strip action slots",
                    "running · Mock terminal · 18m",
                    true,
                    cx,
                ))
                .child(session_row(
                    "s3",
                    State::Done,
                    "Tidy the settings form",
                    "done · Mock UI · 2h",
                    false,
                    cx,
                ))
                .child(div().h_2())
                .child(project_row("darkvoid", "feat/config", false, cx))
                .child(div().h_2())
                .child(
                    div()
                        .id("add-project")
                        .h(ROW_H)
                        .px_1p5()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .rounded(theme.radius_tokens().sm)
                        .cursor_pointer()
                        .text_color(theme.muted_foreground)
                        .hover(|s| s.bg(cx.theme().sidebar_accent))
                        .child(slot(Icon::new(IconName::Plus).small()))
                        .child("Add project…"),
                ),
        )
        .child(div().p_2().child(nav_row(
            "settings",
            IconName::Settings.into(),
            "Settings",
            None,
            false,
            cx,
        )))
}

/// The workspace bar, level with the pane's header: its tile on the marks'
/// centre line, its name in full ink, the switcher and *Hide the rail*.
fn workspace_bar(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .h(BAR_H)
        .flex_none()
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .child(
            div()
                .size_5()
                .flex_none()
                .h_flex()
                .justify_center()
                .rounded(theme.radius)
                .bg(theme.accent)
                .text_xs()
                .font_semibold()
                .text_color(theme.accent_foreground)
                .child("O"),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_medium()
                .text_color(theme.foreground)
                .child("onehand"),
        )
        .child(
            action("switch-workspace")
                .quiet(cx)
                .small()
                .icon(Icon::new(IconName::ChevronsUpDown).text_color(theme.muted_foreground))
                .tooltip("Workspaces, and the projects in this one"),
        )
        .child(div().flex_1())
        .child(
            action("hide-rail")
                .quiet(cx)
                .small()
                .icon(Icon::new(IconName::PanelLeftClose).text_color(theme.muted_foreground))
                .tooltip("Hide the rail"),
        )
}

/// A page's row in the rail: muted at rest; where the person is, the selected
/// fill and the ink paired with it; hover changes the fill only.
fn nav_row(
    id: &'static str,
    icon: Icon,
    label: &'static str,
    end: Option<AnyElement>,
    chosen: bool,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let hover = theme.sidebar_accent;
    div()
        .id(id)
        .h(ROW_H)
        .px_1p5()
        .h_flex()
        .items_center()
        .gap_2()
        .rounded(theme.radius_tokens().sm)
        .cursor_pointer()
        .text_color(match chosen {
            true => theme.accent_foreground,
            false => theme.muted_foreground,
        })
        .when(chosen, |row| row.bg(theme.accent))
        .when(!chosen, |row| row.hover(move |s| s.bg(hover)))
        .child(slot(icon.small()))
        .child(div().flex_1().truncate().child(label))
        .children(end)
}

fn count_pill(n: usize, cx: &App) -> AnyElement {
    let theme = cx.theme();
    Tag::custom(theme.secondary, theme.secondary_foreground, theme.secondary)
        .small()
        .rounded(theme.radius_lg)
        .child(n.to_string())
        .into_any_element()
}

fn project_row(name: &'static str, branch: &'static str, open: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let hover = theme.sidebar_accent;
    div()
        .id(name)
        .group(name)
        .h(ROW_H)
        .px_1p5()
        .h_flex()
        .items_center()
        .gap_2()
        .rounded(theme.radius_tokens().sm)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(slot(
            Icon::new(match open {
                true => IconName::ChevronDown,
                false => IconName::ChevronRight,
            })
            .xsmall()
            .text_color(theme.muted_foreground),
        ))
        .child(slot(
            Icon::new(IconName::Folder)
                .small()
                .text_color(theme.muted_foreground),
        ))
        .child(div().min_w_0().truncate().font_medium().child(name))
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .truncate()
                .child(branch),
        )
        .child(div().flex_1())
        // Kept in its slot while hidden, so showing it moves nothing.
        .child(
            div()
                .h_flex()
                .flex_none()
                .invisible()
                .group_hover(name, |s| s.visible())
                .child(
                    action(SharedString::from(format!("{name}-new")))
                        .quiet(cx)
                        .xsmall()
                        .icon(IconName::Plus)
                        .tooltip("New session"),
                )
                .child(
                    action(SharedString::from(format!("{name}-more")))
                        .quiet(cx)
                        .xsmall()
                        .icon(IconName::Ellipsis)
                        .tooltip("More"),
                ),
        )
}

/// A session under its project: indented by the chevron's slot and its gap,
/// so its state dot sits under the folder and its title starts under the
/// project's name; the meta line starts under the title.
fn session_row(
    id: &'static str,
    state: State,
    title: &'static str,
    meta: &'static str,
    chosen: bool,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let hover = theme.sidebar_accent;
    let meta_ink = match state {
        State::Waiting => status_ink(cx).warning,
        _ => theme.muted_foreground,
    };
    div()
        .id(id)
        .group(id)
        .ml_6()
        .px_1p5()
        .py_1()
        .v_flex()
        .rounded(theme.radius_tokens().sm)
        .cursor_pointer()
        .when(chosen, |row| {
            row.bg(theme.accent).text_color(theme.accent_foreground)
        })
        .when(!chosen, |row| row.hover(move |s| s.bg(hover)))
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(slot(state_dot(state, cx)))
                .child(div().flex_1().min_w_0().truncate().child(title))
                .child(
                    div()
                        .flex_none()
                        .when(!chosen, |d| d.invisible().group_hover(id, |s| s.visible()))
                        .child(
                            action(SharedString::from(format!("{id}-more")))
                                .quiet(cx)
                                .xsmall()
                                .icon(IconName::Ellipsis)
                                .tooltip("More"),
                        ),
                ),
        )
        .child(
            div()
                .pl_6()
                .text_xs()
                .text_color(meta_ink)
                .truncate()
                .child(meta),
        )
}

fn state_dot(state: State, cx: &App) -> impl IntoElement {
    div().size_2().rounded_full().bg(state_color(state, cx))
}

fn state_color(state: State, cx: &App) -> Hsla {
    match state {
        State::Open => cx.theme().muted_foreground,
        State::Running => cx.theme().link,
        State::Waiting => status_ink(cx).warning,
        State::Done => status_ink(cx).success,
    }
}

fn state_word(state: State) -> &'static str {
    match state {
        State::Open => "Open",
        State::Running => "Running",
        State::Waiting => "Waiting on you",
        State::Done => "Done",
    }
}

fn hue(hue: Hue, cx: &App) -> Hsla {
    let theme = cx.theme();
    match hue {
        Hue::Blue => theme.blue,
        Hue::Green => theme.green,
        Hue::Yellow => theme.yellow,
        Hue::Red => theme.red,
        Hue::Magenta => theme.magenta,
    }
}

fn label_tag(name: &'static str, h: Hue, cx: &App) -> impl IntoElement {
    let t = tint(hue(h, cx), cx);
    Tag::custom(t.fill, t.ink, t.fill)
        .small()
        .rounded(cx.theme().radius_lg)
        .child(name)
}

// ── Agent pane ────────────────────────────────────────────────────────────

fn pane(search: &Entity<InputState>, cx: &App) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .v_flex()
        // The header owns the page's name; the body does not repeat it. Its
        // surface runs on into the body's, so no line is drawn under it.
        .child(
            div()
                .h(BAR_H)
                .flex_none()
                .h_flex()
                .items_center()
                .px_3()
                .text_sm()
                .font_medium()
                .child("Issues"),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    div()
                        .id("page")
                        .size_full()
                        .overflow_y_scroll()
                        .h_flex()
                        .justify_center()
                        .items_start()
                        .child(
                            div()
                                .w_full()
                                .max_w(PAGE_WIDE)
                                .p_6()
                                .v_flex()
                                .gap_6()
                                .text_sm()
                                .child(
                                    div()
                                        .v_flex()
                                        .gap_4()
                                        .child(page_head(cx))
                                        .child(filter_bar(search, cx)),
                                )
                                .child(table(cx)),
                        ),
                )
                .child(floating_issue(&ISSUES[PICKED], cx)),
        )
}

/// What the page counts, in muted ink; then its actions, the icon-only ghost
/// first and the page's one primary last.
fn page_head(cx: &App) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{} open", ISSUES.len())),
        )
        .child(
            action("refresh")
                .quiet(cx)
                .tall()
                .icon(Icon::new(AppIcon::Refresh))
                .tooltip("Refresh"),
        )
        .child(action("new-issue").tall().primary().label("New issue"))
}

/// The search, taking what the row leaves, then each filter's trigger.
fn filter_bar(search: &Entity<InputState>, cx: &App) -> impl IntoElement {
    div()
        .h_flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w(SEARCH_W)
                .child(search_input(search, cx).tall()),
        )
        .child(trigger("showing", "Open"))
        .child(trigger("progress", "Progress"))
        .child(trigger("project", "onehand-gpui"))
        .child(trigger("label", "Label"))
}

/// A filter's trigger: what it is set to, then its caret, in the ink its
/// variant pairs with its fill.
fn trigger(id: &'static str, label: &'static str) -> Button {
    action(id)
        .tall()
        .secondary()
        .label(label)
        .dropdown_caret(true)
}

fn table(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let shown = ISSUES.len().min(ISSUE_CAP);
    div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .v_flex()
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .child(
                    div()
                        .h(ROW_H)
                        .px_3()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .border_b_1()
                        .border_color(theme.border)
                        // The mark's and the number's slots, so "Title" stands
                        // over the titles.
                        .child(div().w_4().flex_none())
                        .child(div().w(NUMBER_W).flex_none())
                        .child(div().flex_1().child("Title"))
                        .child(div().w(LABELS_W).flex_none().child("Labels"))
                        .child(div().w(UPDATED_W).flex_none().child("Updated"))
                        .child(div().w(CONTROL_H).flex_none()),
                )
                .children(
                    ISSUES
                        .iter()
                        .take(shown)
                        .enumerate()
                        .map(|(i, issue)| issue_row(i, issue, i + 1 == shown, cx)),
                ),
        )
        .when(ISSUES.len() > shown, |d| {
            d.child(
                div()
                    .px_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} more", ISSUES.len() - shown)),
            )
        })
}

/// A row: its title in full ink, its number beside it in the middle ink, its
/// age in muted ink, its action held in its slot until hover or selection.
/// Picked, it sits on the selected fill and its full ink is that fill's pair.
fn issue_row(i: usize, issue: &Issue, last: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let picked = i == PICKED;
    let hover = theme.list_hover;
    let group = SharedString::from(format!("issue-{i}"));
    let full = match picked {
        true => theme.accent_foreground,
        false => theme.foreground,
    };
    div()
        .id(("issue", i))
        .group(group.clone())
        .h(ROW_H)
        .px_3()
        .h_flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .when(!last, |row| row.border_b_1().border_color(theme.border))
        .text_color(full)
        .when(picked, |row| row.bg(theme.accent))
        .when(!picked, |row| row.hover(move |s| s.bg(hover)))
        .child(slot(state_icon(issue.state, cx)))
        .child(
            div()
                .w(NUMBER_W)
                .flex_none()
                .font_family(theme.mono_font_family.clone())
                .text_size(CODE_TEXT)
                .text_color(middle_ink(full, cx))
                .child(format!("#{}", issue.number)),
        )
        .child(div().flex_1().min_w_0().truncate().child(issue.title))
        .child(
            div()
                .w(LABELS_W)
                .flex_none()
                .h_flex()
                .gap_1()
                .overflow_hidden()
                .children(issue.labels.iter().map(|(name, h)| label_tag(name, *h, cx))),
        )
        .child(
            div()
                .w(UPDATED_W)
                .flex_none()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(issue.updated),
        )
        .child(
            div()
                .w(CONTROL_H)
                .flex_none()
                .when(!picked, |d| {
                    d.invisible().group_hover(group, |s| s.visible())
                })
                .child(
                    action(("issue-more", i))
                        .quiet(cx)
                        .xsmall()
                        .icon(IconName::Ellipsis)
                        .tooltip("More"),
                ),
        )
}

fn state_icon(state: State, cx: &App) -> Icon {
    let icon = match state {
        State::Done => Icon::new(IconName::CircleCheck),
        State::Waiting => Icon::new(IconName::TriangleAlert),
        State::Open | State::Running => Icon::new(AppIcon::CircleDot),
    };
    icon.small().text_color(state_color(state, cx))
}

// ── The floating issue ────────────────────────────────────────────────────

fn floating_issue(issue: &Issue, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .absolute()
        .top_3()
        .right_3()
        .bottom_3()
        .w(ISSUE_W)
        .v_flex()
        .bg(theme.popover)
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius_lg)
        .shadow(lift(cx))
        .overflow_hidden()
        .text_sm()
        .child(issue_trail(issue, cx))
        .child(
            div()
                .id("issue-body")
                .flex_1()
                .overflow_y_scroll()
                .p_4()
                .v_flex()
                .gap_4()
                .child(issue_tags(issue, cx))
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xl()
                                .font_semibold()
                                .line_height(relative(LEADING_TIGHT))
                                .child(issue.title),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Opened by jarviisha · 2 days ago · step 2 of 4, Implement"),
                        ),
                )
                // What it waits on: the state's ink on its mark, then words.
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(slot(
                            Icon::new(IconName::Loader).small().text_color(theme.link),
                        ))
                        .child("Implementing on")
                        .child(
                            div()
                                .font_family(theme.mono_font_family.clone())
                                .text_size(CODE_TEXT)
                                .child("issue/149-strip-slots"),
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .gap_1()
                        .pb_2()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            tab_chip(
                                "tab-desc",
                                "Description".into(),
                                "The issue's body".into(),
                                true,
                                cx,
                            )
                            .pr_2(),
                        )
                        .child(
                            tab_chip(
                                "tab-activity",
                                "Activity 3".into(),
                                "What happened to it".into(),
                                false,
                                cx,
                            )
                            .pr_2(),
                        ),
                )
                .child(issue_body(cx))
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Files it names"),
                        )
                        .children(
                            [
                                "crates/app/src/workbench/panel/strip.rs",
                                "crates/plugin-host/src/tabs.rs",
                            ]
                            .map(|path| {
                                div()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_size(CODE_TEXT)
                                    .text_color(theme.link)
                                    .child(path)
                            }),
                        ),
                ),
        )
}

/// Where the issue sits, and its actions at the end: one primary, one other,
/// the rest behind `⋯`.
fn issue_trail(issue: &Issue, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .h(BAR_H)
        .flex_none()
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(theme.muted_foreground)
                .child("onehand-gpui"),
        )
        .child(
            Icon::new(IconName::ChevronRight)
                .xsmall()
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .text_size(CODE_TEXT)
                .child(format!("#{}", issue.number)),
        )
        .child(div().flex_1())
        .child(action("run").primary().small().label("Run workflow…"))
        .child(action("review").quiet(cx).small().label("Review…"))
        .child(
            action("issue-menu")
                .quiet(cx)
                .small()
                .icon(IconName::Ellipsis)
                .tooltip("More"),
        )
        .child(
            action("close-issue")
                .quiet(cx)
                .small()
                .icon(IconName::Close)
                .tooltip("Close"),
        )
}

fn issue_tags(issue: &Issue, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let state = tint(state_color(issue.state, cx), cx);
    div()
        .h_flex()
        .flex_wrap()
        .gap_1()
        .child(
            Tag::custom(state.fill, state.ink, state.fill)
                .small()
                .rounded(theme.radius_lg)
                .child(state_word(issue.state)),
        )
        .child(
            Tag::custom(theme.secondary, theme.secondary_foreground, theme.secondary)
                .small()
                .rounded(theme.radius_lg)
                .child("onehand-gpui"),
        )
        .children(issue.labels.iter().map(|(name, h)| label_tag(name, *h, cx)))
}

fn issue_body(cx: &App) -> impl IntoElement {
    div()
        .v_flex()
        .gap_3()
        .text_base()
        .line_height(relative(LEADING))
        .child(
            "When the Workbench strip is narrow, the mode label truncates and the buttons \
             at its end shift left as it does. The trailing group should keep its width \
             while the label gives way.",
        )
        .child(
            "To reproduce, narrow the dock until the modes become a select, then hover a \
             button:",
        )
        .child(
            well(cx)
                .p_3()
                .text_size(CODE_TEXT)
                .child("make dev ROOT=~/code/onehand-gpui"),
        )
}

fn main() {
    let light = std::env::args().nth(1).as_deref() == Some("light");
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            shell::install_look(cx);
            Theme::change(
                match light {
                    true => ThemeMode::Light,
                    false => ThemeMode::Dark,
                },
                None,
                cx,
            );
            let bounds = Bounds::centered(None, size(px(1360.), px(860.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| Mockup::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("the mock-up window opens");
            cx.activate(true);
        });
}
