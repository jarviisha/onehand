//! Ending a session on a host from the window: its agent stops there, where
//! *Close session* only stops watching it. Asked first, since a running turn
//! is lost; the conversation stays on the host, read from its project page.

use super::Shell;
use super::confirm::Confirm;
use gpui::{Context, Window};
use gpui_component::WindowExt as _;
use onehand_core::host::wire::HostSession;
use onehand_core::workspace::HostName;

impl Shell {
    /// *End on <host>…* on session `uid`: the host session it watches, once
    /// the host has said which; until then there is nothing there to end.
    pub(crate) fn ask_end_session(
        &mut self,
        uid: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = self.session_label(uid, cx);
        match self.chat.read(cx).host_session_of(uid) {
            Some((host, session)) => self.ask_end_on_host(host, session, label, window, cx),
            None => window.push_notification(
                crate::toast::info(format!("{label} has not started on its host yet")),
                cx,
            ),
        }
    }

    /// Ask, then end host session `session` on `host`, called `label`.
    pub(crate) fn ask_end_on_host(
        &mut self,
        host: HostName,
        session: HostSession,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = Confirm {
            id: "end-host-session",
            title: format!("End {label} on {host}?").into(),
            description: "Its agent stops there, and a running turn is lost. The conversation \
                          stays readable on the project page."
                .into(),
            act: "End",
            ..Default::default()
        };
        self.confirm(ask, window, cx, move |shell, window, cx| {
            shell.end_on_host(host.clone(), session.clone(), label.clone(), window, cx)
        });
    }

    /// End it, as the workspace's drive says: the session watching it here
    /// closes first.
    fn end_on_host(
        &mut self,
        host: HostName,
        session: HostSession,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let watcher = self.chat.read(cx).watching_on(&host, &session);
        let lead = watcher.and_then(crate::control::lead_of);
        let fx = self
            .window
            .workspace
            .end_on_host(host, session, label, watcher, lead);
        self.carry_workspace(fx, window, cx);
    }
}
