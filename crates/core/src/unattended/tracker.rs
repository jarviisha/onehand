//! Where an issue lives, and so how a run finds it, claims it and tells it:
//! on a forge, kept by onehand, or kept by onehand in step with a forge.

use super::{lives_on_forge, named, shown, Issue, IssueRow};
use crate::connector::Connector;
use crate::issues;
use crate::issues::sync::OutsideSync;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where an issue lives, and so where a run on it is claimed, commented on and
/// found.
///
/// Kept apart from the forge a project's pull requests go to, because the two
/// come apart: an issue kept in onehand can be worked in a project on GitHub,
/// where the work still ends in a pull request, or in a project on no forge at
/// all, where it ends on a branch.
#[derive(Clone)]
pub enum Tracker {
    /// On a forge, reached through its connector.
    Forge(&'static dyn Connector),
    /// Kept by onehand, in this project's issue file.
    Local(PathBuf),
    /// Kept by onehand and kept in step with a forge: searched, claimed and
    /// told here, and every one of those reaches the forge through the sync —
    /// so a project that is synced is never searched on the forge as well, and
    /// no issue is found twice.
    Synced {
        file: PathBuf,
        forge: &'static dyn Connector,
    },
}

impl Tracker {
    /// How `issue` is shown to a person: by the forge's number where it lives
    /// on the forge or is kept in step with one, and as a draft where it is
    /// kept in onehand only — the number it is filed under there is a key,
    /// and beside a forge's own it reads as a second issue.
    pub fn shown(&self, issue: &Issue) -> String {
        shown(
            lives_on_forge(self.to_ref()),
            issue.number,
            issue.forge_ref(),
        )
    }

    /// How `issue` is named in a sentence: as [`Tracker::shown`] says, and by
    /// its title where it is kept in onehand only, since *Draft* names none.
    pub fn named(&self, issue: &Issue) -> String {
        named(
            lives_on_forge(self.to_ref()),
            issue.number,
            issue.forge_ref(),
            &issue.title,
        )
    }

    /// How a run's brief names `issue`, in what every step is asked.
    pub(super) fn names(&self, issue: &Issue) -> String {
        let number = issue.number;
        match (self, issue.forge_ref()) {
            (Self::Forge(c), _) => format!("{} issue #{number}", c.name()),
            (Self::Synced { forge, .. }, Some(reference)) => {
                format!("{} issue {reference}", forge.name())
            }
            (Self::Local(_) | Self::Synced { .. }, _) => {
                format!("issue #{number}, which is kept in onehand rather than on a forge")
            }
        }
    }

