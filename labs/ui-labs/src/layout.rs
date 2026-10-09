//! How the content area is shared between the chat and the Workbench, and the
//! seams between regions that resize them.
use super::*;

#[derive(Debug, PartialEq)]
pub(super) enum Presentation {
    Conversation,
    Split,
    FocusWorkbench,
}

/// How the content area is shown, and how wide the Workbench is drawn.
///
/// The width the person dragged to is only ever *read* here: when the window
/// is too narrow for it the dock is drawn narrower (down to `DOCK_MIN`) and the
/// saved width comes back as soon as there is room, so a narrow moment never
/// overwrites the wide layout. Below `DOCK_MIN` beside `CHAT_MIN` the
/// Workbench takes the whole area instead.
///
/// `was_split` gives the threshold a little slack: a split already showing
/// holds until the dock would drop under `DOCK_MIN`, but one coming back needs
/// `SPLIT_SLACK` more, so a window resting on the line does not flicker
/// between the two.
pub(super) fn presentation(
    avail_rem: f32,
    dock_rem: f32,
    workbench_open: bool,
    zoom: f32,
    was_split: bool,
) -> (Presentation, f32) {
    let room = avail_rem - CHAT_MIN * zoom;
    let needed = if was_split {
        DOCK_MIN
    } else {
        DOCK_MIN + SPLIT_SLACK
    };
    if !workbench_open {
        (Presentation::Conversation, 0.0)
    } else if room >= needed {
        (Presentation::Split, dock_rem.clamp(DOCK_MIN, room))
    } else {
        (Presentation::FocusWorkbench, avail_rem)
    }
}

/// A seam that can be dragged: the rail's edge, the Workbench's, or the
/// terminal's top.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Seam {
    Rail,
    Dock,
    Term,
}

impl Labs {
    pub(super) fn hairline_v(p: &Palette) -> gpui::Div {
        div().w(gpui::px(HAIRLINE_PX)).h_full().bg(p.hairline)
    }

    /// The hairline between two regions, which is also where it is dragged
    /// from. The grab area is wider than the line it draws.
    pub(super) fn seam(&self, p: &Palette, seam: Seam, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, across) = match seam {
            Seam::Rail => ("seam-rail", true),
            Seam::Dock => ("seam-dock", true),
            Seam::Term => ("seam-term", false),
        };
        let dragging = self.drag == Some(seam);
        let line = gpui::px(if dragging { SEAM_DRAG_PX } else { HAIRLINE_PX });
        div()
            .id(id)
            .flex()
            .flex_none()
            .justify_center()
            .items_center()
            .when(across, |d| {
                d.w(rems(SEAM_GRAB_W)).h_full().cursor_col_resize()
            })
            .when(!across, |d| {
                d.h(rems(SEAM_GRAB_W)).w_full().cursor_row_resize()
            })
            .group(id)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.drag = Some(seam);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .when(across, |d| d.w(line).h_full())
                    .when(!across, |d| d.h(line).w_full())
                    .bg(if dragging { p.muted } else { p.hairline })
                    .group_hover(id, |d| d.bg(p.muted)),
            )
    }

    /// Follow the pointer, in rems from the window's top-left.
    pub(super) fn drag_to(&mut self, x: f32, y: f32, window_w: f32, window_h: f32) {
        match self.drag {
            Some(Seam::Rail) => self.rail_w = x.clamp(RAIL_MIN_W, RAIL_MAX_W),
            Some(Seam::Dock) => {
                let room = window_w - self.rail_w - SEAM_GRAB_W * 2.0 - CHAT_MIN * self.zoom;
                self.dock_w = (window_w - x).clamp(DOCK_MIN, room.max(DOCK_MIN));
            }
            Some(Seam::Term) => {
                self.term.h = (window_h - y).clamp(TERM_MIN_H, window_h - BAR_H - READING_MIN_H);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_follows_the_budget() {
        let shown = |avail, dock, open, zoom| presentation(avail, dock, open, zoom, false);
        // 800px / 16 = 50rem; minus the rail leaves 35.5rem: no room for 30 + 24.
        assert!(
            shown(800.0 / 16.0 - RAIL_W, DOCK_PREF, true, 1.0).0 == Presentation::FocusWorkbench
        );
        assert!(
            shown(1600.0 / 16.0 - RAIL_W, DOCK_PREF, true, 1.0) == (Presentation::Split, DOCK_PREF)
        );
        // Zoom raises the chat's minimum: 83rem holds 30 + 30 at 100%, but at
        // 200% the chat wants 60 and leaves 23, under the dock's minimum.
        assert!(shown(83.0, DOCK_PREF, true, 1.0) == (Presentation::Split, DOCK_PREF));
        assert!(shown(83.0, DOCK_PREF, true, 2.0).0 == Presentation::FocusWorkbench);
        assert!(shown(35.5, DOCK_PREF, false, 1.0).0 == Presentation::Conversation);
    }

    #[test]
    fn a_narrow_window_draws_the_dock_narrower_without_forgetting_its_width() {
        // Dragged wide to 50rem; 70rem of room leaves the chat its 30.
        assert!(presentation(70.0, 50.0, true, 1.0, true) == (Presentation::Split, 40.0));
        // Down to the dock's minimum before the Workbench takes the area.
        assert!(presentation(54.0, 50.0, true, 1.0, true) == (Presentation::Split, DOCK_MIN));
        assert!(presentation(53.0, 50.0, true, 1.0, true).0 == Presentation::FocusWorkbench);
        // Room again: the dragged width, untouched.
        assert!(presentation(120.0, 50.0, true, 1.0, true) == (Presentation::Split, 50.0));
    }

    #[test]
    fn a_split_holds_at_its_threshold_and_needs_slack_to_come_back() {
        // Exactly the dock's minimum beside the chat: an open split holds...
        let edge = CHAT_MIN + DOCK_MIN;
        assert!(presentation(edge, DOCK_PREF, true, 1.0, true).0 == Presentation::Split);
        // ...but one coming back from focus waits for the slack.
        assert!(presentation(edge, DOCK_PREF, true, 1.0, false).0 == Presentation::FocusWorkbench);
        assert!(
            presentation(edge + SPLIT_SLACK, DOCK_PREF, true, 1.0, false).0 == Presentation::Split
        );
    }
}
