//! What a project's page sends: a worktree cut first when it asked for one,
//! then the session, in the place it picked and run by the agent it picked,
//! its words held as the first prompt until the agent is up.

use super::Shell;
use super::sessions::agent_index;
use crate::chat::pane::{PageSend, StartIn};
use crate::state::Shared;
use gpui::{BorrowAppContext as _, Context, SharedString, Window};
use onehand_core::chat::Link;
use onehand_core::chat::drive::Origin;
use onehand_core::worktree;
use std::path::PathBuf;

impl Shell {
    /// Show the page of the project at row `idx`, ready to write: to start in
    /// `folder` when it names one of its others, run by `agent` when one is
    /// named.
    pub(crate) fn page_to_start(
        &mut self,
        idx: usize,
        folder: Option<PathBuf>,
        agent: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.window.workspace.roots.get(idx) else {
            return;
        };
        let start = match folder {
            Some(folder) if folder != row.path => StartIn::Folder(folder),
            Some(_) | None => StartIn::Project,
        };
        self.show_project_page(idx, window, cx);
        self.chat
            .update(cx, |pane, cx| pane.set_page_start(start, agent, window, cx));
    }

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
            StartIn::Project => match self.local_root(idx).map(PathBuf::from) {
                Some(folder) => self.start_unshown(root, folder, agent, text, window, cx),
                // A host's folder: its session opens there and is shown at
                // once, as a host session always is.
                None => {
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
                    self.page_session_shown(root, uid, text, window, cx);
                }
            },
            StartIn::Folder(folder) => self.start_unshown(root, folder, agent, text, window, cx),
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
            self.page_gave_up(&root, why.to_string(), cx);
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
                    match shell.record_split(&root, dir.clone(), branch, from, window, cx) {
                        Some(_) => shell.start_unshown(root, dir, agent, text, window, cx),
                        None => shell.page_gave_up(
                            &root,
                            format!(
                                "{} was made, but its project could not take it",
                                dir.display()
                            ),
                            cx,
                        ),
                    }
                }
                Err(why) => shell.page_gave_up(&root, why, cx),
            });
        })
        .detach();
    }

    /// Start a session in `folder` for the page of `root`, off screen: its
    /// words are its first prompt, held until its agent is up. The page says
    /// it is starting meanwhile; once the agent is up the session is the
    /// person's, and shown if they still stand on the page; if it never comes
    /// up, the session goes and the page says why, its words kept.
    fn start_unshown(
        &mut self,
        root: PathBuf,
        folder: PathBuf,
        agent: Option<SharedString>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(spec) = Shared::global(cx)
            .settings
            .agents
            .get(agent_index(agent, cx))
            .cloned()
        else {
            self.page_gave_up(&root, "No agent is set up".to_string(), cx);
            return;
        };
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let project = crate::projects::of(&folder, cx);
        let added =
            self.window
                .workspace
                .add_run_session(&folder, spec.clone(), uid, project.as_ref());
        let session = added.and_then(|_| {
            self.chat.update(cx, |pane, cx| {
                pane.open_unshown(uid, folder, &spec, Vec::new(), cx)
            })
        });
        let Some(session) = session else {
            self.page_gave_up(&root, "The session could not be started".to_string(), cx);
            return;
        };
        self.chat.update(cx, |pane, cx| {
            pane.page_busy(&root, Some(format!("Starting {}…", spec.name)), cx)
        });
        session.update(cx, |session, cx| {
            session.prompt_when_up(&text, Origin::Person, cx)
        });
        let wait = cx.observe_in(
            &session,
            window,
            move |shell: &mut Self, session, window, cx| {
                let link = session.read(cx).chat().link;
                match link {
                    Link::Connecting => {}
                    Link::Connected => shell.page_started(uid, window, cx),
                    Link::Lost => {
                        let why = session
                            .read(cx)
                            .chat()
                            .failure()
                            .map_or_else(|| "The agent did not start".to_string(), str::to_string);
                        shell.page_start_failed(uid, why, window, cx);
                    }
                }
            },
        );
        self.page_waits.insert(uid, (root, wait));
        cx.notify();
    }

    /// The agent of `uid`, started for a page, is up: the session is the
    /// person's, shown if they still stand on that page.
    fn page_started(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((root, _)) = self.page_waits.remove(&uid) else {
            return;
        };
        self.page_starting.remove(&root);
        let standing = self.chat.read(cx).page_root().as_deref() == Some(root.as_path());
        self.chat.update(cx, |pane, cx| {
            pane.page_busy(&root, None, cx);
            pane.page_sent(&root, window, cx);
        });
        if standing {
            self.show_session(uid, window, cx);
        }
    }

    /// The agent of `uid`, started for a page, never came up: the session
    /// goes and the page says `why`, its words kept.
    fn page_start_failed(
        &mut self,
        uid: u64,
        why: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((root, _)) = self.page_waits.remove(&uid) else {
            return;
        };
        self.close_finished(uid, window, cx);
        self.page_gave_up(&root, why, cx);
    }

    /// What the page of `root` sent started nothing: it says `why`.
    fn page_gave_up(&mut self, root: &std::path::Path, why: String, cx: &mut Context<Self>) {
        self.page_starting.remove(root);
        self.chat
            .update(cx, |pane, cx| pane.page_failed(root, why, cx));
    }

    /// The session `uid` made and shown for the page of `root`, a host's: its
    /// words go to it as its first prompt once its agent is up, and leave
    /// the page.
    fn page_session_shown(
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
