//! What the view asks of the app: a session on an issue, a workflow run, a
//! task, a conversation or a file put on screen. Each is put once the view
//! is no longer being updated.

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
        self.ask_later(window, cx, move |ask, root, window, cx| {
            let request = Request::WorkIssueHere {
                root,
                number,
                prompt: &prompt,
            };
            ask(&request, window, cx)
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
        self.ask_later(window, cx, move |ask, root, window, cx| {
            ask(&Request::RunIssueWorkflow { root, number }, window, cx)
        });
    }

    /// Put issue `number` of the project on screen on the Issues page.
    pub(super) fn open_in_issues(
        &mut self,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask_later(window, cx, move |ask, root, window, cx| {
            ask(&Request::OpenInIssues { root, number }, window, cx)
        });
    }

    /// Ask the app about task `id`, by `request`.
    pub(super) fn ask_task(
        &mut self,
        id: String,
        request: fn(&str) -> Request<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(&request(&id), window, cx)
        });
    }

    /// Put task `id` on screen, on the Tasks page.
    pub(super) fn open_task(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(&Request::OpenTask(&id), window, cx)
        });
    }

    /// Put the conversation the agent named `session` on screen.
    pub(super) fn open_session(
        &mut self,
        session: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask_later(window, cx, move |ask, _, window, cx| {
            ask(&Request::OpenConversation(&session), window, cx)
        });
    }

    /// Open `path`, relative to the project on screen, in the editor.
    pub(super) fn open_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let path = path.to_string();
        self.ask_later(window, cx, move |ask, root, window, cx| {
            ask(&Request::OpenFile(&root.join(&path)), window, cx)
        });
    }
}
