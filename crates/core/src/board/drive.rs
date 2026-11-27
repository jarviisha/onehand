//! What happens next for every project's board: one drive for the process,
//! as a project's board is the same in every window holding the project.
//!
//! **Decided here, carried out elsewhere.** A [`BoardDrive`] owns every
//! board read at launch or bound since, and answers each change with
//! effects: a board file to write ([`Effect::Write`]), a wait before a burst
//! is written ([`Effect::Arm`]), and that a board changed
//! ([`Effect::Changed`]), which the front end draws again wherever the
//! project is held. It derefs to [`Boards`] and hands out no mutable one.
//!
//! **A board is written whole, in order, and put back whole**: a change
//! shows at once and is written after every write before it; a write that
//! fails puts the board back to the one last known saved, unless a later
//! write carries it, and is said once. The team's track is written once it
//! has been still for [`SAVE_DEBOUNCE`].
//!
//! **A board this build cannot read is never written**: every command on it
//! changes nothing, so a newer onehand's file survives an older one.

use super::model::{Board, FIRST_TEAM};
use crate::connector::Connector;
use crate::issues::Issues;
use crate::project::{ProjectId, Toward};
use crate::saving::{Pending, Saves};
use crate::workflow::Outcome;
use crate::workflow::Template;
use crate::Now;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Duration;

/// How long the team's track must sit still before it is written: the
/// projects' own wait.
pub const SAVE_DEBOUNCE: Duration = crate::project::drive::SAVE_DEBOUNCE;

/// Every board kept, and the ones that could not be read.
#[derive(Debug, Default)]
pub struct Boards {
    boards: HashMap<ProjectId, Board>,
    unreadable: HashMap<ProjectId, String>,
}

impl Boards {
    /// Project `id`'s board, while it has one this build can read.
    pub fn board(&self, id: &ProjectId) -> Option<&Board> {
        self.boards.get(id)
    }

    /// Why project `id`'s board cannot be read, when it cannot.
    pub fn unreadable(&self, id: &ProjectId) -> Option<&str> {
        self.unreadable.get(id).map(String::as_str)
    }

    /// Whether project `id` has a board, read or not.
    pub fn kept(&self, id: &ProjectId) -> bool {
        self.boards.contains_key(id) || self.unreadable.contains_key(id)
    }

    /// The tasks every board holds: those its columns' entries name.
    pub fn held(&self) -> HashSet<String> {
        (self.boards.values())
            .flat_map(|board| board.columns.iter())
            .flat_map(|column| column.entries.iter().filter_map(|e| e.task.clone()))
            .collect()
    }

    /// The projects with a board this build can read.
    pub fn ids(&self) -> impl Iterator<Item = &ProjectId> {
        self.boards.keys()
    }
}

/// One write of a board file: the board as it stood, none to remove the
/// file, where it goes, and which change it carries.
pub struct BoardWrite {
    pub id: ProjectId,
    pub pending: Pending<(), Option<Board>>,
    pub board: Option<Board>,
    pub dir: PathBuf,
    /// Which write of the drive it is, counted from 1, so its answer says
    /// which starts it lets go ahead.
    pub seq: u64,
}

/// What a person is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// Project `id`'s board could not be written: why.
    SaveFailed { id: ProjectId, why: String },
    /// Issue `issue` of project `id`'s board could not start, for `why`:
    /// its row reads as a start that did not land, for a person to start
    /// again.
    NotStarted {
        id: ProjectId,
        issue: u64,
        why: String,
    },
}

