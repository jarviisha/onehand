//! The project page of a folder on a host: the sessions the host keeps
//! there, each attached to from its row.
//!
//! A host keeps its sessions itself, so the list is asked of it rather than
//! read from this machine's store, over the one attach the host shares with
//! every session on it.

use super::super::{ChatPane, ChatPaneEvent};
use crate::chat::conversation::OnHost;
use crate::controls::{Named as _, Quiet as _, Short as _};
use crate::hosts::HostFailure;
use crate::page_parts::{page_card, ruled};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Div, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::spinner::Spinner;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _};
use onehand_core::host::wire::{HostSession, SessionInfo};
use onehand_core::workspace::HostName;
use onehand_ui::TallIcon as _;
use std::path::Path;

/// How many of a host's sessions in one folder the page lists.
///
/// A host keeps every session it has run, ended ones too, and the page is an
/// entrance rather than an archive: the newest are what anyone comes for.
/// What the cap cut off is said on screen.
const HOST_SESSIONS_CAP: usize = 50;

/// What the host said about its sessions in the page's folder: `None` until
/// the first answer lands, then the list or why there is none.
pub(in crate::chat::pane) type Listed = Option<Result<Vec<SessionInfo>, Unlisted>>;

/// Why the host listed nothing.
pub(in crate::chat::pane) enum Unlisted {
    /// What went wrong, naming the host.
    Failed(String),
    /// The host runs another build with sessions mid-turn, and is waited on.
    Waiting,
}

/// Where the page's folder is, as its head says it: `<host>:<path>` for one
/// on a host, whose path this machine's home says nothing about; else the
/// path, shortened to `~/` under `home`.
pub(super) fn place(host: Option<&HostName>, path: &Path, home: Option<&Path>) -> String {
    match host {
        Some(_) => onehand_core::workspace::host_path(host, path),
        None => home
            .and_then(|home| path.strip_prefix(home).ok())
            .map_or_else(
                || path.display().to_string(),
                |rest| format!("~/{}", rest.display()),
            ),
    }
}

/// The sessions of `list` run in `cwd`, newest first, at most `cap` of them,
/// and how many more there are.
pub(super) fn host_rows<'a>(
    list: &'a [SessionInfo],
    cwd: &Path,
    cap: usize,
) -> (Vec<&'a SessionInfo>, usize) {
    let mut here: Vec<&SessionInfo> = list.iter().filter(|info| info.cwd == cwd).collect();
    here.sort_by_key(|info| std::cmp::Reverse(info.session.started()));
    let left_out = here.len().saturating_sub(cap);
    here.truncate(cap);
    (here, left_out)
}

/// The session among `convs` (each by uid, folder and host) already on host
/// session `id` of `host`, in the folder `root`.
fn watching_uid<'a>(
    convs: impl IntoIterator<Item = (u64, &'a Path, Option<&'a OnHost>)>,
    root: &Path,
    host: &HostName,
    id: &HostSession,
) -> Option<u64> {
    convs.into_iter().find_map(|(uid, folder, on)| {
        let on = on?;
        (folder == root && &on.host == host && on.session.as_ref() == Some(id)).then_some(uid)
    })
}

impl ChatPane {
    /// The session in `root` on `host` already attached to host session `id`.
    pub fn watching(&self, root: &Path, host: &HostName, id: &HostSession) -> Option<u64> {
        let convs = self
            .conversations
            .iter()
            .map(|(uid, conv)| (*uid, conv.root.as_path(), conv.host.as_ref()));
        watching_uid(convs, root, host, id)
    }

