//! A project's routed commands, as the shell changes them: the form that
//! adds or changes one, the question before one goes, and the commands the
//! project's folder suggests, read off the UI loop.

use super::Shell;
use super::confirm::Confirm;
use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Focusable as _, Window};
use gpui_component::input::{InputState, TextareaState};
use onehand_core::workspace::RoutedCommand;
use std::path::{Path, PathBuf};

/// The form's fields, for the project at `root`.
pub(crate) struct RouteDraft {
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    /// The command being changed, by its place; none for one being added.
    pub(crate) editing: Option<usize>,
    pub(crate) words: Entity<InputState>,
    /// One path a line.
    pub(crate) artifacts: Entity<TextareaState>,
    /// What the last *Add* or *Save* ran into, until the next one.
    pub(crate) error: Option<String>,
}

/// The commands a folder holding `names` at its top suggests routing.
fn suggested<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'static str> {
    let (mut cargo, mut dotnet) = (false, false);
    for name in names {
        cargo |= name == "Cargo.toml";
        dotnet |= name.ends_with(".sln") || name.ends_with(".csproj");
    }
    let mut found = Vec::new();
    if cargo {
        found.extend(["cargo build", "cargo test", "cargo clippy"]);
    }
    if dotnet {
        found.extend(["dotnet build", "dotnet test"]);
    }
    found
}

/// What the folder at `root` suggests, read from its top; blocking.
fn suggested_in(root: &Path) -> Vec<&'static str> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    suggested(names.iter().map(String::as_str))
}

impl Shell {
    pub(crate) fn route_draft(&self) -> Option<&RouteDraft> {
        self.route_draft.as_ref()
    }

    /// The commands the project at `root` routes, as its project holds them.
    pub(crate) fn routes_of(&self, root: &Path, cx: &gpui::App) -> Vec<RoutedCommand> {
        self.project_of_row(root)
            .and_then(|id| crate::state::Shared::global(cx).projects.get(&id))
            .map(|project| project.routes.clone())
            .unwrap_or_default()
    }

    /// What the folder at `root` suggests routing, once read; nothing until.
    pub(crate) fn route_suggestions(&self, root: &Path) -> &[&'static str] {
        self.route_suggestions.get(root).map_or(&[], Vec::as_slice)
    }

    /// Read what the project the Projects page shows suggests, off the UI
    /// loop, once a folder: what sits at a project's top seldom changes.
    pub(super) fn read_route_suggestions(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.settings_project(cx) else {
            return;
        };
        if self.route_suggestions.contains_key(&root) {
            return;
        }
        // Held empty while it is read, so a second ask reads nothing more.
        self.route_suggestions.insert(root.clone(), Vec::new());
        let reading = cx.background_executor().spawn({
            let root = root.clone();
            async move { suggested_in(&root) }
        });
        cx.spawn(async move |shell, cx| {
            let found = reading.await;
            let _ = shell.update(cx, |shell: &mut Self, cx| {
                shell.route_suggestions.insert(root, found);
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the form on the project at `root`: on its routed command at
    /// `editing`, or empty to add one.
    pub(crate) fn open_route_form(
        &mut self,
        root: PathBuf,
        editing: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.named_build_root(root) else {
            return;
        };
        if self.route_draft.is_some() {
            return;
        }
        let held = match editing {
            Some(at) => match self.routes_of(&root, cx).get(at) {
                Some(route) => Some(route.clone()),
                None => return,
            },
            None => None,
        };
        let label = self
            .root_index(&root)
            .map(|at| self.window.workspace.roots[at].label.clone())
            .unwrap_or_default();
        let (words, artifacts) = match &held {
            Some(route) => (
                route.words.clone(),
                route
                    .artifacts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            None => Default::default(),
        };
        self.remember_focus(window, cx);
        let words = cx.new(|cx| InputState::new(window, cx).default_value(words));
        let artifacts = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 10)
                .default_value(artifacts)
        });
        words.focus_handle(cx).focus(window, cx);
        self.route_draft = Some(RouteDraft {
            root,
            label,
            editing,
            words,
            artifacts,
            error: None,
        });
        cx.notify();
    }

    /// Put a suggested command into the form's *Command* field.
    pub(crate) fn pick_route_suggestion(
        &mut self,
        words: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(draft) = self.route_draft.as_mut() {
            draft.error = None;
            draft
                .words
                .update(cx, |input, cx| input.set_value(words, window, cx));
            cx.notify();
        }
    }

    pub(crate) fn cancel_route(&mut self, cx: &mut Context<Self>) {
        if self.route_draft.take().is_some() {
            cx.notify();
        }
    }

    /// *Add* or *Save*: read the form as a routed command, and ask the drive
    /// to make it one of the project's, in place of the one being changed.
    pub(crate) fn save_route(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.route_draft.as_ref() else {
            return;
        };
        let (root, editing) = (draft.root.clone(), draft.editing);
        let read = RoutedCommand::parse(
            &draft.words.read(cx).value(),
            &draft.artifacts.read(cx).value(),
        );
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        // What refuses it is the drive's to say, and said in the form, which
        // stays.
        let changed = read.and_then(|route| {
            cx.update_global::<crate::state::Shared, _>(|shared, _| {
                shared.projects.put_route(&id, route, editing)
            })
        });
        match changed {
            Ok(fx) => {
                self.route_draft = None;
                crate::projects::carry(fx, cx);
            }
            Err(why) => {
                if let Some(draft) = self.route_draft.as_mut() {
                    draft.error = Some(why);
                }
            }
        }
        cx.notify();
    }

    /// Ask before the routed command at `at` of the project at `root` goes.
    pub(crate) fn confirm_remove_route(
        &mut self,
        root: PathBuf,
        at: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.named_build_root(root) else {
            return;
        };
        let Some(route) = self.routes_of(&root, cx).get(at).cloned() else {
            return;
        };
        let Some(id) = self.project_of_row(&root) else {
            return;
        };
        let ask = Confirm {
            id: "remove-route",
            title: format!("Stop routing {}?", route.words).into(),
            description: "It leaves the project's routed commands. An agent's next run of it \
                          runs on this machine."
                .into(),
            act: "Remove",
            ..Default::default()
        };
        // By the command, not by its place: the list may change while the
        // question is open.
        self.confirm(ask, window, cx, move |_, _, cx| {
            let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
                shared.projects.remove_route(&id, &route)
            });
            crate::projects::carry(fx, cx);
        });
    }
}

#[cfg(test)]
mod tests;
