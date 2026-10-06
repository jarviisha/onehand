//! An issue picked by hand, and a search asked for now: what a person in a
//! window starts, and is told about there.

use super::super::{connector_for, lacks_check_given, opted_in_roots, tick, why_not, with};
use super::{Unstarted, earlier, landed, prepare_blocking, taking_blocking, trackers_blocking};
use gpui::App;
use onehand_core::issues::template;
use onehand_core::unattended::{self as core, IssueRow, Tracker};
use std::path::{Path, PathBuf};

/// What a person can pick from in a project: its open issues from where each
/// lives, the ones it keeps itself first, and whether the list was cut — plus
/// why the forge's could not be read, when its own could.
pub type Pickable = (Vec<(Tracker, IssueRow)>, bool, Option<String>);

/// Read what a person can pick from in `root`, whose own issues are in
/// `issues` if its workspace keeps any. Blocking.
///
/// A forge that cannot be read is an error only when there is nothing else to
/// show; beside issues of the project's own it is said under the list, so one
/// half being down does not hide the other.
pub fn pickable_blocking(root: &Path, issues: Option<PathBuf>) -> Result<Pickable, String> {
    let (mut rows, mut cut, mut unread) = (Vec::new(), false, None);
    let forge = connector_for(root);
    for tracker in trackers_blocking(issues, forge.as_ref().ok().copied()) {
        match core::open_issues_blocking(&tracker, root) {
            Ok((found, more)) => {
                cut |= more;
                rows.extend(found.into_iter().map(|row| (tracker.clone(), row)));
            }
            // The forge's half being down is said beside the rest; the
            // project's own issues failing to read is the whole answer.
            Err(why) => match tracker {
                Tracker::Forge(_) => unread = Some(why),
                Tracker::Local(_) | Tracker::Synced { .. } => return Err(why),
            },
        }
    }
    // A project no connector serves says why only when there is nothing else
    // to list: beside its own issues, the forge it does not have is no news.
    match (rows.is_empty(), unread, forge) {
        (true, Some(why), _) | (true, None, Err(why)) => Err(why),
        (_, unread, _) => Ok((rows, cut, unread)),
    }
}

/// Issue `number` of the ones `root` keeps itself, in `issues`, to pick, or
/// why it cannot be. Blocking.
///
/// Only the project's own issues are read, never its forge's: the Issues tab
/// numbers the issues it keeps, and a forge's numbers are its own. So the list
/// being cut is that list's, and an issue missing from it is said to be older
/// than the newest it holds, or else one a run may not take.
pub fn pickable_one_blocking(
    root: &Path,
    issues: Option<PathBuf>,
    number: u64,
) -> Result<Pickable, String> {
    let forge = connector_for(root).ok();
    let tracker = trackers_blocking(issues, forge)
        .into_iter()
        .find(|tracker| !matches!(tracker, Tracker::Forge(_)))
        .ok_or("this workspace keeps no issues of its own")?;
    let (found, cut) = core::open_issues_blocking(&tracker, root)?;
    let rows: Vec<_> = found
        .into_iter()
        .filter(|row| row.issue.number == number)
        .map(|row| (tracker.clone(), row))
        .collect();
    match (rows.is_empty(), cut) {
        (false, _) => Ok((rows, false, None)),
        (true, true) => Err(format!(
            "issue {number} is not among the newest {} open issues a pick reads; close \
             some of the newer ones to reach it",
            core::ISSUES_SHOWN
        )),
        (true, false) => Err(format!(
            "issue {number} cannot be worked by a run: it is closed, or it was brought in \
             from a forge the project is no longer kept in step with"
        )),
    }
}

