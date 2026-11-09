//! A run's end and what its issue is told: its project kept or dropped,
//! and its reports sent once each, oldest first.

use super::{Effect, Sending, Signal, UnattendedDrive};
use crate::connector;
use crate::task::tasks::link::Seen;
use crate::task::Task;
use crate::unattended::TrackerRef;
use crate::workflow::{Outcome, Stop};

impl UnattendedDrive {
    /// The agent of issue task `id` came up in the session named `session`:
    /// an issue kept in onehand notes which conversation works it. One that
    /// lives only on the forge has nowhere to keep it.
    pub fn came_up<W: Copy>(
        &self,
        id: &str,
        session: Option<String>,
        seen: &Seen<W>,
    ) -> Vec<Effect<W>> {
        let Some(task) = seen.task(id) else {
            return Vec::new();
        };
        let Some(issue) = task.issue() else {
            return Vec::new();
        };
        let (TrackerRef::Local { file } | TrackerRef::Synced { file, .. }) = issue.tracker.clone()
        else {
            return Vec::new();
        };
        let Some(session) = session else {
            return Vec::new();
        };
        let said = format!(
            "Taken up by an unattended run in {}, on branch {}",
            task.setup.dir.display(),
            task.setup.branch.as_deref().unwrap_or_default()
        );
        vec![Effect::NoteTakenUp {
            file,
            number: issue.number,
            said,
            session,
        }]
    }

    /// Issue task `id`'s run has ended and given its place up: drop its
    /// project unless a person has it, then tell the issue how it ended.
    pub fn ended<W: Copy>(&mut self, id: &str, seen: &Seen<W>) -> Vec<Effect<W>> {
        let Some(task) = seen.task(id) else {
            return Vec::new();
        };
        let (Some(issue), Some(run)) = (task.issue(), task.runs.last()) else {
            return Vec::new();
        };
        let Some(outcome) = &run.outcome else {
            return Vec::new();
        };
        // A run picked by hand stays where the person watching it can read
        // how it ended, and a taken-over one is a person's now.
        let keep = issue.picked || *outcome == Outcome::Stopped(Stop::TakenOver);
        let mut fx = vec![Effect::Teardown {
            dir: task.setup.dir.clone(),
            keep,
        }];
        fx.extend(self.deliver(id, seen));
        fx
    }

    /// Send what issue task `id` has not told its issue yet, oldest first.
    /// A delivery already on its way sends nothing more.
    pub fn deliver<W: Copy>(&mut self, id: &str, seen: &Seen<W>) -> Vec<Effect<W>> {
        let Some(task) = seen.task(id) else {
            return Vec::new();
        };
        let Some(issue) = task.issue().filter(|issue| !issue.unsent.is_empty()) else {
            return Vec::new();
        };
        if !self.delivering.insert(id.to_string()) {
            return Vec::new();
        }
        let Some(tracker) = issue.tracker.resolve(self.connectors) else {
            self.delivering.remove(id);
            return vec![Effect::Signal(Signal::Log(format!(
                "issue {} cannot be told how its run ended: its tracker is not available",
                issue.shown()
            )))];
        };
        vec![Effect::Send(Box::new(Sending {
            task: id.to_string(),
            tracker,
            number: issue.number,
            forge: issue
                .forge
                .as_deref()
                .and_then(|name| connector::named(self.connectors, name)),
            base: issue.base.clone(),
            repo: task.setup.repo.clone(),
            dir: task.setup.dir.clone(),
            branch: task.setup.branch.clone().unwrap_or_default(),
            unsent: issue.unsent.clone(),
        }))]
    }

    /// Send every report not yet delivered, of every issue task.
    pub fn deliver_all<W: Copy>(&mut self, seen: &Seen<W>) -> Vec<Effect<W>> {
        let undelivered: Vec<String> = seen
            .all()
            .iter()
            .filter(|task| task.issue().is_some_and(|issue| !issue.unsent.is_empty()))
            .map(|task| task.id.clone())
            .collect();
        undelivered
            .iter()
            .flat_map(|id| self.deliver(id, seen))
            .collect()
    }
}

/// The lines a run's transcript opens with: which issue, how it was chosen,
/// and where the work is happening. Short lines, one fact each.
pub fn opening(task: &Task) -> Vec<String> {
    let Some(issue) = task.issue() else {
        return Vec::new();
    };
    let folder = task.setup.dir.file_name().map_or_else(
        || task.setup.dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    vec![
        format!(
            "Unattended run on issue {}, {}",
            issue.shown(),
            match issue.picked {
                true => "picked by hand",
                false => "found by its label",
            }
        ),
        format!(
            "Branch {} from {}",
            task.setup.branch.as_deref().unwrap_or("?"),
            issue.base
        ),
        format!("Working in {folder}"),
    ]
}
