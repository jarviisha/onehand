//! What happens next for every project onehand keeps: one drive for the
//! process, since a project is the same project in every window holding it.
//!
//! **Decided here, carried out elsewhere.** A [`ProjectsDrive`] owns every
//! project read at launch or placed since, and answers each change with
//! effects: a project file to write ([`Effect::Write`]), a wait before a
//! burst is written ([`Effect::Arm`]), and that a project changed
//! ([`Effect::Changed`]), which the front end tells every window holding it.
//! It derefs to [`Projects`] and hands out no mutable one.
//!
//! **A file is written whole, in order, and put back by thing**, as the
//! workspace file is: a change shows at once and is written after every write
//! before it; a write that fails puts back each thing no later write carries,
//! to its value last known saved, and is said once. A change made in a burst
//! (a check command typed, an offload's moves) is written once things have
//! been still for [`SAVE_DEBOUNCE`].

use super::{MadeBy, Placement, Project, ProjectId, Projects, Worktree};
use crate::saving::{Pending, Saves};
use crate::workspace::{BuildHost, HostName, Offloaded, RoutedCommand};
use std::collections::{BTreeSet, HashMap};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the projects must sit still before a burst of changes is
/// written: the workspace's own wait, for the same reasons.
pub const SAVE_DEBOUNCE: Duration = crate::workspace::drive::SAVE_DEBOUNCE;

/// One thing in a project file a failed write puts back on its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Thing {
    Name,
    Check,
    Cards,
    Builds,
    Routes,
    BuildsOn,
    Worktrees,
    WorktreeParent,
    /// The offload of one folder of the project, by its path here.
    Offload(PathBuf),
}

/// A thing's value, as a failed write puts it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Name(String),
    Check(Option<String>),
    Cards(crate::chat::Cards),
    Builds(Vec<BuildHost>),
    Routes(Vec<RoutedCommand>),
    BuildsOn(Option<HostName>),
    Worktrees(Vec<Worktree>),
    WorktreeParent(Option<PathBuf>),
    Offload {
        folder: PathBuf,
        on: Option<Offloaded>,
    },
}

/// One write of a project file: the project as it stood, where it goes, and
/// which changes it carries.
pub struct ProjectWrite {
    pub id: ProjectId,
    pub pending: Pending<Thing, Value>,
    pub project: Project,
    pub dir: PathBuf,
}

/// What a person is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// The file of project `id`, called `name`, could not be written: why.
    SaveFailed {
        id: ProjectId,
        name: String,
        why: String,
    },
    /// The worktree at `folder` on `branch` was removed, its branch `kept`
    /// for git's reason when git kept it.
    WorktreeRemoved {
        folder: PathBuf,
        branch: Option<String>,
        kept: Option<String>,
    },
    /// Judged again, the worktree stays, for each reason.
    WorktreeStays(Vec<String>),
    /// git did not remove the worktree: why.
    WorktreeNotRemoved(String),
}

/// How removing a worktree that is not a task's came out.
pub enum Removal {
    /// Judged again on the answer, it was refused: why.
    Refused(Vec<String>),
    /// git was asked to remove the checkout at `top`, then `branch`.
    Done {
        top: PathBuf,
        branch: Option<String>,
        result: Result<crate::worktree::removal::Removed, String>,
    },
}

/// What the front end carries out.
pub enum Effect {
    /// Make `write`, after every write before it, and report
    /// [`Input::Written`]. A late answer is **recorded**: the write landed or
    /// it did not, whatever changed since.
    Write(Box<ProjectWrite>),
    /// Report [`Input::Elapsed`] with `token` after `after`. A late answer is
    /// **dropped** by its token: a later change armed a later wait.
    Arm {
        token: u64,
        after: Duration,
    },
    /// The project changed: tell every window holding it.
    Changed(ProjectId),
    Signal(Signal),
}

