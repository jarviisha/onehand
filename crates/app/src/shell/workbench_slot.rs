//! The Workbench's place in the window: the right slot of the dock area's
//! centre, beside the conversation.
//!
//! In the centre rather than the right dock because the area draws the bottom
//! dock under the centre alone, so a terminal under a right dock stopped at
//! the Workbench's edge. Beside the conversation in one split, it spans both.
//!
//! A slot is put away by its panel answering `visible` with `false`, never by
//! taking it out of the tree: a hidden slot takes no room, and the tab group
//! around it, its focus and the split's sizes all survive until it comes back.
//! The same switch on the conversation's slot is how the Workbench takes the
//! whole centre, narrow or maximized, with the terminal left under it.
//!
//! The split grows its last drawn slot, which is the Workbench's whenever it
//! shows: its width is held by sizing the conversation's slot to whatever the
//! centre leaves.

use super::Shell;
use gpui::{App, Context, Focusable as _, Pixels, Window, px};
use gpui_component::dock::{DockArea, DockLayout, DockPlacement, NodeId, PaneRef};

/// A panel as the dock holds it.
type PanelView = Arc<dyn gpui_component::dock::BasePanelView>;
use onehand_core::config::PanelLayout;
use std::sync::Arc;

/// Mount the conversation and the Workbench side by side as the centre, the
/// Workbench's slot at the width `layout` saved. Each is a group of one
/// panel, and the area's skin draws no tab bar over either.
pub(super) fn mount(
    area: &mut DockArea,
    chat: PanelView,
    workbench: PanelView,
    layout: &PanelLayout,
    window: &mut Window,
    cx: &mut Context<DockArea>,
) {
    // The conversation's slot is left to flex until the centre has been
    // measured; the first frame that knows its width sizes it.
    area.set_center(
        DockLayout::h_split()
            .child(DockLayout::tabs().panel_view(chat, cx), None)
            .child(
                DockLayout::tabs().panel_view(workbench, cx),
                Some(px(layout.workbench_w)),
            ),
        window,
        cx,
    );
}

/// Draw the Workbench's slot `width` wide, the conversation's taking the rest
/// of the centre as last measured. Nothing is written when the split already
/// says so, or before the centre has a width.
pub(super) fn set_width(
    area: &mut DockArea,
    width: Pixels,
    window: &mut Window,
    cx: &mut Context<DockArea>,
) {
    let centre = area.bounds().size.width;
    if centre <= px(0.) {
        return;
    }
    let sizes = vec![(centre - width).max(px(0.)), width];
    let Some((node, now)) = split(area) else {
        return;
    };
    if now.iter().copied().eq(sizes.iter().copied().map(Some)) {
        return;
    }
    area.set_split_sizes(node, sizes, window, cx);
}

/// The width the split holds for the Workbench's slot: the last one set, or
/// the one a drag on the divider left.
pub(super) fn width(area: &DockArea, _: &App) -> Option<Pixels> {
    split(area).and_then(|(_, sizes)| sizes.get(1).copied().flatten())
}

/// The centre's split and its slot sizes.
fn split(area: &DockArea) -> Option<(NodeId, Vec<Option<Pixels>>)> {
    let root = area.layout(DockPlacement::Center)?.root();
    match root.kind() {
        PaneRef::Split { sizes, .. } => Some((root.id(), sizes.to_vec())),
        PaneRef::Tabs { .. } => None,
    }
}

/// Put the Workbench on screen or away.
///
/// The caret is given back to the conversation while the Workbench is still
/// drawn, when it was in there: a handle no frame draws answers no key.
pub(super) fn set_open(
    shell: &mut Shell,
    open: bool,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    if !open
        && shell
            .workbench
            .focus_handle(cx)
            .contains_focused(window, cx)
    {
        shell.last_panel = super::FocusedPanel::Chat;
        shell
            .chat
            .update(cx, |pane, cx| pane.reclaim_focus(window, cx));
    }
    if shell.workbench.read(cx).shown() == open {
        return;
    }
    shell
        .workbench
        .update(cx, |panel, cx| panel.set_shown(open, cx));
    shell.dock.update(cx, |_, cx| cx.notify());
}

/// Show the conversation's slot or hide it, the Workbench then taking the
/// whole centre over the terminal.
pub(super) fn set_chat_shown(shell: &mut Shell, shown: bool, cx: &mut Context<Shell>) {
    if gpui_component::dock::BasePanel::visible(shell.chat.read(cx), cx) == shown {
        return;
    }
    shell.chat.update(cx, |pane, cx| pane.set_shown(shown, cx));
    shell.dock.update(cx, |_, cx| cx.notify());
}

#[cfg(test)]
mod tests;
