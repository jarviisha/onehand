//! Every card and popup that lives on the composer, as a gallery of
//! specimens: what is pinned above it (permission, questions, the queue, a
//! reconnect), what opens from it (the `+` menu, `@` and `/` completion,
//! model and effort, the mode and branch menus) and its own states.
use super::*;
use gpui::{AnyElement, Keystroke, SharedString};
use gpui_component::{
    Disableable as _, checkbox::Checkbox, kbd::Kbd, radio::Radio, spinner::Spinner,
};

/// What the composer card itself is showing.
#[derive(Default)]
pub(super) struct ComposerLook {
    pub text: Option<&'static str>,
    pub running: bool,
    pub tray: bool,
    pub open_chip: Option<&'static str>,
}

/// What a popup row does when picked.
pub(super) type Act = std::rc::Rc<dyn Fn(&mut Labs, &mut Window, &mut Context<Labs>)>;

/// A row in a popup. `group` starts a labelled run of rows.
pub(super) struct Item {
    group: Option<&'static str>,
    icon: Option<IconName>,
    label: &'static str,
    detail: &'static str,
    mono: bool,
    current: bool,
    pub(super) on: Option<Act>,
}

pub(super) fn item(label: &'static str, detail: &'static str) -> Item {
    Item {
        group: None,
        icon: None,
        label,
        detail,
        mono: false,
        current: false,
        on: None,
    }
}

impl Item {
    pub(super) fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
    pub(super) fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
    pub(super) fn current(mut self) -> Self {
        self.current = true;
        self
    }
    pub(super) fn group(mut self, g: &'static str) -> Self {
        self.group = Some(g);
        self
    }
    pub(super) fn on(mut self, act: Act) -> Self {
        self.on = Some(act);
        self
    }
}

/// The field's placeholder.
pub(super) const PLACEHOLDER: &str = "Ask the agent…";

/// The composer's card: the field and its row of controls on one surface,
/// with no edge drawn: its own fill and its lift set it apart. The library's
/// shadows are too soft to outline a white card on a white page, so the lift
/// is two of its own: a tight one where the edge would be, a soft one under.
pub(super) fn card(p: &Palette, cx: &App) -> gpui::Div {
    let shadow = |color, y, blur| gpui::BoxShadow {
        color,
        offset: gpui::point(gpui::Pixels::ZERO, gpui::px(y)),
        blur_radius: gpui::px(blur),
        spread_radius: gpui::Pixels::ZERO,
        inset: false,
    };
    v_flex()
        .rounded(cx.theme().radius_lg)
        .bg(p.raised)
        .shadow(vec![
            shadow(p.lift_edge, 0., LIFT_EDGE_BLUR_PX),
            shadow(p.lift, LIFT_DROP_PX, LIFT_BLUR_PX),
        ])
        .p_1p5()
        .gap_1()
}

/// A chip in the composer's row or the strip under it: a ghost button at the
/// height of a small button, with the library's extra-small caret. What it
/// says is its children, so each chip letters it at `text_xs` itself.
pub(super) fn chip(id: &'static str, open: bool) -> Button {
    action(id)
        .ghost()
        .xsmall()
        .selected(open)
        .h_6()
        .px_1p5()
        .gap_1()
}

/// A chip's word, at the composer's metadata size.
fn chip_text(p: &Palette, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .min_w_0()
        .truncate()
        .text_xs()
        .text_color(p.text)
        .child(text.into())
}

/// `+`: everything that can be put into the prompt from a control. Its glyph
/// is drawn larger than a chip's, because it is aimed at by its shape alone.
pub(super) fn plus_chip(open: bool) -> Button {
    chip("plus", open)
        .child(Icon::new(IconName::Plus).size_5())
        .tooltip("Attach files, mention or run a command")
}

