//! The Workbench dock: its mode strip and what each mode shows.
//!
//! Editor, Markdown and Issues share one list/detail rule, decided by the
//! width their container is measured at: side by side from `SPLIT_MIN`, one at
//! a time under a back link naming the list below it. What the person picked,
//! searched, opened and scrolled survives every switch between the two.
use super::composer::item;
use super::*;
use gpui::{Bounds, Entity, Pixels, ScrollHandle, Subscription, canvas};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::switch::Switch;
use std::{cell::Cell, collections::HashSet, rc::Rc};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Editor,
    Markdown,
    Issues,
    Plugins,
    Neovim,
}

const MODES: [(Mode, &str); 5] = [
    (Mode::Editor, "Editor"),
    (Mode::Markdown, "Markdown"),
    (Mode::Issues, "Issues"),
    (Mode::Plugins, "Plugins"),
    (Mode::Neovim, "Neovim"),
];

/// The project tree: path, depth, whether it is a folder.
const TREE: [(&str, usize, bool); 11] = [
    ("src", 0, true),
    ("src/backoff.rs", 1, false),
    ("src/lib.rs", 1, false),
    ("src/retry", 1, true),
    ("src/retry/policy.rs", 2, false),
    ("tests", 0, true),
    ("tests/flaky.rs", 1, false),
    ("benches", 0, true),
    ("benches/retry.rs", 1, false),
    ("Cargo.toml", 0, false),
    ("README.md", 0, false),
];

const DOCS: [(&str, &str); 3] = [
    ("README.md", "atlas-api"),
    ("docs/retry.md", "How retries back off"),
    ("CHANGELOG.md", "0.4.2"),
];

const ISSUES: [(&str, &str, &str); 3] = [
    (
        "Retry test flakes on slow machines",
        "#42",
        "Step 2 of 3 waits for approval",
    ),
    ("Document the config keys", "#39", "No run recorded"),
    (
        "Backoff ignores the jitter setting",
        "#37",
        "Pull request open",
    ),
];

const PLUGINS: [(&str, &str, &str); 4] = [
    (
        "Editor",
        "Opens project files with highlighting for the languages in use.",
        "built in · v0.4.2",
    ),
    (
        "Issues",
        "Reads and answers the project's GitHub issues without leaving the window.",
        "built in · v0.4.2 · needs gh",
    ),
    (
        "Telegram",
        "Sends prompts to a session from a chat, and its answers back.",
        "built in · v0.4.2 · needs a token",
    ),
    (
        "Format on save",
        "Runs the project's formatter after the agent edits a file.",
        "local · v0.1.0 · hooks",
    ),
];

pub(super) struct Wb {
    pub mode: Mode,
    pub maximized: bool,
    mode_menu: bool,
    /// The width the mode's container was last measured at, in rems.
    measured: Rc<Cell<f32>>,
    /// The detail is showing, in the one-at-a-time presentation. The
    /// selection is kept apart from it, so going back keeps what was picked.
    detail: bool,
    file: Option<&'static str>,
    expanded: HashSet<&'static str>,
    search: Entity<InputState>,
    files_scroll: ScrollHandle,
    doc: Option<usize>,
    issue: Option<usize>,
    plugins: [bool; 4],
    _sub: Subscription,
}

impl Wb {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Labs>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search files"));
        let _sub = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            mode: Mode::Editor,
            maximized: false,
            mode_menu: false,
            measured: Rc::new(Cell::new(0.0)),
            detail: true,
            file: Some("src/backoff.rs"),
            expanded: ["src", "tests"].into_iter().collect(),
            search,
            files_scroll: ScrollHandle::new(),
            doc: Some(1),
            issue: Some(0),
            plugins: [true, true, false, true],
            _sub,
        }
    }
}

/// The rows the tree shows: under expanded folders, or every file matching the
/// search, flat.
fn visible(query: &str, expanded: &HashSet<&str>) -> Vec<(&'static str, usize, bool)> {
    let q = query.trim().to_lowercase();
    if !q.is_empty() {
        return TREE
            .iter()
            .filter(|(path, _, dir)| !dir && path.to_lowercase().contains(&q))
            .map(|&(path, _, dir)| (path, 0, dir))
            .collect();
    }
    TREE.iter()
        .filter(|(path, _, _)| {
            // Every folder above it is open.
            let mut parts: Vec<&str> = path.split('/').collect();
            parts.pop();
            (1..=parts.len()).all(|n| expanded.contains(parts[..n].join("/").as_str()))
        })
        .copied()
        .collect()
}

