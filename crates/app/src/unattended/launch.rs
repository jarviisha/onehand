use super::report::tell_issue;
use super::{label, with};
use crate::state::Shared;
use gpui::App;
use onehand_core::task::{Source, Task};
use onehand_core::unattended::{self as core, Claimed, IssueSource, Unstarted, Work};
use onehand_core::workflow::{Setup, Template};

mod answer;
mod pick;

pub use answer::{ReviewRead, read_review, start_answer};
use pick::warn;
pub use pick::{look_now, start_picked};

/// A claim that came back to the app: what core claimed, and the window a
/// person picked it in, rather than the search finding it. Such a run is
/// placed from there and its project stays when it ends — somebody asked
/// for it — so a card it parks is theirs.
pub(super) struct Landed {
    pub(super) claimed: Claimed,
    picked_in: Option<gpui::AnyWindowHandle>,
}

impl Landed {
    /// `claimed`, picked in `window` when a person did.
    pub(super) fn new(claimed: Claimed, picked_in: Option<gpui::AnyWindowHandle>) -> Self {
        Self { claimed, picked_in }
    }

    /// Whether a person picked this run rather than the search finding it.
    fn picked_by_hand(&self) -> bool {
        self.picked_in.is_some()
    }
}

/// Every issue task with a branch of its own, oldest first.
pub(super) fn earlier(cx: &App) -> Vec<core::Earlier> {
    crate::task::each(cx, |task, working| {
        let issue = task.issue()?;
        Some(core::Earlier {
            task: task.id.clone(),
            tracker: issue.tracker.clone(),
            number: issue.number,
            dir: task.setup.dir.clone(),
            branch: task.setup.branch.clone()?,
            working,
            answers_reviews: task.answers_reviews(),
        })
    })
}

/// The claim came back: start the session, or say why not.
///
/// `held` is why nothing was started where something was looked for, when
/// that is so: a search somebody asked for says it rather than that there was
/// nothing to find.
pub(super) fn landed(
    begun: Option<Result<Landed, Unstarted>>,
    asked_from: Option<gpui::AnyWindowHandle>,
    held: Option<String>,
    cx: &mut App,
) {
    with(cx, |u| u.claiming = false);
    cx.refresh_windows();
    let unstarted = match begun {
        None => {
            // A search somebody asked for that found nothing says so; one the
            // tick ran says nothing, as a quiet half hour always has.
            if let Some(window) = asked_from {
                let label = label(cx);
                let said = held.unwrap_or_else(|| {
                    format!(
                        "No open issue of yours labelled `{label}` in the projects switched on."
                    )
                });
                warn(window, said, cx);
            }
            return;
        }
        Some(Err(unstarted)) => unstarted,
        Some(Ok(landed)) => match start(landed, cx) {
            // One claim per search, so a slot still free after it is filled by
            // looking again now rather than at the next tick: with several
            // runs at once, filling them one interval apart left them empty
            // for most of an hour. The look stops itself at the cap.
            Ok(()) => return cx.defer(|cx| super::tick(None, cx)),
            Err(unstarted) => unstarted,
        },
    };
    let said = core::could_not_start(&unstarted.why);
    cx.background_executor()
        .spawn(
            async move { tell_issue(&unstarted.tracker, &unstarted.repo, unstarted.number, &said) },
        )
        .detach();
}