/// Fast mode: the bolt and the word for its state, so the chip says both what
/// it is and where it stands.
pub(super) fn fast_chip(id: &'static str, on: bool, p: &Palette) -> Button {
    chip(id, false)
        .child(
            Icon::empty()
                .path(crate::assets::ZAP)
                .xsmall()
                .text_color(p.muted),
        )
        .child(chip_text(p, if on { "On" } else { "Off" }))
        .tooltip(if on {
            "Fast mode is on: quicker answers"
        } else {
            "Fast mode is off"
        })
}

/// The model in force, its effort after it in muted ink so the two read as a
/// name and its qualifier rather than one name.
pub(super) fn model_chip(p: &Palette, model: &str, effort: &str, open: bool) -> Button {
    chip("model", open)
        .child(chip_text(p, model.to_string()))
        .child(
            div()
                .text_xs()
                .text_color(p.muted)
                .child(effort.to_lowercase()),
        )
        .dropdown_caret(true)
        .tooltip("Choose model and effort")
}

/// A pressable fact on the strip under the card: its glyph in muted ink, its
/// words in full. The branch at the left, the permission mode at the right.
pub(super) fn strip_chip(
    id: &'static str,
    p: &Palette,
    glyph: &'static str,
    text: impl Into<SharedString>,
    open: bool,
) -> Button {
    chip(id, open)
        .child(Icon::empty().path(glyph).xsmall().text_color(p.muted))
        .child(chip_text(p, text))
}

pub(super) fn key(k: &str) -> Kbd {
    Kbd::new(Keystroke::parse(k).expect("a keystroke"))
}

impl Labs {
    // ---- the composer card -------------------------------------------------

