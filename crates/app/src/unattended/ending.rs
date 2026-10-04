use super::with;
use crate::chat::session::{ChatSession, note};
use crate::state::Shared;
use gpui::{App, Entity};
use onehand_core::config::AgentSpec;
use onehand_core::connector::Connector;
use onehand_core::task::Task;
use onehand_core::unattended::{self as core, IssueSource, Tracker, TrackerRef, Verdict};
use onehand_core::workflow::{Outcome, Stop};
use onehand_core::worktree;
use std::path::Path;

/// The agent a run starts, with every build it makes pointed at one shared
/// directory.
///
/// Through `env` rather than a new field threaded down to the process spawn,
/// because the adapter is the one process a run gets to set anything on and
/// everything the agent runs inherits from it.
// ponytail: `env` is POSIX; a Windows build would need the variable threaded
// through the ACP spawn instead.
pub(crate) fn spec_for(agent: Option<&str>, cx: &App) -> Option<AgentSpec> {
    let agents = &Shared::global(cx).agents;
    let base = agent
        .and_then(|name| agents.iter().find(|spec| spec.name == name))
        .or_else(|| agents.first())?
        .clone();
    let Some(target) = core::target_dir() else {
        return Some(base);
    };
    let mut args = vec![
        format!("CARGO_TARGET_DIR={}", target.display()),
        base.command,
    ];
    args.extend(base.args);
    Some(AgentSpec {
        name: base.name,
        command: "env".to_string(),
        args,
    })
}

/// The agent does not offer the mode runs start in: every later run would
/// fail the same way, each on a fresh issue, so runs stop here until the
/// config is fixed and onehand restarted.
pub(crate) fn refuse_mode(why: &str, cx: &mut App) {
    let why = format!(
        "{why}. Unattended runs are paused until unattended.mode is fixed and onehand \
         restarted."
    );
    eprintln!("onehand: {why}");
    with(cx, |u| u.mode_refused = Some(why));
    cx.refresh_windows();
}

/// The agent of issue task `task` is up in `session`: say on an issue kept in
/// onehand which conversation works it, so the issue can open it. An issue
/// that lives only on the forge has nowhere to keep it.
pub(crate) fn started(task: &Task, session: &Entity<ChatSession>, cx: &mut App) {
    let Some(issue) = task.issue() else {
        return;
    };
    let (TrackerRef::Local { file } | TrackerRef::Synced { file, .. }) = issue.tracker.clone()
    else {
        return;
    };
    let Some(id) = session.read(cx).chat.session_id.clone() else {
        return;
    };
    let (number, dir) = (issue.number, task.setup.dir.clone());
    let branch = task.setup.branch.clone().unwrap_or_default();
    cx.background_executor()
        .spawn(async move {
            let said = format!(
                "Taken up by an unattended run in {}, on branch {branch}",
                dir.display()
            );
            if let Err(why) = onehand_core::issues::taken_up_blocking(&file, number, &said, id) {
                eprintln!("onehand: could not note the session on issue: {why}");
            }
        })
        .detach();
}

/// The lines a run's transcript opens with: which issue, how it was chosen,
/// and where the work is happening. Without them it would start at a prompt
/// nobody on screen typed.
///
/// **Short lines, one fact each.** A remark in the transcript is one line down
/// the middle of the column and is cut where the column ends.
pub(crate) fn opening(task: &Task, session: &Entity<ChatSession>, cx: &mut App) {
    let Some(issue) = task.issue() else {
        return;
    };
    let folder = task.setup.dir.file_name().map_or_else(
        || task.setup.dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let lines = [
        format!(
            "Unattended run on issue {}, {}",
            issue.shown(),
            match issue.picked {
                true => "picked by hand",
                false => "found by its label",
            }
        ),
        format!(
            "Branch {} from {}",
            task.setup.branch.as_deref().unwrap_or("?"),
            issue.base
        ),
        format!("Working in {folder}"),
    ];
    for line in lines {
        note(session, line, cx);
    }
}

/// Issue task `id`'s run has ended and given its place up: drop its project
/// unless a person has it, then tell the issue what the run left.
///
/// **The report is kept before it is sent.** It is put in the task's file
/// first and taken out only once the issue has it, so a forge that cannot be
/// reached, or a quit, never loses it: the next tick, or the next start,
/// sends it again.
pub(crate) fn ended(id: &str, cx: &mut App) {
    let Some(task) = crate::task::task(id, cx) else {
        return;
    };
    let (Some(issue), Some(run)) = (task.issue().cloned(), task.runs.last()) else {
        return;
    };
    let Some(outcome) = run.outcome.clone() else {
        return;
    };
    // A run picked by hand stays where the person watching it can read how it
    // ended, and a taken-over one is a person's now; both are kept for good.
    let keep = issue.picked || outcome == Outcome::Stopped(Stop::TakenOver);
    teardown(&task.setup.dir, keep, cx);
    let ended_on = run.visits().last().and_then(|visit| visit.output.clone());
    let forge = issue.forge.as_deref().and_then(connector_named);
    let (repo, dir) = (task.setup.repo.clone(), task.setup.dir.clone());
    let branch = task.setup.branch.clone().unwrap_or_default();
    let id = id.to_string();
    cx.spawn(async move |cx| {
        let said = cx
            .background_executor()
            .spawn(async move {
                let found = verdict_blocking(forge, &repo, &dir, &branch, &issue.base);
                core::report(&outcome, &found, &branch, ended_on.as_deref())
            })
            .await;
        cx.update(|cx| {
            crate::task::update_issue(&id, cx, |issue| issue.unsent.push(said));
            deliver(id, cx);
        });
    })
    .detach();
}

/// Keep or drop the project a run worked in, in every window holding it. Only
/// a project the run added itself is dropped: one a person had open stays.
fn teardown(dir: &Path, keep: bool, cx: &mut App) {
    let holding: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .map(|w| (w.handle, w.shell.clone()))
        .collect();
    for (handle, shell) in holding {
        let Some(shell) = shell.upgrade() else {
            continue;
        };
        let _ = handle.update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| match keep {
                true => shell.adopt_unattended(dir, window, cx),
                false => shell.end_unattended(dir, window, cx),
            })
        });
    }
}

