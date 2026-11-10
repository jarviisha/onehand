//! Each project's check command field, as the shell keeps it: made for every
//! project that has none yet, written to its project and saved as it is
//! typed, and drawn on the project's page beside *Run check*.

use super::Shell;
use gpui::{AppContext as _, Context, Window};
use gpui_component::input::{InputEvent, InputState};

impl Shell {
    /// Hand the project page the check command field of `root`, made first
    /// if it is not yet; none for a project on a host (`None`), where no
    /// check can run.
    pub(super) fn show_check_input(
        &mut self,
        root: Option<&std::path::Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.make_check_inputs(window, cx);
        let input = root.and_then(|root| self.check_inputs.get(root).cloned());
        self.chat
            .update(cx, |pane, cx| pane.set_check_input(input, cx));
    }

    /// A check command field for every project that has none yet. Each edit
    /// is written to its project and saved, debounced, as the workspace name
    /// is: there is no Save button beside it.
    fn make_check_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                move |shell: &mut Self, state, event: &InputEvent, window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    // Put back, field and all, if its write fails; *Run check*
                    // comes and goes with the command.
                    let typed = state.read(cx).value().to_string();
                    let fx = shell.window.workspace.set_check(&at, &typed);
                    shell.carry_workspace(fx, window, cx);
                },
            )
            .detach();
            self.check_inputs.insert(path, input);
        }
    }
}
