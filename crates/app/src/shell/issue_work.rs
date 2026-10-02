//! An issue worked by hand in the checkout its project is open on, or in
//! steps, and the conversation an issue names opened again.

pub(crate) mod steps;

use super::Shell;
use crate::chat::session::ChatEvent;
use gpui::{Context, SharedString, Window};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;
use onehand_core::chat::Link;
use onehand_core::worktree;
use std::path::{Path, PathBuf};

/// What a session working an issue in steps in its checkout needs beyond
/// the session itself.
struct Steps {
    root: PathBuf,
    tracker: onehand_core::unattended::Tracker,
    issue: onehand_core::unattended::Issue,
}

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
        let note = "Taken up by a session on this checkout".to_string();
        self.start_issue_session(
            idx,
            file,
            number,
            prompt.to_string(),
            note,
            None,
            window,
            cx,
        );
    }

    /// Work issue `number` of project `root` in steps in a new worktree,
    /// which is an unattended run picked by hand: claimed, on a branch of the
    /// run's own, timed, checked and taken to a pull request. `extra` is what
    /// the person asks of every step.
    pub(super) fn work_issue_as_run(
        &mut self,
        root: &Path,
        number: u64,
        extra: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.issues_file(root) else {
            return;
        };
        let (root, extra) = (root.to_path_buf(), extra.to_string());
        let handle = window.window_handle();
        cx.spawn_in(window, async move |_, cx| {
            let read = {
                let root = root.clone();
                cx.background_executor()
                    .spawn(
                        async move { crate::unattended::kept_issue_blocking(&root, file, number) },
                    )
                    .await
            };
            let _ = cx.update(|window, cx| {
                let started = read.and_then(|(tracker, row)| {
                    crate::unattended::start_picked(root, tracker, row, Some(extra), handle, cx)
                });
                if let Err(why) = started {
                    window.push_notification(Notification::warning(why), cx);
                }
            });
        })
        .detach();
    }

    /// Work issue `number` of project `root` in steps in the checkout it is
    /// open on: a plan, the change and the project's check, judged by onehand
    /// one by one, with the change left uncommitted. `extra` is what the
    /// person asks of every step.
    pub(super) fn work_issue_in_steps(
        &mut self,
        root: &Path,
        number: u64,
        extra: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.issues_file(root) else {
            return;
        };
        let (root, extra) = (root.to_path_buf(), extra.to_string());
        cx.spawn_in(window, async move |shell, cx| {
            let read = {
                let (root, file) = (root.clone(), file.clone());
                cx.background_executor()
                    .spawn(
                        async move { crate::unattended::kept_issue_blocking(&root, file, number) },
                    )
                    .await
            };
            let _ = shell.update_in(cx, |shell, window, cx| {
                let (tracker, row) = match read {
                    Ok(read) => read,
                    Err(why) => {
                        window.push_notification(
                            Notification::warning(format!("This issue cannot be worked: {why}")),
                            cx,
                        );
                        return;
                    }
                };
                let Some(idx) = shell.root_index(&root) else {
                    return;
                };
                let note = "Taken up by a session on this checkout, in steps".to_string();
                let steps = Steps {
                    root,
                    tracker,
                    issue: row.issue,
                };
                shell.start_issue_session(idx, file, number, extra, note, Some(steps), window, cx);
            });
        })
        .detach();
    }

    /// Make a worktree of project `root` on `branch` (or the first free name
    /// after it), add it to the workspace as a project, and start a session
    /// there as [`Self::work_issue_here`] does, noting it on issue `number` of
    /// `root`, where the issue is kept.
    ///
    /// Still **no run**: the worktree is a project the person asked for, so it
    /// stays in the workspace like one made from the project menu, and nothing
    /// claims the issue or takes the folder away afterwards.
    pub(super) fn work_issue_in_worktree(
        &mut self,
        root: &Path,
        number: u64,
        prompt: &str,
        branch: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.issues_file(root) else {
            return;
        };
        let (root, prompt, branch) = (root.to_path_buf(), prompt.to_string(), branch.to_string());
        cx.spawn_in(window, async move |shell, cx| {
            let made = cx
                .background_executor()
                .spawn(async move {
                    let top = worktree::repo_top_blocking(&root)
                        .ok_or_else(|| format!("{} is not a git repository", root.display()))?;
                    let branch = onehand_core::unattended::free_branch_blocking(&top, &branch);
                    let dir = worktree::worktree_dir(&top, &branch);
                    let made = worktree::add_blocking(&top, &branch, &dir)?;
                    // The folder the project was opened on, in the new tree,
                    // as the worktree form adopts it.
                    let subtree = worktree::subtree_in(&made, &top, &root);
                    let dir = if subtree.is_dir() { subtree } else { made };
                    Ok::<_, String>((dir, branch))
                })
                .await;
            let _ = shell.update_in(cx, |shell, window, cx| {
                let (dir, branch) = match made {
                    Ok(made) => made,
                    Err(why) => {
                        window.push_notification(
                            Notification::warning(format!("No worktree for this issue: {why}")),
                            cx,
                        );
                        return;
                    }
                };
                let idx = shell.window.workspace.add_root(dir);
                shell.refresh_git(cx);
                shell.save_workspace(window, cx);
                let note = format!("Taken up by a session in a worktree on {branch}");
                shell.start_issue_session(idx, file, number, prompt, note, None, window, cx);
            });
        })
        .detach();
    }

    /// Start a session on project `idx` that sends `prompt` once the agent is
    /// up, and writes `note` on issue `number` of `file` once the agent has
    /// named the conversation. Given `steps`, the session works the issue in
    /// them instead, and `prompt` is what is asked of every step.
    #[allow(clippy::too_many_arguments)]
    fn start_issue_session(
        &mut self,
        idx: usize,
        file: PathBuf,
        number: u64,
        prompt: String,
        note: String,
        steps: Option<Steps>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let in_steps = steps.is_some();
        if let Some(Steps {
            root,
            tracker,
            issue,
        }) = steps
        {
            let shell = cx.entity().downgrade();
            steps::watch(
                uid,
                &session,
                shell,
                root,
                tracker,
                issue,
                Some(prompt.clone()),
                cx,
            );
        }
        let workbench = self.workbench.downgrade();
        let (mut sent, mut told) = (false, false);
        cx.subscribe(&session, move |_, session, _: &ChatEvent, cx| {
            let chat = &session.read(cx).chat;
            if !sent && chat.link == Link::Connected {
                sent = true;
                let untouched = !chat.busy && chat.prompts_sent == 0 && chat.queued.is_none();
                if untouched && in_steps {
                    steps::begin(uid, &session, cx);
                } else if untouched {
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
            let (file, note, workbench) = (file.clone(), note.clone(), workbench.clone());
            cx.spawn(async move |_, cx| {
                let done = cx
                    .background_executor()
                    .spawn(async move {
                        onehand_core::issues::taken_up_blocking(&file, number, &note, id)
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

    /// Tell the Workbench which step each run and each session working in
    /// steps is at, when that changed.
    pub(super) fn sync_issue_runs(&mut self, cx: &mut Context<Self>) {
        let mut runs = crate::unattended::runs_by_issue(cx);
        runs.extend(steps::by_issue(cx));
        if runs != self.issue_runs {
            self.workbench
                .update(cx, |panel, cx| panel.issue_runs(&runs, cx));
            self.issue_runs = runs;
        }
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
