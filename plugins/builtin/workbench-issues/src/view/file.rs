//! Writing a project's issues file: every change goes through one
//! read-change-write off the UI loop, and the file shared by every view of
//! it takes what was written and syncs it.

use super::IssuesView;
use gpui::Context;
use onehand_core::issues::{self, Issues, sync};

impl IssuesView {
    /// Keep the active project in step with its forge, if it is kept in step
    /// with one. `now` for a sync something asked for.
    pub(super) fn sync(&mut self, now: bool, cx: &mut Context<Self>) {
        if let Some(file) = self.state_mut().map(|state| state.file.clone()) {
            file.update(cx, |file, cx| file.sync(now, cx));
        }
    }

    /// Start or stop keeping the active project in step with its forge.
    pub(super) fn set_syncing(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(forge) = self.forge(cx) else {
            return;
        };
        self.change(
            move |kept| {
                kept.sync_with(on.then(|| forge.name().to_string()));
                Ok(None)
            },
            cx,
        );
    }

    /// Send issue `number` to the forge, which is the only way anything written
    /// here reaches it.
    pub(super) fn publish(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some((root, path)) = self.file() else {
            return;
        };
        let Some(forge) = self.forge(cx) else {
            return;
        };
        let Some(file) = self.state_mut().map(|state| state.file.clone()) else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { sync::publish_blocking(&path, &root, forge, number) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                view.status = done.err();
                file.update(cx, |file, cx| file.load(cx));
            });
        })
        .detach();
    }

    /// Settle issue `number`'s conflict one way or the other, then sync so the
    /// decision reaches the forge.
    pub(super) fn resolve(&mut self, number: u64, keep_mine: bool, cx: &mut Context<Self>) {
        let now = issues::now();
        self.change(
            move |kept| kept.resolve(number, keep_mine, now).map(|()| Some(number)),
            cx,
        );
    }

    /// Make a change to the active project's issues and write it, then show
    /// what is now kept — selecting the issue the change names, if any.
    ///
    /// The whole read-change-write happens off the UI loop in one call, so it
    /// is made against what is on disk rather than against the copy on screen,
    /// and nothing else in this process can write in between.
    ///
    /// On a project kept in step with its forge the change is then synced
    /// straight away, so an edit reaches the forge without waiting for the
    /// timer.
    pub(super) fn change(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<Option<u64>, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some((root, path)) = self.file() else {
            return;
        };
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { issues::update_blocking(&path, change) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                let Some(state) = view.roots.get_mut(&root) else {
                    return;
                };
                match done {
                    Ok((kept, number)) => {
                        if number.is_some() {
                            state.selected = number;
                        }
                        state.form = None;
                        state.file.update(cx, |file, cx| {
                            file.land(kept, cx);
                            file.sync(true, cx);
                        });
                        view.status = None;
                    }
                    // The form stays open with what was typed in it: a refusal
                    // is something to correct, not a reason to start again.
                    Err(why) => view.status = Some(why),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