    /// Open issues carrying `label` that the user wrote.
    ///
    /// Kept here, that is the ones **written** here — never one brought in
    /// from a forge, linked or not, since somebody else may have written it
    /// and only the forge can say who. Such an issue is found through
    /// [`Self::Synced`], which asks, or not at all: a sync switched off, a
    /// forge unreachable for one tick, or a link lost must not turn somebody
    /// else's text into a prompt run with the user's credentials.
    ///
    /// A sync it made is put in `synced`.
    pub(super) fn labelled_blocking(
        &self,
        root: &Path,
        label: &str,
        synced: &mut Vec<OutsideSync>,
    ) -> Result<Vec<IssueRow>, String> {
        match self {
            Self::Forge(c) => c.my_labelled_issues_blocking(root, label),
            Self::Local(file) => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.written_here() && i.open && i.labels.iter().any(|l| l == label))
                .map(|i| kept_row(i, None))
                .collect()),
            // Synced first, so a label put on at the forge is seen. An issue
            // brought in from the forge may be anybody's, so it is taken only
            // if the forge says the user wrote it — the same rule the forge's
            // own search keeps, asked of the forge because only it knows.
            Self::Synced { file, forge } => {
                let (kept, report) =
                    issues::sync::sync_blocking(file, root, *forge, issues::now())?;
                synced.push(OutsideSync {
                    file: file.clone(),
                    kept: kept.clone(),
                    report,
                });
                let mine: Vec<String> = forge
                    .my_labelled_issues_blocking(root, label)?
                    .into_iter()
                    .map(|row| row.issue.number.to_string())
                    .collect();
                Ok(kept
                    .listed()
                    .into_iter()
                    .filter(|i| i.open && i.labels.iter().any(|l| l == label))
                    .filter_map(|i| {
                        if i.written_here() {
                            return Some(kept_row(i, None));
                        }
                        let link = i.link_on(forge.name()).filter(|l| mine.contains(&l.key))?;
                        Some(kept_row(i, Some(link.reference.clone())))
                    })
                    .collect())
            }
        }
    }

    /// Open issues, newest first, at most `limit` of them, for a person to pick
    /// from. One kept here has no author on its row. Unsynced, only the ones
    /// written here are listed — a linked one is on the forge's list already.
    /// Synced, every one is, and a linked one carries its forge reference so
    /// its pull request names it there.
    pub(super) fn open_blocking(&self, root: &Path, limit: usize) -> Result<Vec<IssueRow>, String> {
        let row = kept_row;
        match self {
            Self::Forge(c) => c.open_issues_blocking(root, limit),
            Self::Local(file) => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.open && i.written_here())
                .take(limit)
                .map(|i| row(i, None))
                .collect()),
            Self::Synced { file, forge } => Ok(issues::load_blocking(file)?
                .listed()
                .into_iter()
                .filter(|i| i.open)
                .take(limit)
                .map(|i| row(i, i.link_on(forge.name()).map(|l| l.reference.clone())))
                .collect()),
        }
    }

    /// Take `label` off issue `number`; a sync it made is put in `synced`.
    pub(super) fn remove_label_blocking(
        &self,
        root: &Path,
        number: u64,
        label: &str,
        synced: &mut Vec<OutsideSync>,
    ) -> Result<(), String> {
        match self {
            Self::Forge(c) => c.remove_label_blocking(root, number, label),
            Self::Local(file) => {
                issues::drive::write_blocking(file, unlabel(number, label), issues::now()).map(drop)
            }
            // Off here first, which is the claim; the sync then takes it off
            // the forge. A sync that fails now is retried by the next, and the
            // issue is not found again meanwhile because only this side is
            // searched.
            Self::Synced { file, forge } => {
                issues::drive::write_blocking(file, unlabel(number, label), issues::now())?;
                if let Ok((kept, report)) =
                    issues::sync::sync_blocking(file, root, *forge, issues::now())
                {
                    synced.push(OutsideSync {
                        file: file.clone(),
                        kept,
                        report,
                    });
                }
                Ok(())
            }
        }
    }

    /// Leave `body` on issue `number`: a comment on a forge, a note on an issue
    /// kept in onehand.
    pub fn comment_blocking(&self, root: &Path, number: u64, body: &str) -> Result<(), String> {
        match self {
            Self::Forge(c) => c.comment_blocking(root, number, body),
            Self::Local(file) => {
                issues::drive::write_blocking(file, note(number, body), issues::now()).map(drop)
            }
            // A note here, and the same words on the forge's issue when it has
            // one — somebody reading it there should hear what happened too.
            Self::Synced { file, forge } => {
                let (kept, _) =
                    issues::drive::write_blocking(file, note(number, body), issues::now())?;
                let key = kept
                    .get(number)
                    .and_then(|i| i.link_on(forge.name()))
                    .and_then(|link| link.key.parse().ok());
                match key {
                    Some(key) => forge.comment_blocking(root, key, body),
                    None => Ok(()),
                }
            }
        }
    }
}

/// `label` taken off issue `number`, as a change to its file.
fn unlabel(number: u64, label: &str) -> issues::drive::Change {
    issues::drive::Change::RemoveLabel {
        number,
        label: label.to_string(),
    }
}

/// `body` said on issue `number`, as a change to its file.
fn note(number: u64, body: &str) -> issues::drive::Change {
    issues::drive::Change::Note {
        number,
        text: body.to_string(),
    }
}

/// An issue kept here as a row: no author, since only a forge knows who
/// wrote one, and known on its forge as `forge_ref` when it is kept in step.
fn kept_row(i: &issues::LocalIssue, forge_ref: Option<String>) -> IssueRow {
    let issue = Issue::from(i);
    IssueRow {
        issue: match forge_ref {
            Some(reference) => issue.at(reference),
            None => issue,
        },
        author: String::new(),
        labels: i.labels.clone(),
    }
}

/// Where an issue lives, as a task keeps it: [`Tracker`] with its connector
/// named rather than held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackerRef {
    Forge { connector: String },
    Local { file: PathBuf },
    Synced { file: PathBuf, connector: String },
}

impl TrackerRef {
    /// The tracker it names, among the connectors in `all`; `None` when its
    /// connector is not one of them.
    pub fn resolve(&self, all: &[&'static dyn Connector]) -> Option<Tracker> {
        let named = |name: &str| crate::connector::named(all, name);
        Some(match self {
            Self::Forge { connector } => Tracker::Forge(named(connector)?),
            Self::Local { file } => Tracker::Local(file.clone()),
            Self::Synced { file, connector } => Tracker::Synced {
                file: file.clone(),
                forge: named(connector)?,
            },
        })
    }
}

impl Tracker {
    /// The tracker as a task keeps it.
    pub fn to_ref(&self) -> TrackerRef {
        match self {
            Self::Forge(forge) => TrackerRef::Forge {
                connector: forge.name().to_string(),
            },
            Self::Local(file) => TrackerRef::Local { file: file.clone() },
            Self::Synced { file, forge } => TrackerRef::Synced {
                file: file.clone(),
                connector: forge.name().to_string(),
            },
        }
    }
}
