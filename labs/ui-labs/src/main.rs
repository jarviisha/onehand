//! ui-labs: a proposed onehand UI drawn with gpui-component, on static data.
//! Nothing here is the app; `make showcase` opens the app as it is.
//!
//! - Resize the window: the chat keeps `CHAT_MIN` beside the Workbench, and
//!   below that the Workbench takes the content area with a way back.
//! - Drag the seams: the rail, the Workbench and the terminal resize.
//! - `Ctrl+\` the Workbench, `` Ctrl+` `` the terminal, `Ctrl+Shift+B` the
//!   rail, `Ctrl+=` / `Ctrl+-` / `Ctrl+0` the reading size.
//! - The palette button swaps light and dark through the theme config, so the
//!   library's own buttons follow.
//! - The rail opens the overview, Tasks, Issues, the composer cards and
//!   Settings; a session returns to the chat; a project's `⋯` asks to delete.
use gpui::{
    App, AppContext, Context, FocusHandle, Hsla, InteractiveElement, IntoElement, KeyBinding,
    MouseButton, MouseMoveEvent, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Window, actions, div, prelude::FluentBuilder, rems,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Root, Selectable as _, Sizable as _, StyledExt as _, Theme,
    ThemeMode,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

mod tokens;
use tokens::*;

mod assets;
mod chat;
mod composer;
mod controls;
mod gallery;
mod layout;
mod live;
mod menus;
mod pages;
mod rail;
mod settings;
mod terminal;
mod theme;
mod workbench;

#[cfg(test)]
mod guards;

use controls::action;
use layout::{Presentation, Seam, presentation};
use theme::{install, set_mode};

actions!(
    labs,
    [
        ToggleWorkbench,
        ToggleTerminal,
        ToggleRail,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        PopupUp,
        PopupDown
    ]
);

/// The window's own keys, bound in the `Labs` context so they work from
/// anywhere inside it, the composer's field included.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-\\", ToggleWorkbench, Some("Labs")),
        KeyBinding::new("ctrl-`", ToggleTerminal, Some("Labs")),
        KeyBinding::new("ctrl-shift-b", ToggleRail, Some("Labs")),
        KeyBinding::new("ctrl-=", ZoomIn, Some("Labs")),
        KeyBinding::new("ctrl-+", ZoomIn, Some("Labs")),
        KeyBinding::new("ctrl--", ZoomOut, Some("Labs")),
        KeyBinding::new("ctrl-0", ZoomReset, Some("Labs")),
        // Only while a popup is open does the composer claim the arrows; the
        // binding sits at the field's depth and is registered after the
        // field's own, so it wins there and nowhere else.
        KeyBinding::new("up", PopupUp, Some("LabsPopup > Input")),
        KeyBinding::new("down", PopupDown, Some("LabsPopup > Input")),
    ]);
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Chat,
    Overview,
    Tasks,
    Issues,
    Composer,
    Settings,
}

struct Labs {
    focus: FocusHandle,
    dark: bool,
    workbench: bool,
    page: Page,
    task_open: bool,
    issue: Option<usize>,
    choice: usize,
    picks: [bool; 4],
    /// What the person dragged to, in rems; drawn through `presentation`.
    rail_w: f32,
    dock_w: f32,
    drag: Option<Seam>,
    /// The split showed last frame, for the threshold's slack.
    was_split: bool,
    /// The Workbench has stepped aside for the chat while it cannot sit
    /// beside it; it is still open.
    chat_over_workbench: bool,
    rail_hidden: bool,
    activity_open: bool,
    /// The reading zoom: the transcript, the composer, documents.
    zoom: f32,
    /// The icon-only button under the pointer, whose glyph is lit.
    hovered: Option<&'static str>,
    /// Projects deleted through the dialog.
    removed: Vec<&'static str>,
    live: live::Live,
    wb: workbench::Wb,
    term: terminal::Term,
    settings: settings::Settings,
}