/// Send what issue task `id` has not told its issue yet, oldest first,
/// dropping each only once it has landed. The first that fails stops the
/// rest, so the issue never hears them out of order.
pub(crate) fn deliver(id: String, cx: &mut App) {
    let Some(task) = crate::task::task(&id, cx) else {
        return;
    };
    let Some(issue) = task.issue().filter(|issue| !issue.unsent.is_empty()) else {
        return;
    };
    let fresh = with(cx, |u| u.delivering.insert(id.clone())).unwrap_or(false);
    if !fresh {
        return;
    }
    let Some(tracker) = issue.tracker.resolve(crate::plugins::connectors()) else {
        eprintln!(
            "onehand: issue {} cannot be told how its run ended: its tracker is not available",
            issue.shown()
        );
        with(cx, |u| u.delivering.remove(&id));
        return;
    };
    let (unsent, number, repo) = (issue.unsent.clone(), issue.number, task.setup.repo.clone());
    cx.spawn(async move |cx| {
        let landed = cx
            .background_executor()
            .spawn(async move { send_blocking(&tracker, &repo, number, &unsent) })
            .await;
        cx.update(|cx| {
            crate::task::update_issue(&id, cx, |issue: &mut IssueSource| {
                issue.unsent.drain(..landed.min(issue.unsent.len()));
            });
            with(cx, |u| u.delivering.remove(&id));
        });
    })
    .detach();
}

/// Send every report not yet delivered, of every issue task.
pub(crate) fn deliver_all(cx: &mut App) {
    for id in crate::task::undelivered(cx) {
        deliver(id, cx);
    }
}

/// Leave each of `unsent` on issue `number`, in order: how many landed.
fn send_blocking(tracker: &Tracker, repo: &Path, number: u64, unsent: &[String]) -> usize {
    let mut landed = 0;
    for body in unsent {
        match tracker.comment_blocking(repo, number, body) {
            Ok(()) => landed += 1,
            Err(why) => {
                eprintln!("onehand: could not tell issue #{number} how its run ended: {why}");
                break;
            }
        }
    }
    landed
}

/// Leave `body` on issue `number`, saying on stderr if that failed: a run
/// that never started has no task to keep it in.
pub(super) fn tell_issue(tracker: &Tracker, repo: &Path, number: u64, body: &str) {
    if let Err(why) = tracker.comment_blocking(repo, number, body) {
        eprintln!("onehand: could not comment on issue #{number}: {why}");
    }
}

/// The connector named `name`, if it is one of this build's.
fn connector_named(name: &str) -> Option<&'static dyn Connector> {
    crate::plugins::connectors()
        .iter()
        .copied()
        .find(|c| c.name() == name)
}

/// What the run left: the pull request on its branch where the project has a
/// forge and one was opened, otherwise its commits past where it was cut.
/// Blocking.
fn verdict_blocking(
    forge: Option<&'static dyn Connector>,
    repo: &Path,
    dir: &Path,
    branch: &str,
    base: &str,
) -> Result<Verdict, String> {
    if let Some(forge) = forge
        && let Some(url) = forge.pull_request_for_blocking(repo, branch)?
    {
        return Ok(Verdict::PullRequest(url));
    }
    worktree::commits_since_blocking(dir, base).map(Verdict::Commits)
}