fn source(path: &str) -> &'static str {
    match path {
        "src/backoff.rs" => {
            "pub fn backoff(attempt: u32, clock: &dyn Clock) -> Duration {\n    let base = Duration::from_millis(200);\n    let wait = base * 2u32.pow(attempt.min(5));\n    clock.sleep(wait);\n    wait\n}"
        }
        "tests/flaky.rs" => {
            "#[test]\nfn three_attempts() {\n    let clock = FakeClock::default();\n    assert!(retry(3, &clock, || Err(())).is_err());\n    assert_eq!(clock.slept(), ms(200 + 400 + 800));\n}"
        }
        "Cargo.toml" => "[package]\nname = \"retry\"\nversion = \"0.4.2\"\nedition = \"2024\"",
        _ => "// Nothing interesting in here yet.",
    }
}

impl Labs {
    /// The dock. `width` is what the layout gave it, in rems; the modes decide
    /// by what their container is measured at, falling back to this until the
    /// first measurement.
    pub(super) fn workbench(
        &self,
        p: &Palette,
        width: f32,
        focus: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let wb = &self.wb;
        let measured = match wb.measured.get() {
            m if m > 0.0 => m,
            _ => width,
        };
        let side_by_side = measured >= SPLIT_MIN;
        let compact_strip = width < DOCK_PREF;

        let body = match wb.mode {
            Mode::Editor => self.editor(p, side_by_side, cx).into_any_element(),
            Mode::Markdown => self.markdown(p, side_by_side, cx).into_any_element(),
            Mode::Issues => self.wb_issues(p, side_by_side, cx).into_any_element(),
            Mode::Plugins => self.plugins(p, cx).into_any_element(),
            Mode::Neovim => self.neovim(p, cx).into_any_element(),
        };
        let cell = wb.measured.clone();
        let measure = canvas(
            move |bounds: Bounds<Pixels>, window, _| {
                let w = bounds.size.width / window.rem_size();
                if (cell.get() - w).abs() > 0.05 {
                    cell.set(w);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        v_flex()
            .when(focus, |d| d.flex_1())
            .when(!focus, |d| d.w(rems(width)).flex_none())
            .h_full()
            .bg(p.panel)
            .child(self.mode_strip(p, focus, compact_strip, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(measure)
                    .child(body),
            )
            .child(
                div()
                    .px(rems(INSET))
                    .py(rems(TIGHT))
                    .text_size(rems(TEXT_XS))
                    .text_color(p.muted)
                    .child(format!(
                        "container {measured:.1}rem · {}",
                        if side_by_side {
                            format!("side by side (≥ {SPLIT_MIN}rem)")
                        } else {
                            format!("one at a time (< {SPLIT_MIN}rem)")
                        }
                    )),
            )
    }

    fn mode_strip(
        &self,
        p: &Palette,
        focus: bool,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let wb = &self.wb;
        let current = MODES
            .iter()
            .find(|(m, _)| *m == wb.mode)
            .map(|(_, l)| *l)
            .unwrap_or("");
        let modes: gpui::AnyElement = if compact {
            // Too narrow for every chip: one control naming the mode, opening
            // the rest, rather than letting clipping decide which survive.
            div()
                .relative()
                .child(
                    action("mode-select")
                        .ghost()
                        .small()
                        .label(current)
                        .icon(IconName::ChevronDown)
                        .selected(wb.mode_menu)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.wb.mode_menu = !this.wb.mode_menu;
                            cx.notify();
                        })),
                )
                .when(wb.mode_menu, |d| {
                    let mono = cx.theme().mono_font_family.clone();
                    let items = MODES
                        .iter()
                        .map(|&(mode, label)| {
                            let it = item(label, "").on(std::rc::Rc::new(
                                move |this: &mut Labs, _: &mut Window, cx: &mut Context<Labs>| {
                                    this.wb.mode = mode;
                                    this.wb.mode_menu = false;
                                    cx.notify();
                                },
                            ));
                            if mode == wb.mode { it.current() } else { it }
                        })
                        .collect();
                    d.child(
                        div()
                            .absolute()
                            .top(gpui::relative(1.))
                            .left_0()
                            .pt(rems(TIGHT))
                            .occlude()
                            .child(Self::popup(
                                cx,
                                p,
                                mono,
                                Some(MENU_W),
                                "Mode",
                                items,
                                usize::MAX,
                                Some(vec![("escape", "close")]),
                                None,
                            )),
                    )
                })
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if this.wb.mode_menu {
                        this.wb.mode_menu = false;
                        cx.notify();
                    }
                }))
                .into_any_element()
        } else {
            h_flex()
                .id("modes")
                .flex_1()
                .min_w_0()
                .overflow_x_scroll()
                .gap(rems(TIGHT))
                .children(MODES.iter().map(|&(mode, label)| {
                    action(label)
                        .ghost()
                        .small()
                        .max_w(rems(TAB_MAX_W))
                        .label(label)
                        .selected(mode == wb.mode)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.wb.mode = mode;
                            cx.notify();
                        }))
                }))
                .into_any_element()
        };
        h_flex()
            .h(rems(BAR_H))
            .flex_none()
            .px(rems(CONTROL))
            .gap(rems(TIGHT))
            .border_b_1()
            .border_color(p.hairline)
            .when(focus && !wb.maximized, |d| {
                d.child(
                    action("back-chat")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowLeft)
                        .label("Conversation")
                        .on_click(cx.listener(|this, _, _, cx| {
                            // Steps aside rather than closing: the Workbench
                            // stays open and the split comes back by itself
                            // once the window has room.
                            this.chat_over_workbench = true;
                            cx.notify();
                        })),
                )
                .child(Self::hairline_v(p).h(rems(ICON)))
            })
            .child(modes)
            .when(compact, |d| d.child(div().flex_1()))
            // Fixed at the end, outside anything that scrolls.
            .child(
                self.icon_button(
                    "max-wb",
                    if wb.maximized {
                        IconName::Minimize
                    } else {
                        IconName::Maximize
                    },
                    if wb.maximized { "Restore" } else { "Maximize" },
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.wb.maximized = !this.wb.maximized;
                    this.hovered = None;
                    cx.notify();
                })),
            )
            .child(
                self.icon_button("hide-wb", IconName::Close, "Hide the Workbench", cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.workbench = false;
                        this.wb.maximized = false;
                        this.hovered = None;
                        cx.notify();
                    })),
            )
    }

    /// The list/detail frame the three list modes share.
    fn list_detail(
        &self,
        p: &Palette,
        side_by_side: bool,
        back: &'static str,
        list: gpui::AnyElement,
        detail: Option<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let showing_detail = self.wb.detail && detail.is_some();
        if side_by_side {
            return h_flex()
                .flex_1()
                .min_w_0()
                .items_start()
                .h_full()
                .child(div().w(rems(LIST_W)).flex_none().h_full().child(list))
                .child(Self::hairline_v(p))
                .child(div().flex_1().min_w_0().h_full().children(detail));
        }
        if showing_detail {
            return v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(
                    h_flex()
                        .h(rems(SUBBAR_H))
                        .flex_none()
                        .px(rems(CONTROL))
                        .border_b_1()
                        .border_color(p.hairline)
                        .child(
                            action("back-list")
                                .ghost()
                                .small()
                                .icon(IconName::ArrowLeft)
                                .label(back)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.wb.detail = false;
                                    cx.notify();
                                })),
                        ),
                )
                .children(detail);
        }
        div().flex_1().min_w_0().h_full().child(list)
    }

    fn editor(&self, p: &Palette, side_by_side: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let wb = &self.wb;
        let query = wb.search.read(cx).value().to_string();
        let rows = visible(&query, &wb.expanded);
        let empty = rows.is_empty();
        let list =
            v_flex()
                .h_full()
                .child(
                    div()
                        .p(rems(CONTROL))
                        .border_b_1()
                        .border_color(p.hairline)
                        .child(Input::new(&wb.search).small()),
                )
                .child(
                    div()
                        .id("files")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&wb.files_scroll)
                        .p(rems(CONTROL))
                        .children(rows.into_iter().map(|(path, depth, dir)| {
                            let name = path.rsplit('/').next().unwrap_or(path);
                            let open = wb.expanded.contains(path);
                            let picked = wb.file == Some(path);
                            h_flex()
                                .id(path)
                                .h(rems(ROW_H))
                                .pl(rems(CONTROL + depth as f32 * RAIL_INDENT * 0.5))
                                .pr(rems(CONTROL))
                                .gap(rems(CONTROL))
                                .rounded(rems(RADIUS_SM))
                                .cursor_pointer()
                                .hover(|d| d.bg(p.selected))
                                .text_color(if picked { p.text } else { p.text2 })
                                .when(picked, |d| d.bg(p.selected))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if dir {
                                        if !this.wb.expanded.remove(path) {
                                            this.wb.expanded.insert(path);
                                        }
                                    } else {
                                        this.wb.file = Some(path);
                                        this.wb.detail = true;
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    Icon::new(match (dir, open) {
                                        (true, true) => IconName::FolderOpen,
                                        (true, false) => IconName::FolderClosed,
                                        (false, _) => IconName::File,
                                    })
                                    .size(rems(ICON))
                                    .text_color(p.muted),
                                )
                                .child(super::controls::full(
                                    format!("file-{path}"),
                                    if query.trim().is_empty() { name } else { path },
                                ))
                        }))
                        .when(empty, |d| {
                            d.child(div().px(rems(CONTROL)).text_color(p.muted).child(format!(
                                "No file matches \u{201c}{}\u{201d}.",
                                query.trim()
                            )))
                        }),
                )
                .into_any_element();
        let detail = wb.file.map(|path| {
            v_flex()
                .h_full()
                .child(
                    div()
                        .px(rems(INSET))
                        .py(rems(CONTROL))
                        .text_size(rems(TEXT_XS))
                        .text_color(p.muted)
                        .child(path),
                )
                .child(
                    div()
                        .px(rems(INSET))
                        .font_family(mono)
                        .text_size(self.read(TEXT_READ_SM))
                        .line_height(self.read(TEXT_READ_SM * LEADING_READ))
                        .text_color(p.text2)
                        .child(source(path)),
                )
                .into_any_element()
        });
        self.list_detail(p, side_by_side, "Files", list, detail, cx)
    }

    fn markdown(
        &self,
        p: &Palette,
        side_by_side: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let wb = &self.wb;
        let list = v_flex()
            .p(rems(CONTROL))
            .children(DOCS.iter().enumerate().map(|(i, (name, note))| {
                let picked = wb.doc == Some(i);
                v_flex()
                    .id(("doc", i))
                    .px(rems(CONTROL))
                    .py(rems(TIGHT))
                    .rounded(rems(RADIUS_SM))
                    .cursor_pointer()
                    .hover(|d| d.bg(p.selected))
                    .when(picked, |d| d.bg(p.selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.wb.doc = Some(i);
                        this.wb.detail = true;
                        cx.notify();
                    }))
                    .child(super::controls::full(format!("doc-{i}"), *name).text_color(p.text))
                    .child(
                        div()
                            .truncate()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child(*note),
                    )
            }))
            .into_any_element();
        let detail = wb.doc.map(|i| {
            div()
                .id("doc-reader")
                .h_full()
                .overflow_y_scroll()
                .child(
                    v_flex()
                        .max_w(rems(DOC_MEASURE))
                        .px(rems(INSET))
                        .py(rems(SECTION))
                        .gap(rems(RELATED))
                        .text_size(self.read(TEXT_READ))
                        .line_height(self.read(TEXT_READ * LEADING_DOC))
                        .text_color(p.text)
                        .child(div().text_size(rems(TEXT_XL)).font_medium().child(DOCS[i].1))
                        .child("Every retry waits twice as long as the one before it, from 200 ms, and gives up after five attempts. The wait is read from the clock the caller passes in, so a test can pass a fake one and run in no time at all.")
                        .child("A setting can cap the wait and add jitter; both default to off, so the sequence is the same on every machine unless a project asks otherwise."),
                )
                .into_any_element()
        });
        self.list_detail(p, side_by_side, "Documents", list, detail, cx)
    }

    fn wb_issues(
        &self,
        p: &Palette,
        side_by_side: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let wb = &self.wb;
        let list = v_flex()
            .p(rems(CONTROL))
            .children(
                ISSUES
                    .iter()
                    .enumerate()
                    .map(|(i, (title, reference, work))| {
                        let picked = wb.issue == Some(i);
                        v_flex()
                            .id(("wb-issue", i))
                            .px(rems(CONTROL))
                            .py(rems(TIGHT))
                            .rounded(rems(RADIUS_SM))
                            .cursor_pointer()
                            .hover(|d| d.bg(p.selected))
                            .when(picked, |d| d.bg(p.selected))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.wb.issue = Some(i);
                                this.wb.detail = true;
                                cx.notify();
                            }))
                            .child(
                                super::controls::full(format!("wb-issue-{i}"), *title)
                                    .text_color(p.text),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(rems(TEXT_XS))
                                    .text_color(if i == 0 { p.warning } else { p.muted })
                                    .child(format!("{reference} · {work}")),
                            )
                    }),
            )
            .into_any_element();
        let detail = wb.issue.map(|i| {
            let (title, reference, work) = ISSUES[i];
            v_flex()
                .px(rems(INSET))
                .py(rems(SECTION))
                .gap(rems(RELATED))
                .text_color(p.text)
                .child(div().text_size(rems(TEXT_LG)).font_medium().child(title))
                .child(
                    div()
                        .text_size(rems(TEXT_XS))
                        .text_color(p.muted)
                        .child(format!("Open · atlas-api {reference} · bug")),
                )
                .child(div().text_color(if i == 0 { p.warning } else { p.text2 }).child(work))
                .child(
                    div()
                        .text_size(self.read(TEXT_READ))
                        .line_height(self.read(TEXT_READ * LEADING_READ))
                        .child("The third attempt's backoff uses wall-clock time, so a slow runner pushes it past the test's timeout."),
                )
                .into_any_element()
        });
        // Search and New issue stay above both halves, so they are reachable in
        // the one-at-a-time presentation too.
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .p(rems(CONTROL))
                    .gap(rems(CONTROL))
                    .border_b_1()
                    .border_color(p.hairline)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .h(rems(CONTROL_H))
                            .px(rems(CONTROL))
                            .gap(rems(CONTROL))
                            .rounded(rems(RADIUS_SM))
                            .border_1()
                            .border_color(p.control)
                            .text_color(p.muted)
                            .child(Icon::new(IconName::Search).size(rems(ICON)))
                            .child(div().truncate().child("Search issues")),
                    )
                    .child(
                        action("wb-new-issue")
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label("New issue"),
                    ),
            )
            .child(self.list_detail(p, side_by_side, "Issues", list, detail, cx))
    }

    fn plugins(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("plugins")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .child(
                v_flex().p(rems(INSET)).child(
                    v_flex()
                        .rounded(rems(RADIUS_MD))
                        .border_1()
                        .border_color(p.hairline)
                        .children(PLUGINS.iter().enumerate().map(|(i, (name, about, meta))| {
                            let on = self.wb.plugins[i];
                            h_flex()
                                .items_start()
                                .px(rems(RELATED))
                                .py(rems(CONTROL))
                                .gap(rems(RELATED))
                                .when(i + 1 < PLUGINS.len(), |d| {
                                    d.border_b_1().border_color(p.hairline)
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(rems(TIGHT * 0.5))
                                        .child(div().font_medium().text_color(p.text).child(*name))
                                        .child(div().text_color(p.text2).child(*about))
                                        // Metadata on a line of its own, never
                                        // squeezed beside the controls.
                                        .child(
                                            div()
                                                .text_size(rems(TEXT_XS))
                                                .text_color(p.muted)
                                                .child(*meta),
                                        ),
                                )
                                .child(
                                    h_flex().flex_none().gap(rems(CONTROL)).child(
                                        Switch::new(("plugin-on", i))
                                            .cursor_pointer()
                                            .checked(on)
                                            .on_click(cx.listener(
                                                move |this, checked: &bool, _, cx| {
                                                    this.wb.plugins[i] = *checked;
                                                    cx.notify();
                                                },
                                            )),
                                    ),
                                )
                        })),
                ),
            )
    }

    fn neovim(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let line = rems(TEXT_READ_SM * LEADING_UI);
        // The grid takes everything under the strip: its own lines, then
        // tildes past the end of the buffer, then the status line at the foot.
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .font_family(mono)
            .text_size(rems(TEXT_READ_SM))
            .line_height(line)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .px(rems(CONTROL))
                    .pt(rems(TIGHT))
                    .text_color(p.text2)
                    .children(
                        source("src/backoff.rs")
                            .lines()
                            .map(|l| div().child(l.to_string())),
                    )
                    .children((0..40).map(|_| div().text_color(p.muted).child("~"))),
            )
            .child(
                h_flex()
                    .flex_none()
                    .px(rems(CONTROL))
                    .bg(p.sunken)
                    .text_color(p.text)
                    .child(div().flex_1().child("NORMAL  src/backoff.rs"))
                    .child(div().text_color(p.muted).child("rust  1:1")),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_shows_what_is_open_and_a_search_shows_every_match() {
        let open: HashSet<&str> = ["src"].into_iter().collect();
        let shown: Vec<&str> = visible("", &open).into_iter().map(|r| r.0).collect();
        assert!(shown.contains(&"src/backoff.rs"));
        assert!(shown.contains(&"src/retry"));
        // Under a closed folder.
        assert!(!shown.contains(&"src/retry/policy.rs"));
        assert!(!shown.contains(&"tests/flaky.rs"));
        // A search reaches into closed folders and lists files only.
        let found: Vec<&str> = visible("retry", &open).into_iter().map(|r| r.0).collect();
        assert_eq!(found, ["src/retry/policy.rs", "benches/retry.rs"]);
    }
}
