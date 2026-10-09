//! The agent pane: its header, the conversation with the composer under it,
//! and the terminal dock below both.
use super::*;
use gpui_component::scroll::ScrollableElement as _;

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
            // No rule under it: the transcript scrolls straight up to the
            // header on the same surface.
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
        let column = v_flex()
            .w_full()
            .max_w(rems(READ_MAX))
            .px_4()
            .py_6()
            .gap_3()
            .text_size(self.read(TEXT_READ))
            .line_height(self.read(TEXT_READ * LEADING_READ))
            .text_color(p.text)
            .children(self.transcript(p, window, cx))
            .children(self.said(p, window, cx));

        let composer_w = (width - GUTTER * 2.0).min(COMPOSER_MAX);
        let term_max = self.term.maximized && self.term.open;
        v_flex()
            .flex_1()
            .min_w(rems(CHAT_MIN))
            .h_full()
            .child(self.header(p, "Fix flaky retry test", cx))
            .when(!term_max, |d| {
                d.child(
                    // The library's scrollbar over the gutter, so the column
                    // keeps its width; the handle is the session's, so a turn
                    // sent or answered can bring the bottom into view.
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id("transcript")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.live.scroll)
                                .child(h_flex().justify_center().child(column)),
                        )
                        .vertical_scrollbar(&self.live.scroll),
                )
                // The stack owns the width, once, so the card and the composer
                // under it cannot disagree about it.
                .child(
                    h_flex().justify_center().px_4().pb_3().child(
                        v_flex()
                            .w_full()
                            .max_w(rems(COMPOSER_MAX))
                            .child(self.live_composer(p, composer_w < COMPOSER_SPLIT, cx)),
                    ),
                )
            })
            .when(self.term.open, |d| {
                d.when(!term_max, |d| d.child(self.seam(p, Seam::Term, cx)))
                    .child(self.terminal(p, height - BAR_H, cx))
            })
    }
}
