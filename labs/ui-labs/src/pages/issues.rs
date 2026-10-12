//! The Issues page: the list alone in the page column, one line a row; the
//! issue picked floats over the list's right side, under the toolbar, and
//! the list never moves when it opens or closes. Esc closes it, and the arrow
//! keys step through the list while it holds the keys.
use super::*;
use gpui::KeyDownEvent;

/// Where an issue's work stands, which is the only thing a row colours.
#[derive(Clone, Copy, PartialEq)]
enum Stand {
    NeedsYou,
    Running,
    Quiet,
}

struct Issue {
    title: &'static str,
    project: &'static str,
    number: &'static str,
    labels: &'static [&'static str],
    /// Where its work stands, in a sentence.
    work: &'static str,
    stand: Stand,
    /// What a one-line row says at its end: what needs the person, a run
    /// going, or how long ago it changed.
    end: &'static str,
    updated: &'static str,
}

const ISSUES: [Issue; 6] = [
    Issue {
        title: "Retry test flakes on slow machines",
        project: "atlas-api",
        number: "#42",
        labels: &["bug", "auto"],
        work: "Step 2 of 3 waits for approval",
        stand: Stand::NeedsYou,
        end: "waits for approval",
        updated: "2h ago",
    },
    Issue {
        title: "Chart legend overlaps the axis",
        project: "dashboard-web",
        number: "#17",
        labels: &["bug"],
        work: "Running · step 1 of 3",
        stand: Stand::Running,
        end: "running",
        updated: "20m ago",
    },
    Issue {
        title: "Document the config keys",
        project: "atlas-api",
        number: "#39",
        labels: &["docs"],
        work: "No run recorded",
        stand: Stand::Quiet,
        end: "1d ago",
        updated: "1d ago",
    },
    Issue {
        title: "Move settings to schema v3",
        project: "infra",
        number: "#8",
        labels: &["refactor"],
        work: "Pull request open",
        stand: Stand::Quiet,
        end: "pull request open",
        updated: "3d ago",
    },
    Issue {
        title: "Search reaches into closed folders",
        project: "dashboard-web",
        number: "#21",
        labels: &["feature"],
        work: "No run recorded",
        stand: Stand::Quiet,
        end: "4d ago",
        updated: "4d ago",
    },
    Issue {
        title: "Check command times out on CI",
        project: "infra",
        number: "#11",
        labels: &["bug", "auto"],
        work: "Queued behind another run on infra",
        stand: Stand::Quiet,
        end: "queued",
        updated: "5d ago",
    },
];

/// The issue's facts on one line: `Open · atlas-api #42 · updated 2h ago ·
/// bug · auto`. *Open* is a plain fact, as a label is.
fn facts(issue: &Issue) -> String {
    let reference = format!("{} {}", issue.project, issue.number);
    let updated = format!("updated {}", issue.updated);
    ["Open", reference.as_str(), updated.as_str()]
        .into_iter()
        .chain(issue.labels.iter().copied())
        .collect::<Vec<_>>()
        .join(" · ")
}

