//! *Answer the pull request review*, pressed on an issue or its task: a
//! second door to the path a re-added trigger label runs, never a second
//! path. What would refuse it is read first (`unattended::review`, asked by
//! the tasks' drive) and judged by the preflight before anything is claimed;
//! then the issue is picked as a person picks one, which finds the open pull
//! request and answers it.

use super::pick::start_picked;
use gpui::{AnyWindowHandle, App};
use onehand_core::unattended::review::ReviewRead;

/// Answer the review `read` found nothing blocking, the project's
/// `has_check` told by the window it was pressed in: the issue is picked as a
/// person picks one. Why it did not start, when it did not.
pub fn start_answer(
    read: ReviewRead,
    has_check: bool,
    window: AnyWindowHandle,
    cx: &mut App,
) -> Result<(), String> {
    let (repo, tracker, row, workflow) = read
        .start
        .ok_or("the issue is closed, or not among the open issues a pick reads")?;
    start_picked(
        repo,
        tracker,
        row,
        workflow,
        String::new(),
        has_check,
        window,
        cx,
    )
}
