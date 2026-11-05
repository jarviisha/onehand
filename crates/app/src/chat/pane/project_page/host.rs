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
use onehand_core::host::copy::{self, copies_dir};
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
    /// The host could not be attached, for why (naming the host); what it
    /// last listed, as this machine kept it, is shown instead.
    Away { why: String, kept: Vec<SessionInfo> },
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
    /// The host the page on screen waits on, its busy sessions to finish.
    pub(crate) fn page_waits_on(&self) -> Option<&HostName> {
        let project = self.empty.as_ref()?;
        match project.listed {
            Some(Err(Unlisted::Waiting)) => project.host.as_ref(),
            Some(Ok(_) | Err(Unlisted::Failed(_) | Unlisted::Away { .. })) | None => None,
        }
    }

    /// The host the page on screen shows what it kept of, out of reach.
    pub(crate) fn page_away_on(&self) -> Option<&HostName> {
        let project = self.empty.as_ref()?;
        match project.listed {
            Some(Err(Unlisted::Away { .. })) => project.host.as_ref(),
            Some(Ok(_) | Err(Unlisted::Failed(_) | Unlisted::Waiting)) | None => None,
        }
    }

    /// The wait for `host`'s busy sessions ended, the host out of reach: a
    /// page waiting on it lists again, which shows what it kept of the host
    /// and waits for it to be back.
    pub(crate) fn host_page_failed(&mut self, host: &HostName, cx: &mut Context<Self>) {
        if self.page_waits_on() == Some(host) {
            self.list_host_sessions(cx);
        }
    }

    /// The wait for `host` to be back ended on `why`, another machine having
    /// taken it or the host refused: a page showing what it kept goes on
    /// showing it, with `why` under the line, which no longer says it tries
    /// again.
    pub(crate) fn host_page_unreached(
        &mut self,
        host: &HostName,
        why: &str,
        cx: &mut Context<Self>,
    ) {
        if self.page_away_on() != Some(host) {
            return;
        }
        if let Some(Some(Err(Unlisted::Away { why: said, .. }))) =
            self.empty.as_mut().map(|project| &mut project.listed)
        {
            *said = why.to_string();
            cx.notify();
        }
    }

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
        // The host's folder: an offloaded project's mirror.
        let path = project.cwd().to_path_buf();
        let attaching = crate::hosts::conn(cx, &host);
        cx.spawn(async move |pane, cx| {
            let (listed, bypass) = match attaching.await {
                Ok(conn) => {
                    let listed = crate::chat::session::host_list(conn.clone(), cx)
                        .await
                        .map_err(|e| {
                            HostFailure::Other(format!(
                                "Could not read the sessions on {host}: {e}"
                            ))
                        });
                    // ponytail: a switch that could not be read is not drawn,
                    // and the next listing reads it again.
                    let bypass = match &listed {
                        Ok(_) => cx
                            .update(|cx| {
                                crate::hosts::on_runtime(
                                    cx,
                                    async move { conn.bypass(&path).await },
                                )
                            })
                            .await
                            .ok(),
                        Err(_) => None,
                    };
                    (listed, bypass)
                }
                Err(failure) => (Err(failure), None),
            };
            // Each list read is kept, and the last one kept is what the page
            // shows while the host cannot be attached.
            let root = copies_dir();
            let kept = match &listed {
                Ok(all) => {
                    let (host, all) = (host.clone(), all.clone());
                    cx.background_executor()
                        .spawn(async move {
                            // ponytail: a list that could not be kept only
                            // leaves an older one to show while away.
                            let _ = copy::save_list(&root, &host, &all);
                        })
                        .detach();
                    Vec::new()
                }
                Err(HostFailure::Other(_) | HostFailure::Refused(_) | HostFailure::Taken(_)) => {
                    let host = host.clone();
                    cx.background_executor()
                        .spawn(async move { copy::load_list(&root, &host) })
                        .await
                }
                Err(HostFailure::Busy { .. }) => Vec::new(),
            };
            let _ = pane.update(cx, |pane: &mut Self, cx| {
                // Only the newest ask: an attach can take long enough for the
                // page to have moved to another project, or to have asked again.
                let Some(project) = pane.empty.as_mut().filter(|project| project.asked == asked)
                else {
                    return;
                };
                project.listing = false;
                project.bypass = bypass;
                project.listed = Some(match listed {
                    Ok(all) => Ok(all),
                    // The shell lists again once the host is back. A host
                    // still attached that could not list is a failure of
                    // its own, which waiting would only ask again at once.
                    Err(HostFailure::Other(why)) => match crate::hosts::state(cx, &host) {
                        crate::hosts::HostState::Lost(_) => {
                            cx.emit(ChatPaneEvent::HostLost {
                                uid: None,
                                host: host.clone(),
                            });
                            Err(Unlisted::Away { why, kept })
                        }
                        crate::hosts::HostState::Unknown
                        | crate::hosts::HostState::Attaching
                        | crate::hosts::HostState::Live
                        | crate::hosts::HostState::Taken => Err(Unlisted::Failed(why)),
                    },
                    // Refused or taken is never tried again by itself: what
                    // was kept is shown, and *Try again* is the person's.
                    Err(HostFailure::Refused(why) | HostFailure::Taken(why)) => {
                        Err(Unlisted::Away { why, kept })
                    }
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

/// The page's *Settings* on a host: whether the host answers the folder's
/// cards by itself, with what that costs said under it, always.
pub(super) fn bypass_box(host: &HostName, on: bool, cx: &mut Context<ChatPane>) -> AnyElement {
    let cost = format!(
        "While on, the host allows every permission an agent asks for, and tells it to decide \
         its own questions and say what it chose. An agent can then do anything the host's user \
         can on {host}, git push and deleting files included. Off, cards wait for you."
    );
    page_card("Settings", None, None, cx)
        .child(
            div()
                .v_flex()
                .gap_1()
                .child(
                    div().h_flex().child(
                        div().flex_none().cursor_pointer().child(
                            onehand_ui::toggle("host-bypass", on)
                                .label("Answer cards on the host")
                                .on_click(cx.listener(|pane, on: &bool, window, cx| {
                                    pane.set_host_bypass(*on, window, cx)
                                })),
                        ),
                    ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(cost),
                ),
        )
        .into_any_element()
}

impl ChatPane {
    /// Turn the host's answering of the page folder's cards on or off. Shown
    /// at once; the host's answer is what stays, and a failure puts the switch
    /// back and says why.
    fn set_host_bypass(&mut self, on: bool, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(project) = self.empty.as_mut() else {
            return;
        };
        let (Some(host), Some(was)) = (project.host.clone(), project.bypass) else {
            return;
        };
        project.bypass = Some(on);
        cx.notify();
        let (path, asked) = (project.cwd().to_path_buf(), project.asked);
        let attaching = crate::hosts::conn(cx, &host);
        cx.spawn_in(window, async move |pane, cx| {
            let set = match attaching.await {
                Ok(conn) => {
                    let at = path.clone();
                    cx.update(|_, cx| {
                        crate::hosts::on_runtime(cx, async move { conn.set_bypass(&at, on).await })
                    })
                    .map_err(|e| e.to_string())?
                    .await
                    .map_err(|e| e.to_string())
                }
                Err(
                    HostFailure::Other(why) | HostFailure::Refused(why) | HostFailure::Taken(why),
                ) => Err(why),
                Err(HostFailure::Busy { host, .. }) => {
                    Err(format!("{host} is waiting for its sessions to finish"))
                }
            };
            pane.update_in(cx, |pane, window, cx| {
                let Some(project) = pane
                    .empty
                    .as_mut()
                    .filter(|project| project.asked == asked && project.cwd() == path)
                else {
                    return;
                };
                project.bypass = Some(set.as_ref().copied().unwrap_or(was));
                if let Err(why) = set {
                    gpui_component::WindowExt::push_notification(
                        window,
                        onehand_ui::toast::failed(&format!("Changing the switch on {host}"), why),
                        cx,
                    );
                }
                cx.notify();
            })
            .map_err(|e| e.to_string())
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
    // What the host listed, or what this machine kept of it while away.
    let shown = match listed {
        Some(Ok(all) | Err(Unlisted::Away { kept: all, .. })) => Some(all),
        Some(Err(Unlisted::Failed(_) | Unlisted::Waiting)) | None => None,
    };
    let (rows, left_out) = shown.map_or((Vec::new(), 0), |all| {
        host_rows(all, cwd, HOST_SESSIONS_CAP)
    });
    // Away with nothing kept for this folder says so, rather than a count
    // of none.
    let away = matches!(listed, Some(Err(Unlisted::Away { .. })));
    let none_kept = away && rows.is_empty();
    let count = shown.filter(|_| !none_kept).map(|_| rows.len() + left_out);
    let now = onehand_core::chat::now_secs();
    let card = page_card(format!("Sessions on {host}"), count, None, cx);
    // How many sessions the host is waited on for: none once the wait has
    // ended and the list is asked again, which reads as attaching.
    let (waited, terms) = crate::hosts::busy(cx, host);
    let (waited, terms) = (waited.len(), terms);
    let attaching = match listed {
        None => true,
        Some(Err(Unlisted::Waiting)) => waited == 0 && terms == 0,
        Some(Ok(_) | Err(Unlisted::Failed(_) | Unlisted::Away { .. })) => false,
    };
    let card = match listed {
        _ if attaching => card.child(
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
            let retry = || {
                crate::controls::action("host-sessions-retry")
                    .secondary()
                    .short()
                    .label("Try again")
                    .loading(listing)
                    .on_click(cx.listener(|pane: &mut ChatPane, _, _, cx| {
                        pane.list_host_sessions(cx);
                    }))
            };
            // Out of reach is calm: what was kept is still there to open,
            // and why the attach failed stands under it in its own words.
            let (said, ink, answer, why) = match unlisted {
                Unlisted::Failed(why) => (
                    why.clone(),
                    crate::theme::status_ink(cx).danger,
                    retry(),
                    None,
                ),
                Unlisted::Away { why, .. } => (
                    crate::hosts::not_connected(host, crate::hosts::retrying(cx, host)),
                    muted,
                    retry(),
                    Some(why.clone()),
                ),
                Unlisted::Waiting => (
                    crate::hosts::waiting_line(host, waited, terms),
                    muted,
                    restart_now(host, cx),
                    None,
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
                            .child(onehand_ui::status_line(said, ink))
                            .children(why.map(|why| onehand_ui::status_line(why, muted))),
                    )
                    .child(answer),
            )
            .when(none_kept, |card| {
                card.child(line().child("No sessions kept on this machine for this folder"))
            })
        }
        Some(Ok(_)) if rows.is_empty() => {
            card.child(line().child(format!("No sessions on {host} for this folder yet")))
        }
        Some(Ok(_)) | None => card,
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
