use super::Shell;
use super::storage::pick_folder;
use gpui::{App, Context, Entity, Focusable as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::input::InputState;
use gpui_component::notification::Notification;
use onehand_core::workspace::{self};
use onehand_core::worktree;
use std::path::PathBuf;

/// A worktree being set up: which project is being split, where its second
/// checkout goes, and how the last attempt went.
///
/// The branch name is not here — it lives in an `InputState` the shell keeps
/// across dialogs, the way the conversation rename does, so the field and its
/// change subscription are built once instead of per opening.
/// What every form here that runs git and waits for it keeps.
///
/// **The three facts, not the whole form.** The two forms below are genuinely
/// different — a rename has no folder to land in and no repository top to land
/// beside — so one struct for both would be two forms keeping each other's
/// fields blank. What they do share is the part that has nothing to do with
/// either: which project it is about, what went wrong, and whether git is still
/// working. Written out twice, those three drifted in their wording while
/// meaning the same thing, and the three moves made on them — open, refuse,
/// start — were written out twice with them.
pub struct Draft {
    /// The project the form is about, held by path rather than by index: a
    /// draft outlives its own frames, and a project removed underneath it must
    /// not hand its index — and with it an operation on the wrong repository —
    /// to whichever project slides into that slot.
    pub root: PathBuf,
    /// What is wrong: the name rule that refused, or git's own words. Cleared
    /// by the next keystroke, because the reader has started answering it.
    pub error: Option<String>,
    /// Git in flight. Even a short one needs this: a button with no answer at
    /// all reads as a press that missed.
    pub busy: bool,
}

impl Draft {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            error: None,
            busy: false,
        }
    }

    /// The complaint, and the form left up to carry it — which is the whole
    /// reason these forms do not close on the press: this is the one place it
    /// can be shown against the name that caused it.
    fn refuse(&mut self, why: impl Into<String>) {
        self.busy = false;
        self.error = Some(why.into());
    }

    /// Handed to git. The last complaint goes with it, or a failed attempt's
    /// words sit over the attempt now running.
    fn start(&mut self) {
        self.error = None;
        self.busy = true;
    }
}

/// The complaint goes, because the keystroke that arrived is the reader
/// answering it: it was about a name that is already being replaced.
pub(super) fn answered<D: std::ops::DerefMut<Target = Draft>>(slot: &mut Option<D>) {
    if let Some(draft) = slot {
        draft.error = None;
    }
}

/// Put a form away, unless git is mid-anything: a draft that vanished while its
/// own operation was running would take with it the only thing that can report
/// how it went. `true` where there was something to dismiss and it went.
fn dismiss<D: std::ops::Deref<Target = Draft>>(
    slot: &mut Option<D>,
    cx: &mut Context<Shell>,
) -> bool {
    if slot.as_ref().is_some_and(|draft| draft.busy) {
        return false;
    }
    if slot.take().is_some() {
        cx.notify();
        return true;
    }
    false
}

/// A `git branch -m` waiting on a name.
pub struct BranchDraft {
    pub form: Draft,
    /// What the branch is called now, which is what the form is about and what
    /// its field opens on.
    pub from: String,
}

impl std::ops::Deref for BranchDraft {
    type Target = Draft;

    fn deref(&self) -> &Draft {
        &self.form
    }
}

impl std::ops::DerefMut for BranchDraft {
    fn deref_mut(&mut self) -> &mut Draft {
        &mut self.form
    }
}

pub struct WorktreeDraft {
    pub form: Draft,
    /// That project's label, for saying out loud what is being split.
    pub label: String,
    /// The repository the project sits in, which is what git will actually
    /// check out. Read off disk just after the form opens, so it is `None` for
    /// the moment before that answer lands -- and for a root that is no
    /// repository at all, which the form is not offered on.
    ///
    /// A project root is often the repository itself, and then this changes
    /// nothing. It matters when the root is a folder *inside* one: the second
    /// checkout has to go beside the repository rather than into it, and be
    /// named after it, because a worktree of half a repository is not a thing
    /// git can make.
    pub top: Option<PathBuf>,
    /// A folder the user chose to put the worktree under. `None` ⇒ beside the
    /// repository it came from, which is what `onehand_core::worktree` decides.
    pub parent: Option<PathBuf>,
}