impl Labs {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            focus,
            dark: false,
            workbench: true,
            page: Page::Overview,
            task_open: false,
            issue: Some(0),
            choice: 0,
            picks: [true, false, false, false],
            rail_w: RAIL_W,
            dock_w: DOCK_PREF,
            drag: None,
            was_split: true,
            chat_over_workbench: false,
            rail_hidden: false,
            activity_open: false,
            zoom: 1.0,
            hovered: None,
            removed: Vec::new(),
            live: live::Live::new(window, cx),
            wb: workbench::Wb::new(window, cx),
            term: terminal::Term::new(),
            settings: settings::Settings::new(window, cx),
        }
    }

    fn palette(&self) -> Palette {
        if self.dark { dark() } else { light() }
    }

    fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        // Snapped to the step, so stepping up and back lands on 100% again.
        self.zoom = ((zoom / ZOOM_STEP).round() * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
        cx.notify();
    }
}

impl Render for Labs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette();
        let rem = window.rem_size();
        let viewport = window.viewport_size();
        let (window_w, window_h) = (viewport.width / rem, viewport.height / rem);
        let chat = self.page == Page::Chat;
        // A maximized Workbench takes the whole window, the rail included.
        let wb_full = chat && self.workbench && self.wb.maximized;
        let rail_on = !self.rail_hidden && !wb_full;
        // Each seam takes its grab width out of the row.
        let avail = window_w
            - if rail_on {
                self.rail_w + SEAM_GRAB_W
            } else {
                0.0
            };
        let (shown, dock) = presentation(
            avail - SEAM_GRAB_W,
            self.dock_w,
            self.workbench,
            self.zoom,
            self.was_split,
        );
        self.was_split = shown == Presentation::Split;
        // A Workbench that stepped aside comes back with the split; until then
        // the chat is what the content area shows.
        let shown = match shown {
            Presentation::Split => {
                self.chat_over_workbench = false;
                shown
            }
            Presentation::FocusWorkbench if self.chat_over_workbench => Presentation::Conversation,
            other => other,
        };
        let chat_w = match shown {
            Presentation::Split => avail - SEAM_GRAB_W - dock,
            _ => avail,
        };

        h_flex()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context("Labs")
            .bg(p.page)
            .text_sm()
            .text_color(p.text)
            .on_action(cx.listener(|this, _: &ToggleWorkbench, _, cx| this.toggle_workbench(cx)))
            .on_action(cx.listener(|this, _: &ToggleTerminal, _, cx| this.toggle_terminal(cx)))
            .on_action(cx.listener(|this, _: &ToggleRail, _, cx| {
                this.rail_hidden = !this.rail_hidden;
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &ZoomIn, _, cx| this.set_zoom(this.zoom + ZOOM_STEP, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ZoomOut, _, cx| this.set_zoom(this.zoom - ZOOM_STEP, cx)),
            )
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.set_zoom(1.0, cx)))
            .when_some(self.drag, |d, seam| {
                if seam == Seam::Term {
                    d.cursor_row_resize()
                } else {
                    d.cursor_col_resize()
                }
            })
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                if this.drag.is_none() {
                    return;
                }
                // Released outside the window: the up never arrived here.
                if e.pressed_button != Some(MouseButton::Left) {
                    this.drag = None;
                } else {
                    this.drag_to(e.position.x / rem, e.position.y / rem, window_w, window_h);
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .when(rail_on, |d| {
                d.child(self.rail(&p, cx))
                    .child(self.seam(&p, Seam::Rail, cx))
            })
            .when(!chat, |d| d.child(self.page_view(&p, avail, cx)))
            .when(wb_full, |d| d.child(self.workbench(&p, window_w, true, cx)))
            .when(
                !wb_full && chat && shown != Presentation::FocusWorkbench,
                |d| d.child(self.chat(&p, chat_w, window_h, window, cx)),
            )
            .when(!wb_full && chat && shown == Presentation::Split, |d| {
                d.child(self.seam(&p, Seam::Dock, cx))
                    .child(self.workbench(&p, dock, false, cx))
            })
            .when(
                !wb_full && chat && shown == Presentation::FocusWorkbench,
                |d| d.child(self.workbench(&p, avail, true, cx)),
            )
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            bind_keys(cx);
            install(cx);
            cx.open_window(Default::default(), |window, cx| {
                let view = cx.new(|cx| Labs::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("window");
            cx.activate(true);
        });
}
