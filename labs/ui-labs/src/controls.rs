//! The small rules every view shares: a button shows the pointer, an
//! icon-only button's glyph is muted until the pointer is on it, and a name cut
//! short shows in full on hover.
use super::*;
use gpui::{RenderOnce, Stateful};
use gpui_component::tooltip::Tooltip;
use std::rc::Rc;

/// Every button, with the pointer cursor. The library sets `cursor_default` on
/// all but link and text buttons, then applies the caller's style last, so the
/// cursor set here wins. The app's buttons go through the same wrapper.
pub(super) fn action(id: impl Into<gpui::ElementId>) -> Button {
    Button::new(id).cursor_pointer()
}

impl Labs {
    /// The glyph for an icon-only control: `muted` at rest, `text` under the
    /// pointer or while `on`.
    pub(super) fn glyph(&self, id: &'static str, icon: impl Into<Icon>, on: bool) -> Icon {
        let p = self.palette();
        let lit = on || self.hovered == Some(id);
        icon.into().text_color(if lit { p.text } else { p.muted })
    }

    /// A ghost icon-only button with its tooltip, its glyph following the
    /// pointer. The library sizes the square and the glyph.
    pub(super) fn icon_button(
        &self,
        id: &'static str,
        icon: impl Into<Icon>,
        tip: &'static str,
        cx: &mut Context<Self>,
    ) -> IconButton {
        let this = cx.entity().downgrade();
        IconButton {
            id,
            button: action(id)
                .ghost()
                .small()
                .tooltip(tip)
                .icon(self.glyph(id, icon, false)),
            on_hover: Rc::new(move |inside, _, cx| {
                let inside = *inside;
                this.update(cx, |labs, cx| {
                    let next = if inside {
                        Some(id)
                    } else if labs.hovered == Some(id) {
                        None
                    } else {
                        labs.hovered
                    };
                    if labs.hovered != next {
                        labs.hovered = next;
                        cx.notify();
                    }
                })
                .ok();
            }),
        }
    }
}

type HoverListener = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// An icon-only button whose glyph follows the pointer.
///
/// The hover is watched on a wrapper rather than on the button: the library's
/// tooltip already claims the button's own hover listener, and an element
/// takes only one.
#[derive(IntoElement)]
pub(super) struct IconButton {
    id: &'static str,
    button: Button,
    on_hover: HoverListener,
}

impl IconButton {
    pub(super) fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.button = self.button.on_click(handler);
        self
    }

    pub(super) fn selected(mut self, selected: bool) -> Self {
        self.button = self.button.selected(selected);
        self
    }

    pub(super) fn xsmall(mut self) -> Self {
        self.button = self.button.xsmall();
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on_hover = self.on_hover;
        div()
            .id(SharedString::from(format!("{}-hover", self.id)))
            .flex_none()
            .on_hover(move |inside, window, cx| on_hover(inside, window, cx))
            .child(self.button)
    }
}

/// One line of text that truncates, and says the whole of itself on hover.
pub(super) fn full(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
) -> Stateful<gpui::Div> {
    let text: SharedString = text.into();
    let tip = text.clone();
    div()
        .id(id.into())
        .min_w_0()
        .truncate()
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(text)
}
