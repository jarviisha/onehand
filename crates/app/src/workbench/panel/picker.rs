//! The two ends of the Workbench's one row, each a view of its own so the bar
//! that draws them can be the panel's or the showing mode's: the way back to
//! the conversation and the mode picker at its start, maximize and hide at its
//! end.
//!
//! Each reads the panel it belongs to as it draws, so neither holds a copy of
//! a fact the panel owns, and redraws whenever the panel does.

use super::{Workbench, WorkbenchEvent};
use crate::controls::Named as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, WeakEntity,
    Window, div,
};
use gpui_component::button::Button;
use gpui_component::{IconName, StyledExt};
use onehand_workbench::{BarEnds, WorkbenchModeSpec};

/// Both ends, built for `panel`.
pub(super) fn bar_ends(panel: &Entity<Workbench>, cx: &mut App) -> BarEnds {
    let lead = cx.new(|cx| ModePicker::new(panel, cx));
    let end = cx.new(|cx| StripEnd::new(panel, cx));
    let weak = panel.downgrade();
    BarEnds {
        lead: lead.into(),
        end: end.into(),
        chrome: std::rc::Rc::new(move |bar, cx| match weak.upgrade() {
            Some(panel) => panel.read(cx).zoom.unscale(bar).into_any_element(),
            None => bar,
        }),
    }
}

/// The row's start: *← Conversation* while the Workbench has the area, then
/// the open mode's name over a menu of the modes offered. One control at every
/// width: a picker is already as narrow as one mode's name.
pub(super) struct ModePicker {
    panel: WeakEntity<Workbench>,
    _redraw: gpui::Subscription,
}

impl ModePicker {
    fn new(panel: &Entity<Workbench>, cx: &mut Context<Self>) -> Self {
        Self {
            panel: panel.downgrade(),
            _redraw: cx.observe(panel, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for ModePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = div().h_flex().items_center().gap_1().px_1();
        let Some(panel) = self.panel.upgrade() else {
            return row;
        };
        let (back, picker) = {
            let panel = panel.read(cx);
            // A maximized panel has its own way back at the other end.
            let back = panel.fills_area && !panel.maximized;
            // While the panel refuses there is no mode to switch to.
            let offered: Vec<WorkbenchModeSpec> = match panel.refused {
                Some(_) => Vec::new(),
                None => panel
                    .modes
                    .iter()
                    .map(|mode| mode.spec())
                    .filter(|spec| panel.offers(*spec))
                    .collect(),
            };
            (
                back,
                (!offered.is_empty()).then(|| (offered, panel.showing())),
            )
        };
        row.when(back, |row| {
            row.child(
                onehand_ui::back_link("step-aside", "Conversation", cx).on_click(cx.listener(
                    |picker: &mut Self, _, _, cx| {
                        picker
                            .panel
                            .update(cx, |_, cx| cx.emit(WorkbenchEvent::StepAside))
                            .ok();
                    },
                )),
            )
        })
        .when_some(picker, |row, (offered, showing)| {
            row.child(mode_menu(self.panel.clone(), offered, showing, cx))
        })
    }
}

/// The open mode's name and a chevron, opening the modes offered.
fn mode_menu(
    panel: WeakEntity<Workbench>,
    offered: Vec<WorkbenchModeSpec>,
    showing: WorkbenchModeSpec,
    cx: &App,
) -> impl IntoElement + use<> {
    crate::controls::menu_below(
        "workbench-mode-menu",
        onehand_ui::tab_select(
            "workbench-mode-select",
            showing.label.into(),
            "Switch the Workbench's mode".into(),
            cx,
        ),
        move |mut menu, _, _| {
            for spec in &offered {
                let (panel, which) = (panel.clone(), spec.id);
                menu = menu.item(
                    crate::controls::menu_item(spec.label)
                        .checked(which == showing.id)
                        .on_click(move |_, _: &mut Window, cx: &mut App| {
                            panel.update(cx, |panel, cx| panel.set_mode(which, cx)).ok();
                        }),
                );
            }
            menu
        },
    )
}

/// The row's end: maximize and hide, outside anything that gives way.
pub(super) struct StripEnd {
    panel: WeakEntity<Workbench>,
    _redraw: gpui::Subscription,
}

impl StripEnd {
    fn new(panel: &Entity<Workbench>, cx: &mut Context<Self>) -> Self {
        Self {
            panel: panel.downgrade(),
            _redraw: cx.observe(panel, |_, _, cx| cx.notify()),
        }
    }

    fn tell(&self, event: WorkbenchEvent, cx: &mut App) {
        self.panel.update(cx, |_, cx| cx.emit(event)).ok();
    }
}

impl Render for StripEnd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let full = self
            .panel
            .upgrade()
            .is_some_and(|panel| panel.read(cx).maximized);
        div()
            .h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .child(maximize_control(full, cx))
            .child(hide_control(cx))
    }
}

/// Fill the window, or go back to the dock while it fills it. The glyph draws
/// the state and not the action: the situation that matters is the panel
/// filling the window and the user looking for the way back.
fn maximize_control(full: bool, cx: &mut Context<StripEnd>) -> Button {
    onehand_ui::strip_control(
        "maximize-workbench",
        match full {
            true => IconName::Minimize,
            false => IconName::Maximize,
        },
        false,
        cx,
    )
    .named_with_action(
        match full {
            true => "Back to the dock",
            false => "Fill the window",
        },
        &crate::shell::ToggleMaximize,
        None,
    )
    .on_click(cx.listener(|end: &mut StripEnd, _, _, cx| {
        end.tell(WorkbenchEvent::ToggleMaximize, cx);
    }))
}

/// Hide the Workbench: a minus, the mark a window's own chrome uses for the
/// thing that goes away and comes back, and the same one the terminal's strip
/// carries: nothing here is being closed, the dock is being put down.
///
/// `Minus` and never `Dash`, which is the same drawing under another name
/// with `stroke` written into it as literal black -- so it ignores
/// `text_color` and comes out invisible on a dark panel.
fn hide_control(cx: &mut Context<StripEnd>) -> Button {
    onehand_ui::strip_control("hide-workbench", IconName::Minus, false, cx)
        .named_with_action(
            "Hide the Workbench",
            &crate::shell::ToggleWorkbenchVisibility,
            None,
        )
        .on_click(cx.listener(|end: &mut StripEnd, _, _, cx| {
            end.tell(WorkbenchEvent::Hide, cx);
        }))
}
