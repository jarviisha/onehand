//! An issue picked by hand, and a search asked for now: what a person in a
//! window starts, and is told about there.

use super::super::{roots, world};
use gpui::App;
use onehand_core::unattended::drive::Pick;
use onehand_core::unattended::{IssueRow, Tracker};
use std::path::PathBuf;

/// Work `row`, picked by hand from a project's open issues, now, with the
/// workflow `workflow`, adding `instructions` to what its brief asks; or why
/// it is refused before anything is claimed. After the claim, what stops it
/// is said in the window it was picked from and on the issue as well.
/// `has_check` is whether the project has a check command, told by the
/// window it was picked in, which is being updated and so cannot be asked.
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
    let pick = Pick {
        repo,
        tracker,
        row,
        workflow,
        instructions,
        has_check,
    };
    let world = world(cx);
    crate::task::with_unattended(cx, |u, seen| match u.pick(pick, window, seen, &world) {
        Ok(fx) => (Some(Ok(())), fx),
        Err(why) => (Some(Err(why)), Vec::new()),
    })
    .unwrap_or_else(|| Err("Unattended runs are not set up.".to_string()))
}

/// Look for a labelled issue now, rather than at the next tick, and say what
/// came of it in `window`: the person pressed a button and is waiting to hear.
pub fn look_now(window: gpui::AnyWindowHandle, cx: &mut App) {
    let (roots, world) = (roots(cx), world(cx));
    crate::task::with_unattended(cx, |u, seen| ((), u.look_now(window, seen, roots, &world)));
}