/// What the front end reports back.
pub enum Input {
    /// The write [`Effect::Write`] asked for landed, or failed.
    Written {
        id: ProjectId,
        pending: Pending<Thing, Value>,
        result: Result<(), String>,
    },
    /// The wait [`Effect::Arm`] asked for ran out.
    Elapsed { token: u64 },
}

/// Every project's drive. See the module header.
pub struct ProjectsDrive {
    projects: Projects,
    /// `<data root>/projects`, each project's folder named by its id inside.
    dir: PathBuf,
    saves: HashMap<ProjectId, Saves<Thing, Value>>,
    /// The projects with a change waiting for things to be still.
    waiting: BTreeSet<ProjectId>,
    /// The newest wait armed, while one waits.
    soon: Option<u64>,
    minted: u64,
    /// The worktrees a removal is judging or removing, so a second press on
    /// one is not taken.
    removing: std::collections::HashSet<PathBuf>,
}

impl Deref for ProjectsDrive {
    type Target = Projects;
    fn deref(&self) -> &Projects {
        &self.projects
    }
}

impl ProjectsDrive {
    /// A drive over `projects`, whose files go in `dir`.
    pub fn new(projects: Projects, dir: PathBuf) -> Self {
        Self {
            projects,
            dir,
            saves: HashMap::new(),
            waiting: BTreeSet::new(),
            soon: None,
            minted: 0,
            removing: Default::default(),
        }
    }

    /// Put `folder` on `host` where `placement` says, made by `made` if it
    /// becomes a worktree, or a project of its own named `fresh`. Answers the
    /// project it is a folder of.
    ///
    /// A new folder something already placed since `placement` was worked
    /// out is that project's, and makes none of its own.
    #[must_use = "its effects are to be carried"]
    pub fn place(
        &mut self,
        host: Option<HostName>,
        folder: PathBuf,
        placement: Placement,
        made: MadeBy,
        fresh: ProjectId,
    ) -> (ProjectId, Vec<Effect>) {
        if let Some((project, _)) = self.projects.find(host.as_ref(), &folder) {
            return (project.id.clone(), Vec::new());
        }
        match placement {
            Placement::Known(id, _) if self.projects.get(&id).is_some() => (id, Vec::new()),
            Placement::WorktreeOf { project, branch } if self.projects.get(&project).is_some() => {
                let worktree = Worktree {
                    path: folder,
                    branch,
                    from: None,
                    made_by: made,
                    offload: None,
                };
                let fx = self.record_worktree(&project, worktree);
                (project, fx)
            }
            Placement::Known(..) | Placement::WorktreeOf { .. } | Placement::New => {
                let project = Project::new(fresh, host, folder);
                let id = project.id.clone();
                self.projects.put(project);
                let mut fx: Vec<Effect> = self.write(&id).into_iter().collect();
                fx.push(Effect::Changed(id.clone()));
                (id, fx)
            }
        }
    }

    /// Record `worktree` as one of project `id`'s, written now: what a split
    /// and a run's cut do. A folder the project already has is left as it is.
    #[must_use = "its effects are to be carried"]
    pub fn record_worktree(&mut self, id: &ProjectId, worktree: Worktree) -> Vec<Effect> {
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if project
            .folder(project.host.as_ref(), &worktree.path)
            .is_some()
        {
            return Vec::new();
        }
        let before = project.worktrees.clone();
        project.worktrees.push(worktree);
        let after = project.worktrees.clone();
        self.change(
            id,
            Thing::Worktrees,
            Value::Worktrees(before),
            Value::Worktrees(after),
            false,
        )
    }

    /// Forget the worktree at `path` of project `id`, written now: what
    /// *Forget* on a missing one and a removal through onehand do. Its folder
    /// is not touched, and a session in it keeps running there.
    #[must_use = "its effects are to be carried"]
    pub fn forget_worktree(&mut self, id: &ProjectId, path: &Path) -> Vec<Effect> {
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if !project.worktrees.iter().any(|w| w.path == path) {
            return Vec::new();
        }
        let before = project.worktrees.clone();
        project.worktrees.retain(|w| w.path != path);
        let after = project.worktrees.clone();
        self.change(
            id,
            Thing::Worktrees,
            Value::Worktrees(before),
            Value::Worktrees(after),
            false,
        )
    }

