//! The three pages that take the agent pane: overview, Tasks (with a task's
//! detail in the same column) and Issues (list beside the issue when wide,
//! one at a time under a back link when not).
use super::*;
use gpui::{AnyElement, FontWeight};

/// One row of a sectioned list: a title over its facts, a state dot in a
/// stable column, and whatever the row offers at the end.
struct Row {
    id: &'static str,
    title: &'static str,
    meta: &'static str,
    dot: Option<Hsla>,
    end: Option<AnyElement>,
}

impl Labs {
    pub(super) fn page_view(
        &self,
        p: &Palette,
        avail: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (title, body): (&str, AnyElement) = match self.page {
            Page::Overview => ("Workspace overview", self.overview(p).into_any_element()),
            Page::Tasks if self.task_open => ("Tasks", self.task_detail(p, cx).into_any_element()),
            Page::Tasks => ("Tasks", self.tasks(p, cx).into_any_element()),
            Page::Issues => ("Issues", self.issues(p, avail, cx).into_any_element()),
            Page::Composer => (
                "Composer cards",
                self.composer_gallery(p, cx).into_any_element(),
            ),
            Page::Chat => unreachable!("the chat is not a page"),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .h(rems(BAR_H))
                    .flex_none()
                    .px(rems(INSET))
                    .border_b_1()
                    .border_color(p.hairline)
                    .child(div().flex_1().font_medium().text_color(p.text).child(title))
                    .child(
                        Button::new("theme-page")
                            .ghost()
                            .small()
                            .icon(IconName::Palette)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dark = !this.dark;
                                set_mode(this.dark, window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(body)
    }

    /// A scrolling page column: one inset, one section rhythm.
    pub(super) fn column(id: &'static str) -> gpui::Stateful<gpui::Div> {
        div().id(id).flex_1().overflow_y_scroll()
    }

    pub(super) fn inner(p: &Palette) -> gpui::Div {
        v_flex()
            .w_full()
            .max_w(rems(PAGE_MAX))
            .mx_auto()
            .px(rems(INSET))
            .py(rems(SECTION))
            .gap(rems(SECTION))
            .text_color(p.text)
    }

    fn section(
        p: &Palette,
        title: &'static str,
        count: Option<usize>,
        extra: Option<AnyElement>,
        body: impl IntoElement,
    ) -> impl IntoElement {
        v_flex()
            .gap(rems(CONTROL))
            .child(
                h_flex()
                    .gap(rems(CONTROL))
                    .child(div().text_size(rems(TEXT_MD)).font_medium().child(title))
                    .children(count.map(|n| {
                        div()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child(n.to_string())
                    }))
                    .child(div().flex_1())
                    .children(extra),
            )
            .child(body)
    }

    /// Rows in one hairline box, divided by hairlines. An empty list says so.
    fn rows(p: &Palette, rows: Vec<Row>, empty: &'static str) -> impl IntoElement {
        let boxed = v_flex()
            .rounded(rems(RADIUS_MD))
            .border_1()
            .border_color(p.hairline)
            .bg(p.panel);
        if rows.is_empty() {
            return boxed.child(
                div()
                    .px(rems(RELATED))
                    .py(rems(CONTROL))
                    .text_color(p.muted)
                    .child(empty),
            );
        }
        let last = rows.len() - 1;
        boxed.children(rows.into_iter().enumerate().map(|(i, r)| {
            h_flex()
                .id(r.id)
                .px(rems(RELATED))
                .py(rems(CONTROL))
                .gap(rems(RELATED))
                .when(i != last, |d| d.border_b_1().border_color(p.hairline))
                .child(
                    div()
                        .w(rems(DOT))
                        .flex_none()
                        .children(r.dot.map(|d| div().size(rems(DOT)).rounded_full().bg(d))),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(r.title))
                        .child(
                            div()
                                .truncate()
                                .text_size(rems(TEXT_XS))
                                .text_color(p.muted)
                                .child(r.meta),
                        ),
                )
                .children(r.end)
        }))
    }

    fn badge(p: &Palette, label: &'static str, ink: Hsla, fill: Option<Hsla>) -> impl IntoElement {
        div()
            .flex_none()
            .px(rems(CONTROL))
            .rounded_full()
            .border_1()
            .border_color(if fill.is_some() {
                ink.opacity(0.0)
            } else {
                p.control
            })
            .when_some(fill, |d, f| d.bg(f))
            .text_size(rems(TEXT_XS))
            .text_color(ink)
            .child(label)
    }

    fn action(id: &'static str, label: &'static str) -> AnyElement {
        Button::new(id)
            .ghost()
            .small()
            .label(label)
            .into_any_element()
    }

    fn overview(&self, p: &Palette) -> impl IntoElement {
        let tile =
            |name: &'static str, git: &'static str, sessions: &'static str, dot: Option<Hsla>| {
                v_flex()
                    .w(rems(TILE_W))
                    .p(rems(RELATED))
                    .gap(rems(TIGHT))
                    .rounded(rems(RADIUS_MD))
                    .border_1()
                    .border_color(p.hairline)
                    .bg(p.panel)
                    .child(
                        h_flex()
                            .gap(rems(CONTROL))
                            .child(
                                Icon::new(IconName::Folder)
                                    .size(rems(ICON))
                                    .text_color(p.muted),
                            )
                            .child(div().flex_1().truncate().font_medium().child(name))
                            .children(dot.map(|d| div().size(rems(DOT)).rounded_full().bg(d))),
                    )
                    .child(
                        div()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child(git),
                    )
                    .child(
                        div()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.text2)
                            .child(sessions),
                    )
            };
        Self::column("overview").child(
            Self::inner(p)
                .child(Self::section(
                    p,
                    "Waiting on you",
                    Some(2),
                    None,
                    Self::rows(
                        p,
                        vec![
                            Row {
                                id: "w1",
                                title: "Inspect dashboard",
                                meta: "atlas-api · claude · waiting for permission",
                                dot: Some(p.warning),
                                end: Some(Self::action("w1-open", "Open session")),
                            },
                            Row {
                                id: "w2",
                                title: "Retry with backoff — step 2 of 3",
                                meta: "atlas-api · issue #42 · waiting for approval",
                                dot: Some(p.warning),
                                end: Some(Self::action("w2-review", "Review…")),
                            },
                        ],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Working",
                    Some(1),
                    None,
                    Self::rows(
                        p,
                        vec![Row {
                            id: "k1",
                            title: "Fix flaky retry test",
                            meta: "atlas-api · claude · running 4 min",
                            dot: Some(p.accent),
                            end: None,
                        }],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Projects",
                    None,
                    None,
                    h_flex()
                        .flex_wrap()
                        .gap(rems(RELATED))
                        .child(tile(
                            "atlas-api",
                            "main · 3 changes",
                            "4 sessions · 1 waiting",
                            Some(p.warning),
                        ))
                        .child(tile(
                            "dashboard-web",
                            "feat/charts · clean",
                            "2 sessions",
                            Some(p.accent),
                        ))
                        .child(tile("infra", "main · clean", "No sessions", None)),
                ))
                .child(Self::section(
                    p,
                    "Recent conversations",
                    None,
                    None,
                    Self::rows(
                        p,
                        vec![
                            Row {
                                id: "r1",
                                title: "Rename config keys",
                                meta: "atlas-api · 2 h ago",
                                dot: None,
                                end: None,
                            },
                            Row {
                                id: "r2",
                                title: "Chart legend overlaps",
                                meta: "dashboard-web · yesterday",
                                dot: None,
                                end: None,
                            },
                        ],
                        "",
                    ),
                )),
        )
    }

    fn tasks(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let filter = Button::new("filter")
            .ghost()
            .small()
            .label("All projects")
            .icon(IconName::ChevronDown)
            .into_any_element();
        Self::column("tasks").child(
            Self::inner(p)
                .child(Self::section(
                    p,
                    "Needs attention",
                    Some(2),
                    Some(filter),
                    Self::rows(
                        p,
                        vec![
                            Row {
                                id: "t1",
                                title: "Retry with backoff",
                                meta: "Fix an issue · step 2 of 3 waits for approval · atlas-api",
                                dot: Some(p.warning),
                                end: Some(
                                    Button::new("t1-open")
                                        .ghost()
                                        .small()
                                        .label("Open")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.task_open = true;
                                            cx.notify();
                                        }))
                                        .into_any_element(),
                                ),
                            },
                            Row {
                                id: "t2",
                                title: "Migrate settings schema",
                                meta: "Check failed · cargo test · infra",
                                dot: Some(p.danger),
                                end: Some(Self::action("t2-retry", "Retry")),
                            },
                        ],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Running",
                    Some(1),
                    None,
                    Self::rows(
                        p,
                        vec![Row {
                            id: "t3",
                            title: "Chart legend overlaps",
                            meta: "Fix an issue · step 1 of 3 · dashboard-web",
                            dot: Some(p.accent),
                            end: Some(Self::action("t3-stop", "Stop")),
                        }],
                        "",
                    ),
                ))
                .child(Self::section(
                    p,
                    "Queued",
                    Some(0),
                    None,
                    Self::rows(p, vec![], "Nothing queued."),
                ))
                .child(Self::section(
                    p,
                    "Finished",
                    Some(1),
                    None,
                    Self::rows(
                        p,
                        vec![Row {
                            id: "t4",
                            title: "Run check",
                            meta: "Passed · true · atlas-api · 10:42",
                            dot: Some(p.success),
                            end: Some(Self::action("t4-dismiss", "Dismiss")),
                        }],
                        "",
                    ),
                )),
        )
    }

