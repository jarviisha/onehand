//! The Workbench's strip: the way back while it has the area, the modes, then
//! maximize and hide fixed at the end, outside anything that gives way.

use super::{Workbench, WorkbenchEvent};
use gpui::prelude::FluentBuilder as _;
use gpui::{App, Context, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme, Icon, IconName, Selectable as _, Sizable as _, StyledExt};
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};

/// The width the Workbench opens at, in rems. Narrower, the modes cannot each
/// keep their name beside the controls at the strip's end, so they fold into
/// one control naming the showing mode and opening the others, rather than
/// letting clipping decide which survive.
const DOCK_PREF: f32 = 30.;

impl Workbench {
    pub(super) fn strip(
        &self,
        specs: Vec<WorkbenchModeSpec>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let showing = self.showing();
        let full = self.maximized;
        let compact = self.width.get() < DOCK_PREF;
        div()
            .h_flex()
            .flex_none()
            .items_center()
            // The height of the conversation's header beside it, so the two
            // bars line up across the seam.
            .h(crate::controls::BAR_H)
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            // The way back to the conversation, while this panel has taken the
            // area because the window cannot hold the two side by side. A
            // maximized panel has its own way back at the other end.
            .when(self.focused_area && !full, |strip| {
                strip
                    .child(
                        crate::controls::action("step-aside")
                            .ghost()
                            .small()
                            .flex_none()
                            .icon(Icon::new(IconName::ArrowLeft))
                            .label("Conversation")
                            .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                                cx.emit(WorkbenchEvent::StepAside);
                            })),
                    )
                    .child(div().flex_none().w_px().h_3p5().bg(cx.theme().border))
            })
            // The modes are the one part of the row that gives way, so the
            // controls at its end are on screen at any width.
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .map(|modes| match compact {
                        true => modes.child(mode_select(specs, showing.label, showing.id, cx)),
                        false => modes.children(
                            specs
                                .into_iter()
                                .map(|spec| mode_tab(spec.label, spec.id, showing.id, cx)),
                        ),
                    }),
            )
            // Muted, like the terminal strip's pair and the conversation
            // header's controls: the library draws a ghost button in full
            // foreground ink, which on a chrome row is the brightest thing in
            // the panel. The hover fill brings the ink back on the one about to
            // be pressed.
            .child(
                crate::controls::action("maximize-workbench")
                    .ghost()
                    .small()
                    .flex_none()
                    .text_color(cx.theme().muted_foreground)
                    .icon(Icon::new(match full {
                        true => IconName::Minimize,
                        false => IconName::Maximize,
                    }))
                    .tooltip(match full {
                        true => "Back to the dock",
                        false => "Fill the window",
                    })
                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                        cx.emit(WorkbenchEvent::ToggleMaximize);
                    })),
            )
            // A minus, the mark a window's own chrome uses for the thing that
            // goes away and comes back, and the same one the terminal's strip
            // carries: nothing here is being closed, the dock is being put down.
            //
            // `Minus` and never `Dash`, which is the same drawing under another
            // name with `stroke` written into it as literal black -- so it
            // ignores `text_color` and comes out invisible on a dark panel.
            .child(
                crate::controls::action("hide-workbench")
                    .ghost()
                    .small()
                    .flex_none()
                    .text_color(cx.theme().muted_foreground)
                    .icon(Icon::new(IconName::Minus))
                    .tooltip("Hide the Workbench")
                    .on_click(cx.listener(|_: &mut Self, _, _, cx| {
                        cx.emit(WorkbenchEvent::Hide);
                    })),
            )
    }
}

/// One mode on the strip: a flat tab, the showing one on the selected fill and
/// the rest taking the hover fill under the pointer, at most a tab's width and
/// truncating.
fn mode_tab(
    label: &'static str,
    which: PluginId,
    active: PluginId,
    cx: &mut Context<Workbench>,
) -> Button {
    crate::controls::action(which.as_str())
        .ghost()
        .small()
        .max_w(onehand_plugin_host::TAB_MAX_W)
        .selected(which == active)
        .child(div().min_w_0().truncate().child(label))
        .on_click(cx.listener(move |panel: &mut Workbench, _, _, cx| {
            panel.set_mode(which, cx);
        }))
}

/// The modes as one control naming the showing one and opening the others,
/// for a strip too narrow to hold them side by side.
fn mode_select(
    specs: Vec<WorkbenchModeSpec>,
    label: &'static str,
    showing: PluginId,
    cx: &mut Context<Workbench>,
) -> impl IntoElement + use<> {
    let panel = cx.entity().downgrade();
    crate::controls::menu_below(
        "workbench-mode-menu",
        onehand_plugin_host::tab_select("workbench-mode-select", label.into(), label.into(), cx)
            .small(),
        move |mut menu, _, _| {
            for spec in &specs {
                let (panel, which) = (panel.clone(), spec.id);
                menu = menu.item(
                    crate::controls::menu_item(spec.label)
                        .checked(which == showing)
                        .on_click(move |_, _: &mut Window, cx: &mut App| {
                            panel.update(cx, |panel, cx| panel.set_mode(which, cx)).ok();
                        }),
                );
            }
            menu
        },
    )
}
