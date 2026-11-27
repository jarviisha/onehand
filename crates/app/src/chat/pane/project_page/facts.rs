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
    InspectorElementId, IntoElement, JustifyContent, LayoutId, Length, Pixels, Refineable,
    SharedString, Style, TextStyle, TextStyleRefinement, Window,
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
        give: Give::Shrinks(shrink),
        lettered: TextStyleRefinement::default(),
    }
}

/// `child`, which draws `text` as [`natural`]'s does, in the room the parts
/// before it leave over, up to the width `text` takes whole, once a
/// [`Facts::ahead`] beside it holds its own; it is not drawn once that room
/// cannot hold `lead`, the start of `text` that says what it is.
pub(in crate::chat::pane) fn leftover(
    text: impl Into<SharedString>,
    lead: impl Into<SharedString>,
    child: AnyElement,
) -> Natural {
    Natural {
        text: text.into(),
        child,
        give: Give::Grows(lead.into()),
        lettered: TextStyleRefinement::default(),
    }
}

/// How far ahead of a [`leftover`] a [`Facts::ahead`] takes the room left
/// over: flex shares it by grow factor, so this far apart the leftover gets
/// all but nothing until the facts hold their whole width.
const AHEAD: f32 = 1e6;

pub(in crate::chat::pane) struct Natural {
    text: SharedString,
    child: AnyElement,
    give: Give,
    lettered: TextStyleRefinement,
}

enum Give {
    /// On its whole width, shrinking at this factor.
    Shrinks(f32),
    /// On none, growing into the room left up to its whole width, and drawn
    /// only while that holds this lead.
    Grows(SharedString),
}

impl Natural {
    /// Measure `text` lettered as `child` letters it where that differs from
    /// what it inherits (a size of its own).
    pub(in crate::chat::pane) fn lettered(mut self, style: TextStyleRefinement) -> Self {
        self.lettered = style;
        self
    }
}

/// The width `text` takes on one line in `style`, up to the next device
/// pixel: layout rounds each edge to one, and a width off that grid can come
/// out a fraction short, which cuts the text by its last character.
fn whole_width(text: &SharedString, style: &TextStyle, window: &Window) -> Pixels {
    let size = style.font_size.to_pixels(window.rem_size());
    // Shaped as one line, which a hand-written title or a folder's name may
    // not be: the shaper refuses a line break.
    let line: SharedString = match text.contains('\n') {
        true => text.replace('\n', " ").into(),
        false => text.clone(),
    };
    let run = style.to_run(line.len());
    let width = window
        .text_system()
        .shape_line(line, size, &[run], None)
        .width;
    let scale = window.scale_factor();
    (width * scale).ceil() / scale
}

/// Whether `room` holds `width`, in device pixels, where layout put both
/// edges, so a float's fraction does not drop what fits.
fn holds(room: Pixels, width: Pixels, window: &Window) -> bool {
    let scale = window.scale_factor();
    (room * scale).round() >= (width * scale).round()
}

impl IntoElement for Natural {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Natural {
    /// The least width it is drawn at.
    type RequestLayoutState = Pixels;
    type PrepaintState = bool;

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
    ) -> (LayoutId, Pixels) {
        let mut text = window.text_style();
        text.refine(&self.lettered);
        let whole = whole_width(&self.text, &text, window);
        let child = self.child.request_layout(window, cx);
        let (mut style, least) = match &self.give {
            Give::Shrinks(shrink) => (
                Style {
                    flex_basis: Length::from(whole),
                    flex_shrink: *shrink,
                    ..Style::default()
                },
                Pixels::ZERO,
            ),
            Give::Grows(lead) => {
                let mut style = Style {
                    flex_basis: Length::from(Pixels::ZERO),
                    flex_grow: 1.,
                    ..Style::default()
                };
                style.max_size.width = Length::from(whole);
                (style, whole_width(lead, &text, window))
            }
        };
        style.min_size.width = Length::from(Pixels::ZERO);
        (window.request_layout(style, [child], cx), least)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        least: &mut Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let shown = holds(bounds.size.width, *least, window);
        if shown {
            self.child.prepaint(window, cx);
        }
        shown
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Pixels,
        shown: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *shown {
            self.child.paint(window, cx);
        }
    }
}

/// `lead`, then `rest`, on one line in the room its row leaves over, at its
/// end if `end`: `lead` keeps its width and `rest` takes what is left, cut as
/// its own style says; neither is drawn once the room cannot hold `lead`.
pub(in crate::chat::pane) fn facts(lead: AnyElement, rest: AnyElement, end: bool) -> Facts {
    Facts {
        lead,
        rest,
        end,
        ahead: None,
    }
}

pub(in crate::chat::pane) struct Facts {
    lead: AnyElement,
    rest: AnyElement,
    end: bool,
    ahead: Option<(SharedString, TextStyleRefinement)>,
}

impl Facts {
    /// Up to the width `text`, all it says lettered as `lettered`, takes
    /// whole, and that ahead of a [`leftover`] beside it, which gets what is
    /// left once these hold it; at the row's end still if `end`.
    pub(in crate::chat::pane) fn ahead(
        mut self,
        text: impl Into<SharedString>,
        lettered: TextStyleRefinement,
    ) -> Self {
        self.ahead = Some((text.into(), lettered));
        self
    }
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
        if let Some((text, lettered)) = &self.ahead {
            let mut letters = window.text_style();
            letters.refine(lettered);
            style.max_size.width = Length::from(whole_width(text, &letters, window));
            style.flex_grow = AHEAD;
            // Held to its width, it still stands at the row's end.
            if self.end {
                style.margin.left = Length::Auto;
            }
        }
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
        let lead = window.layout_bounds(laid.lead).size.width;
        laid.shown = holds(bounds.size.width, lead, window);
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