    /// Removing project `id`'s worktree at `folder` came out as `removal`: a
    /// checkout git removed is forgotten, written now; each outcome is said.
    #[must_use = "its effects are to be carried"]
    pub fn worktree_removal(
        &mut self,
        id: &ProjectId,
        folder: &Path,
        removal: Removal,
    ) -> Vec<Effect> {
        match removal {
            Removal::Refused(why) => {
                self.removing.remove(folder);
                vec![Effect::Signal(Signal::WorktreeStays(why))]
            }
            Removal::Done {
                result: Err(why), ..
            } => {
                self.removing.remove(folder);
                vec![Effect::Signal(Signal::WorktreeNotRemoved(why))]
            }
            Removal::Done {
                top,
                branch,
                result: Ok(removed),
            } => {
                self.removing.remove(folder);
                let mut fx = self.forget_worktree(id, folder);
                fx.push(Effect::Signal(Signal::WorktreeRemoved {
                    folder: top,
                    branch,
                    kept: removed.branch_kept,
                }));
                fx
            }
        }
    }

    /// Take in `listed`, a checkout git lists, as a worktree of project `id`,
    /// on its branch or `HEAD`, written now.
    #[must_use = "its effects are to be carried"]
    pub fn adopt_worktree(
        &mut self,
        id: &ProjectId,
        listed: crate::worktree::Listed,
    ) -> Vec<Effect> {
        let worktree = Worktree {
            branch: listed.branch_or_head(),
            path: listed.path,
            from: None,
            made_by: crate::project::MadeBy::Added,
            offload: None,
        };
        self.record_worktree(id, worktree)
    }

    /// A removal of the worktree at `folder` begins: false when one is under
    /// way already, which a second press does not start again.
    pub fn begin_removal(&mut self, folder: &Path) -> bool {
        self.removing.insert(folder.to_path_buf())
    }

    /// The removal of `folder` is judged and waits on the person's answer,
    /// or was let go: it is under way no more.
    pub fn removal_asked(&mut self, folder: &Path) {
        self.removing.remove(folder);
    }

    /// Whether a removal of the worktree at `folder` is under way.
    pub fn removing(&self, folder: &Path) -> bool {
        self.removing.contains(folder)
    }

    /// The checkout at `top` was removed by another way than its project's
    /// (a task's, after its pull request merged): every recorded worktree
    /// in it, a project's sub-folder of it too, is forgotten, written now.
    #[must_use = "its effects are to be carried"]
    pub fn worktree_gone(&mut self, top: &Path) -> Vec<Effect> {
        let gone: Vec<(ProjectId, PathBuf)> = (self.projects.iter())
            .flat_map(|project| {
                (project.worktrees.iter())
                    .filter(|w| w.path.starts_with(top))
                    .map(|w| (project.id.clone(), w.path.clone()))
            })
            .collect();
        gone.into_iter()
            .flat_map(|(id, path)| self.forget_worktree(&id, &path))
            .collect()
    }

    /// Put project `id`'s new worktrees in `parent`, or beside the repository
    /// with none, written now; unchanged changes nothing.
    #[must_use = "its effects are to be carried"]
    pub fn set_worktree_parent(&mut self, id: &ProjectId, parent: Option<PathBuf>) -> Vec<Effect> {
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if project.worktree_parent == parent {
            return Vec::new();
        }
        let before = std::mem::replace(&mut project.worktree_parent, parent.clone());
        self.change(
            id,
            Thing::WorktreeParent,
            Value::WorktreeParent(before),
            Value::WorktreeParent(parent),
            false,
        )
    }