    fn task_detail(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let step = |label: &'static str, state: Option<bool>| {
            h_flex()
                .gap(rems(CONTROL))
                .py(rems(TIGHT))
                .child(match state {
                    Some(true) => Icon::new(IconName::Check)
                        .size(rems(ICON))
                        .text_color(p.muted),
                    Some(false) => Icon::new(IconName::ChevronRight)
                        .size(rems(ICON))
                        .text_color(p.warning),
                    None => Icon::new(IconName::Minus)
                        .size(rems(ICON))
                        .text_color(p.muted),
                })
                .child(
                    div()
                        .when(state.is_none(), |d| d.text_color(p.muted))
                        .child(label),
                )
        };
        Self::column("task").child(
            Self::inner(p)
                .child(
                    v_flex()
                        .gap(rems(CONTROL))
                        .child(
                            div().child(
                                Button::new("back-tasks")
                                    .ghost()
                                    .small()
                                    .icon(IconName::ArrowLeft)
                                    .label("Tasks")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.task_open = false;
                                        cx.notify();
                                    })),
                            ),
                        )
                        .child(
                            h_flex()
                                .gap(rems(RELATED))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(rems(TEXT_XL))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child("Retry with backoff"),
                                )
                                .child(Button::new("open-session").outline().small().label("Open session")),
                        )
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap(rems(CONTROL))
                                .text_size(rems(TEXT_XS))
                                .text_color(p.muted)
                                .child(Self::badge(p, "Awaiting approval", p.warning, Some(p.warning_bg)))
                                .child("Fix an issue · atlas-api · issue #42 · started 14:02 · 12 min"),
                        ),
                )
                .child(Self::section(
                    p,
                    "Steps",
                    None,
                    None,
                    v_flex()
                        .child(step("Plan", Some(true)))
                        .child(step("Implement", Some(false)))
                        .child(step("Open pull request", None)),
                ))
                .child(Self::section(
                    p,
                    "Awaiting approval",
                    None,
                    None,
                    v_flex()
                        .gap(rems(RELATED))
                        .child(
                            div()
                                .p(rems(RELATED))
                                .rounded(rems(RADIUS_MD))
                                .bg(p.sunken)
                                .font_family(mono)
                                .text_size(rems(TEXT_READ_SM))
                                .text_color(p.text2)
                                .child("Backoff now reads an injected clock. tests/flaky.rs passes 200 runs in a row.\n2 files changed, +38 −11"),
                        )
                        .child(
                            h_flex()
                                .gap(rems(CONTROL))
                                .justify_end()
                                .child(Button::new("revise").ghost().small().label("Revise…"))
                                .child(Button::new("continue").primary().small().label("Continue")),
                        ),
                )),
        )
    }

    fn issues(&self, p: &Palette, avail: f32, cx: &mut Context<Self>) -> impl IntoElement {
        const ISSUES: [(&str, &str, &str, bool, &[&str]); 4] = [
            (
                "Retry test flakes on slow machines",
                "atlas-api #42",
                "Step 2 of 3 waits for approval",
                true,
                &["bug", "auto"],
            ),
            (
                "Chart legend overlaps the axis",
                "dashboard-web #17",
                "Running · step 1 of 3",
                false,
                &["bug"],
            ),
            (
                "Document the config keys",
                "atlas-api #39",
                "No run recorded",
                false,
                &["docs"],
            ),
            (
                "Move settings to schema v3",
                "infra #8",
                "Pull request open",
                false,
                &["refactor"],
            ),
        ];
        let side_by_side = avail >= ISSUE_LIST_W + DETAIL_MIN;
        let picked = self.issue;

        let search = h_flex()
            .h(rems(CONTROL_H))
            .flex_1()
            .px(rems(CONTROL))
            .gap(rems(CONTROL))
            .rounded(rems(RADIUS_SM))
            .border_1()
            .border_color(p.control)
            .bg(p.panel)
            .text_color(p.muted)
            .child(Icon::new(IconName::Search).size(rems(ICON)))
            .child("Search issues");
        let list = v_flex()
            .when(side_by_side, |d| d.w(rems(ISSUE_LIST_W)).flex_none())
            .when(!side_by_side, |d| d.flex_1())
            .h_full()
            .child(
                v_flex()
                    .p(rems(RELATED))
                    .gap(rems(CONTROL))
                    .border_b_1()
                    .border_color(p.hairline)
                    .child(
                        h_flex().gap(rems(CONTROL)).child(search).child(
                            Button::new("new-issue")
                                .primary()
                                .small()
                                .icon(IconName::Plus)
                                .label("New issue"),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap(rems(TIGHT))
                            .child(
                                Button::new("open-n")
                                    .ghost()
                                    .small()
                                    .label("Open 4")
                                    .selected(true),
                            )
                            .child(Button::new("closed-n").ghost().small().label("Closed 40"))
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(rems(TEXT_XS))
                                    .text_color(p.muted)
                                    .child("read 2m ago"),
                            ),
                    ),
            )
            .child(
                div()
                    .id("issue-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(ISSUES.iter().enumerate().map(
                        |(i, (title, reference, work, needs_you, labels))| {
                            v_flex()
                                .id(i)
                                .px(rems(RELATED))
                                .py(rems(CONTROL))
                                .gap(rems(TIGHT * 0.5))
                                .border_b_1()
                                .border_color(p.hairline)
                                .when(picked == Some(i), |d| d.bg(p.selected))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.issue = Some(i);
                                    cx.notify();
                                }))
                                .child(div().truncate().text_color(p.text).child(*title))
                                .child(
                                    div()
                                        .text_size(rems(TEXT_XS))
                                        .text_color(p.muted)
                                        .child(*reference),
                                )
                                .child(
                                    h_flex()
                                        .gap(rems(CONTROL))
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .text_size(rems(TEXT_XS))
                                                .text_color(if *needs_you {
                                                    p.warning
                                                } else {
                                                    p.text2
                                                })
                                                .child(*work),
                                        )
                                        .children(
                                            labels.iter().map(|l| Self::badge(p, l, p.text2, None)),
                                        ),
                                )
                        },
                    )),
            );

        let detail = |cx: &mut Context<Self>| {
            let (title, reference, _, _, labels) = ISSUES[picked.unwrap_or(0)];
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .when(!side_by_side, |d| {
                    d.child(
                        h_flex()
                            .h(rems(SUBBAR_H))
                            .px(rems(CONTROL))
                            .border_b_1()
                            .border_color(p.hairline)
                            .child(
                                Button::new("back-issues")
                                    .ghost()
                                    .small()
                                    .icon(IconName::ArrowLeft)
                                    .label("Issues")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.issue = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .child(
                    div().id("issue").flex_1().overflow_y_scroll().child(
                        v_flex()
                            .max_w(rems(READ_MAX))
                            .px(rems(INSET))
                            .py(rems(SECTION))
                            .gap(rems(SECTION))
                            .text_color(p.text)
                            .child(
                                v_flex()
                                    .gap(rems(CONTROL))
                                    .child(div().text_size(rems(TEXT_XL)).font_medium().child(title))
                                    .child(
                                        h_flex()
                                            .flex_wrap()
                                            .gap(rems(CONTROL))
                                            .text_size(rems(TEXT_XS))
                                            .text_color(p.muted)
                                            .child(Self::badge(p, "Open", p.success, None))
                                            .child(format!("{reference} · opened by jarviisha · 3 days ago"))
                                            .children(labels.iter().map(|l| Self::badge(p, l, p.text2, None))),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .p(rems(RELATED))
                                    .gap(rems(CONTROL))
                                    .rounded(rems(RADIUS_MD))
                                    .border_1()
                                    .border_color(p.hairline)
                                    .bg(p.panel)
                                    .child(
                                        h_flex()
                                            .gap(rems(CONTROL))
                                            .child(div().font_medium().child("Where it stands"))
                                            .child(div().text_size(rems(TEXT_XS)).text_color(p.muted).child("step 2 of 3")),
                                    )
                                    .child(
                                        div()
                                            .text_color(p.warning)
                                            .child("Implement is done and waits for your approval before the pull request."),
                                    )
                                    .child(
                                        h_flex()
                                            .gap(rems(CONTROL))
                                            .child(Button::new("i-review").primary().small().label("Review…"))
                                            .child(Button::new("i-open").ghost().small().label("Open session"))
                                            .child(div().flex_1())
                                            .child(Button::new("i-more").ghost().small().icon(IconName::Ellipsis)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(rems(TEXT_READ))
                                    .line_height(rems(TEXT_READ * LEADING_READ))
                                    .child("The retry test fails about one run in five on CI. The third attempt's backoff uses wall-clock time, so a slow runner pushes it past the test's 5s timeout."),
                            )
                            .child(Self::section(
                                p,
                                "What the work left",
                                None,
                                None,
                                Self::rows(
                                    p,
                                    vec![
                                        Row { id: "b", title: "fix/retry-clock", meta: "branch · 2 commits ahead of main", dot: None, end: None },
                                        Row { id: "c", title: "Check passed", meta: "cargo test · 14:10", dot: Some(p.success), end: None },
                                    ],
                                    "",
                                ),
                            )),
                    ),
                )
        };

        let body = h_flex().flex_1().min_h_0().items_start();
        if side_by_side {
            body.child(list)
                .child(Self::hairline_v(p))
                .child(detail(cx))
        } else if picked.is_some() {
            body.child(detail(cx))
        } else {
            body.child(list)
        }
    }
}