    /// Ask the host the page stands on for its sessions, off the UI loop.
    /// What was listed stays on screen until the answer lands.
    pub fn list_host_sessions(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.empty.as_mut() else {
            return;
        };
        let Some(host) = project.host.clone() else {
            return;
        };
        // From the pane, not the page: a page made again for the same folder
        // must not take an answer meant for the one before it.
        self.host_asks += 1;
        let asked = self.host_asks;
        project.asked = asked;
        project.listing = true;
        cx.notify();
        let attaching = crate::hosts::conn(cx, &host);
        cx.spawn(async move |pane, cx| {
            let listed = match attaching.await {
                Ok(conn) => crate::chat::session::host_list(conn, cx)
                    .await
                    .map_err(|e| {
                        HostFailure::Other(format!("Could not read the sessions on {host}: {e}"))
                    }),
                Err(failure) => Err(failure),
            };
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // Only the newest ask: an attach can take long enough for the
                // page to have moved to another project, or to have asked again.
                let Some(project) = pane.empty.as_mut().filter(|project| project.asked == asked)
                else {
                    return;
                };
                project.listing = false;
                project.listed = Some(match listed {
                    Ok(all) => Ok(all),
                    Err(HostFailure::Other(why)) => Err(Unlisted::Failed(why)),
                    // The shell waits with the host and lists again once
                    // it is done.
                    Err(HostFailure::Busy { host, .. }) => {
                        cx.emit(ChatPaneEvent::HostBusy { uid: None, host });
                        Err(Unlisted::Waiting)
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }
}

/// *Refresh*, for the page's head: ask the host again.
pub(super) fn refresh(host: &HostName, listing: bool, cx: &mut Context<ChatPane>) -> AnyElement {
    crate::controls::action("host-sessions-refresh")
        .tall_icon()
        .quiet_icon(false, cx)
        .icon(Icon::new(crate::icons::Icon::RefreshCw))
        .loading(listing)
        .named(format!("Refresh the sessions on {host}"))
        .on_click(cx.listener(|pane: &mut ChatPane, _, _, cx| {
            pane.list_host_sessions(cx);
        }))
        .into_any_element()
}

/// *Restart now*, for what waits on `host`'s busy sessions: the shell asks
/// before the service is restarted.
fn restart_now(host: &HostName, cx: &mut Context<ChatPane>) -> gpui_component::button::Button {
    let host = host.clone();
    crate::controls::action("host-sessions-restart")
        .secondary()
        .short()
        .label("Restart now")
        .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
            cx.emit(ChatPaneEvent::RestartHost(host.clone()));
        }))
}

/// The box of the host's sessions in `cwd`, each row attaching to its
/// session.
pub(super) fn host_sessions(
    host: &HostName,
    cwd: &Path,
    listed: &Listed,
    listing: bool,
    cx: &mut Context<ChatPane>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let line = || div().text_xs().text_color(muted);
    let (rows, left_out) = match listed {
        Some(Ok(all)) => host_rows(all, cwd, HOST_SESSIONS_CAP),
        Some(Err(_)) | None => (Vec::new(), 0),
    };
    let count = listed
        .as_ref()
        .and_then(|listed| listed.as_ref().ok())
        .map(|_| rows.len() + left_out);
    let now = onehand_core::chat::now_secs();
    let card = page_card(format!("Sessions on {host}"), count, None, cx);
    let card = match listed {
        None => card.child(
            line()
                .h_flex()
                .items_center()
                .gap_2()
                .child(Spinner::new().xsmall().color(muted))
                .child(format!("Attaching to {host}…")),
        ),
        // A condition standing under the box's title, and what answers it
        // beside it.
        Some(Err(unlisted)) => {
            let (said, ink, answer) = match unlisted {
                Unlisted::Failed(why) => (
                    why.clone(),
                    crate::theme::status_ink(cx).danger,
                    crate::controls::action("host-sessions-retry")
                        .secondary()
                        .short()
                        .label("Try again")
                        .loading(listing)
                        .on_click(cx.listener(|pane: &mut ChatPane, _, _, cx| {
                            pane.list_host_sessions(cx);
                        })),
                ),
                Unlisted::Waiting => (
                    crate::hosts::waiting_line(host, crate::hosts::busy(cx, host).len()),
                    muted,
                    restart_now(host, cx),
                ),
            };
            card.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(onehand_ui::status_line(said, ink)),
                    )
                    .child(answer),
            )
        }
        Some(Ok(_)) if rows.is_empty() => {
            card.child(line().child(format!("No sessions on {host} for this folder yet")))
        }
        Some(Ok(_)) => card,
    };
    let running = cx.theme().info;
    card.child(ruled(
        rows.into_iter().enumerate().map(|(i, info)| {
            let agent = SharedString::from(info.agent.clone());
            let session = info.session.clone();
            let since = onehand_core::rel_time(now, info.session.started().0);
            let (state, ink) = match info.busy {
                true => ("running", running),
                false if info.ended => ("ended", muted),
                // Neither mid-turn nor over: an agent waiting for its next
                // prompt is still running there.
                false => ("running", muted),
            };
            onehand_ui::row(false, cx)
                .id(("host-session", i))
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .rounded_none()
                .child(div().flex_1().min_w_0().truncate().child(agent.clone()))
                .child(
                    div()
                        .flex_none()
                        .h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("started {since}"))
                        .child("·")
                        .child(div().text_color(ink).child(state)),
                )
                .tooltip(onehand_ui::tip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new("Attach to this session")
                        .build(window, cx)
                }))
                .on_click(cx.listener(move |_: &mut ChatPane, _, _, cx| {
                    cx.emit(ChatPaneEvent::WatchHostSession {
                        agent: agent.clone(),
                        session: session.clone(),
                    });
                }))
                .into_any_element()
        }),
        cx,
    ))
    .when(left_out > 0, |card| {
        card.child(line().child(format!("{left_out} more on this host")))
    })
}

#[cfg(test)]
mod tests;