    /// Name project `id` `name`, trimmed, written now; a name blank or
    /// unchanged changes nothing.
    #[must_use = "its effects are to be carried"]
    pub fn rename(&mut self, id: &ProjectId, name: &str) -> Vec<Effect> {
        let name = name.trim();
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if name.is_empty() || project.name == name {
            return Vec::new();
        }
        let before = std::mem::replace(&mut project.name, name.to_string());
        self.change(
            id,
            Thing::Name,
            Value::Name(before),
            Value::Name(name.to_string()),
            false,
        )
    }

    /// Set project `id`'s check command to `typed`, none when blank; written
    /// once the typing stops. Unchanged, nothing happens.
    #[must_use = "its effects are to be carried"]
    pub fn set_check(&mut self, id: &ProjectId, typed: &str) -> Vec<Effect> {
        let typed = typed.trim();
        let check = (!typed.is_empty()).then(|| typed.to_string());
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if project.check == check {
            return Vec::new();
        }
        let before = std::mem::replace(&mut project.check, check.clone());
        self.change(
            id,
            Thing::Check,
            Value::Check(before),
            Value::Check(check),
            true,
        )
    }

    /// Set what a session sent from project `id`'s page answers of its cards.
    #[must_use = "its effects are to be carried"]
    pub fn set_cards(&mut self, id: &ProjectId, cards: crate::chat::Cards) -> Vec<Effect> {
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if project.cards == cards {
            return Vec::new();
        }
        let before = std::mem::replace(&mut project.cards, cards);
        self.change(
            id,
            Thing::Cards,
            Value::Cards(before),
            Value::Cards(cards),
            true,
        )
    }

    /// Make `entry` one of project `id`'s build hosts: in place of the one at
    /// `replacing`, or added at the end. Refused, in words, for a project
    /// whose sessions run on a host, which builds there already, and for a
    /// host the project already builds on. Written now; unchanged, nothing
    /// happens.
    pub fn put_build_host(
        &mut self,
        id: &ProjectId,
        entry: BuildHost,
        replacing: Option<usize>,
    ) -> Result<Vec<Effect>, String> {
        let Some(project) = self.projects.get(id) else {
            return Ok(Vec::new());
        };
        if project.host.is_some() || project.offload.is_some() {
            return Err(format!(
                "{} runs its sessions on a host; it builds there already",
                project.name
            ));
        }
        let taken = project
            .builds
            .iter()
            .enumerate()
            .any(|(at, held)| Some(at) != replacing && held.host == entry.host);
        if taken {
            return Err(format!("{} already builds on {}", project.name, entry.host));
        }
        let mut builds = project.builds.clone();
        match replacing.and_then(|at| builds.get_mut(at)) {
            Some(held) => *held = entry,
            None => builds.push(entry),
        }
        Ok(self.set_builds(id, builds))
    }

    /// Stop project `id` building on `entry`: named by what it is, not by its
    /// place, which may have moved since it was shown. One that is gone
    /// already changes nothing.
    #[must_use = "its effects are to be carried"]
    pub fn remove_build_host(&mut self, id: &ProjectId, entry: &BuildHost) -> Vec<Effect> {
        let Some(project) = self.projects.get(id) else {
            return Vec::new();
        };
        let builds: Vec<BuildHost> = project
            .builds
            .iter()
            .filter(|held| *held != entry)
            .cloned()
            .collect();
        if project.builds == builds {
            return Vec::new();
        }
        // The pick goes with its host, in the same write.
        if project.builds_on.as_ref() == Some(&entry.host) {
            let project = self.projects.get_mut(id).expect("just read");
            let before = project.builds_on.take();
            self.saves.entry(id.clone()).or_default().change(
                Thing::BuildsOn,
                Value::BuildsOn(before),
                Value::BuildsOn(None),
            );
        }
        self.set_builds(id, builds)
    }

