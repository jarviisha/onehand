//! A project's build hosts, as the shell changes them: the form that adds
//! or changes one, the question before one goes, and the page kept told.

use super::Shell;
use super::confirm::Confirm;
use gpui::{AppContext as _, Context, Entity, Focusable as _, Window};
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

    /// The project on screen, when its folder is on this machine.
    fn build_root(&self) -> Option<PathBuf> {
        self.local_root(self.window.workspace.active_root)
            .map(Path::to_path_buf)
    }

    /// Hand the project page the build hosts of `root`: none for a project
    /// that is not on this machine, which has none.
    pub(super) fn show_build_hosts(&mut self, root: Option<&Path>, cx: &mut Context<Self>) {
        let builds = root.map(|root| self.builds_of(root)).unwrap_or_default();
        self.chat
            .update(cx, |pane, cx| pane.set_build_hosts(builds, cx));
    }

    /// Open the form on the project on screen: on its build host at
    /// `editing`, or empty to add one.
    pub(crate) fn open_build_host_form(
        &mut self,
        editing: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.build_root() else {
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

    /// *Add* or *Save*: read the form as a build host, and make it one of the
    /// project's, in place of the one being changed. What refuses it is said
    /// in the form, which stays.
    pub(crate) fn save_build_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.build_host_draft.as_ref() else {
            return;
        };
        let (root, editing) = (draft.root.clone(), draft.editing);
        let read = BuildHost::parse(
            &draft.host.read(cx).value(),
            &draft.artifacts.read(cx).value(),
            &draft.into.read(cx).value(),
        )
        .and_then(|entry| {
            match self
                .window
                .workspace
                .build_host_refusal(&root, &entry, editing)
            {
                Some(why) => Err(why),
                None => Ok(entry),
            }
        });
        let entry = match read {
            Ok(entry) => entry,
            Err(why) => {
                if let Some(draft) = self.build_host_draft.as_mut() {
                    draft.error = Some(why);
                }
                cx.notify();
                return;
            }
        };
        let mut builds = self.builds_of(&root);
        match editing.and_then(|at| builds.get_mut(at)) {
            Some(held) => *held = entry,
            None => builds.push(entry),
        }
        self.build_host_draft = None;
        self.set_build_hosts(&root, builds, window, cx);
    }

    /// Ask before the build host at `at` of the project on screen goes.
    pub(crate) fn confirm_remove_build_host(
        &mut self,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.build_root() else {
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
        self.confirm(ask, window, cx, move |shell, window, cx| {
            let mut builds = shell.builds_of(&root);
            builds.retain(|held| *held != entry);
            shell.set_build_hosts(&root, builds, window, cx);
        });
    }

    /// Make `builds` the project's build hosts, and tell its page.
    fn set_build_hosts(
        &mut self,
        root: &Path,
        builds: Vec<BuildHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fx = self.window.workspace.set_builds(root, builds);
        self.carry_workspace(fx, window, cx);
        self.show_build_hosts(Some(root), cx);
        cx.notify();
    }
}
