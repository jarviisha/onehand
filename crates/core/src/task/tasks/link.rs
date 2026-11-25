//! The link between the tasks and the runs under way: what a run's write
//! and its end become for the task it is of.
//!
//! **Routing is behaviour, so it lives here.** That a run's save writes its
//! whole task, and that its place is held while its session's turn still
//! goes, are each a rule a front end would otherwise restate, untested. A front end
//! calls these with the drives it holds and carries out what comes back.

use super::{Effect, OffScreen, TasksDrive};
use crate::chat::Chat;
use crate::project::Project;
use crate::task::drive::RunDrive;
use crate::task::{Task, Working};
use crate::workspace::{drive as workspace, WorkspaceDrive};

/// `run` asked to be saved: the task it is of, keeping the run as it now
/// stands, to write.
pub fn save_run<W: Copy>(tasks: &mut TasksDrive<W>, run: &RunDrive) -> Option<Task> {
    tasks.keep_run(run.task(), run.run().clone())
}

/// `run` finished, its current lane's session `chat` if it had one, on
/// sessions `uids` in `window`, every lane's: what its task does about it.
pub fn finished<W: Copy>(
    tasks: &mut TasksDrive<W>,
    run: &RunDrive,
    chat: Option<&Chat>,
    uids: Vec<u64>,
    window: W,
) -> Vec<Effect<W>> {
    tasks.run_ended(
        run.task(),
        run.run().clone(),
        run.pinned(),
        chat.is_some_and(|chat| chat.busy),
        uids,
        window,
    )
}

/// Carry [`Effect::Keep`] for `start`, with the workspace of the window it
/// was asked from if that is still open and the project its folder is of,
/// when one holds it: the folder kept there, listed but not put on screen,
/// then the task kept and asking for its place. With the window gone, the
/// start is refused. What the tasks do
/// next, and what the workspace does.
#[must_use]
pub fn keep<W: Copy>(
    tasks: &mut TasksDrive<W>,
    workspace: Option<&mut WorkspaceDrive>,
    project: Option<&Project>,
    start: OffScreen<W>,
) -> (Vec<Effect<W>>, Vec<workspace::Effect>) {
    let dir = start.launch.dir().to_path_buf();
    let kept = workspace.map(|workspace| workspace.keep_project_quietly(dir, project));
    let mut fx = Vec::new();
    tasks.kept(start, kept.is_some(), &mut fx);
    (fx, kept.unwrap_or_default())
}

/// Every task and the runs under way, as another drive reads them: what each
/// task is doing, its run's word included. Reads only; nothing here changes
/// a task.
pub struct Seen<'a, W> {
    tasks: &'a TasksDrive<W>,
    runs: Vec<&'a RunDrive>,
}

impl<'a, W: Copy> Seen<'a, W> {
    /// `tasks`, with the drives of the runs under way.
    pub fn new(tasks: &'a TasksDrive<W>, runs: impl IntoIterator<Item = &'a RunDrive>) -> Self {
        Self {
            tasks,
            runs: runs.into_iter().collect(),
        }
    }

    /// The drive of task `id`'s run under way, if one is.
    pub fn run(&self, id: &str) -> Option<&'a RunDrive> {
        self.runs.iter().copied().find(|run| run.task() == id)
    }

    /// What task `id` is doing: its place's word, and a run holding the place
    /// that waits on a person (an approval, or a card), its drive's.
    pub fn working(&self, id: &str) -> Option<Working> {
        match (self.tasks.working(id), self.run(id)) {
            (Some(Working::Running), Some(run)) if run.waits_on_person() => Some(Working::Waiting),
            (working @ (Some(Working::Queued | Working::Running | Working::Waiting) | None), _) => {
                working
            }
        }
    }

    /// Whether task `id` is queued, running or waiting on a person.
    pub fn busy(&self, id: &str) -> bool {
        self.tasks.busy(id)
    }

    /// Every task kept, oldest first.
    pub fn all(&self) -> &'a [Task] {
        self.tasks.tasks()
    }

    /// Task `id` as it is kept.
    pub fn task(&self, id: &str) -> Option<&'a Task> {
        self.tasks.task(id)
    }
}

/// What removing a task's worktree came to (`input`, an
/// [`super::Input::WorktreeRemoved`]): what the task does, and, the checkout
/// gone, what its project does on forgetting every worktree it recorded in it.
pub fn worktree_removed<W: Copy>(
    tasks: &mut TasksDrive<W>,
    projects: &mut crate::project::ProjectsDrive,
    input: super::Input<W>,
) -> (Vec<Effect<W>>, Vec<crate::project::drive::Effect>) {
    let gone = match &input {
        super::Input::WorktreeRemoved {
            folder,
            removed: Ok(_),
            ..
        } => Some(folder.clone()),
        _ => None,
    };
    let fx = tasks.handle(input);
    let said = gone.map_or_else(Vec::new, |top| projects.worktree_gone(&top));
    (fx, said)
}
