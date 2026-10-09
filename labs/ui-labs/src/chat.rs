//! The agent pane: its header, the conversation with the composer under it,
//! and the terminal dock below both.
use super::*;

/// The commands behind the activity summary: what ran, whether it failed, and
/// the tail of what it printed.
const ACTIVITY: [(&str, bool, &str); 2] = [
    (
        "cargo build -p retry",
        false,
        "Finished `dev` profile [optimized + debuginfo] in 4.1s",
    ),
    (
        "cargo test -p retry -- flaky",
        true,
        "thread 'backoff_caps' panicked at 'elapsed 5.2s > 5s'\ntest result: FAILED. 11 passed; 1 failed",
    ),
];

impl Labs {
    /// A reading size at the current reading zoom. Chrome sizes never go
    /// through here.
    pub(super) fn read(&self, size: f32) -> gpui::Rems {
        rems(size * self.zoom)
    }

    pub(super) fn header(
        &self,
        p: &Palette,
        title: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let shell_alive = !self.term.shells.is_empty();
        h_flex()
            .h(rems(BAR_H))
            .flex_none()
            .px_4()
            .gap_2()
            .border_b_1()
            .border_color(p.hairline)
            .when(self.rail_hidden, |d| {
                d.child(
                    self.icon_button("show-rail", IconName::PanelLeftOpen, "Show the rail", cx)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.rail_hidden = false;
                            this.hovered = None;
                            cx.notify();
                        })),
                )
            })
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
                self.icon_button("theme", IconName::Palette, "Switch light and dark", cx)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dark = !this.dark;
                        set_mode(this.dark, window, cx);
                        cx.notify();
                    })),
            )
            .child(
                // A dot on the glyph while a shell is alive, so a hidden
                // terminal still says it is running something.
                div()
                    .relative()
                    .child(
                        self.icon_button("term", IconName::SquareTerminal, "Terminal (Ctrl+`)", cx)
                            .selected(self.term.open)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_terminal(cx))),
                    )
                    .when(shell_alive && !self.term.open, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top_0p5()
                                .right_0p5()
                                .size(rems(BADGE_DOT))
                                .rounded_full()
                                .bg(p.accent),
                        )
                    }),
            )
            .child(
                self.icon_button("wb", IconName::PanelRight, "Workbench (Ctrl+\\)", cx)
                    .selected(self.workbench && !self.chat_over_workbench)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_workbench(cx))),
            )
    }

    pub(super) fn toggle_workbench(&mut self, cx: &mut Context<Self>) {
        // Back from the chat to a Workbench that only stepped aside;
        // otherwise open or close it.
        if self.workbench && self.chat_over_workbench {
            self.chat_over_workbench = false;
        } else {
            self.workbench = !self.workbench;
            self.chat_over_workbench = false;
            self.wb.maximized = false;
        }
        cx.notify();
    }

    /// The activity summary, and the commands under it once opened. A failure
    /// is named in the summary, so it is seen without opening anything.
    fn activity(&self, p: &Palette, mono: SharedString, cx: &mut Context<Self>) -> gpui::Div {
        let open = self.activity_open;
        let failed = ACTIVITY.iter().filter(|(_, f, _)| *f).count();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .id("activity")
                    .gap_2()
                    .cursor_pointer()
                    .text_size(self.read(TEXT_READ_SM))
                    .text_color(p.text2)
                    .hover(|d| d.text_color(p.text))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.activity_open = !this.activity_open;
                        cx.notify();
                    }))
                    .child(
                        Icon::new(IconName::ChevronRight)
                            .xsmall()
                            .rotate(gpui::radians(if open {
                                std::f32::consts::FRAC_PI_2
                            } else {
                                0.0
                            })),
                    )
                    .child(format!("Ran {} commands", ACTIVITY.len()))
                    .when(failed > 0, |d| {
                        d.child(div().text_color(p.muted).child("·"))
                            .child(div().text_color(p.danger).child(format!("{failed} failed")))
                    }),
            )
            .when(open, |d| {
                d.children(ACTIVITY.iter().map(|(cmd, failed, out)| {
                    v_flex()
                        .gap_1()
                        .pl_4()
                        .child(
                            h_flex()
                                .gap_2()
                                .text_size(self.read(TEXT_READ_SM))
                                .child(
                                    div()
                                        .font_family(mono.clone())
                                        .text_color(p.text)
                                        .child(*cmd),
                                )
                                .child(
                                    div()
                                        .text_color(if *failed { p.danger } else { p.muted })
                                        .child(if *failed { "failed" } else { "ok" }),
                                ),
                        )
                        .child(
                            div()
                                .p_3()
                                .rounded(cx.theme().radius_lg)
                                .bg(p.sunken)
                                .font_family(mono.clone())
                                .text_size(self.read(TEXT_READ_SM))
                                .line_height(self.read(TEXT_READ_SM * LEADING_READ))
                                .text_color(p.text2)
                                .child(*out),
                        )
                }))
            })
    }

    /// The agent pane. `width` and `height` are what it is drawn at, in rems:
    /// the composer's strip splits by the first, the terminal is bounded by
    /// the second.
    pub(super) fn chat(
        &self,
        p: &Palette,
        width: f32,
        height: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let column = v_flex()
            .w_full()
            .max_w(rems(READ_MAX))
            .px_4()
            .py_6()
            .gap_3()
            .text_size(self.read(TEXT_READ))
            .line_height(self.read(TEXT_READ * LEADING_READ))
            .text_color(p.text)
            .child(
                h_flex().justify_end().child(
                    div()
                        .max_w(rems(BUBBLE_MAX))
                        .px_3()
                        .py_2()
                        .rounded(cx.theme().radius_lg)
                        .bg(p.sunken)
                        .child("The retry test fails about one run in five. Find out why and fix it."),
                ),
            )
            .child("I read the test and the retry function. The backoff uses wall-clock time, so on a slow machine the third attempt runs past the test's timeout.")
            .child(self.activity(p, mono, cx))
            .child("I will move the backoff onto a fake clock in the test, then run it again.")
            .children(self.said(p, cx));

        let composer_w = (width - GUTTER * 2.0).min(COMPOSER_MAX);
        let term_max = self.term.maximized && self.term.open;
        v_flex()
            .flex_1()
            .min_w(rems(CHAT_MIN))
            .h_full()
            .child(self.header(p, "Fix flaky retry test", cx))
            .when(!term_max, |d| {
                d.child(
                    div()
                        .id("transcript")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(h_flex().justify_center().child(column)),
                )
                // The stack owns the width, once, so the card and the composer
                // under it cannot disagree about it.
                .child(
                    h_flex().justify_center().px_4().pb_3().child(
                        v_flex()
                            .w_full()
                            .max_w(rems(COMPOSER_MAX))
                            .child(self.live_composer(p, composer_w < COMPOSER_SPLIT, window, cx)),
                    ),
                )
            })
            .when(self.term.open, |d| {
                d.when(!term_max, |d| d.child(self.seam(p, Seam::Term, cx)))
                    .child(self.terminal(p, height - BAR_H, cx))
            })
    }
}