/// Work `row`, picked by hand from a project's open issues, now, with the
/// workflow `workflow`, adding `instructions` to what its brief asks.
///
/// **Refused while the cap is reached**, the rule for picked and found alike —
/// a run waiting on a person does not count — and the refusal names the
/// issues being worked so the person knows what they are waiting on. Anything that stops it before
/// the claim — a claim refused where the issue lives — is said in the window it
/// was picked from; after the claim, on the issue as well. `has_check` is
/// whether the project has a check command, told by the window it was picked
/// in, which is being updated and so cannot be asked.
#[allow(clippy::too_many_arguments)]
pub fn start_picked(
    repo: PathBuf,
    tracker: Tracker,
    row: IssueRow,
    workflow: String,
    instructions: String,
    has_check: bool,
    window: gpui::AnyWindowHandle,
    cx: &mut App,
) -> Result<(), String> {
    let (full, refused) = why_not(cx);
    // Said apart from what stops every run: this is the one workflow picked,
    // on this one project, and another pick may well start.
    let unfit = lacks_check_given(has_check, &workflow, cx);
    let label = with(cx, |u| {
        if let Some(why) = full {
            return Err(why);
        }
        if u.claiming {
            return Err("An unattended run is starting — one at a time.".to_string());
        }
        // Refused before the claim: the run would fail, and the issue would
        // be claimed and commented on for nothing.
        if let Some(why) = refused {
            return Err(format!("Nothing can be started: {why}"));
        }
        if let Some(why) = unfit {
            return Err(format!("Cannot start on this project: {why}"));
        }
        u.claiming = true;
        Ok(u.label.clone())
    })
    .ok_or("Unattended runs are not set up.")??;
    let earlier = earlier(cx);
    let lacks =
        template::lacking(row.issue.body_text(), &template::shipped()).map(|lacks| lacks.note());
    cx.spawn(async move |cx| {
        let number = row.issue.number;
        let named = tracker.named(&row.issue);
        let begun = cx
            .background_executor()
            .spawn({
                let named = named.clone();
                async move {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        // An issue on the forge goes back to that forge; one kept
                        // here goes to whichever forge serves the project, if any.
                        let forge = match &tracker {
                            Tracker::Forge(forge) | Tracker::Synced { forge, .. } => Some(*forge),
                            Tracker::Local(_) => connector_for(&repo).ok(),
                        };
                        // Refused before the claim: claimed twice, it would be
                        // worked twice, on two branches.
                        let taking = taking_blocking(&earlier, &tracker, number, &repo, forge)
                            .map_err(|skip| skip.said(&named))?;
                        core::claim_picked_blocking(
                            &tracker,
                            &repo,
                            &row,
                            &label,
                            taking.answering(),
                        )
                        .map_err(|why| format!("Could not start on issue {named}: {why}"))?;
                        Ok(prepare_blocking(
                            repo,
                            tracker,
                            forge,
                            row.issue,
                            workflow,
                            taking,
                            Some(window),
                        )
                        .map(|claimed| super::Claimed {
                            instructions,
                            // The form said it, but its report is read later,
                            // by whoever judges why the run went wrong.
                            notes: lacks.into_iter().collect(),
                            ..claimed
                        }))
                    }))
                    .unwrap_or_else(|_| {
                        Err("onehand panicked while claiming the issue".to_string())
                    })
                }
            })
            .await;
        cx.update(|cx| {
            let unstarted = match begun {
                // Nothing was claimed, so the issue has nothing to be told; the
                // window it was picked from is where the person is.
                Err(why) => {
                    with(cx, |u| u.claiming = false);
                    warn(window, why, cx);
                    return;
                }
                Ok(begun) => begun,
            };
            if let Err(Unstarted { why, .. }) = &unstarted {
                warn(
                    window,
                    format!("Could not start on issue {named}: {why}"),
                    cx,
                );
            }
            landed(Some(unstarted), None, cx);
        });
    })
    .detach();
    Ok(())
}

/// Say `why` in `window`, as the transient notice it is.
pub(super) fn warn(window: gpui::AnyWindowHandle, why: String, cx: &mut App) {
    use gpui_component::WindowExt as _;
    let _ = window.update(cx, |_, window, cx| {
        window.push_notification(gpui_component::notification::Notification::warning(why), cx);
    });
}

/// Look for a labelled issue now, rather than at the next tick, and say what
/// came of it in `window` — the person pressed a button and is waiting to hear.
/// Every way it can do nothing is said rather than left to look like nothing
/// happened.
pub fn look_now(window: gpui::AnyWindowHandle, cx: &mut App) {
    let why_not = if opted_in_roots(cx).is_empty() {
        Some("No project is switched on for unattended runs.".to_string())
    } else {
        let (full, refused) = why_not(cx);
        with(cx, |u| {
            if full.is_some() {
                full
            } else if u.claiming {
                Some("A run is already starting.".to_string())
            } else {
                u.blocked
                    .clone()
                    .or(refused)
                    .map(|why| format!("Nothing can be picked up: {why}"))
            }
        })
        .flatten()
    };
    match why_not {
        Some(why) => warn(window, why, cx),
        None => tick(Some(window), cx),
    }
}