    pub(super) fn composer_card(p: &Palette, cx: &App, look: ComposerLook) -> impl IntoElement {
        let open = |id: &str| look.open_chip == Some(id);
        let tray = look.tray.then(|| {
            h_flex()
                .flex_wrap()
                .gap_1()
                .child(Self::attachment(p, cx, IconName::File, "retry.rs", "4 KB"))
                .child(Self::attachment(
                    p,
                    cx,
                    IconName::Frame,
                    "screenshot.png",
                    "182 KB",
                ))
                .child(Self::attachment(
                    p,
                    cx,
                    IconName::File,
                    "ci-log.txt",
                    "51 KB",
                ))
        });
        let send = if look.running {
            h_flex()
                .gap_1()
                .when(look.text.is_some(), |d| {
                    d.child(action("queue").outline().small().label("Queue"))
                })
                .child(action("stop").danger().small().label("Stop"))
                .into_any_element()
        } else {
            action("send")
                .primary()
                .small()
                .icon(IconName::ArrowUp)
                .tooltip("Send")
                .disabled(look.text.is_none() && !look.tray)
                .into_any_element()
        };
        v_flex()
            .w_full()
            .child(
                card(p, cx)
                    .children(tray)
                    .child(
                        div()
                            .min_h_10()
                            .text_size(rems(TEXT_READ))
                            .line_height(rems(TEXT_READ * LEADING_READ))
                            .text_color(if look.text.is_some() { p.text } else { p.muted })
                            .child(look.text.unwrap_or(PLACEHOLDER)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(plus_chip(open("plus")))
                            .child(fast_chip("fast", false, p))
                            .child(model_chip(p, "Sonnet 5", "High", open("model")))
                            .child(div().flex_1())
                            .child(send),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .px_1p5()
                    .pt_1p5()
                    .child(strip_chip(
                        "branch",
                        p,
                        crate::assets::GIT_BRANCH,
                        "main · 3 changes",
                        open("branch"),
                    ))
                    .child(strip_chip(
                        "mode",
                        p,
                        crate::assets::SHIELD,
                        "Ask before edits",
                        open("mode"),
                    )),
            )
    }

    fn attachment(
        p: &Palette,
        cx: &App,
        icon: IconName,
        name: &'static str,
        size: &'static str,
    ) -> impl IntoElement {
        h_flex()
            .h_6()
            .pl_2()
            .pr_1()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(p.hairline)
            .bg(p.sunken)
            .text_xs()
            .child(Icon::new(icon).xsmall().text_color(p.muted))
            .child(div().text_color(p.text).child(name))
            .child(div().text_color(p.muted).child(size))
            .child(
                action(name)
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Remove"),
            )
    }

    // ---- pinned cards ------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    /// A pinned card: what it asks, the body, and a footer of keys and answers.
    fn pinned(
        p: &Palette,
        cx: &App,
        icon: IconName,
        ink: Hsla,
        title: &'static str,
        meta: &'static str,
        body: Option<AnyElement>,
        keys: Vec<(&'static str, &'static str)>,
        actions: Vec<AnyElement>,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(p.control)
            .bg(p.panel)
            .child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Icon::new(icon).small().text_color(ink))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().font_medium().text_color(p.text).child(title))
                                    // Who asks, on a line of its own, so a long
                                    // question keeps the whole width.
                                    .child(div().text_xs().text_color(p.muted).child(meta)),
                            ),
                    )
                    .children(body),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(p.hairline)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .text_xs()
                            .text_color(p.muted)
                            .children(
                                keys.into_iter()
                                    .map(|(k, what)| h_flex().gap_1().child(key(k)).child(what)),
                            ),
                    )
                    .children(actions),
            )
    }

    fn well(
        p: &Palette,
        cx: &App,
        mono: SharedString,
        max_lines: f32,
        text: &'static str,
    ) -> gpui::Div {
        div()
            .p_3()
            .rounded(cx.theme().radius_lg)
            .bg(p.sunken)
            .font_family(mono)
            .text_size(rems(TEXT_READ_SM))
            .line_height(rems(TEXT_READ_SM * LEADING_READ))
            .text_color(p.text2)
            .child(
                div()
                    .max_h(rems(TEXT_READ_SM * LEADING_READ * max_lines))
                    .overflow_hidden()
                    .child(text),
            )
    }

    pub(super) fn permission_command(
        p: &Palette,
        cx: &App,
        mono: SharedString,
    ) -> impl IntoElement {
        Self::pinned(
            p, cx,
            IconName::SquareTerminal,
            p.warning,
            "Run a command?",
            "claude · Bash",
            Some(
                v_flex()
                    .gap_2()
                    .child(Self::well(
                        p, cx,
                        mono,
                        6.0,
                        "cargo test -p retry -- --test-threads=1 flaky \\\n  && cargo clippy -p retry -- -D warnings \\\n  && git diff --stat",
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted)
                            .child("in atlas-api · runs tests, then lints, then reads the diff"),
                    )
                    .into_any_element(),
            ),
            vec![("enter", "allow once"), ("escape", "deny")],
            vec![
                Self::act("deny", "Deny", false),
                action("always").outline().small().label("Always allow").into_any_element(),
                Self::act("once", "Allow once", true),
            ],
        )
    }

    pub(super) fn permission_edit(p: &Palette, cx: &App, mono: SharedString) -> impl IntoElement {
        let line = |sign: &'static str, text: &'static str, ink: Hsla| {
            h_flex().gap_2().text_color(ink).child(sign).child(text)
        };
        Self::pinned(
            p,
            cx,
            IconName::File,
            p.warning,
            "Edit src/backoff.rs?",
            "claude · Edit · +2 −1",
            Some(
                v_flex()
                    .p_3()
                    .rounded(cx.theme().radius_lg)
                    .bg(p.sunken)
                    .font_family(mono)
                    .text_size(rems(TEXT_READ_SM))
                    .line_height(rems(TEXT_READ_SM * LEADING_READ))
                    .child(line(
                        " ",
                        "pub fn backoff(attempt: u32) -> Duration {",
                        p.text2,
                    ))
                    .child(line("−", "    let now = Instant::now();", p.danger))
                    .child(line(
                        "+",
                        "pub fn backoff(attempt: u32, clock: &dyn Clock) -> Duration {",
                        p.success,
                    ))
                    .child(line("+", "    let now = clock.now();", p.success))
                    .into_any_element(),
            ),
            vec![("enter", "allow once"), ("escape", "deny")],
            vec![
                Self::act("deny-e", "Deny", false),
                action("always-e")
                    .outline()
                    .small()
                    .label("Allow edits this session")
                    .into_any_element(),
                Self::act("once-e", "Allow once", true),
            ],
        )
    }

    pub(super) fn question_single(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let options = [
            (
                "Inject a clock",
                "Backoff takes a Clock; tests pass a fake one.",
            ),
            ("Raise the timeout", "Keep real time, give the test 15s."),
            ("Mark it flaky", "Retry the test up to 3 times on CI."),
        ];
        let tabs = h_flex()
            .gap_1()
            .child(
                action("tab-approach")
                    .ghost()
                    .small()
                    .label("Approach")
                    .selected(true),
            )
            .child(action("tab-scope").ghost().small().label("Scope"))
            .child(div().text_xs().text_color(p.muted).child("1 of 2"));
        let rows =
            v_flex()
                .gap_0p5()
                .children(options.iter().enumerate().map(|(i, (label, why))| {
                    let on = self.choice == i;
                    h_flex()
                        .id(("choice", i))
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .gap_2()
                        .rounded(cx.theme().radius)
                        .when(on, |d| d.bg(p.selected))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choice = i;
                            cx.notify();
                        }))
                        .child(Radio::new(("radio", i)).checked(on))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_color(p.text).child(*label))
                                .child(div().text_xs().text_color(p.muted).child(*why)),
                        )
                        .child(key(&(i + 1).to_string()))
                }));
        let other = h_flex()
            .px_2()
            .py_1()
            .gap_2()
            .child(Radio::new("radio-other").checked(false))
            .child(div().text_color(p.muted).child("Other — type an answer"));
        Self::pinned(
            p,
            cx,
            IconName::Info,
            p.warning,
            "How should the flaky test be fixed?",
            "claude asks",
            Some(
                v_flex()
                    .gap_2()
                    .child(tabs)
                    .child(rows)
                    .child(other)
                    .into_any_element(),
            ),
            vec![("1", "pick"), ("enter", "next"), ("escape", "skip")],
            vec![
                Self::act("skip-q", "Skip", false),
                Self::act("next-q", "Next", true),
            ],
        )
    }

    pub(super) fn question_multi(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let options = [
            "Unit tests",
            "Integration tests",
            "Benchmarks",
            "Docs examples",
        ];
        let rows = v_flex()
            .gap_0p5()
            .children(options.iter().enumerate().map(|(i, label)| {
                let on = self.picks[i];
                h_flex()
                    .id(("pick", i))
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .rounded(cx.theme().radius)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.picks[i] = !this.picks[i];
                        cx.notify();
                    }))
                    .child(Checkbox::new(("check", i)).checked(on))
                    .child(div().flex_1().text_color(p.text).child(*label))
                    .child(key(&(i + 1).to_string()))
            }));
        let n = self.picks.iter().filter(|b| **b).count();
        Self::pinned(
            p,
            cx,
            IconName::Info,
            p.warning,
            "Which suites should run after the change?",
            "claude asks · pick any",
            Some(rows.into_any_element()),
            vec![("space", "toggle"), ("enter", "submit")],
            vec![
                Self::act("skip-m", "Skip", false),
                action("submit-m")
                    .primary()
                    .small()
                    .label(format!("Submit {n}"))
                    .disabled(n == 0)
                    .into_any_element(),
            ],
        )
    }

    pub(super) fn question_text(p: &Palette, cx: &App) -> impl IntoElement {
        Self::pinned(
            p,
            cx,
            IconName::Info,
            p.warning,
            "What should the new config key be called?",
            "claude asks",
            Some(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.text2)
                            .child("It replaces retry_ms and is read by both services."),
                    )
                    .child(
                        div()
                            .min_h_10()
                            .px_2()
                            .py_1()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(p.control)
                            .text_color(p.muted)
                            .child("Answer"),
                    )
                    .into_any_element(),
            ),
            vec![("enter", "submit"), ("escape", "skip")],
            vec![
                Self::act("skip-t", "Skip", false),
                Self::act("submit-t", "Submit", true),
            ],
        )
    }

    pub(super) fn queued(p: &Palette) -> impl IntoElement {
        h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .font_medium()
                    .child("Queued"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(p.text)
                    .child("Then open a pull request with the fix and link issue #42"),
            )
            .child(Self::act("q-edit", "Edit", false))
            .child(
                action("q-drop")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip("Remove from the queue"),
            )
    }

    pub(super) fn connecting(p: &Palette) -> impl IntoElement {
        h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .text_color(p.text2)
            .child(Spinner::new().small())
            .child(div().flex_1().child("Connecting… the agent is restarting"))
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child("Send waits until it is up"),
            )
    }

    // ---- popups ------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    /// A popup: a pinned header, a capped list, a footer of keys.
    pub(super) fn popup(
        cx: &mut Context<Self>,
        p: &Palette,
        mono: SharedString,
        width: Option<f32>,
        header: &'static str,
        items: Vec<Item>,
        highlight: usize,
        footer: Option<Vec<(&'static str, &'static str)>>,
        empty: Option<String>,
    ) -> impl IntoElement {
        let total = items.len();
        let shown = items
            .into_iter()
            .take(POPUP_LIST_CAP)
            .enumerate()
            .map(|(i, it)| {
                v_flex()
                    .children(it.group.map(|g| {
                        div()
                            .px_2()
                            .pt_2()
                            .pb_1()
                            .text_xs()
                            .text_color(p.muted)
                            .child(g)
                    }))
                    .child(
                        h_flex()
                            .id(("popup-row", i))
                            .h(rems(ROW_H))
                            .px_2()
                            .gap_2()
                            .rounded(cx.theme().radius)
                            .when(i == highlight, |d| d.bg(p.selected))
                            .when_some(it.on, |d, on| {
                                d.cursor_pointer()
                                    .hover(|d| d.bg(p.selected))
                                    .on_click(cx.listener(move |this, _, w, cx| on(this, w, cx)))
                            })
                            .children(it.icon.map(|ic| Icon::new(ic).small().text_color(p.muted)))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(p.text)
                                    .when(it.mono, |d| {
                                        d.font_family(mono.clone()).text_size(rems(TEXT_READ_SM))
                                    })
                                    .child(it.label),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child(it.detail),
                            )
                            .when(it.current, |d| {
                                d.child(Icon::new(IconName::Check).small().text_color(p.text))
                            }),
                    )
            });
        v_flex()
            .when_some(width, |d, w| d.w(rems(w)))
            .when(width.is_none(), |d| d.w_full())
            .p_1p5()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(p.control)
            .bg(p.panel)
            .shadow_lg()
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .mb_1()
                    .border_b_1()
                    .border_color(p.hairline)
                    .text_xs()
                    .font_medium()
                    .text_color(p.muted)
                    .child(header),
            )
            .children(shown)
            .children(empty.map(|e| div().px_2().py_2().text_color(p.muted).child(e)))
            .when(total > POPUP_LIST_CAP, |d| {
                d.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(p.muted)
                        .child(format!(
                            "{} more — keep typing to narrow",
                            total - POPUP_LIST_CAP
                        )),
                )
            })
            .children(footer.map(|keys| {
                h_flex()
                    .mt_1()
                    .pt_2()
                    .px_2()
                    .gap_3()
                    .border_t_1()
                    .border_color(p.hairline)
                    .text_xs()
                    .text_color(p.muted)
                    .children(
                        keys.into_iter()
                            .map(|(k, what)| h_flex().gap_1().child(key(k)).child(what)),
                    )
            }))
    }

    pub(super) fn nav_keys() -> Option<Vec<(&'static str, &'static str)>> {
        Some(vec![("enter", "insert"), ("escape", "close")])
    }

    fn act(id: &'static str, label: &'static str, primary: bool) -> AnyElement {
        let b = action(id).small().label(label);
        if primary { b.primary() } else { b.ghost() }.into_any_element()
    }
}