/// Keep the claimed issue as a task of its workflow, then ask for
/// its place in the window holding its project: the one it was picked in
/// when that one does, so its session comes up in front of the person who
/// asked for it.
fn start(landed: Landed, cx: &mut App) -> Result<(), Unstarted> {
    let picked = landed.picked_by_hand();
    let Landed { claimed, picked_in } = landed;
    let Some(timeout) = with(cx, |u| u.timeout.clone()) else {
        return Err(Unstarted::of(claimed, "unattended runs are off"));
    };
    let mode = super::run_mode(cx);
    let agent = super::run_agent(cx);
    let holding: Vec<_> = Shared::global(cx)
        .windows
        .iter()
        .filter(|w| {
            w.shell
                .upgrade()
                .is_some_and(|s| s.read(cx).holds_root(&claimed.repo))
        })
        .map(|w| (w.handle, w.shell.clone()))
        .collect();
    let Some((window, shell)) = holding
        .iter()
        .find(|(handle, _)| Some(*handle) == picked_in)
        .or(holding.first())
        .cloned()
    else {
        return Err(Unstarted::of(
            claimed,
            "the project was closed before the run could start",
        ));
    };
    let Some(shell) = shell.upgrade() else {
        return Err(Unstarted::of(claimed, "the project's window was closed"));
    };
    let (branch, base, dir) = match claimed.work.clone() {
        Work::Cut { branch, base, dir } => (branch, base, dir),
        // A review is answered on the task's own snapshot, as any retry is:
        // the configured workflow is for new tasks only.
        Work::Review { task, note } => {
            return answer_review(claimed, task, note, window, &shell, cx);
        }
    };
    let template = match workflow(&claimed.workflow, &timeout, cx) {
        Ok(template) => template,
        Err(why) => return Err(Unstarted::of(claimed, &why)),
    };
    let check = shell.read(cx).check_of(&claimed.repo);
    if let Some(why) = core::check_missing(&template, check.is_some()) {
        return Err(Unstarted::of(claimed, &why));
    }
    let setup = Setup {
        repo: claimed.repo.clone(),
        dir,
        branch: Some(branch),
        agent,
        check,
        // Left empty, the agent stays in the mode it starts in, which asks
        // more rather than less.
        mode: onehand_core::config::mode_set(&mode),
        forge: claimed.forge.map(|forge| forge.name().to_string()),
    };
    let brief = core::brief_for(&claimed.tracker, &claimed.issue, &claimed.instructions);
    let task_id = onehand_core::task::new_id();
    let mut task = Task::new(task_id.clone(), template, brief, setup);
    task.source = Source::Issue(IssueSource {
        tracker: claimed.tracker.to_ref(),
        number: claimed.issue.number,
        forge_ref: claimed.issue.forge_ref().map(str::to_string),
        forge: claimed.forge.map(|forge| forge.name().to_string()),
        base,
        picked,
        unsent: Vec::new(),
        notes: claimed.notes,
    });
    // Counted against the cap until it has asked for its place.
    with(cx, |u| u.starting.insert(task_id.clone()));
    crate::task::add(task, cx);
    let asked = {
        let task_id = task_id.clone();
        window.update(cx, |_, window, cx| {
            shell.update(cx, |_, cx| crate::task::request(task_id, window, cx))
        })
    };
    if asked.is_err() {
        super::placed(&task_id, cx);
    }
    Ok(())
}

/// Retry issue task `task` to answer the review on its pull request, telling
/// its new run `note`, from the step its status checks send back to; then ask
/// for its place in `window`.
fn answer_review(
    claimed: Claimed,
    task: String,
    note: String,
    window: gpui::AnyWindowHandle,
    shell: &gpui::Entity<crate::shell::Shell>,
    cx: &mut App,
) -> Result<(), Unstarted> {
    let Some(last) = crate::task::task(&task, cx).and_then(|t| t.runs.last().cloned()) else {
        return Err(Unstarted::of(
            claimed,
            "the task of its pull request is gone",
        ));
    };
    let from = last.template.repair_step().map(str::to_string);
    if !crate::task::retry(&task, last.template, from.as_deref(), Some(note), cx) {
        return Err(Unstarted::of(claimed, "a run is still working on it"));
    }
    with(cx, |u| u.starting.insert(task.clone()));
    let asked = window.update(cx, |_, window, cx| {
        shell.update(cx, |_, cx| crate::task::request(task.clone(), window, cx))
    });
    if asked.is_err() {
        super::placed(&task, cx);
    }
    Ok(())
}

/// The workflow `id` as a run of it starts: its timeout put to the config's,
/// and fit for an issue, or why it cannot run.
pub(super) fn workflow(id: &str, timeout: &str, cx: &App) -> Result<Template, String> {
    found(id, timeout, cx).and_then(core::fit_for_issue)
}

/// The workflow `id` with its timeout put to `timeout`, not yet judged, or
/// why there is none.
pub(super) fn found(id: &str, timeout: &str, cx: &App) -> Result<Template, String> {
    let mut template = crate::workflow::templates(cx)
        .into_iter()
        .filter_map(|entry| entry.template.ok())
        .find(|template| template.id == id)
        .ok_or_else(|| format!("there is no workflow `{id}`"))?;
    template.timeout = timeout.to_string();
    Ok(template)
}