/// What the front end carries out.
pub enum Effect {
    /// Make `write`, after every write before it, and report
    /// [`Input::Written`]. A late answer is **recorded**: the write landed or
    /// it did not, whatever changed since.
    Write(Box<BoardWrite>),
    /// Report [`Input::Elapsed`] with `token` after `after`. A late answer is
    /// **dropped** by its token: a later change armed a later wait.
    Arm {
        token: u64,
        after: Duration,
    },
    /// Project `id`'s board changed: draw it again wherever it is held.
    Changed(ProjectId),
    Signal(Signal),
    /// Read project `id`'s issues and what the tasks and the library say of
    /// its board, and report [`Input::IssuesRead`]. A late answer is
    /// **dropped** in effect: it is a look, and a later one looks again.
    ReadIssues {
        id: ProjectId,
    },
    /// Give task `task`, an extend waiting for a place on its board's team,
    /// that place.
    GivePlace {
        task: String,
    },
    /// Start a task `task` of `template` on issue `issue` of project `id`,
    /// off screen, and report [`Input::Landed`]. A late answer is
    /// **recorded**: a task kept is the board's whatever moved since.
    Start {
        id: ProjectId,
        issue: u64,
        task: String,
        template: Template,
        /// The name of the column it starts in.
        column: String,
    },
    /// Start run `run` of `template` on task `task`, which works issue
    /// `issue` of project `id` and entered column `column`, off screen, and
    /// report [`Input::Landed`]. A late answer is **recorded**, as a start's.
    NextRun {
        id: ProjectId,
        issue: u64,
        task: String,
        run: String,
        template: Template,
        column: String,
    },
    /// Close issue `issue` of project `id`'s own tracker.
    CloseIssue {
        id: ProjectId,
        issue: u64,
    },
    /// The tasks the boards hold now, every one of them: none of them is let
    /// go of as history, nor dismissed.
    Held {
        tasks: HashSet<String>,
    },
}

/// What the front end reports back.
pub enum Input {
    /// The write [`Effect::Write`] asked for landed, or failed.
    Written {
        id: ProjectId,
        pending: Pending<(), Option<Board>>,
        seq: u64,
        result: Result<(), String>,
    },
    /// The wait [`Effect::Arm`] asked for ran out.
    Elapsed { token: u64 },
    /// What [`Effect::ReadIssues`] read: the issues and the forge serving
    /// the project, or why not, and what was read beside them.
    IssuesRead {
        id: ProjectId,
        read: Result<(Issues, Option<&'static dyn Connector>), String>,
        facts: Box<Facts>,
    },
    /// Where [`Effect::Start`] landed: started or queued, or refused for
    /// why; `kept` says whether its task was made before it was refused.
    Landed {
        id: ProjectId,
        issue: u64,
        task: String,
        kept: bool,
        result: Result<(), String>,
    },
    /// A board task of project `id` on `issue` ended as `outcome`, and let
    /// its place go.
    Ended {
        id: ProjectId,
        issue: u64,
        task: String,
        run: Option<String>,
        outcome: Option<Outcome>,
    },
    /// The tasks were read, each board task as the boards see it.
    TasksRead { tasks: Vec<Seen> },
}

/// A start waiting for the write that names it: project, write, issue.
struct Held {
    id: ProjectId,
    seq: u64,
    issue: u64,
    start: Effect,
}

/// Every board's drive. See the module header.
pub struct BoardDrive {
    boards: Boards,
    /// `<data root>/projects`, each project's folder named by its id inside.
    dir: PathBuf,
    saves: HashMap<ProjectId, Saves<(), Option<Board>>>,
    /// The boards with a change waiting for things to be still.
    waiting: BTreeSet<ProjectId>,
    /// The newest wait armed, while one waits.
    soon: Option<u64>,
    minted: u64,
    /// Each board's issues taken out of To do to start whose task has not
    /// landed yet. Never written: a start cut off by an exit is just gone.
    starting: HashMap<ProjectId, BTreeSet<u64>>,
    /// The issues whose board task ended done, waiting for their tracker to
    /// be read to tell whether to close them.
    closing: HashMap<ProjectId, BTreeSet<u64>>,
    /// The writes made, counted.
    seq: u64,
    /// Each board's newest write.
    newest: HashMap<ProjectId, u64>,
    /// The starts waiting for the write that names them: asked once a write
    /// at least that new lands, dropped when it fails and none newer was made.
    held: Vec<Held>,
}

impl Deref for BoardDrive {
    type Target = Boards;
    fn deref(&self) -> &Boards {
        &self.boards
    }
}

impl BoardDrive {
    /// A drive over `boards` and the `unreadable` ones, whose files go in
    /// `dir`.
    pub fn new(
        boards: HashMap<ProjectId, Board>,
        unreadable: HashMap<ProjectId, String>,
        dir: PathBuf,
    ) -> Self {
        Self {
            boards: Boards { boards, unreadable },
            dir,
            saves: HashMap::new(),
            waiting: BTreeSet::new(),
            soon: None,
            minted: 0,
            starting: HashMap::new(),
            closing: HashMap::new(),
            seq: 0,
            newest: HashMap::new(),
            held: Vec::new(),
        }
    }

