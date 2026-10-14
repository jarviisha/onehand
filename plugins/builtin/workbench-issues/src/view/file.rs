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
        self.change_now(
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
        self.publishing = self.key(&root, number);
        cx.notify();
        cx.spawn(async move |view, cx| {
            let done = cx
                .background_executor()
                .spawn(async move { sync::publish_blocking(&path, &root, forge, number) })
                .await;
            let _ = view.update(cx, |view: &mut Self, cx| {
                view.publishing = None;
                if let Err(why) = done {
                    view.failed = Some(("Publishing the issue", why));
                }
                file.update(cx, |file, cx| file.load(cx));
            });
        })
        .detach();
    }

    /// Settle issue `number`'s conflict one way or the other, then sync so the
    /// decision reaches the forge.
    pub(super) fn resolve(&mut self, number: u64, keep_mine: bool, cx: &mut Context<Self>) {
        let now = issues::now();
        self.change_now(
            move |kept| kept.resolve(number, keep_mine, now).map(|()| Some(number)),
            cx,
        );
    }

    /// Make a change to the active project's issues, show it at once and
    /// write it after; a write that fails is said once, and the newest one
    /// failing puts back what is on disk.
    pub(super) fn change_now(
        &mut self,
        change: impl Fn(&mut Issues) -> Result<Option<u64>, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if let Some(file) = self.state_mut().map(|state| state.file.clone()) {
            file.update(cx, |file, cx| file.show(&change, cx));
        }
        self.write(change, false, cx);
    }

    /// Make a change to the active project's issues and write it, then show
    /// what is now kept — selecting the issue the change names, if any. The
    /// project's form waits on its *Save* meanwhile.
    pub(super) fn change(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<Option<u64>, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.state_mut() {
            state.writing = true;
        }
        cx.notify();
        self.write(change, true, cx);
    }

    /// The whole read-change-write happens off the UI loop in one call, after
    /// every change made before it, so it is made against what is on disk
    /// rather than against the copy on screen.
    ///
    /// On a project kept in step with its forge the change is then synced
    /// straight away, so an edit reaches the forge without waiting for the
    /// timer.
    ///
    /// Only the newest change's write moves the selection to the issue it
    /// names, and only while the person has picked nothing else since it was
    /// made and no draft is open: an older one landing would take them back.
    fn write(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<Option<u64>, String> + Send + 'static,
        waited: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((root, _)) = self.file() else {
            return;
        };
        let Some((file, picked)) = self
            .state_mut()
            .map(|state| (state.file.clone(), state.selected))
        else {
            return;
        };
        let (of, written) = file.update(cx, |file, _| file.write(change));
        cx.spawn(async move |view, cx| {
            let done = written.await;
            // The file takes every write whether or not this view is still
            // there, so what was read meanwhile is not held for ever.
            let newest = file.update(cx, |file, cx| {
                let newest = file.newest(of);
                match &done {
                    Ok((kept, _)) => {
                        file.land(Some(kept.clone()), cx);
                        file.sync(true, cx);
                    }
                    Err(_) => {
                        file.land(None, cx);
                        if newest {
                            file.load(cx);
                        }
                    }
                }
                newest
            });
            let _ = view.update(cx, |view: &mut Self, cx| {
                let state = view.roots.get_mut(&root);
                match done {
                    Ok((_, number)) => {
                        if let Some(state) = state {
                            if waited {
                                state.writing = false;
                                state.form = None;
                            }
                            if newest
                                && number.is_some()
                                && state.selected == picked
                                && state.form.is_none()
                            {
                                state.selected = number;
                            }
                        }
                        view.status = None;
                    }
                    // The form stays open with what was typed in it: a refusal
                    // is something to correct, not a reason to start again.
                    Err(why) => {
                        if let Some(state) = state
                            && waited
                        {
                            state.writing = false;
                        }
                        view.failed = Some(("Saving the issue", why));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
