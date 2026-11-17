//! What the dock area draws around the window's three panels: a box each.
//! The conversation and the Workbench share the centre's split, and the
//! terminal is the bottom dock under both.
//!
//! Each region holds one panel in a group of its own, and no group draws a tab
//! bar -- a single tab that can never have a sibling is a duplicate title, and
//! every panel here draws its own chrome. What a group does draw is the
//! panel's box ([`onehand_ui::region`]), inset by the region gap along its
//! leading edges, its left and its top: every box carries its own gap to the
//! neighbour before it, so two boxes side by side or one over the other stand
//! one gap apart in any split, and the shell pads the window's trailing edges
//! to match. A dock adds the edge it is dragged by, and that edge, hugging
//! the dock's leading side, lies in the gap rather than over the box.
//!
//! **Every panel but the conversation is drawn cached.** A panel that has not
//! notified is painted from its last frame, so a conversation streaming an
//! answer -- a frame per chunk -- does not lay out the terminal grid, Neovim
//! and the editor again with every one. The conversation itself is not:
//! it is the one that changes every frame, and caching it buys nothing.

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Axis, Context, DispatchPhase, Empty, EntityId,
    IntoElement, MouseMoveEvent, MouseUpEvent, ParentElement as _, Render, StyleRefinement,
    Styled as _, WeakEntity, Window, canvas, div, prelude::FluentBuilder as _,
};
use gpui_base::{HandleEdge, ResizeHandleContext};
use gpui_component::ActiveTheme as _;
use gpui_component::dock::{
    DockArea, DockAreaRenderer, DockContext, DockEvent, DockPlacement, PanelId, TabGroupContext,
    TabGroupRenderer,
};
use onehand_ui::tokens::REGION_GAP;
use std::cell::Cell;
use std::rc::Rc;

pub(super) struct DockSkin {
    area: WeakEntity<DockArea>,
    chat: PanelId,
    /// The dock whose edge is being dragged, if any. Only one can be.
    resizing: Rc<Cell<Option<DockPlacement>>>,
}

impl DockSkin {
    /// The skin for the area being built in `cx`, around the conversation
    /// panel `chat`.
    pub(super) fn new(chat: EntityId, cx: &mut Context<DockArea>) -> Rc<Self> {
        Rc::new(Self {
            area: cx.weak_entity(),
            chat: PanelId::from(chat),
            resizing: Rc::default(),
        })
    }
}

/// The payload a dock's edge drags. It draws nothing: the edge is the
/// affordance.
#[derive(Clone)]
struct ResizeDock;

impl Render for ResizeDock {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl DockAreaRenderer for DockSkin {
    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        let placement = dock.placement();
        // The terminal is the one dock the window mounts: the Workbench sits
        // in the centre beside the conversation. One id per placement all the
        // same, since the docks render under one stateful ancestor and a
        // shared id would share the handles' state.
        let (id, edge, axis) = match placement {
            DockPlacement::Bottom => ("dock-edge-bottom", HandleEdge::Leading, Axis::Vertical),
            DockPlacement::Left => ("dock-edge-left", HandleEdge::Trailing, Axis::Horizontal),
            _ => ("dock-edge-side", HandleEdge::Leading, Axis::Horizontal),
        };
        let resizing = self.resizing.clone();
        let handle = onehand_ui::edge_handle(id, axis, edge)
            .with_appearance(Rc::new(|handle, _, cx| Some(gap_seam(handle, cx))))
            .on_drag(ResizeDock, move |drag, _, _, cx| {
                cx.stop_propagation();
                resizing.set(Some(placement));
                cx.new(|_| drag.clone())
            });
        div()
            .flex()
            .size_full()
            .relative()
            .child(content)
            .child(handle)
            .child(self.tracker(dock.clone()))
            .into_any_element()
    }

    fn render_split_handle(
        &self,
        handle: &ResizeHandleContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        Some(gap_seam(handle, cx))
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(BareGroup { chat: self.chat })
    }
}

impl DockSkin {
    /// Turn the window's pointer into the drag of one dock's edge. A drag
    /// follows the pointer anywhere in the window, not only over the edge, so
    /// it is heard at the window rather than on the handle.
    fn tracker(&self, dock: DockContext) -> impl IntoElement + use<> {
        let placement = dock.placement();
        let (resizing, area) = (self.resizing.clone(), self.area.clone());
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                window.on_mouse_event({
                    let (dock, resizing) = (dock.clone(), resizing.clone());
                    move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && resizing.get() == Some(placement) {
                            dock.resize_to(event.position, window, cx);
                        }
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || resizing.get() != Some(placement) {
                        return;
                    }
                    resizing.set(None);
                    dock.end_resize(window, cx);
                    // The size lives on the dock rather than in the layout,
                    // so nothing else says there is a new one to save.
                    _ = area.update(cx, |_, cx| cx.emit(DockEvent::LayoutChanged));
                });
            },
        )
        .absolute()
        .size_0()
    }
}

/// The held line of a split's divider or a dock's edge, drawn down the middle
/// of the gap the later box leaves before it rather than against either box.
/// Both bands start their line at the gap's leading edge, so half a gap in
/// centres it. Nothing at rest, as every seam in the app.
fn gap_seam(handle: &ResizeHandleContext, cx: &App) -> AnyElement {
    let mid = REGION_GAP / 2.;
    onehand_ui::seam_line(handle, cx.theme().ring)
        .map(|line| match handle.axis() {
            Axis::Horizontal => line.ml(mid),
            Axis::Vertical => line.mt(mid),
        })
        .into_any_element()
}

/// A group that draws only its panel, in its box.
struct BareGroup {
    chat: PanelId,
}

impl TabGroupRenderer for BareGroup {
    fn render_tab_bar(&self, _: &TabGroupContext, _: &mut Window, _: &mut App) -> AnyElement {
        Empty.into_any_element()
    }

    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        _: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        // A closed dock draws nothing, and a cached view would still paint.
        if group.is_collapsed() {
            return Empty.into_any_element();
        }
        let live = PanelId::from(panel.entity_id()) == self.chat;
        // The box is the positioned parent, inside the gap, so a cached view
        // laid out absolute fills the box and not the gap with it.
        let boxed = onehand_ui::region(cx).relative().map(|slot| match live {
            true => slot.child(panel),
            false => slot.child(panel.cached(StyleRefinement::default().absolute().size_full())),
        });
        div()
            .flex_1()
            .size_full()
            .pl(REGION_GAP)
            .pt(REGION_GAP)
            .child(boxed)
            .into_any_element()
    }
}
