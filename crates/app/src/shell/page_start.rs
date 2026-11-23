//! What a project's page sends: a worktree cut first when it asked for one,
//! then the session, in the place it picked and run by the agent it picked,
//! its words held as the first prompt until the agent is up.

use super::Shell;
use super::sessions::agent_index;
use crate::chat::pane::{PageSend, StartIn};
use crate::state::Shared;
use gpui::{Context, SharedString, Window};
use onehand_core::chat::drive::Origin;
use onehand_core::workspace::drive::StartsOn;
use onehand_core::worktree;
use std::path::PathBuf;

impl Shell {
    /// Start what the page of the project at `root` sent; see the module.
    pub(super) fn start_from_page(
        &mut self,
        send: PageSend,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PageSend {
            root,
            text,
            agent,
            start,
            branch,
        } = send;
        // The page on screen is the active project's; a send from one the
        // person has since left is not taken.
        let idx = self.window.workspace.active_root;
        if self.window.workspace.roots.get(idx).map(|row| &row.path) != Some(&root) {
            return;
        }
        // One send at a time from a page: a second press while a worktree is
        // cut would cut it twice.
        if !self.page_starting.insert(root.clone()) {
            return;
        }
        match start {
            StartIn::Project => {
                let spec = Shared::global(cx)
                    .settings
                    .agents
                    .get(agent_index(agent, cx))
                    .cloned();
                let uid = self.add_by(
                    |workspace, uid| workspace.add_session_in(idx, spec, uid),
                    window,
                    cx,
                    |_, _| {},
                );
                self.page_session(root, uid, text, window, cx);
            }
            StartIn::Folder(folder) => {
                let uid = self.session_in_folder(idx, folder, agent, StartsOn::Fresh, window, cx);
                self.page_session(root, uid, text, window, cx);
            }
            StartIn::NewWorktree => {
                let branch = branch.unwrap_or_default();
                self.cut_for_page(root, branch, agent, text, window, cx);
            }
        }
    }

    /// Cut the worktree the page asked for, then start its session there.
    fn cut_for_page(
        &mut self,
        root: PathBuf,
        branch: String,
        agent: Option<SharedString>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(why) = worktree::validate_branch(&branch) {
            self.page_starting.remove(&root);
            self.chat
                .update(cx, |pane, cx| pane.page_failed(&root, why.to_string(), cx));
            return;
        }
        let parent = crate::projects::worktree_parent(&root, cx);
        self.chat.update(cx, |pane, cx| {
            pane.page_busy(&root, Some(format!("Making worktree {branch}…")), cx)
        });
        cx.spawn_in(window, async move |shell, cx| {
            let made = {
                let (root, branch) = (root.clone(), branch.clone());
                cx.background_executor()
                    .spawn(async move {
                        let top = worktree::repo_top_blocking(&root);
                        let repo = top.as_deref().unwrap_or(&root);
                        let dir = match parent.as_deref() {
                            Some(parent) => worktree::worktree_dir_in(parent, repo, &branch),
                            None => worktree::worktree_dir(repo, &branch),
                        };
                        super::drafts::make_worktree_blocking(
                            &root,
                            &branch,
                            &dir,
                            top.as_deref(),
                            parent.as_deref(),
                        )
                    })
                    .await
            };
            let _ = shell.update_in(cx, |shell: &mut Self, window, cx| match made {
                Ok((dir, from)) => {
                    shell
                        .chat
                        .update(cx, |pane, cx| pane.page_busy(&root, None, cx));
                    let row = shell.record_split(&root, dir.clone(), branch, from, window, cx);
                    let uid = row.and_then(|idx| {
                        shell.session_in_folder(idx, dir, agent, StartsOn::Fresh, window, cx)
                    });
                    shell.page_session(root, uid, text, window, cx);
                }
                Err(why) => {
                    shell.page_starting.remove(&root);
                    shell
                        .chat
                        .update(cx, |pane, cx| pane.page_failed(&root, why, cx));
                }
            });
        })
        .detach();
    }

    /// The session `uid` made for the page of `root`: its words go to it as
    /// its first prompt once its agent is up, and leave the page.
    fn page_session(
        &mut self,
        root: PathBuf,
        uid: Option<u64>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page_starting.remove(&root);
        let session = uid.and_then(|uid| self.chat.read(cx).session_entity(uid));
        let Some(session) = session else {
            self.chat.update(cx, |pane, cx| {
                pane.page_failed(&root, "The session could not be started".to_string(), cx)
            });
            return;
        };
        session.update(cx, |session, cx| {
            session.prompt_when_up(&text, Origin::Person, cx)
        });
        self.chat
            .update(cx, |pane, cx| pane.page_sent(&root, window, cx));
    }
}
