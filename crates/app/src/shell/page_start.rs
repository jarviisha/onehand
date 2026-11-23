//! What a project's page sends: a worktree cut first when it asked for one,
//! then the session, in the place it picked and run by the agent it picked,
//! its words held as the first prompt until the agent is up.

use super::Shell;
use super::sessions::agent_index;
use crate::chat::pane::{PageSend, StartIn};
use crate::state::Shared;
use gpui::{BorrowAppContext as _, Context, SharedString, Window};
use onehand_core::chat::drive::Origin;
use onehand_core::workspace::page_starts::Effect as PageEffect;
use onehand_core::worktree;
use std::path::PathBuf;

/// What a page asked of the session it sends: the agent to run it, its first
/// prompt and what was picked of the agent's offer.
struct Ask {
    agent: Option<SharedString>,
    text: String,
    picks: Vec<(String, String)>,
    staged: Vec<onehand_core::attachment::StagedAttachment>,
    cards: onehand_core::chat::Cards,
    /// The past conversation to hand over first, when there is one.
    continuing: Option<crate::chat::pane::Continuing>,
    /// The title of the conversation it continues, once handed over.
    continued: Option<String>,
    /// Whether the page's project is a host's row.
    on_host: bool,
}

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
            picks,
            staged,
            cards,
            continuing,
        } = send;
        // The page on screen is the active project's; whether the send is
        // taken is the page starts' to say.
        let idx = self.window.workspace.active_root;
        let on_screen = self
            .window
            .workspace
            .roots
            .get(idx)
            .map(|row| row.path.clone());
        if !self.page_starts.take(&root, on_screen.as_deref()) {
            return;
        }
        // A host's row is told from a row here on the same path by its host.
        let on_host = self.window.workspace.roots[idx].host.is_some();
        let ask = Ask {
            agent,
            text,
            picks,
            staged,
            cards,
            continuing,
            continued: None,
            on_host,
        };
        match start {
            // The row's own folder: a host's runs there, an offloaded one's
            // in its mirror, as the session's start finds.
            StartIn::Project => {
                let folder = self.window.workspace.roots[idx].path.clone();
                self.start_unshown(root, folder, ask, window, cx)
            }
            StartIn::Folder(folder) => self.start_unshown(root, folder, ask, window, cx),
            StartIn::NewWorktree => {
                let branch = branch.unwrap_or_default();
                self.cut_for_page(root, branch, ask, window, cx);
            }
        }
    }

    /// Cut the worktree the page asked for, then start its session there.
    fn cut_for_page(
        &mut self,
        root: PathBuf,
        branch: String,
        ask: Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(why) = worktree::validate_branch(&branch) {
            let fx = self.page_starts.gave_up(&root, why.to_string());
            self.carry_page(fx, window, cx);
            return;
        }
        let parent = crate::projects::worktree_parent(&root, cx);
        let fx = self.page_starts.cutting(&root, &branch);
        self.carry_page(fx, window, cx);
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
                        Some(_) => shell.start_unshown(root, dir, ask, window, cx),
                        None => shell.give_up(
                            &root,
                            format!(
                                "{} was made, but its project could not take it",
                                dir.display()
                            ),
                            window,
                            cx,
                        ),
                    }
                }
                Err(why) => shell.give_up(&root, why, window, cx),
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
        mut ask: Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A past conversation is written where the session can read it
        // first, off the UI loop, and its words then point the agent at it.
        if let Some(carried) = ask.continuing.take() {
            let fx = self.page_starts.writing(&root, &carried.title);
            self.carry_page(fx, window, cx);
            cx.spawn_in(window, async move |shell, cx| {
                let at = folder.clone();
                let kept = carried.dir.clone();
                let written = cx
                    .background_executor()
                    .spawn(async move { onehand_core::chat::handoff::write_blocking(&kept, &at) })
                    .await;
                let _ = shell.update_in(cx, |shell: &mut Self, window, cx| match written {
                    Ok(file) => {
                        ask.text = onehand_core::chat::handoff::lead_in(
                            &ask.text,
                            &file,
                            &carried.title,
                            carried.summarise,
                        );
                        ask.continued = Some(carried.title);
                        shell.start_unshown(root, folder, ask, window, cx);
                    }
                    Err(why) => shell.give_up(
                        &root,
                        format!("The earlier conversation could not be written out: {why}"),
                        window,
                        cx,
                    ),
                });
            })
            .detach();
            return;
        }
        let Ask {
            agent,
            text,
            picks,
            staged,
            cards,
            continued,
            on_host,
            ..
        } = ask;
        let Some(spec) = Shared::global(cx)
            .settings
            .agents
            .get(agent_index(agent, cx))
            .cloned()
        else {
            self.give_up(&root, "No agent is set up".to_string(), window, cx);
            return;
        };
        let uid = cx.update_global::<Shared, _>(|shared, _| shared.next_uid());
        let row = self
            .window
            .workspace
            .roots
            .iter()
            .position(|row| row.path == root && row.host.is_some() == on_host);
        // Where the session runs: a host's folder, or an offloaded folder's
        // mirror, over the host's attach; else here.
        let on = row.and_then(|row| {
            self.window.workspace.roots[row]
                .host_of(&folder)
                .map(|(host, cwd)| (host.clone(), cwd.to_path_buf()))
        });
        let added = match on_host {
            true => row.and_then(|row| {
                self.window
                    .workspace
                    .add_session_unshown(row, spec.clone(), uid)
            }),
            false => {
                let project = crate::projects::of(&folder, cx);
                self.window
                    .workspace
                    .add_run_session(&folder, spec.clone(), uid, project.as_ref())
            }
        };
        let session = added.and_then(|_| {
            self.chat.update(cx, |pane, cx| {
                pane.open_unshown(uid, folder, on, &spec, Vec::new(), cx)
            })
        });
        let Some(session) = session else {
            self.give_up(
                &root,
                "The session could not be started".to_string(),
                window,
                cx,
            );
            return;
        };
        let fx = self.page_starts.started(&root, uid, &spec.name);
        self.carry_page(fx, window, cx);
        // Armed before the words, so what was picked is in force when the
        // agent reads them.
        session.update(cx, |session, cx| {
            session.drive(cx, |chat, _| {
                chat.arm_start_picks(picks);
                if let Some(title) = continued {
                    chat.continues(title);
                }
                ((), chat.set_cards(cards))
            });
            session.prompt_when_up(&text, &staged, Origin::Person, cx)
        });
        // The session's link is what the page starts hear of it; what that
        // comes to is theirs.
        let wait = cx.observe_in(
            &session,
            window,
            move |shell: &mut Self, session, window, cx| {
                let chat = session.read(cx).chat();
                let (link, why) = (chat.link, chat.failure().map(str::to_string));
                let stands = shell
                    .page_starts
                    .page_of(uid)
                    .is_some_and(|page| shell.chat.read(cx).page_root().as_deref() == Some(page));
                let fx = shell.page_starts.link(uid, link, why, stands);
                if !shell.page_starts.waits_on(uid) {
                    shell.page_waits.remove(&uid);
                }
                shell.carry_page(fx, window, cx);
            },
        );
        self.page_waits.insert(uid, wait);
        cx.notify();
    }

    /// `root`'s send started nothing, for `why`.
    fn give_up(
        &mut self,
        root: &std::path::Path,
        why: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fx = self.page_starts.gave_up(root, why);
        self.carry_page(fx, window, cx);
    }

    /// Carry out what the page starts answered.
    fn carry_page(&mut self, fx: Vec<PageEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in fx {
            match effect {
                PageEffect::Busy { root, said } => self
                    .chat
                    .update(cx, |pane, cx| pane.page_busy(&root, said, cx)),
                PageEffect::Failed { root, why } => self
                    .chat
                    .update(cx, |pane, cx| pane.page_failed(&root, why, cx)),
                PageEffect::Sent { root } => self
                    .chat
                    .update(cx, |pane, cx| pane.page_sent(&root, window, cx)),
                PageEffect::Show { uid } => self.show_session(uid, window, cx),
                PageEffect::Close { uid } => self.close_finished(uid, window, cx),
            }
        }
    }
}
