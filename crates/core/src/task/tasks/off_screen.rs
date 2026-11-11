//! A task a schedule or one of onehand's tools starts, off screen: refused
//! with what blocks it, on a worktree cut for it first when its workflow
//! works on one, its project kept in the window without being put on
//! screen, then kept as a task and asking for its place like any other.
//! Where it landed is said to whoever asked ([`Effect::Told`]).

use super::{Effect, Placed, TasksDrive};
use crate::task::{Launch, Worktree};

/// A worktree cut for a task, or why not.
pub type Cut = Result<Worktree, String>;

/// A start off screen under way: the task it is to be, what it launches,
/// and the window it was asked from.
#[derive(Debug, Clone, PartialEq)]
pub struct OffScreen<W> {
    pub task: String,
    pub launch: Box<Launch>,
    pub window: W,
}

impl<W: Copy> TasksDrive<W> {
    /// Start `launch` off screen as task `id`, from `window`, or say why it
    /// cannot. `id` is the task's whether or not it is kept, so whoever
    /// asked hears of it either way.
    #[must_use]
    pub fn start_off_screen(
        &mut self,
        id: String,
        launch: Result<Launch, String>,
        window: W,
    ) -> Vec<Effect<W>> {
        let launch = match launch {
            Ok(launch) => launch,
            Err(why) => return vec![refused(id, why)],
        };
        let cuts = launch.cuts();
        let start = OffScreen {
            task: id,
            launch: Box::new(launch),
            window,
        };
        match cuts {
            true => vec![Effect::Cut(start)],
            false => vec![Effect::Keep(start)],
        }
    }

    /// What [`Effect::Cut`] came to.
    pub(super) fn cut(&mut self, start: OffScreen<W>, made: Cut) -> Effect<W> {
        match made {
            Ok(cut) => Effect::Keep(OffScreen {
                launch: Box::new(start.launch.on_worktree(cut)),
                ..start
            }),
            Err(why) => refused(start.task, why),
        }
    }

    /// What [`Effect::Keep`] came to: with the window still open, the task
    /// is kept, driven off screen the first time, and asks for its place.
    pub(super) fn kept(&mut self, start: OffScreen<W>, open: bool, fx: &mut Vec<Effect<W>>) {
        let OffScreen {
            task,
            launch,
            window,
        } = start;
        if !open {
            return fx.push(refused(task, "the window closed".to_string()));
        }
        fx.extend(self.add(launch.task(task.clone())));
        self.drive_off_screen(&task);
        fx.push(Effect::Ask { task, window });
    }
}

fn refused<W>(task: String, why: String) -> Effect<W> {
    Effect::Told {
        task,
        placed: Placed::Refused(why),
    }
}
