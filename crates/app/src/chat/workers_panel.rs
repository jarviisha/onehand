//! The workers' region as a dock panel: mounted in the Workbench's place
//! while it shows, and drawn by the conversation's pane, which holds the
//! workers' conversations.

use super::pane::ChatPane;
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, WeakEntity, Window, div,
};
use gpui_base::dock::{Panel, PanelEvent};
use gpui_component::StyledExt as _;

pub struct WorkersPanel {
    pane: WeakEntity<ChatPane>,
    focus_handle: FocusHandle,
    zoom: crate::zoom::Zoom,
    maximized: bool,
    /// The pane notifies whenever a conversation it holds changes, workers'
    /// included, and this panel draws from them.
    _watch: Subscription,
}

impl WorkersPanel {
    pub fn new(pane: &gpui::Entity<ChatPane>, cx: &mut Context<Self>) -> Self {
        Self {
            pane: pane.downgrade(),
            focus_handle: cx.focus_handle(),
            zoom: crate::zoom::Zoom::default(),
            maximized: false,
            _watch: cx.observe(pane, |_, _, cx| cx.notify()),
        }
    }

    pub fn zoom(&self) -> crate::zoom::Zoom {
        self.zoom
    }

    pub fn set_zoom(&mut self, zoom: crate::zoom::Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        cx.notify();
    }

    /// Tell the strip which way round the maximize is.
    pub fn set_maximized(&mut self, maximized: bool, cx: &mut Context<Self>) {
        if self.maximized != maximized {
            self.maximized = maximized;
            cx.notify();
        }
    }
}

/// The behaviour half of the dock's panel trait alone, as the Workbench's:
/// the strip is the region's own, and the dock draws none.
impl Panel for WorkersPanel {
    fn panel_name(&self) -> &'static str {
        "Workers"
    }
}

impl EventEmitter<PanelEvent> for WorkersPanel {}

impl Focusable for WorkersPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WorkersPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (maximized, zoom) = (self.maximized, self.zoom);
        let body = self
            .pane
            .update(cx, |pane, cx| {
                pane.workers_region(maximized, zoom, window, cx)
            })
            .unwrap_or_else(|_| div().into_any_element());
        let panel = div()
            .size_full()
            .bg(crate::theme::dock_surface(cx))
            // Mounted bare, as the Workbench is, so the panel tracks its own
            // focus and its keys reach only it.
            .track_focus(&self.focus_handle)
            .key_context("Workers")
            .v_flex()
            .child(self.zoom.scale(body));
        // The Workbench's region for a drag: the two never show at once.
        crate::text_drag::region(crate::text_drag::Region::Workbench, panel, cx)
    }
}
