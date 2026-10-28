//! Forking a conversation from one of its prompts: the seed read off disk, the
//! prompt staged, and the hand-off to the shell, which mints the session.

use super::{ChatPane, ChatPaneEvent};
use crate::chat::composer::Draft;
use gpui::Context;
use onehand_core::chat::{ForkSeed, TranscriptItemId, conversations_dir, fork_seed};

/// A fork read and ready, waiting for the session the shell mints for it.
pub(crate) struct StagedFork {
    /// The session it is for, once the shell has minted one.
    uid: Option<u64>,
    pub(super) from: String,
    pub(super) message: String,
    pub(super) seed: ForkSeed,
    pub(super) draft: Draft,
}

impl ChatPane {
    /// Fork session `uid` from its prompt at `target`: read the original's
    /// saved transcript off the UI loop, cut it, and ask the shell for a
    /// session on the same project and agent.
    pub(super) fn begin_fork(
        &mut self,
        uid: u64,
        target: TranscriptItemId,
        cx: &mut Context<Self>,
    ) {
        let Some(conv) = self.conversations.get(&uid) else {
            return;
        };
        let Some(point) = conv
            .session()
            .and_then(|session| session.read(cx).chat.fork_point(target))
        else {
            return;
        };
        let (root, spec) = (conv.root.clone(), conv.spec.clone());
        cx.spawn(async move |pane, cx| {
            let staged = cx
                .background_executor()
                .spawn(async move {
                    let seed = fork_seed(&conversations_dir(), &point)?;
                    Some(StagedFork {
                        uid: None,
                        draft: Draft::staged(point.text, &point.attachments),
                        from: point.from,
                        message: point.message,
                        seed,
                    })
                })
                .await;
            let _ = pane.update(cx, |pane, cx| match staged {
                Some(staged) => {
                    pane.pending_fork = Some(staged);
                    cx.emit(ChatPaneEvent::Fork { root, spec });
                }
                None => cx.emit(ChatPaneEvent::ForkFailed {
                    uid: None,
                    why: "the conversation is not saved up to that prompt".into(),
                }),
            });
        })
        .detach();
    }

    /// The fork read last is for session `uid`, which the shell is about to
    /// show.
    pub fn fork_next(&mut self, uid: u64) {
        if let Some(staged) = &mut self.pending_fork {
            staged.uid = Some(uid);
        }
    }

    /// The fork waiting for session `uid`, taken.
    pub(super) fn take_fork(&mut self, uid: u64) -> Option<StagedFork> {
        self.pending_fork.take_if(|staged| staged.uid == Some(uid))
    }
}
