//! An issue worked by hand in the checkout its project is open on.

use super::Shell;
use crate::chat::session::ChatEvent;
use gpui::{Context, Window};
use gpui_component::WindowExt as _;
use onehand_core::chat::drive::Origin;
use onehand_core::issues::link::{NotHere, TakenUp, work_here};
use std::path::Path;

impl Shell {
    /// Start a session on project `root` with `prompt` as its first message,
    /// and say on issue `number` which session took it up.
    ///
    /// **No worktree, no branch, no run.** This is the checkout the person is
    /// working in: it is an ordinary session that was handed its first
    /// message. The prompt goes once the agent is up, since
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
        let kept = self.issues_file(root, cx);
        // The note is written as from the project's own folder.
        let place = kept
            .as_ref()
            .map_or(root.to_path_buf(), |kept| kept.home.clone());
        let (idx, file) = match work_here(self.root_index(root), kept.map(|kept| kept.file)) {
            Ok(here) => here,
            Err(why) => {
                let said = not_here(why);
                #[cfg(test)]
                self.refused.push(said);
                window.push_notification(crate::toast::warning(said), cx);
                return;
            }
        };
        self.select_root(idx, window, cx);
        let Some(uid) = self.start_session(None, None, window, cx) else {
            return;
        };
        let Some(session) = self.chat.read(cx).session_entity(uid) else {
            window.push_notification(
                crate::toast::error("Starting the session for this issue failed"),
                cx,
            );
            return;
        };
        // Sent once the agent is up, unless a person prompted it first.
        session.update(cx, |session, cx| {
            session.prompt_when_up(prompt, &[], Origin::Issue, cx)
        });
        let at = self.chat.read(cx).window_handle();
        let mut owed = TakenUp::new(file, number);
        cx.subscribe(&session, move |_, session, _: &ChatEvent, cx| {
            let Some((file, change)) = owed.heard(session.read(cx).chat()) else {
                return;
            };
            // Through the file's drive, which every view of it shares: each
            // shows the note as it lands.
            onehand_issues::write(
                &place,
                file,
                change,
                crate::features::connectors(),
                move |done, cx| {
                    // Said once: the note is written once per session.
                    if let Err(why) = done {
                        let _ = at.update(cx, |_, window, cx| {
                            window.push_notification(
                                crate::toast::failed("Noting the session on the issue", why),
                                cx,
                            )
                        });
                    }
                },
                cx,
            );
        })
        .detach();
    }

    /// Tell the pages that list projects which projects this window has:
    /// asked when the git sweep lands, which is also when the projects have
    /// changed.
    pub(super) fn tell_pages_projects(&mut self, cx: &mut Context<Self>) {
        self.tell_issues_page_projects(cx);
        self.tell_workflows_page_projects(cx);
    }

    /// An issues file of this window's was written outside its views: what
    /// reads the file itself, the workspace page, reads it again while it is
    /// what is showing. The Issues views share the file's drive, which shows
    /// the write already.
    pub(crate) fn issues_written(&mut self, cx: &mut Context<Self>) {
        if self.workspace_shown(cx) {
            self.workspace_page.update(cx, |page, cx| page.reload(cx));
        }
    }
}

/// Why an issue cannot be worked by hand here, in the words the window says.
fn not_here(why: NotHere) -> &'static str {
    match why {
        NotHere::ProjectGone => super::carry::PROJECT_GONE,
        NotHere::NoIssuesFile => {
            "That folder's project keeps no issues here, so that issue cannot be noted"
        }
    }
}

#[cfg(test)]
mod tests;
