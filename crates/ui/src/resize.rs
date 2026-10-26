//! The app's splits and dock edges: a seam that draws nothing until it is held.
//!
//! Every seam a split or a dock edge sits on is already marked by the panels
//! either side of it -- a line down the rail's or the Workbench's edge, or the
//! terminal's edged block -- so a resting line there would be a second edge
//! beside the first. Pointing
//! at one draws nothing either: the cursor already says it can be dragged.
//! Only while it is pressed or dragged is the seam the thing being looked at,
//! and then it is one line in the focus colour, as a dragged panel's edge is.
//!
//! The band, its cursor and the drag are the library's; this is only what is
//! painted inside it.

use gpui::{
    AnyElement, Axis, ElementId, IntoElement, Pixels, Render, Styled as _, div,
    prelude::FluentBuilder as _, px,
};
use gpui_base::{
    HandleEdge, ResizablePanelGroup, ResizeHandle, ResizeHandleContext, ResizeHandleRenderer,
};
use gpui_component::ActiveTheme as _;
use std::rc::Rc;

/// How thick the held seam's line is: the hairline every other edge is.
const SEAM_LINE: Pixels = px(1.);

/// A split laid out left to right, its dividers drawn the app's way.
pub fn h_resizable(id: impl Into<ElementId>) -> ResizablePanelGroup {
    gpui_base::h_resizable(id).with_handle_appearance(seam())
}

/// A split laid out top to bottom, its dividers drawn the app's way.
pub fn v_resizable(id: impl Into<ElementId>) -> ResizablePanelGroup {
    gpui_base::v_resizable(id).with_handle_appearance(seam())
}

/// A lone handle hugging `edge` of its container, for an edge no split owns:
/// a dock's. What the drag resizes is the caller's.
pub fn edge_handle<T: 'static, E: 'static + Render>(
    id: impl Into<ElementId>,
    axis: Axis,
    edge: HandleEdge,
) -> ResizeHandle<T, E> {
    gpui_base::resize_handle(id, axis)
        .inside(edge)
        .with_appearance(seam())
}

fn seam() -> ResizeHandleRenderer {
    Rc::new(|handle, _, cx| Some(seam_line(handle, cx.theme().ring)))
}

/// The painted part of a handle: nothing at rest or under the pointer, the
/// line while held. Never `None`, which would put the library's own resting
/// line back.
fn seam_line(handle: &ResizeHandleContext, ink: gpui::Hsla) -> AnyElement {
    div()
        .flex_none()
        .map(|line| match handle.axis() {
            Axis::Horizontal => line.h_full().w(SEAM_LINE),
            Axis::Vertical => line.w_full().h(SEAM_LINE),
        })
        .when(handle.is_active(), |line| line.bg(ink))
        .into_any_element()
}