    /// Give project `id` `board`, or take its board away with none. Written
    /// now; unchanged, nothing happens.
    #[must_use = "its effects are to be carried"]
    pub fn set_board(&mut self, id: &ProjectId, board: Option<Board>) -> Vec<Effect> {
        self.edit(id, false, |_| board)
    }

    /// Bind `template` as project `id`'s board, as read when it is on offer,
    /// or none when it is gone or cannot be read: refused, with why, when a
    /// board cannot run it. A board bound again keeps its team and its To
    /// do; a new one starts with a small team and nothing to do.
    pub fn bind(
        &mut self,
        id: &ProjectId,
        template: Option<&Template>,
        now: Now,
    ) -> Result<Vec<Effect>, String> {
        if let Some(why) = self.boards.unreadable(id) {
            return Err(format!(
                "Its board cannot be read, and is left as it is: {why}"
            ));
        }
        let workflow = super::usable(template)?.id.clone();
        Ok(self.edit(id, false, |board| match board {
            Some(kept) => Some(kept.rebound(workflow)),
            None => Some(Board::made(workflow, FIRST_TEAM, &[], now.secs)),
        }))
    }

    /// Bind `copy`, a copy of workflow `from` that project `id`'s board runs,
    /// wherever the board runs it: refused, with why, when a board cannot
    /// run it or cannot be read; none when the board runs no `from`.
    pub fn bind_copy(
        &mut self,
        id: &ProjectId,
        from: &str,
        copy: &Template,
        _now: Now,
    ) -> Option<Result<Vec<Effect>, String>> {
        if let Some(why) = self.boards.unreadable(id) {
            return Some(Err(format!(
                "Its board cannot be read, and is left as it is: {why}"
            )));
        }
        if !self.boards.board(id)?.runs(from) {
            return None;
        }
        Some(super::usable(Some(copy)).map(|copy| {
            let to = copy.id.clone();
            self.edit(id, false, |board| board.map(|b| b.rebound_from(from, to)))
        }))
    }

    /// Take project `id`'s board away: its file goes.
    #[must_use = "its effects are to be carried"]
    pub fn unbind(&mut self, id: &ProjectId) -> Vec<Effect> {
        let mut fx = self.set_board(id, None);
        if !fx.is_empty() {
            fx.push(self.say_held());
        }
        fx
    }

    /// Take `issue` off project `id`'s board, back to the Backlog: its task
    /// stays as history. Refused while its task works (`working`).
    pub fn remove(
        &mut self,
        id: &ProjectId,
        issue: u64,
        working: bool,
    ) -> Result<Vec<Effect>, String> {
        if working {
            return Err("It is at work: Stop it first".to_string());
        }
        if self.starting.get(id).is_some_and(|s| s.contains(&issue)) {
            return Err("It is starting: wait for it to start".to_string());
        }
        let mut fx = self.edit(id, false, |board| board.map(|b| b.removed(issue)));
        if !fx.is_empty() {
            fx.push(self.say_held());
        }
        Ok(fx)
    }

    /// Whether taking project `id`'s board away asks a person first: its To
    /// do goes with it and cannot be had back.
    pub fn unbinding_asks(&self, id: &ProjectId) -> bool {
        self.boards.board(id).is_some_and(|b| !b.todo().is_empty())
    }

    /// Set the size of project `id`'s team, kept within 1 to 8; written once
    /// the changing stops.
    #[must_use = "its effects are to be carried"]
    pub fn set_team(&mut self, id: &ProjectId, team: u8) -> Vec<Effect> {
        self.edit(id, true, |b| b.map(|b| b.with_team(team)))
    }

    /// Put `issue` last in project `id`'s To do, unless it is there.
    #[must_use = "its effects are to be carried"]
    pub fn add_todo(&mut self, id: &ProjectId, issue: u64, now: Now) -> Vec<Effect> {
        self.edit_todo(id, now, |todo| {
            if !todo.contains(&issue) {
                todo.push(issue);
            }
        })
    }

