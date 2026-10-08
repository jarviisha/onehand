//! The terminal dock under the agent pane: shell tabs that scroll, with `+`,
//! maximize and hide kept outside the scroll, over a grid of canned output.
//! It opens at `TERM_H` and never takes the conversation's last readable
//! height.
use super::*;
use gpui::ScrollHandle;

pub(super) struct Term {
    pub open: bool,
    pub maximized: bool,
    /// What the person dragged to, in rems; drawn bounded by the room left.
    pub h: f32,
    pub shells: Vec<String>,
    pub active: usize,
    tabs: ScrollHandle,
}

impl Term {
    pub(super) fn new() -> Self {
        Self {
            open: false,
            maximized: false,
            h: TERM_H,
            shells: vec!["atlas-api · zsh".into(), "cargo watch -x test".into()],
            active: 0,
            tabs: ScrollHandle::new(),
        }
    }
}

/// How tall the terminal is drawn in a pane `pane_h` tall: what was dragged
/// to, but never so tall that less than `READING_MIN_H` of conversation is
/// left above it, and never shorter than `TERM_MIN_H`.
pub(super) fn term_height(dragged: f32, pane_h: f32) -> f32 {
    dragged.min(pane_h - READING_MIN_H).max(TERM_MIN_H)
}

const OUTPUT: &str = "$ cargo test -p retry -- flaky\n   Compiling retry v0.4.2\n    Finished `test` profile in 3.8s\n     Running unittests src/lib.rs\ntest backoff::caps ... ok\ntest backoff::grows ... ok\ntest flaky::three_attempts ... ok\n\ntest result: ok. 12 passed; 0 failed\n$ ";

impl Labs {
    pub(super) fn toggle_terminal(&mut self, cx: &mut Context<Self>) {
        self.term.open = !self.term.open;
        if !self.term.open {
            self.term.maximized = false;
        }
        if self.term.open && self.term.shells.is_empty() {
            self.term.shells.push("atlas-api · zsh".into());
            self.term.active = 0;
        }
        cx.notify();
    }

    /// The dock. `pane_h` is the room under the agent header, in rems.
    pub(super) fn terminal(
        &self,
        p: &Palette,
        pane_h: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let term = &self.term;
        let tabs = h_flex()
            .id("shell-tabs")
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&term.tabs)
            .gap(rems(TIGHT))
            .children(term.shells.iter().enumerate().map(|(i, name)| {
                let on = i == term.active;
                h_flex()
                    .id(("shell", i))
                    .flex_none()
                    .max_w(rems(TAB_MAX_W))
                    .h(rems(CONTROL_H_SM))
                    .pl(rems(CONTROL))
                    .gap(rems(TIGHT))
                    .rounded(rems(RADIUS_SM))
                    .cursor_pointer()
                    .text_color(if on { p.text } else { p.text2 })
                    .when(on, |d| d.bg(p.selected))
                    .hover(|d| d.bg(p.selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.term.active = i;
                        cx.notify();
                    }))
                    .child(super::controls::full(
                        format!("shell-name-{i}"),
                        name.clone(),
                    ))
                    .child(
                        action(("close-shell", i))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Close).text_color(p.muted))
                            .tooltip("Close this shell")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.term.shells.remove(i);
                                if this.term.active >= this.term.shells.len() {
                                    this.term.active = this.term.shells.len().saturating_sub(1);
                                }
                                if this.term.shells.is_empty() {
                                    this.term.open = false;
                                    this.term.maximized = false;
                                }
                                cx.notify();
                            })),
                    )
            }));

        let strip = h_flex()
            .h(rems(BAR_H))
            .flex_none()
            .px(rems(CONTROL))
            .gap(rems(TIGHT))
            .border_b_1()
            .border_color(p.hairline)
            .child(tabs)
            // Outside the scroll: these never scroll out of reach.
            .child(
                self.icon_button("new-shell", IconName::Plus, "New shell", cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let n = this.term.shells.len() + 1;
                        this.term.shells.push(format!("atlas-api · zsh {n}"));
                        this.term.active = this.term.shells.len() - 1;
                        cx.notify();
                    })),
            )
            .child(
                self.icon_button(
                    "max-term",
                    if term.maximized {
                        IconName::Minimize
                    } else {
                        IconName::Maximize
                    },
                    if term.maximized {
                        "Restore"
                    } else {
                        "Maximize"
                    },
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.term.maximized = !this.term.maximized;
                    cx.notify();
                })),
            )
            .child(
                self.icon_button("hide-term", IconName::Close, "Hide the terminal", cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.term.open = false;
                        this.term.maximized = false;
                        cx.notify();
                    })),
            );

        let grid = div()
            .id("grid")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(rems(RELATED))
            .py(rems(CONTROL))
            .font_family(mono)
            .text_size(rems(TEXT_READ_SM))
            .line_height(rems(TEXT_READ_SM * LEADING_UI))
            .text_color(p.text2)
            .child(OUTPUT);

        v_flex()
            .when(term.maximized, |d| d.flex_1())
            .when(!term.maximized, |d| {
                d.h(rems(term_height(term.h, pane_h))).flex_none()
            })
            .bg(p.panel)
            .child(strip)
            .child(grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_leaves_the_conversation_room_to_read() {
        // Room for both: the dragged height.
        assert_eq!(term_height(TERM_H, 60.0), TERM_H);
        // Dragged tall: cut where the conversation would drop under its minimum.
        assert_eq!(term_height(50.0, 40.0), 40.0 - READING_MIN_H);
        // A short window never shrinks it to nothing.
        assert_eq!(term_height(TERM_H, READING_MIN_H), TERM_MIN_H);
    }
}
