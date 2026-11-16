use super::*;
use gpui::{
    AppContext as _, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    Styled as _, TestAppContext, VisualTestContext, div, size,
};
use gpui_component::ElementExt as _;
use gpui_component::dock::{DockLayout, PanelEvent};
use onehand_ui::tokens::REGION_GAP;
use std::cell::Cell;
use std::rc::Rc;

/// A panel that can be hidden and records the width its box gave it.
struct Probe {
    focus_handle: FocusHandle,
    shown: bool,
    width: Rc<Cell<Pixels>>,
}

impl gpui_component::dock::BasePanel for Probe {
    fn panel_name(&self) -> &'static str {
        "Probe"
    }

    fn visible(&self, _: &App) -> bool {
        self.shown
    }
}

impl EventEmitter<PanelEvent> for Probe {}

impl Focusable for Probe {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.clone();
        div()
            .size_full()
            .on_prepaint(move |bounds, _, _| width.set(bounds.size.width))
    }
}

struct Rig<'a> {
    area: Entity<DockArea>,
    chat: (Entity<Probe>, Rc<Cell<Pixels>>),
    workbench: (Entity<Probe>, Rc<Cell<Pixels>>),
    bottom: Rc<Cell<Pixels>>,
    cx: &'a mut VisualTestContext,
}

impl Rig<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        for _ in 0..3 {
            self.cx.update(|window, _| window.refresh());
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    /// Hide or show one probe, as the panel's own flag does.
    fn show(&mut self, probe: &Entity<Probe>, shown: bool) {
        let area = self.area.clone();
        self.cx.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.shown = shown;
                cx.notify();
            });
            area.update(cx, |_, cx| cx.notify());
        });
        self.draw();
    }

    /// The dock area's width, and the gap each box keeps on its left.
    fn centre_and_gap(&mut self) -> (Pixels, Pixels) {
        let area = self.area.clone();
        self.cx.update(|window, cx| {
            (
                area.read(cx).bounds().size.width,
                REGION_GAP.to_pixels(window.rem_size()),
            )
        })
    }

    fn set_width(&mut self, width: Pixels) {
        let area = self.area.clone();
        self.cx
            .update(|window, cx| area.update(cx, |area, cx| set_width(area, width, window, cx)));
        self.draw();
    }
}

/// A dock area in the app's skin, the two probes mounted as the shell mounts
/// the conversation and the Workbench, and a probe in the bottom dock.
fn rig(cx: &mut TestAppContext, layout: PanelLayout) -> Rig<'_> {
    cx.update(gpui_component::init);
    let made = |shown: bool, cx: &mut App| {
        let width = Rc::new(Cell::new(px(0.)));
        let probe = cx.new(|cx| Probe {
            focus_handle: cx.focus_handle(),
            shown,
            width: width.clone(),
        });
        (probe, width)
    };
    let chat = cx.update(|cx| made(true, cx));
    let workbench = cx.update(|cx| made(layout.workbench_open, cx));
    let (bottom_probe, bottom) = cx.update(|cx| made(true, cx));
    let chat_id = chat.0.entity_id();
    let (area, cx) = cx.add_window_view(|window, cx| {
        let skin = super::super::dock_skin::DockSkin::new(chat_id, cx);
        DockArea::new("slot", None, window, cx).with_renderer(skin)
    });
    cx.simulate_resize(size(px(1400.), px(900.)));
    let (chat_view, workbench_view) = (chat.0.clone(), workbench.0.clone());
    cx.update(|window, cx| {
        area.update(cx, |area, cx| {
            mount(
                area,
                Arc::new(chat_view),
                Arc::new(workbench_view),
                &layout,
                window,
                cx,
            );
            area.set_dock(
                DockPlacement::Bottom,
                DockLayout::tabs().panel(bottom_probe),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Bottom, px(200.), window, cx);
        })
    });
    let mut rig = Rig {
        area,
        chat,
        workbench,
        bottom,
        cx,
    };
    rig.draw();
    rig
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

fn open(width: f32) -> PanelLayout {
    PanelLayout {
        workbench_w: width,
        workbench_open: true,
        ..PanelLayout::default()
    }
}