    /// Move `issue` one place up (or down) in project `id`'s To do.
    #[must_use = "its effects are to be carried"]
    pub fn move_todo(
        &mut self,
        id: &ProjectId,
        issue: u64,
        toward: Toward,
        now: Now,
    ) -> Vec<Effect> {
        self.edit_todo(id, now, |todo| {
            let Some(at) = todo.iter().position(|&n| n == issue) else {
                return;
            };
            let to = match toward {
                Toward::Up => at.checked_sub(1),
                Toward::Down => Some(at + 1),
            };
            if let Some(to) = to.filter(|&to| to < todo.len()) {
                todo.swap(at, to);
            }
        })
    }

    /// Put `issue` at place `at` in project `id`'s To do, taking it from
    /// where it was; `at` counts after that and is held within the list.
    #[must_use = "its effects are to be carried"]
    pub fn place_todo(&mut self, id: &ProjectId, issue: u64, at: usize, now: Now) -> Vec<Effect> {
        self.edit_todo(id, now, |todo| {
            todo.retain(|&n| n != issue);
            todo.insert(at.min(todo.len()), issue);
        })
    }

    /// Put `issue` just before `target` in project `id`'s To do, taking it
    /// from where it was; dropped on itself, or on an issue not in To do, it
    /// moves nothing.
    #[must_use = "its effects are to be carried"]
    pub fn place_todo_before(
        &mut self,
        id: &ProjectId,
        issue: u64,
        target: u64,
        now: Now,
    ) -> Vec<Effect> {
        let todo = self.boards.board(id).map(Board::todo).unwrap_or_default();
        match place_before(&todo, issue, target) {
            Some(at) => self.place_todo(id, issue, at, now),
            None => Vec::new(),
        }
    }

    /// Take `issue` out of project `id`'s To do.
    #[must_use = "its effects are to be carried"]
    pub fn drop_todo(&mut self, id: &ProjectId, issue: u64, now: Now) -> Vec<Effect> {
        self.edit_todo(id, now, |todo| todo.retain(|&n| n != issue))
    }

    /// Write every change still waiting on its burst, now: what the front end
    /// does as it quits.
    #[must_use = "its effects are to be carried"]
    pub fn flush(&mut self) -> Vec<Effect> {
        self.soon = None;
        let waiting = std::mem::take(&mut self.waiting);
        waiting.iter().filter_map(|id| self.write(id)).collect()
    }

    /// Take what came of an effect.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input, now: Now) -> Vec<Effect> {
        match input {
            Input::Written {
                id,
                pending,
                seq,
                result,
            } => {
                let landed = result.is_ok();
                // Put back first: what a failed write leaves on the board
                // decides whether its starts wait for the next one.
                let said =
                    (self.saves.get_mut(&id)).and_then(|saves| saves.landed(pending, result));
                let mut fx = Vec::new();
                if let Some((why, back)) = said {
                    if let Some(board) = back.into_iter().last() {
                        self.put(&id, board);
                    }
                    fx.push(Effect::Changed(id.clone()));
                    fx.push(Effect::Signal(Signal::SaveFailed {
                        id: id.clone(),
                        why,
                    }));
                    fx.push(self.say_held());
                }
                let mut starts = self.starts_after(&id, seq, landed);
                starts.extend(fx);
                starts
            }
            Input::Elapsed { token } => {
                if self.soon != Some(token) {
                    return Vec::new();
                }
                self.flush()
            }
            Input::IssuesRead { id, read, facts } => self.issues_read(&id, read, *facts, now),
            Input::Landed {
                id,
                issue,
                task: _,
                kept,
                result,
            } => self.landed(&id, issue, kept, result),
            Input::Ended {
                id,
                issue,
                task,
                run: _,
                outcome,
            } => self.ended(&id, issue, &task, outcome.as_ref()),
            Input::TasksRead { tasks } => self.tasks_read(&tasks, now),
        }
    }

    fn edit_todo(
        &mut self,
        id: &ProjectId,
        now: Now,
        edit: impl FnOnce(&mut Vec<u64>),
    ) -> Vec<Effect> {
        self.edit(id, false, |board| {
            board.map(|b| {
                let mut todo = b.todo();
                edit(&mut todo);
                b.with_todo(&todo, now.secs)
            })
        })
    }

