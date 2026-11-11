//! Writing a project's issues file: every change goes through one
//! read-change-write off the UI loop, and the file shared by every view of
//! it takes what was written and syncs it.

use super::IssuesView;
use gpui::Context;
use onehand_core::issues::drive::{Change, Signal};
use std::path::Path;

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
        self.change_now(Change::SyncWith(on.then(|| forge.name().to_string())), cx);
    }

    /// Send issue `number` to the forge, which is the only way anything written
    /// here reaches it.
    pub(super) fn publish(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some((root, _)) = self.file() else {
            return;
        };
        let Some(file) = self.state_mut().map(|state| state.file.clone()) else {
            return;
        };
        self.publish_asked = Some((root, number));
        file.update(cx, |file, cx| file.publish(number, cx));
    }

    /// Settle issue `number`'s conflict one way or the other, then sync so the
    /// decision reaches the forge.
    pub(super) fn resolve(&mut self, number: u64, keep_mine: bool, cx: &mut Context<Self>) {
        self.change_now(Change::Resolve { number, keep_mine }, cx);
    }

    /// Make a change to the active project's issues, show it at once and
    /// write it after; a write that fails is said once, and the newest one
    /// failing puts back what is on disk.
    pub(super) fn change_now(&mut self, change: Change, cx: &mut Context<Self>) {
        if let Some(file) = self.state_mut().map(|state| state.file.clone()) {
            file.update(cx, |file, cx| file.show(&change, cx));
        }
        self.write(change, false, cx);
    }

    /// Make a change to the active project's issues and write it, then show
    /// what is now kept — selecting the issue the change names, if any. The
    /// project's form waits on its *Save* meanwhile.
    pub(super) fn change(&mut self, change: Change, cx: &mut Context<Self>) {
        if let Some(state) = self.state_mut() {
            state.writing = true;
        }
        cx.notify();
        self.write(change, true, cx);
    }

    /// The whole read-change-write happens off the UI loop in one call, after
    /// every change made before it, so it is made against what is on disk
    /// rather than against the copy on screen; on a project kept in step with
    /// its forge it is then synced straight away. What it came to is heard in
    /// [`Self::heard`].
    fn write(&mut self, change: Change, waited: bool, cx: &mut Context<Self>) {
        let Some((root, _)) = self.file() else {
            return;
        };
        let Some((file, picked)) = self
            .state_mut()
            .map(|state| (state.file.clone(), state.selected))
        else {
            return;
        };
        let of = file.update(cx, |file, cx| file.write(change, cx));
        self.asked.insert((root, of), Asked { waited, picked });
    }

    /// What `root`'s file said.
    ///
    /// Only the newest change's write moves the selection to the issue it
    /// names, and only while the person has picked nothing else since it was
    /// made and no draft is open: an older one landing would take them back.
    /// A refused change keeps the form open with what was typed in it: a
    /// refusal is something to correct, not a reason to start again.
    pub(super) fn heard(&mut self, root: &Path, signal: &Signal, cx: &mut Context<Self>) {
        match signal {
            Signal::Wrote { of, done } => {
                let Some(asked) = self.asked.remove(&(root.to_path_buf(), *of)) else {
                    return;
                };
                let Some(state) = self.roots.get_mut(root) else {
                    return;
                };
                let newest = state.file.read(cx).newest(*of);
                if asked.waited {
                    state.writing = false;
                }
                match done {
                    Ok(number) => {
                        if asked.waited {
                            state.form = None;
                        }
                        if newest
                            && number.is_some()
                            && state.selected == asked.picked
                            && state.form.is_none()
                        {
                            state.selected = *number;
                        }
                        self.status = None;
                    }
                    Err(why) => self.failed = Some(("Saving the issue", why.clone())),
                }
            }
            Signal::PublishFailed { number, why } => {
                if self.publish_asked.as_ref() == Some(&(root.to_path_buf(), *number)) {
                    self.failed = Some(("Publishing the issue", why.clone()));
                }
            }
        }
        cx.notify();
    }
}

/// A change this view made, waiting for its write: whether its form waits on
/// it, and what was selected when it was made.
pub(super) struct Asked {
    waited: bool,
    picked: Option<u64>,
}

#[cfg(test)]
mod tests;