#[gpui::test]
fn the_terminal_spans_the_conversation_and_the_workbench(cx: &mut TestAppContext) {
    let mut rig = rig(cx, open(400.));
    rig.set_width(px(400.));
    let (_, gap) = rig.centre_and_gap();
    let (chat, workbench, bottom) = (rig.chat.1.get(), rig.workbench.1.get(), rig.bottom.get());
    assert!(
        chat > px(0.) && workbench > px(0.),
        "{chat:?} {workbench:?}"
    );
    assert!(
        near(workbench, px(400.) - gap),
        "the Workbench's slot is the width set: {workbench:?}"
    );
    assert!(
        near(bottom, chat + workbench + gap),
        "the terminal spans both: {bottom:?} against {chat:?} + {workbench:?} + {gap:?}"
    );
    let area = rig.area.clone();
    let read = rig.cx.update(|_, cx| width(area.read(cx), cx));
    assert_eq!(read, Some(px(400.)), "the width set is the width read back");
}

#[gpui::test]
fn a_hidden_workbench_gives_the_conversation_the_width(cx: &mut TestAppContext) {
    let mut rig = rig(cx, open(400.));
    rig.set_width(px(400.));
    let workbench = rig.workbench.0.clone();
    rig.show(&workbench, false);
    let (centre, gap) = rig.centre_and_gap();
    let chat = rig.chat.1.get();
    assert!(
        near(chat, centre - gap),
        "the conversation takes the centre: {chat:?} of {centre:?}"
    );
    assert!(near(rig.bottom.get(), centre - gap));
}

#[gpui::test]
fn hiding_the_chat_leaves_the_terminal(cx: &mut TestAppContext) {
    let mut rig = rig(cx, open(400.));
    rig.set_width(px(400.));
    let chat = rig.chat.0.clone();
    rig.bottom.set(px(-1.));
    rig.show(&chat, false);
    let (centre, gap) = rig.centre_and_gap();
    let workbench = rig.workbench.1.get();
    assert!(
        near(workbench, centre - gap),
        "the Workbench takes the centre: {workbench:?} of {centre:?}"
    );
    assert!(rig.bottom.get() > px(0.), "the terminal is still drawn");
}

/// A launch with the Workbench closed at its saved width: opened, it comes
/// back at that width, not at a share of the centre.
#[gpui::test]
fn the_saved_width_comes_back_when_reopened(cx: &mut TestAppContext) {
    let (_, shell, cx) =
        crate::shell::rig::shell(cx, crate::shell::rig::folder("slot-reopen"), Vec::new());
    cx.simulate_resize(size(px(1600.), px(1000.)));
    let draw = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        for _ in 0..3 {
            cx.update(|window, _| window.refresh());
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    };
    draw(cx);
    let saved = cx.update(|_, cx| shell.read(cx).window.workspace.layout);
    assert!(!saved.workbench_open);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.toggle_workbench(window, cx)));
    draw(cx);
    let drawn = cx.update(|_, cx| {
        let shell = shell.read(cx);
        width(shell.dock.read(cx), cx)
    });
    assert_eq!(drawn, Some(px(saved.workbench_w)));
    let (box_w, gap) = cx.update(|window, cx| {
        let rem = window.rem_size();
        let shell = shell.read(cx);
        (
            shell.workbench.read(cx).drawn_width().to_pixels(rem),
            REGION_GAP.to_pixels(rem),
        )
    });
    assert!(
        near(box_w, px(saved.workbench_w) - gap),
        "drawn at the saved width: {box_w:?}"
    );
    let layout = cx.update(|_, cx| shell.read(cx).window.workspace.layout);
    assert_eq!(layout.workbench_w, saved.workbench_w, "nothing read a drag");
}

/// Closing the Workbench with the caret in it hands the caret to the
/// conversation, so the window's keys still have somewhere to land.
#[gpui::test]
fn closing_the_workbench_gives_its_caret_back(cx: &mut TestAppContext) {
    let (_, shell, cx) =
        crate::shell::rig::shell(cx, crate::shell::rig::folder("slot-caret"), Vec::new());
    cx.simulate_resize(size(px(1600.), px(1000.)));
    let draw = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        for _ in 0..3 {
            cx.update(|window, _| window.refresh());
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    };
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.toggle_workbench(window, cx)));
    draw(cx);
    cx.update(|window, cx| {
        let handle = shell.read(cx).workbench.focus_handle(cx);
        handle.focus(window, cx);
    });
    draw(cx);
    let holds = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let shell = shell.read(cx);
            (
                shell
                    .workbench
                    .focus_handle(cx)
                    .contains_focused(window, cx),
                shell.chat.focus_handle(cx).contains_focused(window, cx),
            )
        })
    };
    assert_eq!(holds(cx), (true, false));
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.hide_workbench(window, cx)));
    draw(cx);
    assert_eq!(holds(cx), (false, true));
}