    /// Replace project `id`'s board by what `make` answers of it, written now
    /// (`soon` false) or after the burst. A project without a board stays so
    /// unless `make` gives it one; one whose board cannot be read is left.
    fn edit(
        &mut self,
        id: &ProjectId,
        soon: bool,
        make: impl FnOnce(Option<Board>) -> Option<Board>,
    ) -> Vec<Effect> {
        if self.boards.unreadable.contains_key(id) {
            return Vec::new();
        }
        let before = self.boards.boards.get(id).cloned();
        let after = make(before.clone());
        if after == before {
            return Vec::new();
        }
        self.put(id, after.clone());
        self.saves
            .entry(id.clone())
            .or_default()
            .change((), before, after);
        let mut fx = match soon {
            true => self.save_soon(id),
            false => self.write(id).into_iter().collect(),
        };
        fx.push(Effect::Changed(id.clone()));
        fx
    }

    /// Show `board` as project `id`'s.
    fn put(&mut self, id: &ProjectId, board: Option<Board>) {
        match board {
            Some(board) => self.boards.boards.insert(id.clone(), board),
            None => self.boards.boards.remove(id),
        };
    }

    fn save_soon(&mut self, id: &ProjectId) -> Vec<Effect> {
        self.waiting.insert(id.clone());
        self.minted += 1;
        self.soon = Some(self.minted);
        vec![Effect::Arm {
            token: self.minted,
            after: SAVE_DEBOUNCE,
        }]
    }

    /// Project `id`'s board as it now stands, and where it goes. Carries a
    /// change waiting on a burst too, so that one is not written again.
    fn write(&mut self, id: &ProjectId) -> Option<Effect> {
        self.waiting.remove(id);
        let pending = self.saves.get_mut(id)?.write_all();
        self.seq += 1;
        self.newest.insert(id.clone(), self.seq);
        Some(Effect::Write(Box::new(BoardWrite {
            id: id.clone(),
            pending,
            board: self.boards.boards.get(id).cloned(),
            dir: self.dir.join(id.as_str()),
            seq: self.seq,
        })))
    }

    /// Whether project `id`'s board, as shown, holds `issue` begun: an entry
    /// of it with the run its start asked for.
    fn names_issue(&self, id: &ProjectId, issue: u64) -> bool {
        (self.boards.board(id)).is_some_and(|b| b.at(issue).is_some_and(|(_, e)| e.run.is_some()))
    }

    /// The starts project `id`'s write `seq` lets go ahead: once it landed,
    /// those it or an older write named. When it failed, a newer write
    /// carries them and they wait for it; with none, they are dropped, no
    /// longer starting, the board having been put back without them.
    fn starts_after(&mut self, id: &ProjectId, seq: u64, landed: bool) -> Vec<Effect> {
        let newest = self.newest.get(id).copied().unwrap_or(seq);
        let held = std::mem::take(&mut self.held);
        let mut fx = Vec::new();
        for held in held {
            if held.id != *id || held.seq > seq {
                self.held.push(held);
                continue;
            }
            match (landed, newest > seq) {
                (true, _) => fx.push(held.start),
                (false, true) => self.held.push(Held {
                    seq: newest,
                    ..held
                }),
                // The board shown still names it, a change of its own having
                // kept it there: the next write carries it.
                (false, false) if self.names_issue(id, held.issue) => self.held.push(held),
                (false, false) => {
                    if let Some(starting) = self.starting.get_mut(id) {
                        starting.remove(&held.issue);
                    }
                }
            }
        }
        fx
    }
}

/// Where in `todo` issue `dragged` goes to stand just before `target`: the
/// target's place once the dragged issue is taken out. None when dropped on
/// itself, which moves nothing.
fn place_before(todo: &[u64], dragged: u64, target: u64) -> Option<usize> {
    if dragged == target {
        return None;
    }
    todo.iter()
        .filter(|&&n| n != dragged)
        .position(|&n| n == target)
}

mod columns;
mod look;
mod seen;
pub use look::Facts;
pub use seen::Seen;

#[cfg(test)]
mod tests;
