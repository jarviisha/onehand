//! A text drag stays in the region it began in.
//!
//! The library's text selection reads the whole window as one document: every
//! selectable block registers with the window's one selection, and a drag ends
//! on whichever block lies under the pointer, or the nearest one above it. A
//! drag begun in the conversation therefore ran on into the Workbench's
//! Markdown, and one begun there ran back into the conversation.
//!
//! The only partition the library offers is a scope, and only the window's
//! active scope takes part, which the library's root sets on every frame (the
//! window's own, or the open dialog's). So the other regions are not given a
//! scope of their own at rest: while a drag that began in one region is held,
//! every other region is drawn under a scope that is never active, and its
//! blocks drop out of the drag. Releasing the button gives them back, so the
//! next press anywhere starts as it always did.

use gpui::{
    App, Div, Global, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    MouseUpEvent, Window,
};
use gpui_base::{ElementExt as _, TextSelectionScopeId};

/// A part of the window a text drag cannot leave.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    Conversation,
    Workbench,
}

/// The region the held drag began in, and the scope the others wait under.
struct Held {
    region: Option<Region>,
    aside: TextSelectionScopeId,
}

impl Global for Held {}

fn held(cx: &mut App) -> &mut Held {
    if !cx.has_global::<Held>() {
        cx.set_global(Held {
            region: None,
            aside: TextSelectionScopeId::new(),
        });
    }
    cx.global_mut::<Held>()
}

/// `body` as `region`: a press in it claims the drag for it, the release
/// (wherever it lands) frees it, and while another region holds the drag its
/// text takes no part in the selection.
pub(crate) fn region(region: Region, body: Div, cx: &mut App) -> impl IntoElement + use<> {
    let state = held(cx);
    let scope = match state.region {
        Some(other) if other != region => state.aside,
        _ => TextSelectionScopeId::default(),
    };
    // A press anywhere in the region, in the capture phase, so a block that
    // stops the press for its own selection still claims the drag here.
    let claim = move |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
        if held(cx).region != Some(region) {
            held(cx).region = Some(region);
            cx.refresh_windows();
        }
    };
    let free = |_: &MouseUpEvent, _: &mut Window, cx: &mut App| {
        if held(cx).region.take().is_some() {
            cx.refresh_windows();
        }
    };
    body.capture_any_mouse_down(claim)
        .capture_any_mouse_up(free)
        .on_mouse_up_out(MouseButton::Left, free)
        .text_selection_scope(scope)
}
