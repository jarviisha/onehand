//! Starting a task off screen for a tool or a schedule: the workflow it
//! names, the preflight the launcher asks, the worktree its place needs, and
//! where the task landed.

use crate::task::Placed;
use gpui::App;
use onehand_core::workflow::{Brief, Place, Setup, Template};
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

/// What a start off screen carries beside its workflow and brief: who asked,
/// what its sessions may use, and the agent and mode it runs with (none: the
/// first agent, and its own mode).
pub(crate) struct Started {
    pub(crate) origin: onehand_core::task::Origin,
    pub(crate) grant: onehand_core::task::Grant,
    pub(crate) agent: Option<String>,
    pub(crate) mode: Option<String>,
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
    let started = Started {
        origin: onehand_core::task::Origin::Tool,
        grant: onehand_core::task::Grant::default(),
        agent: None,
        mode: None,
    };
    match start_task(window, shell, root, template, brief, started, cx).await? {
        (id, Placed::Started) => Ok(format!("Task {id} started: {title}")),
        (id, Placed::Queued(said)) => Ok(format!("Task {id} queued: {said}")),
        (_, Placed::Refused(why)) => Err(why),
    }
}

/// Start a task of `template` on `root` with `brief`, as the launcher does
/// and off screen, once what blocks the launcher's Run blocks nothing:
/// on the checkout, or on a worktree cut for it first. Answers the task's id
/// and where it landed.
pub(crate) async fn start_task(
    window: gpui::AnyWindowHandle,
    shell: gpui::Entity<crate::shell::Shell>,
    root: PathBuf,
    template: Template,
    brief: Brief,
    started: Started,
    cx: &mut gpui::AsyncApp,
) -> Result<(String, Placed), String> {
    let Started {
        origin,
        grant,
        agent,
        mode,
    } = started;
    let (blocks, check) = cx.update(|cx| {
        let shell = shell.read(cx);
        let found = shell.run_preflight(
            &root,
            Ok(template.clone()),
            None,
            agent.as_deref(),
            mode.as_deref(),
            cx,
        );
        let blocks: Vec<String> = found
            .into_iter()
            .filter(|f| f.blocks)
            .map(|f| f.text)
            .collect();
        (blocks, shell.check_of(&root))
    });
    if !blocks.is_empty() {
        return Err(blocks.join("; "));
    }
    let mut setup = Setup {
        repo: root.clone(),
        dir: root.clone(),
        branch: None,
        agent,
        check,
        mode,
        forge: None,
    };
    match template.place {
        Place::Checkout => {}
        Place::Worktree => {
            let title = brief.title.clone();
            let (dir, branch, forge) = cx
                .background_executor()
                .spawn(async move { crate::shell::cut_worktree_blocking(&root, &title) })
                .await?;
            setup = Setup {
                dir,
                branch: Some(branch),
                forge,
                ..setup
            };
        }
    }
    let (id, told) = cx
        .update(|cx| {
            crate::state::with_shell(window, cx, |shell, window, cx| {
                shell.start_workflow_told(setup, template, brief, (origin, grant), window, cx)
            })
        })
        .ok_or("the window closed")?;
    let placed = told
        .await
        .map_err(|_| "the task did not start".to_string())?;
    Ok((id, placed))
}
