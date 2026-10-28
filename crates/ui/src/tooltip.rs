//! Tooltips that stand for what is under the pointer, one at a time.

use gpui::{AnyView, App, AppContext as _, IntoElement, Render, Window};

/// Put every tooltip away whenever what it stood for may have gone: a key
/// pressed, as gpui's own tooltips do, a press of the pointer, and one of the
/// window's dialogs or sheets coming or going ([`tooltip_guard`]).
///
/// The library's tooltip is put away when its trigger reports the pointer
/// left, and a trigger that stops being drawn reports nothing. A row's
/// actions are drawn only while the row is hovered, and a key pressed ends
/// the hover (the window stops counting the pointer once the keyboard is
/// used); a dialog closes by itself under a pointer resting on its button.
/// Each left a tooltip on screen, across pages, until another replaced it.
///
/// And a press puts away its control's tooltip only for a moment: the
/// control counts as hovered again once the press is over, or once the menu
/// it opened has settled, and asks for the same tooltip anew, which then
/// stood over the menu. So a tooltip asked for while the pointer rests where
/// it was pressed is put away as it shows. The tooltips live in one overlay
/// per window, kept as each is made.
pub fn put_tooltips_away(cx: &mut gpui::App) {
    cx.observe_new(|_: &mut gpui_base::TooltipOverlay, _, cx| {
        let overlay = cx.weak_entity();
        cx.default_global::<Overlays>().overlays.push(overlay);
        cx.observe_self(|overlay, cx| {
            if cx
                .try_global::<Overlays>()
                .is_some_and(|o| o.pressed.is_some())
            {
                overlay.hide(cx);
            }
        })
        .detach();
    })
    .detach();
    cx.intercept_keystrokes(|_, _, cx| hide_tooltips(cx))
        .detach();
}

/// How far the pointer moves from where it was pressed before a tooltip may
/// show again: past a hand's tremble, short of reaching another control.
const PRESS_SLOP: gpui::Pixels = gpui::px(2.);

/// Every window's tooltip overlay, whether each window last drew a dialog or
/// sheet, and where the pointer was pressed while it has not moved away.
#[derive(Default)]
struct Overlays {
    overlays: Vec<gpui::WeakEntity<gpui_base::TooltipOverlay>>,
    layered: std::collections::HashMap<gpui::WindowId, bool>,
    pressed: Option<gpui::Point<gpui::Pixels>>,
}

impl gpui::Global for Overlays {}

fn hide_tooltips(cx: &mut gpui::App) {
    let overlays = std::mem::take(&mut cx.default_global::<Overlays>().overlays);
    let live = overlays
        .into_iter()
        .filter(|overlay| overlay.update(cx, |o, cx| o.hide(cx)).is_ok())
        .collect();
    cx.default_global::<Overlays>().overlays = live;
}

/// What the window mounts once, beside its layers, so a press of the pointer
/// and a dialog or sheet (`layered`) coming or going put tooltips away.
/// Listened for on the way down, past every layer: a dialog, a menu or a
/// loading button that stops a press stops it only after this.
pub fn tooltip_guard(
    layered: bool,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> impl gpui::IntoElement + use<> {
    use gpui::{DispatchPhase, MouseDownEvent, MouseMoveEvent, Styled as _};
    let id = window.window_handle().window_id();
    let was = cx.default_global::<Overlays>().layered.insert(id, layered);
    if was.is_some_and(|was| was != layered) {
        hide_tooltips(cx);
    }
    gpui::canvas(
        |_, _, _| {},
        |_, _, window, _| {
            window.on_mouse_event(|event: &MouseDownEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture {
                    cx.default_global::<Overlays>().pressed = Some(event.position);
                    hide_tooltips(cx);
                }
            });
            window.on_mouse_event(|event: &MouseMoveEvent, phase, _, cx| {
                let pressed = &mut cx.default_global::<Overlays>().pressed;
                if phase == DispatchPhase::Capture
                    && pressed
                        .is_some_and(|at| (event.position - at).magnitude() > PRESS_SLOP.into())
                {
                    *pressed = None;
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// A native tooltip's builder (`div().tooltip(..)`), kept away while a press
/// rests where it landed.
///
/// The overlay above shows only the library's controls' tooltips; a `div`'s
/// own is gpui's, which a press puts away only once it is showing. One still
/// waiting out its delay when the press lands shows after it, over whatever
/// the press opened, and one put away is asked for again by the next tremble
/// of the pointer. Its builder is the one thing of it this side can reach, so
/// while the press rests it builds nothing; gpui counts that as shown, and
/// asks again only once the pointer has left the control and come back.
pub fn tip(
    build: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| {
        if cx
            .try_global::<Overlays>()
            .is_some_and(|o| o.pressed.is_some())
        {
            cx.new(|_| Nothing).into()
        } else {
            build(window, cx)
        }
    }
}

/// What a tooltip kept away draws.
struct Nothing;

impl Render for Nothing {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}