impl std::ops::Deref for WorktreeDraft {
    type Target = Draft;

    fn deref(&self) -> &Draft {
        &self.form
    }
}

impl std::ops::DerefMut for WorktreeDraft {
    fn deref_mut(&mut self) -> &mut Draft {
        &mut self.form
    }
}

impl Shell {
    /// Open the split-onto-a-branch form on a project.
    ///
    /// Offered on git repositories only, which the rail decides from the status
    /// it already holds -- so this is reached with a repository in hand and has
    /// no check of its own to make beyond the root still being there.
    pub fn begin_worktree(&mut self, root_idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.roots.get(root_idx) else {
            return;
        };
        let (root, label) = (root.path.clone(), root.label.clone());
        self.worktree_branch
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.worktree_draft = Some(WorktreeDraft {
            form: Draft::new(root.clone()),
            label,
            top: None,
            parent: None,
        });
        self.worktree_branch.focus_handle(cx).focus(window, cx);
        cx.notify();

        // Asked off the UI loop, like every other thing this app learns from a
        // process. Nothing is showing that depends on the answer yet -- the
        // folder line stays blank until a branch is named -- so it lands well
        // before it is read.
        cx.spawn(async move |shell, cx| {
            let found = cx
                .background_executor()
                .spawn({
                    let root = root.clone();
                    async move { worktree::repo_top_blocking(&root) }
                })
                .await;
            shell
                .update(cx, |shell: &mut Self, cx| {
                    // Only the draft it was asked for: the form can have been
                    // closed and reopened on another project in the meantime,
                    // and a repository answered for one project is not an
                    // answer about another.
                    if let Some(draft) = shell
                        .worktree_draft
                        .as_mut()
                        .filter(|draft| draft.root == root)
                    {
                        draft.top = found;
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Open the rename-this-branch form on a project.
    ///
    /// Offered on repositories only, like the worktree form and decided the
    /// same way -- by whether the last sweep found a status for this root, which
    /// is also where the current name comes from. So it is reached with both
    /// facts in hand and has no question of its own to ask git.
    ///
    /// The field opens on the **name as it is**, unlike the worktree form's,
    /// which opens empty. That one is naming something that does not exist yet;
    /// this one is editing something that does, and a blank field would make the
    /// user retype the part they are keeping to change the part they are not.
    pub fn begin_branch_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.window.workspace.active_root() else {
            return;
        };
        let root = root.path.clone();
        let Some(from) = self
            .window
            .git
            .get(&root)
            .map(|status| status.branch.clone())
        else {
            return;
        };
        self.branch_input
            .update(cx, |state, cx| state.set_value(&from, window, cx));
        self.branch_draft = Some(BranchDraft {
            form: Draft::new(root),
            from,
        });
        self.branch_input.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub fn branch_draft(&self) -> Option<&BranchDraft> {
        self.branch_draft.as_ref()
    }

    pub fn branch_input(&self) -> &Entity<InputState> {
        &self.branch_input
    }

    /// Put the form away, unless git is mid-rename.
    pub fn cancel_branch_rename(&mut self, cx: &mut Context<Self>) -> bool {
        dismiss(&mut self.branch_draft, cx)
    }

    /// Rename the branch, and leave the form up until git answers.
    ///
    /// Up, for the reason the worktree form stays up: this is the one place the
    /// complaint can be shown against the name that caused it, and a dialog that
    /// closed on the press would have to report the failure as a toast about a
    /// name the reader can no longer see.
    ///
    /// **A name that has not changed closes the form and does nothing.** git
    /// accepts renaming a branch to what it is already called, so the check is
    /// not about correctness -- it is that a sweep, a notification and a line in
    /// the strip flashing for a no-op reads as something having happened.
    pub fn commit_branch_rename(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.branch_draft.as_ref() else {
            return;
        };
        if draft.busy {
            return;
        }
        let (root, from) = (draft.root.clone(), draft.from.clone());
        let name = self.branch_input.read(cx).value().trim().to_string();
        if name == from {
            self.branch_draft = None;
            cx.notify();
            return;
        }
        if let Err(why) = worktree::validate_branch(&name) {
            if let Some(draft) = self.branch_draft.as_mut() {
                draft.refuse(why.to_string());
            }
            cx.notify();
            return;
        }
        if let Some(draft) = self.branch_draft.as_mut() {
            draft.start();
        }
        cx.notify();

        cx.spawn(async move |shell, cx| {
            let done = {
                let (root, name) = (root.clone(), name.clone());
                cx.background_executor()
                    .spawn(async move { worktree::rename_branch_blocking(&root, &name) })
                    .await
            };
            shell
                .update_in(cx, |shell: &mut Self, window, cx| {
                    match done {
                        Ok(()) => {
                            // Only the form this was started from, for the
                            // reason the worktree commit says: the form refuses
                            // to close while it is working, so in every ordinary
                            // case this is the one on screen -- and clearing
                            // whatever happens to be there instead would throw
                            // away a form somebody had since opened.
                            shell
                                .branch_draft
                                .take_if(|draft| draft.root == root && draft.busy);
                            // The strip, the rail and the file tree all read the
                            // branch off the last sweep, so the rename is not on
                            // screen anywhere until one runs.
                            shell.refresh_git(cx);
                            window.push_notification(
                                Notification::info(format!("Renamed {from} to {name}")),
                                cx,
                            );
                        }
                        Err(why) => {
                            if let Some(draft) = shell.branch_draft.as_mut() {
                                draft.refuse(why);
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    pub fn worktree_draft(&self) -> Option<&WorktreeDraft> {
        self.worktree_draft.as_ref()
    }

    pub fn worktree_branch(&self) -> &Entity<InputState> {
        &self.worktree_branch
    }

    /// Where the worktree would land, given the name typed so far. `None` while
    /// there is no name to derive a folder from.
    ///
    /// Derived on every read rather than mirrored into a second field: the
    /// folder is a *reading* of the branch name, and a copy of it would be one
    /// more thing to keep in step with a field being typed into.
    pub fn worktree_target(&self, cx: &App) -> Option<PathBuf> {
        let draft = self.worktree_draft.as_ref()?;
        let branch = self.worktree_branch.read(cx).value();
        let branch = branch.trim();
        if branch.is_empty() {
            return None;
        }
        // The repository, not the project, wherever the two differ: git checks
        // out repositories, so a project that is a folder inside one has its
        // second checkout placed and named after the repository -- next to it
        // rather than into its own file tree.
        let repo = draft.top.as_deref().unwrap_or(&draft.root);
        Some(match draft.parent.as_deref() {
            Some(parent) => worktree::worktree_dir_in(parent, repo, branch),
            None => worktree::worktree_dir(repo, branch),
        })
    }

    /// Choose a folder to put the worktree under, instead of the one beside the
    /// project.
    ///
    /// The *parent* is picked, not the worktree itself: the native picker only
    /// offers folders that already exist, and `git worktree add` wants one that
    /// does not. So the choice is where to put it, and the name stays derived
    /// from the branch -- which also means changing the branch after choosing
    /// still moves the folder with it.
    pub fn pick_worktree_parent(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |shell, cx| {
            let Some(dir) = pick_folder(cx).await else {
                return;
            };
            shell
                .update(cx, |shell: &mut Self, cx| {
                    if let Some(draft) = shell.worktree_draft.as_mut() {
                        draft.parent = Some(dir);
                        draft.error = None;
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Close the form without creating anything.
    ///
    /// **Refused while git is working**, and refused here rather than at each
    /// control, because there are three ways out of this form -- the button,
    /// Esc, and the dialog's own ✕ -- and a rule held in one of them is a rule
    /// two ways round it. A `git worktree add` in flight cannot be called back:
    /// it is a process already writing a checkout to disk. Letting the form go
    /// would leave that checkout to finish into a workspace with nowhere to put
    /// it -- a folder that appeared on disk, on a branch, belonging to nothing.
    pub fn cancel_worktree(&mut self, cx: &mut Context<Self>) {
        dismiss(&mut self.worktree_draft, cx);
    }

    /// Create the worktree, then adopt it as a project root of its own.
    ///
    /// A root, not a mode of the project it came from: a worktree is a whole
    /// second checkout, so its file tree, its terminal, its git status and its
    /// sessions are all different from the original's -- and every one of those
    /// is already keyed by path, so the workspace tree holds it correctly with
    /// nothing added.
    ///
    /// The form stays on screen until git answers. It is the one place the
    /// error can be shown against the name that caused it, and a dialog that
    /// closed on the press would have to report a failure as a toast about a
    /// folder the user can no longer see the name of.
    pub fn commit_worktree(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.worktree_draft.as_mut() else {
            return;
        };
        if draft.busy {
            return;
        }
        let branch = self.worktree_branch.read(cx).value().trim().to_string();
        if let Err(why) = worktree::validate_branch(&branch) {
            if let Some(draft) = self.worktree_draft.as_mut() {
                draft.refuse(why.to_string());
            }
            cx.notify();
            return;
        }
        let Some(dir) = self.worktree_target(cx) else {
            return;
        };
        let Some(draft) = self.worktree_draft.as_mut() else {
            return;
        };
        let root = draft.root.clone();
        let top = draft.top.clone();
        draft.start();
        cx.notify();

        cx.spawn(async move |shell, cx| {
            let made = {
                let (root, branch, top) = (root.clone(), branch.clone(), top.clone());
                cx.background_executor()
                    .spawn(async move {
                        worktree::add_blocking(&root, &branch, &dir).map(|made| {
                            // What git made is a checkout of the whole
                            // repository. What is adopted is the part of it the
                            // user actually split -- the same folder, in the new
                            // tree. Falling back to the checkout itself when
                            // that folder is not on this branch, because a root
                            // pointing at nothing is worse than a wider one.
                            let Some(top) = top else { return made };
                            let subtree = worktree::subtree_in(&made, &top, &root);
                            if subtree.is_dir() { subtree } else { made }
                        })
                    })
                    .await
            };
            shell
                .update_in(cx, |shell: &mut Self, window, cx| {
                    match made {
                        Ok(dir) => {
                            // Only the draft this was started from. It is the
                            // one on screen in every ordinary case -- the form
                            // refuses to close while it is working -- and
                            // clearing whatever happens to be there instead
                            // would throw away a form somebody had since opened.
                            shell
                                .worktree_draft
                                .take_if(|draft| draft.root == root && draft.busy);
                            let label = workspace::label_for(&dir);
                            let idx = shell.window.workspace.add_root(dir);
                            shell.window.workspace.select_root(idx);
                            shell.show_active_session(window, cx);
                            shell.refresh_git(cx);
                            shell.save_workspace(window, cx);
                            window.push_notification(
                                Notification::info(format!("Added {label}, on {branch}")),
                                cx,
                            );
                        }
                        Err(why) => {
                            if let Some(draft) = shell.worktree_draft.as_mut() {
                                draft.refuse(why);
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Open the rename field on a conversation.
    ///
    /// Prefilled with the name the user set, **not** with the title derived
    /// from the first prompt: accepting a prefilled guess would freeze that
    /// guess in place as an explicit choice, and the derived title is supposed
    /// to keep tracking the conversation.
    pub fn begin_rename(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.chat.read(cx).custom_title(uid, cx).unwrap_or_default();
        self.rename_input
            .update(cx, |state, cx| state.set_value(current, window, cx));
        self.renaming = Some(uid);
        self.rename_input.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Commit whatever is in the rename field, if it is a name at all.
    pub fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.renaming.take() else {
            return;
        };
        let title = self.rename_input.read(cx).value().to_string();
        self.chat.update(cx, |pane, cx| {
            pane.rename(uid, &title, cx);
        });
        cx.notify();
    }

    /// Close the rename field without changing anything.
    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            cx.notify();
        }
    }

    /// Give a conversation its derived title back.
    pub fn reset_conversation_title(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.renaming.take() else {
            return;
        };
        self.chat.update(cx, |pane, cx| pane.reset_title(uid, cx));
        cx.notify();
    }
}
