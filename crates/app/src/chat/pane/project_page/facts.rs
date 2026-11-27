//! Parts of a line that give way in an order: a name that keeps its own
//! width until nothing else is left to give, and a line of facts that takes
//! only the room left over, cut as that room shrinks and not drawn at all
//! once it cannot hold its lead whole, since a lead cut short says nothing.
//!
//! Elements of their own rather than `div`s, for two reasons. Text that
//! truncates answers layout with the width it was cut to on the pass that
//! measured it, not the width it would take, so flex cannot see how wide a
//! name wants to be: [`natural`] shapes the text itself and says so as its
//! basis. And whether to draw the facts hangs on their lead's laid-out
//! width, which only exists once layout has run; a measure taken there and
//! used on the next frame would draw one frame wrong at every resize.

use gpui::{
    AlignItems, AnyElement, App, Bounds, Display, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, JustifyContent, LayoutId, Length, Pixels, SharedString, Style,
    Window,
};

/// `child`, which draws `text` in the style it inherits and cuts it as its
/// own style says, on a basis of the width `text` takes whole, shrinking
/// `shrink` times as fast as a part that shrinks at one.
pub(in crate::chat::pane) fn natural(
    text: impl Into<SharedString>,
    child: AnyElement,
    shrink: f32,
) -> Natural {
    Natural {
        text: text.into(),
        child,
        shrink,
    }
}

pub(in crate::chat::pane) struct Natural {
    text: SharedString,
    child: AnyElement,
    shrink: f32,
}

impl IntoElement for Natural {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Natural {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let text = window.text_style();
        let size = text.font_size.to_pixels(window.rem_size());
        let run = text.to_run(self.text.len());
        let whole = window
            .text_system()
            .shape_line(self.text.clone(), size, &[run], None)
            .width;
        // Up to the next device pixel: layout rounds each edge to one, and a
        // width off that grid can come out a fraction short, which cuts the
        // name by its last character. One on it comes out whole.
        let scale = window.scale_factor();
        let whole = (whole * scale).ceil() / scale;
        let child = self.child.request_layout(window, cx);
        let mut style = Style {
            flex_basis: Length::from(whole),
            flex_shrink: self.shrink,
            ..Style::default()
        };
        style.min_size.width = Length::from(Pixels::ZERO);
        (window.request_layout(style, [child], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// `lead`, then `rest`, on one line in the room its row leaves over, at its
/// end if `end`: `lead` keeps its width and `rest` takes what is left, cut as
/// its own style says; neither is drawn once the room cannot hold `lead`.
pub(in crate::chat::pane) fn facts(lead: AnyElement, rest: AnyElement, end: bool) -> Facts {
    Facts { lead, rest, end }
}

pub(in crate::chat::pane) struct Facts {
    lead: AnyElement,
    rest: AnyElement,
    end: bool,
}

/// The lead's layout, and whether the line is drawn.
pub(in crate::chat::pane) struct Laid {
    lead: LayoutId,
    shown: bool,
}

impl IntoElement for Facts {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Facts {
    type RequestLayoutState = Laid;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Laid) {
        let lead = self.lead.request_layout(window, cx);
        let rest = self.rest.request_layout(window, cx);
        // On no basis of its own, it grows into what the parts before it
        // leave and never takes room from them.
        let mut style = Style {
            display: Display::Flex,
            align_items: Some(AlignItems::Center),
            justify_content: Some(match self.end {
                true => JustifyContent::FlexEnd,
                false => JustifyContent::FlexStart,
            }),
            flex_grow: 1.,
            flex_basis: Length::from(Pixels::ZERO),
            ..Style::default()
        };
        style.min_size.width = Length::from(Pixels::ZERO);
        let id = window.request_layout(style, [lead, rest], cx);
        (id, Laid { lead, shown: false })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        laid: &mut Laid,
        window: &mut Window,
        cx: &mut App,
    ) {
        // In device pixels, where layout put both edges, so a float's
        // fraction does not drop a lead that fits.
        let scale = window.scale_factor();
        let lead = window.layout_bounds(laid.lead).size.width;
        laid.shown = (bounds.size.width * scale).round() >= (lead * scale).round();
        if laid.shown {
            self.lead.prepaint(window, cx);
            self.rest.prepaint(window, cx);
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        laid: &mut Laid,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if laid.shown {
            self.lead.paint(window, cx);
            self.rest.paint(window, cx);
        }
    }
}
