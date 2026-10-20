//! What the view asks of the app: a file put on screen goes to the Workbench,
//! which knows the mode that edits files; a session on an issue, a workflow
//! run, a task or a conversation goes to the shell, whose they are. Each is
//! put once the view is no longer being updated.

use super::*;

impl IssuesView {
    /// Put a request to the Workbench about the project on screen, once this
    /// view is no longer being updated: the Workbench puts what it is asked to
    /// every mode, this one included, and a press inside this view lands while
    /// the view is mid-update.
    pub(super) fn ask_later(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        put: impl FnOnce(&Ask, &Path, &mut Window, &mut App) + 'static,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let ask = self.ask.clone();
        window.defer(cx, move |window, cx| put(&ask, &root, window, cx));
    }

    /// Tell the shell something about the project on screen, once this view
    /// is no longer being updated, for the reason [`Self::ask_later`] waits:
    /// the shell reaches back into this view.
    pub(super) fn tell_later(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        put: impl FnOnce(&Path) -> IssuesEvent + 'static,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let tell = self.tell.clone();
        window.defer(cx, move |window, cx| tell(put(&root), window, cx));
    }

    /// Start a session on the project on screen, in the checkout it is open
    /// on, with issue `number` as its first message.
    pub(super) fn work_here(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self
            .root
            .as_deref()
            .and_then(|root| self.issues_of(root, cx)?.get(number))
            .map(issues::work_here_prompt)
        else {
            return;
        };
        self.tell_later(window, cx, move |root| IssuesEvent::WorkIssueHere {
            root: root.to_path_buf(),
            number,
            prompt,
        });
    }

    /// Choose a workflow and work issue `number` of the project on screen
    /// with it, on a worktree of its own.
    pub(super) fn run_workflow(
        &mut self,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tell_later(window, cx, move |root| IssuesEvent::RunIssueWorkflow {
            root: root.to_path_buf(),
            number,
        });
    }

    /// Put issue `number` of the project on screen on the Issues page.
    pub(super) fn open_in_issues(
        &mut self,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tell_later(window, cx, move |root| IssuesEvent::OpenInIssues {
            root: root.to_path_buf(),
            number,
        });
    }

    /// Tell the shell about task `id`, by `event`.
    pub(super) fn ask_task(
        &mut self,
        id: String,
        event: fn(String) -> IssuesEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tell_later(window, cx, move |_| event(id));
    }

    /// Put task `id` on screen, on the Tasks page.
    pub(super) fn open_task(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.tell_later(window, cx, move |_| IssuesEvent::OpenTask(id));
    }

    /// Put the conversation the agent named `session` on screen.
    pub(super) fn open_session(
        &mut self,
        session: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tell_later(window, cx, move |_| IssuesEvent::OpenConversation(session));
    }

    /// Open `path`, relative to the project on screen, in the editor.
    pub(super) fn open_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.to_string();
        self.ask_later(window, cx, move |ask, root, window, cx| {
            ask(&Request::OpenFile(&root.join(&path)), window, cx)
        });
    }
}
