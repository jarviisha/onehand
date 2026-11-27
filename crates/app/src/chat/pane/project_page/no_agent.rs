//! What the pane shows in a project page's place while no agent is set up.

use super::super::{ChatPane, ChatPaneEvent};
use crate::controls::Short as _;
use gpui::{Context, IntoElement, ParentElement, Styled, div};
use gpui_component::button::ButtonVariants as _;
use gpui_component::{ActiveTheme, StyledExt};

/// What the pane shows while no agent is set up: the line saying so, each
/// built-in agent offered when it is on this machine and its install command
/// when it is not, and the way to the agent settings, where any other agent
/// is declared.
///
/// **More than a hint's one line, on purpose.** Every action in the window
/// waits on an agent, so the state names what fixes it rather than only that
/// something is missing; the commands stand in the code face, as a command
/// line does everywhere else.
pub(super) fn no_agent(cx: &mut Context<ChatPane>) -> gpui::AnyElement {
    let muted = cx.theme().muted_foreground;
    let mono = cx.theme().mono_font_family.clone();
    let on_machine = crate::state::Shared::global(cx).on_machine.clone();
    // An agent already installed is offered rather than explained: the list can
    // be empty with one on the machine (it was emptied, or written while
    // nothing was installed), and telling someone to install what they have
    // is a way out that leads back here.
    let install = onehand_core::config::Preset::ALL.map(|preset| {
        if on_machine.contains(&preset) {
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(format!("{} is on this machine", preset.info().shown_name))
                .child(
                    crate::controls::action(("no-agent-add", preset as usize))
                        .secondary()
                        .short()
                        .label("Add")
                        .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                            cx.emit(ChatPaneEvent::AddFoundAgent(preset));
                        })),
                )
                .into_any_element()
        } else {
            div()
                .font_family(mono.clone())
                .child(format!(
                    "{}: {}",
                    preset.info().shown_name,
                    preset.info().install
                ))
                .into_any_element()
        }
    });
    div()
        .flex_1()
        .v_flex()
        .items_center()
        .justify_center()
        .gap_2()
        .text_xs()
        .text_color(muted)
        .child("No agent is set up")
        .children(install)
        .child("Then restart onehand, or add another agent in the settings")
        .child(
            crate::controls::action("no-agent-settings")
                .secondary()
                .short()
                .label("Open the agent settings")
                .on_click(cx.listener(|_: &mut ChatPane, _, _, cx| {
                    cx.emit(ChatPaneEvent::OpenAgentSettings);
                })),
        )
        .into_any_element()
}
