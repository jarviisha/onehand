//! Each project's check command field, as the shell keeps it: made for every
//! project that has none yet, written to its project and saved as it is
//! typed, and drawn on Settings' Projects page.

use super::Shell;
use gpui::{AppContext as _, BorrowAppContext as _, Context, Window};
use gpui_component::input::{InputEvent, InputState};

impl Shell {
    /// The check command field of the project at `root`, once made.
    pub(crate) fn check_input(
        &self,
        root: &std::path::Path,
    ) -> Option<gpui::Entity<gpui_component::input::InputState>> {
        self.check_inputs.get(root).cloned()
    }

    /// A check command field for every project that has none yet. Each edit
    /// is written to its project and saved, debounced, as the workspace name
    /// is: there is no Save button beside it.
    pub(super) fn make_check_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let roots: Vec<_> = self
            .window
            .workspace
            .roots
            .iter()
            .filter(|root| {
                !root.transient && root.runs_here() && !self.check_inputs.contains_key(&root.path)
            })
            .map(|root| (root.path.clone(), root.check.clone().unwrap_or_default()))
            .collect();
        for (path, check) in roots {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("None: a command step that names none cannot run here")
                    .default_value(check)
            });
            let at = path.clone();
            cx.subscribe_in(
                &input,
                window,
                move |shell: &mut Self, state, event: &InputEvent, _, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    // Put back, field and all, if its write fails; *Run check*
                    // comes and goes with the command.
                    let typed = state.read(cx).value().to_string();
                    let Some(id) = shell.project_of_row(&at) else {
                        return;
                    };
                    let fx = cx.update_global::<crate::state::Shared, _>(|shared, _| {
                        shared.projects.set_check(&id, &typed)
                    });
                    crate::projects::carry(fx, cx);
                },
            )
            .detach();
            self.check_inputs.insert(path, input);
        }
    }
}
