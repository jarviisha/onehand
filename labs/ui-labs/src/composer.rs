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

/// A row in a popup. `group` starts a labelled run of rows.
struct Item {
    group: Option<&'static str>,
    icon: Option<IconName>,
    label: &'static str,
    detail: &'static str,
    mono: bool,
    current: bool,
}

fn item(label: &'static str, detail: &'static str) -> Item {
    Item {
        group: None,
        icon: None,
        label,
        detail,
        mono: false,
        current: false,
    }
}

/// The gallery is a little wider than a stack, for its labels.
const GALLERY_MAX: f32 = 48.0;

impl Item {
    fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
    fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
    fn current(mut self) -> Self {
        self.current = true;
        self
    }
    fn group(mut self, g: &'static str) -> Self {
        self.group = Some(g);
        self
    }
}

fn key(k: &str) -> Kbd {
    Kbd::new(Keystroke::parse(k).expect("a keystroke"))
}

impl Labs {
    // ---- the composer card -------------------------------------------------

    pub(super) fn composer_card(p: &Palette, look: ComposerLook) -> impl IntoElement {
        let chip = |id: &'static str, label: &'static str| {
            let b = Button::new(id)
                .ghost()
                .small()
                .selected(look.open_chip == Some(id));
            if label.is_empty() { b } else { b.label(label) }
        };
        let tray = look.tray.then(|| {
            h_flex()
                .flex_wrap()
                .gap(rems(TIGHT))
                .px(rems(TIGHT))
                .child(Self::attachment(p, IconName::File, "retry.rs", "4 KB"))
                .child(Self::attachment(
                    p,
                    IconName::Frame,
                    "screenshot.png",
                    "182 KB",
                ))
                .child(Self::attachment(p, IconName::File, "ci-log.txt", "51 KB"))
        });
        let send = if look.running {
            h_flex()
                .gap(rems(TIGHT))
                .when(look.text.is_some(), |d| {
                    d.child(Button::new("queue").outline().small().label("Queue"))
                })
                .child(Button::new("stop").danger().small().label("Stop"))
                .into_any_element()
        } else {
            Button::new("send")
                .primary()
                .small()
                .icon(IconName::ArrowUp)
                .disabled(look.text.is_none() && !look.tray)
                .into_any_element()
        };
        v_flex()
            .w_full()
            .gap(rems(TIGHT))
            .child(
                v_flex()
                    .rounded(rems(RADIUS_XL))
                    .border_1()
                    .border_color(p.control)
                    .bg(p.panel)
                    .p(rems(CONTROL))
                    .gap(rems(CONTROL))
                    .children(tray)
                    .child(
                        div()
                            .px(rems(TIGHT))
                            .min_h(rems(INPUT_MIN_H))
                            .text_size(rems(TEXT_READ))
                            .line_height(rems(TEXT_READ * LEADING_READ))
                            .text_color(if look.text.is_some() { p.text } else { p.muted })
                            .child(look.text.unwrap_or("Ask the agent…")),
                    )
                    .child(
                        h_flex()
                            .gap(rems(TIGHT))
                            .child(chip("plus", "").icon(IconName::Plus))
                            .child(chip("fast", "Fast: Off"))
                            .child(chip("model", "Sonnet 5 · high").icon(IconName::ChevronDown))
                            .child(div().flex_1())
                            .child(send),
                    ),
            )
            .child(
                h_flex()
                    .px(rems(RELATED))
                    .gap(rems(CONTROL))
                    .text_size(rems(TEXT_XS))
                    .text_color(p.muted)
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .when(look.open_chip == Some("branch"), |d| d.text_color(p.text))
                            .child("main · 3 changes"),
                    )
                    .child(
                        div()
                            .when(look.open_chip == Some("mode"), |d| d.text_color(p.text))
                            .child("Ask before edits"),
                    ),
            )
    }

    fn attachment(
        p: &Palette,
        icon: IconName,
        name: &'static str,
        size: &'static str,
    ) -> impl IntoElement {
        h_flex()
            .h(rems(CONTROL_H))
            .pl(rems(CONTROL))
            .pr(rems(TIGHT))
            .gap(rems(CONTROL))
            .rounded(rems(RADIUS_SM))
            .border_1()
            .border_color(p.hairline)
            .bg(p.sunken)
            .text_size(rems(TEXT_XS))
            .child(Icon::new(icon).size(rems(ICON_SM)).text_color(p.muted))
            .child(div().text_color(p.text).child(name))
            .child(div().text_color(p.muted).child(size))
            .child(Button::new(name).ghost().xsmall().icon(IconName::Close))
    }

    // ---- pinned cards ------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    /// A pinned card: what it asks, the body, and a footer of keys and answers.
    fn pinned(
        p: &Palette,
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
            .rounded(rems(RADIUS_LG))
            .border_1()
            .border_color(p.control)
            .bg(p.panel)
            .child(
                v_flex()
                    .p(rems(RELATED))
                    .gap(rems(CONTROL))
                    .child(
                        h_flex()
                            .gap(rems(CONTROL))
                            .child(Icon::new(icon).size(rems(ICON)).text_color(ink))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_medium()
                                    .text_color(p.text)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(rems(TEXT_XS))
                                    .text_color(p.muted)
                                    .child(meta),
                            ),
                    )
                    .children(body),
            )
            .child(
                h_flex()
                    .px(rems(RELATED))
                    .py(rems(CONTROL))
                    .gap(rems(CONTROL))
                    .border_t_1()
                    .border_color(p.hairline)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(rems(RELATED))
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .children(keys.into_iter().map(|(k, what)| {
                                h_flex().gap(rems(TIGHT)).child(key(k)).child(what)
                            })),
                    )
                    .children(actions),
            )
    }

    fn well(p: &Palette, mono: SharedString, max_lines: f32, text: &'static str) -> gpui::Div {
        div()
            .max_h(rems(TEXT_READ_SM * 1.6 * max_lines + 1.5))
            .overflow_hidden()
            .p(rems(RELATED))
            .rounded(rems(RADIUS_MD))
            .bg(p.sunken)
            .font_family(mono)
            .text_size(rems(TEXT_READ_SM))
            .line_height(rems(TEXT_READ_SM * LEADING_READ))
            .text_color(p.text2)
            .child(text)
    }

    fn permission_command(p: &Palette, mono: SharedString) -> impl IntoElement {
        Self::pinned(
            p,
            IconName::SquareTerminal,
            p.warning,
            "Run a command?",
            "claude · Bash",
            Some(
                v_flex()
                    .gap(rems(CONTROL))
                    .child(Self::well(
                        p,
                        mono,
                        6.0,
                        "cargo test -p retry -- --test-threads=1 flaky \\\n  && cargo clippy -p retry -- -D warnings \\\n  && git diff --stat",
                    ))
                    .child(
                        div()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child("in atlas-api · runs tests, then lints, then reads the diff"),
                    )
                    .into_any_element(),
            ),
            vec![("enter", "allow once"), ("escape", "deny")],
            vec![
                Self::act("deny", "Deny", false),
                Button::new("always").outline().small().label("Always allow").into_any_element(),
                Self::act("once", "Allow once", true),
            ],
        )
    }

    fn permission_edit(p: &Palette, mono: SharedString) -> impl IntoElement {
        let line = |sign: &'static str, text: &'static str, ink: Hsla| {
            h_flex()
                .gap(rems(CONTROL))
                .text_color(ink)
                .child(div().w(rems(ICON_SM)).child(sign))
                .child(text)
        };
        Self::pinned(
            p,
            IconName::File,
            p.warning,
            "Edit src/backoff.rs?",
            "claude · Edit · +2 −1",
            Some(
                v_flex()
                    .p(rems(RELATED))
                    .rounded(rems(RADIUS_MD))
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
                Button::new("always-e")
                    .outline()
                    .small()
                    .label("Allow edits this session")
                    .into_any_element(),
                Self::act("once-e", "Allow once", true),
            ],
        )
    }

    fn question_single(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let options = [
            (
                "Inject a clock",
                "Backoff takes a Clock; tests pass a fake one.",
            ),
            ("Raise the timeout", "Keep real time, give the test 15s."),
            ("Mark it flaky", "Retry the test up to 3 times on CI."),
        ];
        let tabs = h_flex()
            .gap(rems(TIGHT))
            .child(
                Button::new("tab-approach")
                    .ghost()
                    .small()
                    .label("Approach")
                    .selected(true),
            )
            .child(Button::new("tab-scope").ghost().small().label("Scope"))
            .child(
                div()
                    .text_size(rems(TEXT_XS))
                    .text_color(p.muted)
                    .child("1 of 2"),
            );
        let rows = v_flex()
            .gap(rems(TIGHT * 0.5))
            .children(options.iter().enumerate().map(|(i, (label, why))| {
                let on = self.choice == i;
                h_flex()
                    .id(("choice", i))
                    .px(rems(CONTROL))
                    .py(rems(TIGHT))
                    .gap(rems(CONTROL))
                    .rounded(rems(RADIUS_SM))
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
                            .child(
                                div()
                                    .text_size(rems(TEXT_XS))
                                    .text_color(p.muted)
                                    .child(*why),
                            ),
                    )
                    .child(key(&(i + 1).to_string()))
            }));
        let other = h_flex()
            .px(rems(CONTROL))
            .py(rems(TIGHT))
            .gap(rems(CONTROL))
            .child(Radio::new("radio-other").checked(false))
            .child(div().text_color(p.muted).child("Other — type an answer"));
        Self::pinned(
            p,
            IconName::Info,
            p.warning,
            "How should the flaky test be fixed?",
            "claude asks",
            Some(
                v_flex()
                    .gap(rems(CONTROL))
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

    fn question_multi(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let options = [
            "Unit tests",
            "Integration tests",
            "Benchmarks",
            "Docs examples",
        ];
        let rows = v_flex()
            .gap(rems(TIGHT * 0.5))
            .children(options.iter().enumerate().map(|(i, label)| {
                let on = self.picks[i];
                h_flex()
                    .id(("pick", i))
                    .px(rems(CONTROL))
                    .py(rems(TIGHT))
                    .gap(rems(CONTROL))
                    .rounded(rems(RADIUS_SM))
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
            IconName::Info,
            p.warning,
            "Which suites should run after the change?",
            "claude asks · pick any",
            Some(rows.into_any_element()),
            vec![("space", "toggle"), ("enter", "submit")],
            vec![
                Self::act("skip-m", "Skip", false),
                Button::new("submit-m")
                    .primary()
                    .small()
                    .label(format!("Submit {n}"))
                    .disabled(n == 0)
                    .into_any_element(),
            ],
        )
    }

    fn question_text(p: &Palette) -> impl IntoElement {
        Self::pinned(
            p,
            IconName::Info,
            p.warning,
            "What should the new config key be called?",
            "claude asks",
            Some(
                v_flex()
                    .gap(rems(CONTROL))
                    .child(
                        div()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.text2)
                            .child("It replaces retry_ms and is read by both services."),
                    )
                    .child(
                        div()
                            .min_h(rems(INPUT_MIN_H))
                            .px(rems(CONTROL))
                            .py(rems(TIGHT + 0.125))
                            .rounded(rems(RADIUS_SM))
                            .border_1()
                            .border_color(p.muted)
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

    fn queued(p: &Palette) -> impl IntoElement {
        h_flex()
            .w_full()
            .px(rems(RELATED))
            .py(rems(CONTROL))
            .gap(rems(CONTROL))
            .rounded(rems(RADIUS_LG))
            .border_1()
            .border_color(p.hairline)
            .bg(p.panel)
            .child(
                div()
                    .text_size(rems(TEXT_XS))
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
            .child(Button::new("q-drop").ghost().small().icon(IconName::Close))
    }

    fn connecting(p: &Palette) -> impl IntoElement {
        h_flex()
            .w_full()
            .px(rems(RELATED))
            .py(rems(CONTROL))
            .gap(rems(CONTROL))
            .rounded(rems(RADIUS_LG))
            .border_1()
            .border_color(p.hairline)
            .bg(p.panel)
            .text_color(p.text2)
            .child(Spinner::new().small())
            .child(div().flex_1().child("Connecting… the agent is restarting"))
            .child(
                div()
                    .text_size(rems(TEXT_XS))
                    .text_color(p.muted)
                    .child("Send waits until it is up"),
            )
    }

    // ---- popups ------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    /// A popup: a pinned header, a capped list, a footer of keys.
    fn popup(
        p: &Palette,
        mono: SharedString,
        width: Option<f32>,
        header: &'static str,
        items: Vec<Item>,
        highlight: usize,
        footer: Option<Vec<(&'static str, &'static str)>>,
        empty: Option<&'static str>,
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
                            .px(rems(CONTROL))
                            .pt(rems(CONTROL))
                            .pb(rems(TIGHT))
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child(g)
                    }))
                    .child(
                        h_flex()
                            .h(rems(ROW_H))
                            .px(rems(CONTROL))
                            .gap(rems(CONTROL))
                            .rounded(rems(RADIUS_SM))
                            .when(i == highlight, |d| d.bg(p.selected))
                            .children(
                                it.icon
                                    .map(|ic| Icon::new(ic).size(rems(ICON)).text_color(p.muted)),
                            )
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
                                    .text_size(rems(TEXT_XS))
                                    .text_color(p.muted)
                                    .child(it.detail),
                            )
                            .when(it.current, |d| {
                                d.child(
                                    Icon::new(IconName::Check)
                                        .size(rems(ICON))
                                        .text_color(p.text),
                                )
                            }),
                    )
            });
        v_flex()
            .when_some(width, |d, w| d.w(rems(w)))
            .when(width.is_none(), |d| d.w_full())
            .p(rems(TIGHT + 0.125))
            .rounded(rems(RADIUS_MD))
            .border_1()
            .border_color(p.control)
            .bg(p.panel)
            .shadow_lg()
            .child(
                div()
                    .px(rems(CONTROL))
                    .pb(rems(TIGHT))
                    .mb(rems(TIGHT))
                    .border_b_1()
                    .border_color(p.hairline)
                    .text_size(rems(TEXT_XS))
                    .font_medium()
                    .text_color(p.muted)
                    .child(header),
            )
            .children(shown)
            .children(empty.map(|e| {
                div()
                    .px(rems(CONTROL))
                    .py(rems(CONTROL))
                    .text_color(p.muted)
                    .child(e)
            }))
            .when(total > POPUP_LIST_CAP, |d| {
                d.child(
                    div()
                        .px(rems(CONTROL))
                        .py(rems(TIGHT))
                        .text_size(rems(TEXT_XS))
                        .text_color(p.muted)
                        .child(format!(
                            "{} more — keep typing to narrow",
                            total - POPUP_LIST_CAP
                        )),
                )
            })
            .children(footer.map(|keys| {
                h_flex()
                    .mt(rems(TIGHT))
                    .pt(rems(CONTROL))
                    .px(rems(CONTROL))
                    .gap(rems(RELATED))
                    .border_t_1()
                    .border_color(p.hairline)
                    .text_size(rems(TEXT_XS))
                    .text_color(p.muted)
                    .children(
                        keys.into_iter()
                            .map(|(k, what)| h_flex().gap(rems(TIGHT)).child(key(k)).child(what)),
                    )
            }))
    }

    fn nav_keys() -> Option<Vec<(&'static str, &'static str)>> {
        Some(vec![
            ("up", "move"),
            ("enter", "insert"),
            ("escape", "close"),
        ])
    }

    fn act(id: &'static str, label: &'static str, primary: bool) -> AnyElement {
        let b = Button::new(id).small().label(label);
        if primary { b.primary() } else { b.ghost() }.into_any_element()
    }

    // ---- the gallery -------------------------------------------------------

    fn specimen(
        p: &Palette,
        n: usize,
        name: &'static str,
        note: &'static str,
        stack: impl IntoElement,
    ) -> impl IntoElement {
        v_flex()
            .gap(rems(CONTROL))
            .child(
                h_flex()
                    .gap(rems(CONTROL))
                    .child(
                        div()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child(format!("{n:02}")),
                    )
                    .child(div().font_medium().text_color(p.text).child(name))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(rems(TEXT_XS))
                            .text_color(p.muted)
                            .child(note),
                    ),
            )
            .child(
                v_flex()
                    .p(rems(INSET))
                    .rounded(rems(RADIUS_MD))
                    .border_1()
                    .border_color(p.hairline)
                    .bg(p.page)
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(rems(COMPOSER_MAX))
                            .mx_auto()
                            .gap(rems(STACK_GAP))
                            .child(stack),
                    ),
            )
    }

    pub(super) fn composer_gallery(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let idle = || Self::composer_card(p, ComposerLook::default());
        let typed = |chip: Option<&'static str>, text: &'static str| {
            Self::composer_card(
                p,
                ComposerLook {
                    text: Some(text),
                    open_chip: chip,
                    ..Default::default()
                },
            )
        };
        let stack = || v_flex().w_full().gap(rems(STACK_GAP));
        let mut n = 0;
        let mut next = || {
            n += 1;
            n
        };

        let files = vec![
            item("src/backoff.rs", "modified")
                .mono()
                .icon(IconName::File)
                .group("Changed"),
            item("tests/flaky.rs", "modified")
                .mono()
                .icon(IconName::File),
            item("src/retry.rs", "")
                .mono()
                .icon(IconName::File)
                .group("Files"),
            item("src/retry/policy.rs", "").mono().icon(IconName::File),
            item("benches/retry.rs", "").mono().icon(IconName::File),
            item("docs/retry.md", "").mono().icon(IconName::File),
            item("README.md", "").mono().icon(IconName::File),
            item("Cargo.toml", "").mono().icon(IconName::File),
        ];
        let commands = vec![
            item("/review", "Review the working tree")
                .mono()
                .group("Agent"),
            item("/compact", "Summarise the conversation so far").mono(),
            item("/init", "Write a CLAUDE.md for this project").mono(),
            item("/run-check", "Run the project's check command")
                .mono()
                .group("onehand"),
        ];
        let settings = vec![
            item("Sonnet 5", "fast, most tasks")
                .current()
                .group("Model"),
            item("Opus 5.5", "slower, hardest tasks"),
            item("Haiku 4.5", "quickest"),
            item("Low", "").group("Effort"),
            item("High", "").current(),
        ];
        let plus = vec![
            item("Attach files…", "").icon(IconName::Plus),
            item("Mention a file", "@").icon(IconName::File),
            item("Run a command", "/").icon(IconName::SquareTerminal),
            item("Run a workflow…", "").icon(IconName::GalleryVerticalEnd),
        ];
        let modes = vec![
            item("Ask before edits", "every edit and command asks").current(),
            item("Accept edits", "commands still ask"),
            item("Plan only", "reads, never writes"),
            item("Bypass permissions", "nothing asks"),
        ];
        let branches = vec![
            item("main", "3 changes").mono().current().group("Branches"),
            item("fix/retry-clock", "2 ahead").mono(),
            item("feat/charts", "").mono(),
            item("New branch…", "").icon(IconName::Plus),
        ];

        let left = |popup: AnyElement| h_flex().child(popup);
        let right = |popup: AnyElement| h_flex().justify_end().child(popup);

        Self::column("composer-gallery").child(
            Self::inner(p)
                .max_w(rems(GALLERY_MAX))
                .child(div().text_color(p.text2).child(
                    "Everything that rests on the composer. Pinned cards stack in the order they were asked; popups open above the input and float over the transcript.",
                ))
                .child(div().text_size(rems(TEXT_MD)).font_medium().child("Pinned above the composer"))
                .child(Self::specimen(p, next(), "Permission · command", "a long command is capped in its well", stack().child(Self::permission_command(p, mono.clone())).child(idle())))
                .child(Self::specimen(p, next(), "Permission · edit", "the diff keeps its signs, not just colour", stack().child(Self::permission_edit(p, mono.clone())).child(idle())))
                .child(Self::specimen(p, next(), "Question · one choice", "click a row; tabs for a form with several fields", stack().child(self.question_single(p, cx)).child(idle())))
                .child(Self::specimen(p, next(), "Question · any choices", "click to toggle; Submit counts", stack().child(self.question_multi(p, cx)).child(idle())))
                .child(Self::specimen(p, next(), "Question · free text", "the description once, a short placeholder", stack().child(Self::question_text(p)).child(idle())))
                .child(Self::specimen(
                    p,
                    next(),
                    "Queued prompt while a turn runs",
                    "Stop stays; Queue joins it over a draft",
                    stack()
                        .child(Self::queued(p))
                        .child(Self::composer_card(p, ComposerLook { text: Some("Also bump the crate version"), running: true, ..Default::default() })),
                ))
                .child(Self::specimen(p, next(), "Reconnecting", "the transcript stays; sending waits", stack().child(Self::connecting(p)).child(idle())))
                .child(Self::specimen(
                    p,
                    next(),
                    "Stacked",
                    "permission, then question, then the queue, then the composer",
                    stack()
                        .child(Self::permission_command(p, mono.clone()))
                        .child(Self::question_text(p))
                        .child(Self::queued(p))
                        .child(Self::composer_card(p, ComposerLook { running: true, ..Default::default() })),
                ))
                .child(div().text_size(rems(TEXT_MD)).font_medium().child("Opening from the composer"))
                .child(Self::specimen(p, next(), "+ menu", "the way to @ and / when an IME swallows them", stack().child(left(Self::popup(p, mono.clone(), Some(MENU_W), "Add to the prompt", plus, 0, None, None).into_any_element())).child(Self::composer_card(p, ComposerLook { open_chip: Some("plus"), ..Default::default() }))))
                .child(Self::specimen(p, next(), "@ mention", "changed files first; capped, says how many are left", stack().child(Self::popup(p, mono.clone(), None, "Mention a file", files, 0, Self::nav_keys(), None)).child(typed(None, "Look at @"))))
                .child(Self::specimen(p, next(), "/ command", "the agent's commands, then onehand's", stack().child(Self::popup(p, mono.clone(), None, "Run a command", commands, 1, Self::nav_keys(), None)).child(typed(None, "/c"))))
                .child(Self::specimen(p, next(), "@ with no match", "says what it looked for", stack().child(Self::popup(p, mono.clone(), None, "Mention a file", vec![], 0, None, Some("No matches for \u{201c}flakey\u{201d}"))).child(typed(None, "Look at @flakey"))))
                .child(Self::specimen(p, next(), "Model and effort", "one popup, two groups, the current one checked", stack().child(left(Self::popup(p, mono.clone(), Some(MENU_WIDE_W), "Settings", settings, 0, None, None).into_any_element())).child(Self::composer_card(p, ComposerLook { open_chip: Some("model"), ..Default::default() }))))
                .child(Self::specimen(p, next(), "Permission mode", "opens from the strip, right-aligned to it", stack().child(right(Self::popup(p, mono.clone(), Some(MENU_WIDE_W), "Mode", modes, 0, None, None).into_any_element())).child(Self::composer_card(p, ComposerLook { open_chip: Some("mode"), ..Default::default() }))))
                .child(Self::specimen(p, next(), "Branch", "opens from the strip, left-aligned to it", stack().child(left(Self::popup(p, mono.clone(), Some(MENU_W), "Branch", branches, 0, None, None).into_any_element())).child(Self::composer_card(p, ComposerLook { open_chip: Some("branch"), ..Default::default() }))))
                .child(div().text_size(rems(TEXT_MD)).font_medium().child("The composer itself"))
                .child(Self::specimen(p, next(), "Empty", "Send is spent until there is something to send", idle()))
                .child(Self::specimen(p, next(), "Attachments", "a tray inside the card, each removable", Self::composer_card(p, ComposerLook { text: Some("Why does the CI log show a timeout?"), tray: true, ..Default::default() })))
                .child(Self::specimen(p, next(), "Running, nothing typed", "Stop alone", Self::composer_card(p, ComposerLook { running: true, ..Default::default() }))),
        )
    }
}