impl Labs {
    pub(super) fn issues(&self, p: &Palette, avail: f32, cx: &mut Context<Self>) -> AnyElement {
        let list = div()
            .id("issues-list-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(rems(PAGE_MAX))
                    .mx_auto()
                    .px_4()
                    .pb_6()
                    .child(self.issue_lines(p, avail, cx)),
            );
        // The panel sits on the list's area, so the toolbar above it stays in
        // reach, and the list under it stays where it was.
        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(list)
            .children(self.issue.map(|i| self.issue_panel(p, i, avail, cx)));
        v_flex()
            .id("issues-page")
            .track_focus(&self.issues_focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.issues_key(e, cx)))
            .flex_1()
            .min_h_0()
            .text_color(p.text)
            .child(self.issue_toolbar(p, cx))
            .child(body)
            .into_any_element()
    }

    /// Esc closes the issue; the arrows pick the row above or below.
    fn issues_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let last = ISSUES.len() - 1;
        self.issue = match (e.keystroke.key.as_str(), self.issue) {
            ("escape", Some(_)) => None,
            ("up", Some(i)) => Some(i.saturating_sub(1)),
            ("down", Some(i)) => Some((i + 1).min(last)),
            ("up" | "down", None) => Some(0),
            _ => return,
        };
        cx.stop_propagation();
        cx.notify();
    }

    /// One row of controls over the list, wrapping when narrow: the search,
    /// open or closed, the three filters, and *New issue*.
    fn issue_toolbar(&self, p: &Palette, cx: &App) -> impl IntoElement {
        let filter = |id: &'static str, label: &'static str| {
            action(id).ghost().small().label(label).dropdown_caret(true)
        };
        let search = h_flex()
            .h_6()
            .w(rems(ISSUE_SEARCH_W))
            .px_2()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(p.control)
            .bg(p.panel)
            .text_color(p.muted)
            .child(Icon::new(IconName::Search).small())
            .child("Search issues");
        div().w_full().child(
            h_flex()
                .w_full()
                .max_w(rems(PAGE_MAX))
                .mx_auto()
                .flex_wrap()
                .gap_2()
                .px_4()
                .pt_6()
                .pb_3()
                .child(search)
                .child(
                    h_flex()
                        .gap_0p5()
                        .child(
                            action("open-n")
                                .ghost()
                                .small()
                                .label("Open 6")
                                .selected(true),
                        )
                        .child(action("closed-n").ghost().small().label("Closed 40")),
                )
                .child(filter("progress", "Progress"))
                .child(filter("project", "All projects"))
                .child(filter("label", "Label"))
                .child(div().flex_1())
                .child(
                    action("new-issue-listed")
                        .primary()
                        .small()
                        .icon(IconName::Plus)
                        .label("New issue"),
                ),
        )
    }

    /// The list in one hairline box, a row a line: the state dot, the title,
    /// then in columns the labels, the project and number, and the end.
    fn issue_lines(&self, p: &Palette, avail: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let columns = avail >= ISSUE_COLUMNS_MIN;
        let last = ISSUES.len() - 1;
        let mono = cx.theme().mono_font_family.clone();
        v_flex()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(p.hairline)
            .bg(p.panel)
            .overflow_hidden()
            .children(ISSUES.iter().enumerate().map(|(i, issue)| {
                let ink = match issue.stand {
                    Stand::NeedsYou => p.warning,
                    Stand::Running => p.accent,
                    Stand::Quiet => p.muted,
                };
                let dot = match issue.stand {
                    Stand::Quiet => None,
                    _ => Some(ink),
                };
                h_flex()
                    .id(("issue-line", i))
                    .h(rems(ISSUE_ROW_H))
                    .px_3()
                    .gap_3()
                    .cursor_pointer()
                    .when(i != last, |d| d.border_b_1().border_color(p.hairline))
                    .when(self.issue == Some(i), |d| d.bg(p.selected))
                    .hover(|d| d.bg(p.selected))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.issue = Some(i);
                        this.issues_focus.focus(window, cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(rems(DOT_COLUMN))
                            .flex_none()
                            .children(dot.map(|d| div().size(rems(DOT)).rounded_full().bg(d))),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(issue.title))
                    .when(columns, |d| {
                        d.child(
                            div()
                                .w(rems(ISSUE_LABELS_W))
                                .flex_none()
                                .truncate()
                                .text_xs()
                                .text_color(p.muted)
                                .child(issue.labels.join(" · ")),
                        )
                        .child(
                            h_flex()
                                .w(rems(ISSUE_PROJECT_W))
                                .flex_none()
                                .gap_1()
                                .text_xs()
                                .text_color(p.muted)
                                .child(div().min_w_0().truncate().child(issue.project))
                                .child(
                                    div()
                                        .flex_none()
                                        .font_family(mono.clone())
                                        .child(issue.number),
                                ),
                        )
                    })
                    .child(
                        div()
                            .w(rems(ISSUE_END_W))
                            .flex_none()
                            .truncate()
                            .text_xs()
                            .text_color(ink)
                            .child(issue.end),
                    )
            }))
    }

    /// The title over its facts.
    fn issue_head(p: &Palette, issue: &Issue) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(div().text_xl().font_medium().child(issue.title))
            .child(div().text_xs().text_color(p.muted).child(facts(issue)))
    }

    /// Everything under the head: where the work
    /// stands with its one primary, the body, what the work left.
    fn issue_parts(&self, p: &Palette, issue: &Issue, cx: &App) -> [AnyElement; 3] {
        let (ink, primary) = match issue.stand {
            Stand::NeedsYou => (p.warning, "Review…"),
            Stand::Running => (p.accent, "Open session"),
            Stand::Quiet => (p.text2, "Run workflow…"),
        };
        let stands = v_flex()
            .p_3()
            .gap_2()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(p.hairline)
            .bg(p.panel)
            .child(div().font_medium().child("Where it stands"))
            .child(div().text_color(ink).child(issue.work))
            .child(
                h_flex()
                    .gap_2()
                    .child(action("i-primary").primary().small().label(primary))
                    .child(div().flex_1())
                    .child(
                        action("i-more")
                            .ghost()
                            .small()
                            .icon(IconName::Ellipsis)
                            .tooltip("More"),
                    ),
            );
        let body = div()
            .text_size(self.read(TEXT_READ))
            .line_height(self.read(TEXT_READ * LEADING_READ))
            .child("The retry test fails about one run in five on CI. The third attempt's backoff uses wall-clock time, so a slow runner pushes it past the test's 5s timeout.");
        let left = Self::section(
            p,
            "What the work left",
            None,
            None,
            Self::rows(
                p,
                cx,
                vec![
                    Row {
                        id: "b",
                        title: "fix/retry-clock",
                        meta: "branch · 2 commits ahead of main",
                        dot: None,
                        end: None,
                    },
                    Row {
                        id: "c",
                        title: "Check passed",
                        meta: "cargo test · 14:10",
                        dot: Some(p.success),
                        end: None,
                    },
                ],
                "",
            ),
        );
        [
            stands.into_any_element(),
            body.into_any_element(),
            left.into_any_element(),
        ]
    }

    /// The issue floating over the list's right side, at a reading
    /// measure; where the area cannot hold it beside a strip of the list, it
    /// takes the whole area. Its edge and lift say it floats.
    fn issue_panel(&self, p: &Palette, i: usize, avail: f32, cx: &mut Context<Self>) -> AnyElement {
        let issue = &ISSUES[i];
        let whole = avail < PEEK_W + PEEK_LIST_MIN;
        let mono = cx.theme().mono_font_family.clone();
        v_flex()
            .id("issue-panel")
            .occlude()
            .absolute()
            .top_0()
            .right_4()
            .bottom_4()
            .when(whole, |d| d.left_4())
            .when(!whole, |d| d.w(rems(PEEK_W)))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(p.hairline)
            .bg(p.panel)
            .shadow_lg()
            .text_color(p.text)
            .child(
                h_flex()
                    .h(rems(SUBBAR_H))
                    .flex_none()
                    .pl_4()
                    .pr_2()
                    .gap_2()
                    .text_xs()
                    .text_color(p.muted)
                    .child(
                        div()
                            .flex_1()
                            .child(format!("{} {}", issue.project, issue.number))
                            .font_family(mono),
                    )
                    .child(div().child(format!("{} of {}", i + 1, ISSUES.len())))
                    .child(
                        action("issue-panel-close")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .tooltip("Close (Esc)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.issue = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("issue-panel-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .px_4()
                            .pt_2()
                            .pb_6()
                            .gap_6()
                            .child(Self::issue_head(p, issue))
                            .children(self.issue_parts(p, issue, cx)),
                    ),
            )
            .into_any_element()
    }
}
