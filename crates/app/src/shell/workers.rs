//! Where a session is in the workspace, and where a worker stands: under its
//! lead's project, whose row in the rail it is drawn in, while that lead is a
//! session of this window.

use super::Shell;
use super::confirm::{Answer, Confirm};

impl Shell {
    /// The project session `uid` stands under in the rail: its lead's for a
    /// worker whose lead is in this window, else its own. A worker's own is
    /// mostly a worktree kept for it alone, which nothing else belongs in.
    pub(crate) fn home_of(&self, uid: u64) -> Option<usize> {
        self.window
            .workspace
            .home_of(uid, crate::control::lead_of(uid))
    }
}

impl Shell {
    /// Ask before closing lead `uid`, which leads workers: close them with it,
    /// or the lead alone, leaving them open under their own projects. Each
    /// worker is named, with what closing it would stop.
    ///
    /// Asked whether or not anything is running: a lead closed by accident
    /// loses its hold on every worker, and a worker left behind holds its
    /// agent and a place under the cap with nobody to give it work.
    pub(super) fn ask_close_lead(
        &mut self,
        uid: u64,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let workers: Vec<u64> = crate::control::workers_of(uid)
            .into_iter()
            .map(|worker| worker.uid)
            .collect();
        let lines = workers
            .iter()
            .map(|&worker| {
                let name = self.session_label(worker, cx);
                match self.stops(worker, cx) {
                    Some(what) => format!("{name}: {what}"),
                    None => name,
                }
            })
            .collect();
        let label = self.session_label(uid, cx);
        let count = match workers.len() {
            1 => "a worker".to_string(),
            n => format!("{n} workers"),
        };
        let running = match self.stops(uid, cx) {
            Some(_) => " Its own turn is running, and is lost.",
            None => "",
        };
        let ask = Confirm {
            id: "close-lead",
            title: format!("Close {label} and its workers?").into(),
            description: format!(
                "It leads {count}. Closing all stops their agents; their conversations and \
                 worktrees stay. Closing the lead only leaves them open, each under its own \
                 project.{running}"
            )
            .into(),
            act: "Close all",
            alt: Some("Close the lead only"),
            lines,
            ..Default::default()
        };
        // By uid, not by place: the list can shift while the question is open.
        self.confirm_either(ask, window, cx, move |shell, answer, window, cx| {
            if answer == Answer::Act {
                // The workers first, while their lead is still there to be
                // shown in place of one on screen.
                for worker in crate::control::workers_of(uid) {
                    shell.end_session(worker.uid, window, cx);
                }
            }
            shell.end_session(uid, window, cx)
        });
    }

    /// What closing session `uid` would stop, when anything: a card waiting
    /// on the person, or a turn running.
    fn stops(&self, uid: u64, cx: &gpui::App) -> Option<&'static str> {
        let session = self.chat.read(cx).session_entity(uid)?;
        let chat = session.read(cx).chat();
        match (chat.awaiting_permission(), chat.busy) {
            (true, _) => Some("waiting on you"),
            (false, true) => Some("a turn is running"),
            (false, false) => None,
        }
    }
}
