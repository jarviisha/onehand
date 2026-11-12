//! The conversation an issue or a task names, opened again.

use super::Shell;
use gpui::{Context, SharedString, Window};
use gpui_component::WindowExt as _;

impl Shell {
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
                        crate::toast::warning("That session's conversation was not saved"),
                        cx,
                    );
                    return;
                };
                let idx = match shell.root_index(&root) {
                    Some(idx) => idx,
                    None if there => {
                        let (idx, fx) = shell.window.workspace.keep_project(root.clone());
                        shell.carry_workspace(fx, window, cx);
                        idx
                    }
                    None => {
                        window.push_notification(
                            crate::toast::warning(format!(
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