    fn set_builds(&mut self, id: &ProjectId, builds: Vec<BuildHost>) -> Vec<Effect> {
        let Some(project) = self.projects.get_mut(id) else {
            return Vec::new();
        };
        if project.builds == builds {
            return Vec::new();
        }
        let before = std::mem::replace(&mut project.builds, builds.clone());
        self.change(
            id,
            Thing::Builds,
            Value::Builds(before),
            Value::Builds(builds),
            false,
        )
    }

    /// Say where the sessions of `folder` on `host` run: on `on`, offloaded,
    /// or here again with none. Each folder of a project is offloaded on its
    /// own. Written once things are still, as an offload's other writes are;
    /// `None` when no project holds the folder.
    #[must_use = "its effects are to be carried"]
    pub fn set_offload(
        &mut self,
        host: Option<&HostName>,
        folder: &Path,
        on: Option<Offloaded>,
    ) -> Option<Vec<Effect>> {
        let (project, at) = self.projects.find(host, folder)?;
        let id = project.id.clone();
        let before = project.offload_of(at).cloned();
        if before == on {
            return Some(Vec::new());
        }
        self.put_offload(&id, folder, on.clone());
        let folder = folder.to_path_buf();
        Some(self.change(
            &id,
            Thing::Offload(folder.clone()),
            Value::Offload {
                folder: folder.clone(),
                on: before,
            },
            Value::Offload { folder, on },
            true,
        ))
    }

    fn put_offload(&mut self, id: &ProjectId, folder: &Path, on: Option<Offloaded>) {
        let Some(project) = self.projects.get_mut(id) else {
            return;
        };
        if project.path == folder {
            project.offload = on;
        } else if let Some(worktree) = project.worktrees.iter_mut().find(|w| w.path == folder) {
            worktree.offload = on;
        }
    }

    /// Take into project `id` the facts the row of `folder` carried before
    /// it was placed (a check command, build hosts, the folder's offload),
    /// each only where the project has none: a row an older build wrote, or
    /// one a migration that could not write left with its own. Written now.
    #[must_use = "its effects are to be carried"]
    pub fn take_facts(
        &mut self,
        id: &ProjectId,
        folder: &Path,
        check: Option<String>,
        builds: Vec<BuildHost>,
        offload: Option<Offloaded>,
    ) -> Vec<Effect> {
        let Some(project) = self.projects.get(id) else {
            return Vec::new();
        };
        let Some(at) = project.folder(project.host.as_ref(), folder) else {
            return Vec::new();
        };
        let local = project.host.is_none();
        let check = check.filter(|_| project.check.is_none());
        let builds = (!builds.is_empty() && project.builds.is_empty()).then_some(builds);
        let offload = offload.filter(|_| local && project.offload_of(at).is_none());
        let mut changes = Vec::new();
        if let Some(check) = check {
            changes.push((Thing::Check, Value::Check(None), Value::Check(Some(check))));
        }
        if let Some(builds) = builds {
            changes.push((
                Thing::Builds,
                Value::Builds(Vec::new()),
                Value::Builds(builds),
            ));
        }
        if let Some(on) = offload {
            let folder = folder.to_path_buf();
            changes.push((
                Thing::Offload(folder.clone()),
                Value::Offload {
                    folder: folder.clone(),
                    on: None,
                },
                Value::Offload {
                    folder,
                    on: Some(on),
                },
            ));
        }
        if changes.is_empty() {
            return Vec::new();
        }
        let saves = self.saves.entry(id.clone()).or_default();
        for (thing, before, after) in &changes {
            saves.change(thing.clone(), before.clone(), after.clone());
        }
        for (_, _, after) in changes {
            self.put(id, after);
        }
        let mut fx: Vec<Effect> = self.write(id).into_iter().collect();
        fx.push(Effect::Changed(id.clone()));
        fx
    }

