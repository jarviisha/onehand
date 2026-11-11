//! A task a schedule or one of onehand's tools starts, off screen: refused
//! with what blocks it, on a worktree cut for it first when its workflow
//! works on one, its project kept in the window without being put on
//! screen, then kept as a task and asking for its place like any other.
//! Where it landed is said to whoever asked ([`Effect::Told`]).

use super::{Effect, Placed, TasksDrive};
use crate::task::Launch;
use std::path::PathBuf;

/// A worktree cut for a task: the folder it works in, its branch, and the
/// forge the branch goes to.
pub type Cut = Result<(PathBuf, String, Option<String>), String>;

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
        match launch.cuts() {
            true => vec![Effect::Cut {
                task: id,
                launch: Box::new(launch),
                window,
            }],
            false => vec![Effect::Keep {
                task: id,
                launch: Box::new(launch),
                window,
            }],
        }
    }

    /// What [`Effect::Cut`] came to.
    pub(super) fn cut(&mut self, id: String, launch: Launch, window: W, made: Cut) -> Effect<W> {
        match made {
            Ok((dir, branch, forge)) => Effect::Keep {
                task: id,
                launch: Box::new(launch.on_worktree(dir, branch, forge)),
                window,
            },
            Err(why) => refused(id, why),
        }
    }

    /// What [`Effect::Keep`] came to: with the window still open, the task
    /// is kept, driven off screen the first time, and asks for its place.
    pub(super) fn kept(
        &mut self,
        id: String,
        launch: Launch,
        window: W,
        open: bool,
        fx: &mut Vec<Effect<W>>,
    ) {
        if !open {
            return fx.push(refused(id, "the window closed".to_string()));
        }
        fx.extend(self.add(launch.task(id.clone())));
        self.drive_off_screen(&id);
        fx.push(Effect::Ask { task: id, window });
    }
}

fn refused<W>(task: String, why: String) -> Effect<W> {
    Effect::Told {
        task,
        placed: Placed::Refused(why),
    }
}
