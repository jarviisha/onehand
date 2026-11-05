//! The project page's box on an offloaded project: which side holds its code
//! and when the code last moved.

use super::super::ChatPane;
use crate::offload::{Moving, Offload};
use crate::page_parts::page_card;
use gpui::{App, Div, ParentElement as _, Styled as _, div};
use gpui_component::{ActiveTheme as _, StyledExt as _};
use onehand_core::host::wire::Holder;
use std::path::Path;

/// What the box says first: where the code is, or where it is going.
fn holder_line(o: &Offload) -> String {
    let host = &o.host;
    match (o.moving, o.state.as_ref()) {
        (Some(Moving::Up), _) => format!("Sending the code to {host}…"),
        (Some(Moving::Back), _) => format!("Bringing the code back from {host}…"),
        (None, None) => "Reading what was kept…".into(),
        (None, Some(state)) => match (state.key, &state.stopped) {
            (Holder::Laptop, _) => {
                format!("The code is here; it goes to {host} with the next prompt")
            }
            (Holder::Host, Some(changes)) => {
                let n = changes.files.len() + changes.refs.len();
                format!(
                    "Bringing the code back stopped: {n} {} changed here while {host} held \
                     the code, and {host} keeps it meanwhile",
                    if n == 1 { "thing" } else { "things" }
                )
            }
            (Holder::Host, None) => crate::offload::lock_line(host),
        },
    }
}

/// The box *On <host>*, for the offloaded project at `path`.
pub(super) fn offload_box(path: &Path, cx: &mut gpui::Context<ChatPane>) -> Option<Div> {
    let o = crate::offload::get(cx, path)?;
    let muted = cx.theme().muted_foreground;
    let line = |text: String, cx: &App| {
        div()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(text)
    };
    let stopped = o.state.as_ref().is_some_and(|s| s.stopped.is_some());
    let ink = match stopped {
        true => crate::theme::status_ink(cx).warning,
        false => cx.theme().foreground,
    };
    let moved = o.state.as_ref().map(|s| s.moved).filter(|at| *at > 0);
    let now = onehand_core::chat::now_secs();
    let failed = o.failed.clone();
    let danger = crate::theme::status_ink(cx).danger;
    Some(
        page_card(format!("On {}", o.host), None, None, cx).child(
            div()
                .v_flex()
                .gap_1()
                .child(line(holder_line(o), cx).text_color(ink))
                .children(moved.map(|at| {
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("Last moved {}", onehand_core::rel_time(now, at)))
                }))
                .children(failed.map(|why| div().text_xs().text_color(danger).child(why))),
        ),
    )
}