    /// Write every project whose change still waits on a burst, now: what a
    /// window closing or the app quitting asks, so the change made just
    /// before it is not lost with it.
    #[must_use = "its effects are to be carried"]
    pub fn flush(&mut self) -> Vec<Effect> {
        self.soon = None;
        let waiting = std::mem::take(&mut self.waiting);
        waiting.iter().filter_map(|id| self.write(id)).collect()
    }

    /// What the front end reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Written {
                id,
                pending,
                result,
            } => {
                let Some(saves) = self.saves.get_mut(&id) else {
                    return Vec::new();
                };
                let Some((why, back)) = saves.landed(pending, result) else {
                    return Vec::new();
                };
                for value in back {
                    self.put(&id, value);
                }
                let name = self
                    .projects
                    .get(&id)
                    .map(|project| project.name.clone())
                    .unwrap_or_default();
                vec![
                    Effect::Changed(id.clone()),
                    Effect::Signal(Signal::SaveFailed { id, name, why }),
                ]
            }
            Input::Elapsed { token } => {
                if self.soon != Some(token) {
                    return Vec::new();
                }
                self.soon = None;
                let waiting = std::mem::take(&mut self.waiting);
                waiting.iter().filter_map(|id| self.write(id)).collect()
            }
        }
    }

    /// Put `value` back into project `id`.
    fn put(&mut self, id: &ProjectId, value: Value) {
        match value {
            Value::Offload { folder, on } => self.put_offload(id, &folder, on),
            Value::Name(name) => {
                if let Some(project) = self.projects.get_mut(id) {
                    project.name = name;
                }
            }
            Value::Check(check) => {
                if let Some(project) = self.projects.get_mut(id) {
                    project.check = check;
                }
            }
            Value::Builds(builds) => {
                if let Some(project) = self.projects.get_mut(id) {
                    project.builds = builds;
                }
            }
            Value::Routes(routes) => {
                if let Some(project) = self.projects.get_mut(id) {
                    project.routes = routes;
                }
            }
            Value::BuildsOn(host) => {
                if let Some(project) = self.projects.get_mut(id) {
                    project.builds_on = host;
                }
            }
            Value::Cards(cards) => {
                if let Some(project) = self.projects.get_mut(id) {
                    project.cards = cards;
                }
            }
            Value::WorktreeParent(parent) => {
                if let Some(project) = self.projects.get_mut(id) {
                    project.worktree_parent = parent;
                }
            }
            Value::Worktrees(mut worktrees) => {
                if let Some(project) = self.projects.get_mut(id) {
                    // Each folder's offload is a thing of its own, and stays
                    // as it now is.
                    for worktree in &mut worktrees {
                        if let Some(now) =
                            project.worktrees.iter().find(|w| w.path == worktree.path)
                        {
                            worktree.offload = now.offload.clone();
                        }
                    }
                    project.worktrees = worktrees;
                }
            }
        }
    }

    /// Change `thing` of project `id` from `before` to `after`, as already
    /// done to the project, and write it now (`soon` false) or after the
    /// burst; then say the project changed.
    fn change(
        &mut self,
        id: &ProjectId,
        thing: Thing,
        before: Value,
        after: Value,
        soon: bool,
    ) -> Vec<Effect> {
        self.saves
            .entry(id.clone())
            .or_default()
            .change(thing, before, after);
        let mut fx = match soon {
            true => self.save_soon(id),
            false => self.write(id).into_iter().collect(),
        };
        fx.push(Effect::Changed(id.clone()));
        fx
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

    /// The whole of project `id` as it now stands, and where it goes. Carries
    /// a change waiting on a burst too, so that one is not written again.
    fn write(&mut self, id: &ProjectId) -> Option<Effect> {
        let project = self.projects.get(id)?.clone();
        self.waiting.remove(id);
        let pending = self.saves.entry(id.clone()).or_default().write_all();
        Some(Effect::Write(Box::new(ProjectWrite {
            id: id.clone(),
            pending,
            dir: self.dir.join(id.as_str()),
            project,
        })))
    }
}

mod routes;

#[cfg(test)]
mod tests;
