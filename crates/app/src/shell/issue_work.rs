//! An issue worked by hand in the checkout its project is open on, and the
//! conversation an issue names opened again.

use super::Shell;
use crate::chat::session::ChatEvent;
use gpui::{Context, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use onehand_core::chat::Link;
use std::path::Path;

impl Shell {
    /// Start a session on project `root` with `prompt` as its first message,
    /// and say on issue `number` which session took it up.
    ///
    /// **No worktree, no branch, no run.** This is the checkout the person is
    /// working in, so nothing here claims the issue, times the session out or
    /// takes the project away afterwards: it is an ordinary session that was
    /// handed its first message. The prompt goes once the agent is up, since
    /// a session refuses one before; it is not sent at all if somebody typed
    /// into the session first, which makes it theirs. The issue hears about it
    /// once the agent has named the conversation, which is what lets the issue
    /// open it again after a restart.
    pub(super) fn work_issue_here(
        &mut self,
        root: &Path,
        number: u64,
        prompt: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(idx), Some(file)) = (self.root_index(root), self.issues_file(root)) else {
            return;
        };
        self.select_root(idx, window, cx);
        let Some(uid) = self.start_session(None, None, window, cx) else {
            return;
        };
        let Some(session) = self.chat.read(cx).session_entity(uid) else {
            window.push_notification(
                Notification::warning("The session for this issue did not start"),
                cx,
            );
            return;
        };
        let (prompt, workbench) = (prompt.to_string(), self.workbench.downgrade());
        let (mut sent, mut told) = (false, false);
        cx.subscribe(&session, move |_, session, _: &ChatEvent, cx| {
            let chat = &session.read(cx).chat;
            if !sent && chat.link == Link::Connected {
                sent = true;
                if !chat.busy && chat.prompts_sent == 0 && chat.queued.is_none() {
                    // The mode chosen for an issue's sessions, when the agent
                    // offers it; otherwise the session keeps the one it opened
                    // in, since a person is at the window to answer it.
                    let mode = crate::unattended::mode(cx);
                    session.update(cx, |session, cx| {
                        if session.chat.modes.iter().any(|m| m.id == mode) {
                            session.chat.set_mode(&mode);
                        }
                        session.submit(&prompt, &[], cx)
                    });
                }
            }
            let Some(id) = session.read(cx).chat.session_id.clone().filter(|_| !told) else {
                return;
            };
            told = true;
            let (file, workbench) = (file.clone(), workbench.clone());
            cx.spawn(async move |_, cx| {
                let done = cx
                    .background_executor()
                    .spawn(async move {
                        onehand_core::issues::taken_up_blocking(
                            &file,
                            number,
                            "Taken up by a session on this checkout",
                            id,
                        )
                    })
                    .await;
                if let Err(why) = done {
                    eprintln!("onehand: could not note the session on issue: {why}");
                }
                let _ = workbench.update(cx, |panel, cx| panel.rescan(cx));
            })
            .detach();
        })
        .detach();
    }

    /// Put the conversation the agent named `session` on screen: the live
    /// session in this window that holds it, else the saved one reopened on
    /// the project it ran in.
    ///
    /// **That project may have left the workspace**: an unattended run's
    /// worktree is dropped from it when the run ends, while the folder stays
    /// on disk. Asked to open what was said there, the folder is added back —
    /// a person pressing for that conversation is asking for its project too —
    /// and only a folder that is gone is a refusal, said with its path.
    pub(super) fn open_conversation(
        &mut self,
        session: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(uid) = self.chat.read(cx).uid_of_conversation(session, cx) {
            self.show_session(uid, window, cx);
            return;
        }
        let session = session.to_string();
        cx.spawn_in(window, async move |shell, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let found = onehand_core::chat::find_conversation(
                        &onehand_core::chat::conversations_dir(),
                        &session,
                    )?;
                    let there = found.0.is_dir();
                    Some((found, there))
                })
                .await;
            let _ = shell.update_in(cx, |shell, window, cx| {
                let Some(((root, conv), there)) = found else {
                    window.push_notification(
                        Notification::warning("That session's conversation was not saved"),
                        cx,
                    );
                    return;
                };
                let idx = match shell.root_index(&root) {
                    Some(idx) => idx,
                    None if there => {
                        let idx = shell.window.workspace.add_root(root.clone());
                        shell.refresh_git(cx);
                        shell.save_workspace(window, cx);
                        idx
                    }
                    None => {
                        window.push_notification(
                            Notification::warning(format!(
                                "The folder that conversation ran in is gone: {}",
                                root.display()
                            )),
                            cx,
                        );
                        return;
                    }
                };
                shell.select_root(idx, window, cx);
                let _ = shell.start_session(
                    Some(SharedString::from(conv.agent)),
                    Some(conv.dir),
                    window,
                    cx,
                );
            });
        })
        .detach();
    }
}
