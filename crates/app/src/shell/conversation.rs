//! The conversation an issue or a task names, opened again: where it is
//! live, gathered from every window, and what the workspace's drive makes of
//! it, carried out; its saved copy read off the UI loop.

use super::Shell;
use crate::state::Shared;
use gpui::{AnyWindowHandle, BorrowAppContext as _, Context, Window};
use onehand_core::workspace::drive::{Input, Live, Reach, Saved};

impl Shell {
    /// Put the conversation the agent named `session` on screen: its live
    /// session wherever it is, else the saved one, resumed on the project it
    /// ran in.
    pub(super) fn open_conversation(
        &mut self,
        session: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let live = self.live_conversation(session, cx);
        let reach = self.window.workspace.open_conversation(session, live);
        self.carry_reach(reach, window, cx);
    }

    /// Put task `id`'s session on screen wherever it is, or say it has none.
    pub(super) fn open_task_session(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let here = window.window_handle();
        let live = match crate::task::session_of(id, cx) {
            Some((uid, at)) if at == here => Live::Here(uid),
            Some((uid, at)) => Live::There(uid, at),
            None => Live::Nowhere,
        };
        let reach = self.window.workspace.show_task_session(live);
        self.carry_reach(reach, window, cx);
    }

    fn carry_reach(
        &mut self,
        reach: Reach<AnyWindowHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match reach {
            Reach::Here(fx) => self.carry_workspace(fx, window, cx),
            Reach::There(uid, at) => self.show_session_in(uid, at, window, cx),
        }
    }

    /// Where the session holding conversation `id` is live: this window
    /// first, then every other one.
    fn live_conversation(&self, id: &str, cx: &Context<Self>) -> Live<AnyWindowHandle> {
        if let Some(uid) = self.chat.read(cx).uid_of_conversation(id, cx) {
            return Live::Here(uid);
        }
        // This shell is being updated, so it is never read again here.
        let me = cx.entity_id();
        Shared::global(cx)
            .windows
            .iter()
            .filter_map(|w| Some((w.handle, w.shell.upgrade()?)))
            .filter(|(_, shell)| shell.entity_id() != me)
            .find_map(|(at, shell)| {
                let uid = shell.read(cx).chat.read(cx).uid_of_conversation(id, cx)?;
                Some(Live::There(uid, at))
            })
            .unwrap_or(Live::Nowhere)
    }

    /// Read the conversation the agent named `id` as it was saved, off the UI
    /// loop, and tell the drive what was found.
    pub(super) fn read_conversation(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |shell, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let (root, conv) = onehand_core::chat::find_conversation(
                        &onehand_core::chat::conversations_dir(),
                        &id,
                    )?;
                    Some(Saved {
                        there: root.is_dir(),
                        root,
                        agent: conv.agent,
                        resume: conv.dir,
                    })
                })
                .await;
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| {
                shell.conversation_read(found, window, cx)
            });
        })
        .detach();
    }

    /// Tell the drive what a conversation's read found, and carry what it
    /// makes of it.
    pub(super) fn conversation_read(
        &mut self,
        found: Option<Saved>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let agents = Shared::global(cx).settings.agents.clone();
        let fx = self
            .window
            .workspace
            .handle(Input::ConversationRead { found, agents, uid });
        self.carry_workspace(fx, window, cx);
    }
}
