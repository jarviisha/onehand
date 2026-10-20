//! An issue picked by hand, and a search asked for now: what a person in a
//! window starts, and is told about there.

use super::super::{connector_for, lacks_check_given, opted_in_roots, tick, why_not, with};
use super::{Landed, earlier, landed};
use gpui::App;
use onehand_core::issues::template;
use onehand_core::unattended::{
    self as core, IssueRow, Tracker, Unstarted, prepare_blocking, taking_blocking,
};
use std::path::PathBuf;

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
                        let lacks = template::lacking(
                            row.issue.body_text(),
                            &template::for_project_blocking(&repo),
                        )
                        .map(|lacks| lacks.note());
                        Ok(
                            prepare_blocking(repo, tracker, forge, row.issue, workflow, taking)
                                .map(|claimed| {
                                    let claimed = core::Claimed {
                                        instructions,
                                        // The form said it, but its report is
                                        // read later, by whoever judges why
                                        // the run went wrong.
                                        notes: lacks.into_iter().collect(),
                                        ..claimed
                                    };
                                    Landed::new(claimed, Some(window))
                                }),
                        )
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
                    cx.refresh_windows();
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
            landed(Some(unstarted), None, None, cx);
        });
    })
    .detach();
    Ok(())
}

/// Say `why` in `window`, as the transient notice it is.
pub(super) fn warn(window: gpui::AnyWindowHandle, why: String, cx: &mut App) {
    use gpui_component::WindowExt as _;
    let _ = window.update(cx, |_, window, cx| {
        window.push_notification(crate::toast::warning(why), cx);
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
        None => {
            tick(Some(window), cx);
            // Its button waits while the look claims what it found.
            cx.refresh_windows();
        }
    }
}
