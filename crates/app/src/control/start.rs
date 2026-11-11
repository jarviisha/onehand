//! Starting a task off screen for a tool or a schedule: the workflow it
//! names, the preflight the launcher asks, and where the task landed.

use crate::task::Placed;
use gpui::App;
use onehand_core::task::{Launch, Starting};
use onehand_core::workflow::{Brief, Template};
use std::path::PathBuf;

/// The library's workflow whose id is `id`, read.
pub(super) fn template(id: &str, cx: &App) -> Result<Template, String> {
    let entries = crate::workflow::templates(cx);
    let entry = entries
        .iter()
        .find(|entry| entry.template.as_ref().is_ok_and(|t| t.id == id))
        .ok_or_else(|| format!("no workflow with id {id}; list_workflows gives them"))?;
    entry.template.clone()
}

/// A tool's start, said as the caller reads it.
pub(super) async fn start(
    window: gpui::AnyWindowHandle,
    shell: gpui::Entity<crate::shell::Shell>,
    root: PathBuf,
    template: Template,
    brief: Brief,
    cx: &mut gpui::AsyncApp,
) -> Result<String, String> {
    let title = brief.title.clone();
    let starting = Starting {
        project: root,
        template,
        brief,
        origin: onehand_core::task::Origin::Tool,
        grant: onehand_core::task::Grant::default(),
        agent: None,
        mode: None,
    };
    match start_task(window, shell, starting, cx).await? {
        (id, Placed::Started) => Ok(format!("Task {id} started: {title}")),
        (id, Placed::Queued(said)) => Ok(format!("Task {id} queued: {said}")),
        (_, Placed::Refused(why)) => Err(why),
    }
}

/// Start `starting` off screen in `window`, with what the launcher's
/// preflight finds of it there, and answer the task's id and where it
/// landed. What blocks it, the worktree it needs and its task are the tasks'
/// drive's.
pub(crate) async fn start_task(
    window: gpui::AnyWindowHandle,
    shell: gpui::Entity<crate::shell::Shell>,
    starting: Starting,
    cx: &mut gpui::AsyncApp,
) -> Result<(String, Placed), String> {
    let id = onehand_core::task::new_id();
    let told = cx.update(|cx| {
        let shell = shell.read(cx);
        let found = shell.run_preflight(
            &starting.project,
            Ok(starting.template.clone()),
            None,
            starting.agent.as_deref(),
            starting.mode.as_deref(),
            cx,
        );
        let check = shell.check_of(&starting.project);
        let first = crate::shell::first_agent(cx);
        let launch = Launch::off_screen(&found, starting, check, first);
        crate::task::start_off_screen(id.clone(), launch, window, cx)
    });
    let placed = told
        .await
        .map_err(|_| "the task did not start".to_string())?;
    Ok((id, placed))
}

#[cfg(test)]
mod tests;
