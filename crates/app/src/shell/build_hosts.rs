//! A project's build hosts, as the shell changes them: the form that adds
//! or changes one, the question before one goes, and the page kept told.

use super::Shell;
use super::confirm::Confirm;
use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Focusable as _, Window};
use gpui_component::input::{InputState, TextareaState};
use onehand_core::workspace::BuildHost;
use std::path::{Path, PathBuf};

/// The form's fields, for the project at `root`.
pub(crate) struct BuildHostDraft {
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    /// The entry being changed, by its place; none for one being added.
    pub(crate) editing: Option<usize>,
    pub(crate) host: Entity<InputState>,
    /// One path a line.
    pub(crate) artifacts: Entity<TextareaState>,
    pub(crate) into: Entity<InputState>,
    /// What the last *Add* or *Save* ran into, until the next one.
    pub(crate) error: Option<String>,
}

impl Shell {
    pub(crate) fn build_host_draft(&self) -> Option<&BuildHostDraft> {
        self.build_host_draft.as_ref()
    }

    /// `root` when it is one of the window's projects on this machine.
    pub(super) fn named_build_root(&self, root: PathBuf) -> Option<PathBuf> {
        self.local_root(self.root_index(&root)?)
            .filter(|own| *own == root)
            .map(Path::to_path_buf)
    }

    /// Open the form on the project at `root`: on its build host at
    /// `editing`, or empty to add one.
    pub(crate) fn open_build_host_form(
        &mut self,
        root: PathBuf,
        editing: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.named_build_root(root) else {
            return;
        };
        if self.build_host_draft.is_some() {
            return;
        }
        let builds = self.builds_of(&root);
        let held = match editing {
            Some(at) => match builds.get(at) {
                Some(entry) => Some(entry.clone()),
                None => return,
            },
            None => None,
        };
        let label = self
            .root_index(&root)
            .map(|at| self.window.workspace.roots[at].label.clone())
            .unwrap_or_default();
        let (host, artifacts, into) = match &held {
            Some(entry) => (
                entry.host.to_string(),
                entry
                    .artifacts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
                entry
                    .into
                    .as_ref()
                    .map(|into| into.display().to_string())
                    .unwrap_or_default(),
            ),
            None => Default::default(),
        };
        self.remember_focus(window, cx);
        let host = cx.new(|cx| InputState::new(window, cx).default_value(host));
        let artifacts = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 10)
                .default_value(artifacts)
        });
        let into = cx.new(|cx| InputState::new(window, cx).default_value(into));
        host.focus_handle(cx).focus(window, cx);
        self.build_host_draft = Some(BuildHostDraft {
            root,
            label,
            editing,
            host,
            artifacts,
            into,
            error: None,
        });
        cx.notify();
    }

    /// Put a host picked from the form's menu into its field.
    pub(crate) fn pick_build_host(
        &mut self,
        host: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.build_host_draft.as_mut() {
            draft.error = None;
            draft
                .host
                .update(cx, |input, cx| input.set_value(host, window, cx));
            cx.notify();
        }
    }

    pub(crate) fn cancel_build_host(&mut self, cx: &mut Context<Self>) {
        if self.build_host_draft.take().is_some() {
            cx.notify();
        }
    }

    /// *Add* or *Save*: read the form as a build host, and ask the drive to
    /// make it one of the project's, in place of the one being changed.
    pub(crate) fn save_build_host(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.build_host_draft.as_ref() else {
            return;
        };
        let (root, editing) = (draft.root.clone(), draft.editing);
        let read = BuildHost::parse(
            &draft.host.read(cx).value(),
            &draft.artifacts.read(cx).value(),
            &draft.into.read(cx).value(),
        );
        // The drive says whether the project may build there, and makes the
        // change; what refuses it is said in the form, which stays.
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let changed = read.and_then(|entry| {
            cx.update_global::<crate::state::Shared, _>(|shared, _| {
                shared.projects.put_build_host(&id, entry, editing)
            })
        });
        match changed {
            Ok(fx) => {
                self.build_host_draft = None;
                crate::projects::carry(fx, cx);
                cx.notify();
            }
            Err(why) => {
                if let Some(draft) = self.build_host_draft.as_mut() {
                    draft.error = Some(why);
                }
                cx.notify();
            }
        }
    }

    /// Ask before the build host at `at` of the project at `root` goes.
    pub(crate) fn confirm_remove_build_host(
        &mut self,
        root: PathBuf,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.named_build_root(root) else {
            return;
        };
        let Some(entry) = self.builds_of(&root).get(at).cloned() else {
            return;
        };
        let ask = Confirm {
            id: "remove-build-host",
            title: format!("Stop building on {}?", entry.host).into(),
            description: "It leaves the project's build hosts. What it built stays on the host."
                .into(),
            act: "Remove",
            ..Default::default()
        };
        // By the entry, not by its place: the list may change while the
        // question is open.
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        self.confirm(ask, window, cx, move |_, _, cx| {
            let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
                shared.projects.remove_build_host(&id, &entry)
            });
            crate::projects::carry(fx, cx);
        });
    }
}
