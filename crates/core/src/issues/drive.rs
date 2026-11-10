//! What happens next for one project's issues file: reading it, showing a
//! change before its write lands, keeping it in step with its forge, and
//! sending an issue there.
//!
//! **One per file for the whole process.** Every view of a file (each window's
//! Issues tab and Issues page) reads the one drive, so a change through one is
//! seen by all at once and a sync runs once however many views are open.
//! What a view picks, filters or types stays with that view.
//!
//! **A change is shown at once, and what comes in meanwhile waits.** A change
//! is made on the copy on screen, then written after every change before it
//! by a read-change-write of the file; a read or a sync landing before that
//! write would put the change back, so while any write is on its way what
//! comes in is held, and once the last lands the newest of what came in and
//! what the writes left is shown ([`Pending`]).
//!
//! **A sync runs when it is due.** Only on a file kept in step with the forge
//! serving its project; at once when something asked for it (a change to
//! send, a press of *Sync now*), else once [`SYNC_GAP`] has passed since the
//! last. Asked for while one runs, it runs once more when that lands. The
//! drive never reads the clock: the time is told it.

use super::sync::Report;
use super::template::{self, IssueTemplate};
use super::{Issues, LocalIssue};
use crate::connector::{self, Connector};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a file kept in step waits between the syncs that only come round.
pub const SYNC_GAP: Duration = Duration::from_secs(60);

/// What a file is, read through its drive.
#[derive(Default)]
pub struct Kept {
    /// `None` until the first read lands.
    pub issues: Option<Issues>,
    /// Why the last read failed, when it did.
    pub failed: Option<String>,
    /// The forge that serves the project, if one does: found as the file is
    /// read, since asking reads the project's git remote.
    pub forge: Option<&'static dyn Connector>,
    /// The issue templates the project offers, its own or the shipped ones.
    pub templates: Vec<IssueTemplate>,
    /// A sync is on its way.
    pub syncing: bool,
    /// When the last sync finished, in seconds since the epoch, and what it
    /// came to. For this process only: a restart syncs again at once.
    pub synced: Option<(u64, Result<Report, String>)>,
    /// The issue being sent to the forge, while it is.
    pub publishing: Option<u64>,
}

/// What the front end carries out.
pub enum Effect {
    /// Read the file, the forge serving its project and its templates
    /// ([`read_blocking`]), off the UI loop, and report [`Input::Read`] with
    /// `token`. A late answer is **dropped** by its token: a newer read
    /// supersedes it.
    Read { token: u64 },
    /// Keep the file in step with `forge` ([`super::sync::sync_blocking`] at `now`),
    /// off the UI loop, and report [`Input::Synced`]. A late answer is
    /// **recorded**: what it wrote is on disk whatever changed since.
    Sync {
        forge: &'static dyn Connector,
        now: u64,
    },
    /// Send issue `number` to `forge` ([`super::sync::publish_blocking`]), off the UI
    /// loop, and report [`Input::Published`]. A late answer is **recorded**:
    /// the issue may be on the forge.
    Publish {
        number: u64,
        forge: &'static dyn Connector,
    },
    /// What the file is changed: whatever draws it draws it again.
    Changed,
}

/// What the front end reports back.
pub enum Input {
    /// What read `token` found, landing at `now`.
    Read {
        token: u64,
        read: Result<Issues, String>,
        forge: Option<&'static dyn Connector>,
        templates: Vec<IssueTemplate>,
        now: u64,
    },
    /// What the sync came to, at `now`.
    Synced {
        done: Result<(Issues, Report), String>,
        now: u64,
    },
    /// Change `of`'s write landed, with what it left on disk if it was done.
    Written {
        of: u64,
        kept: Option<Issues>,
        now: u64,
    },
    /// Sending issue `number` to the forge came to this.
    Published {
        number: u64,
        done: Result<(), String>,
    },
}

/// One issues file's drive. See the module header.
pub struct FileDrive {
    kept: Kept,
    root: PathBuf,
    path: PathBuf,
    connectors: &'static [&'static dyn Connector],
    /// Something other than a view may have written the file (a run leaving
    /// a note), so it is read again the next time a view draws it.
    stale: bool,
    /// The newest read's token.
    reading: u64,
    /// A sync was asked for while one ran.
    again: bool,
    pending: Pending,
}

impl Deref for FileDrive {
    type Target = Kept;
    fn deref(&self) -> &Kept {
        &self.kept
    }
}

impl FileDrive {
    /// The file `path`, keeping the issues of the project at `root`, the
    /// forge found among `connectors`; nothing read yet.
    pub fn new(
        root: PathBuf,
        path: PathBuf,
        connectors: &'static [&'static dyn Connector],
    ) -> Self {
        Self {
            kept: Kept {
                templates: template::shipped(),
                ..Kept::default()
            },
            root,
            path,
            connectors,
            stale: true,
            reading: 0,
            again: false,
            pending: Pending::default(),
        }
    }

    /// The project the file keeps the issues of, and the file.
    pub fn place(&self) -> (&Path, &Path) {
        (&self.root, &self.path)
    }

    /// The connectors a read looks for the serving forge among.
    pub fn connectors(&self) -> &'static [&'static dyn Connector] {
        self.connectors
    }

    /// Something may have written the file: read it the next time it is drawn.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Read the file if something marked it since it was last read.
    #[must_use = "its effects are to be carried"]
    pub fn load_if_stale(&mut self) -> Vec<Effect> {
        match std::mem::take(&mut self.stale) {
            true => self.load(),
            false => Vec::new(),
        }
    }

    /// Read the file, then sync it if that is due.
    #[must_use = "its effects are to be carried"]
    pub fn load(&mut self) -> Vec<Effect> {
        self.stale = false;
        self.reading += 1;
        vec![Effect::Read {
            token: self.reading,
        }]
    }

    /// Show `change` at once, before its write; a change that does not apply
    /// to what is shown is left for its write to refuse.
    #[must_use = "its effects are to be carried"]
    pub fn show<R>(
        &mut self,
        change: impl FnOnce(&mut Issues) -> Result<R, String>,
    ) -> Vec<Effect> {
        match self.kept.issues.as_mut().map(change) {
            Some(Ok(_)) => vec![Effect::Changed],
            Some(Err(_)) | None => Vec::new(),
        }
    }

    /// A change is about to be written: which one it is.
    pub fn send(&mut self) -> u64 {
        self.pending.send()
    }

    /// Whether change `of` is the newest one made.
    pub fn newest(&self, of: u64) -> bool {
        self.pending.newest(of)
    }

    /// Keep the file in step with its forge, at `now`, if it is kept in step
    /// with one and a sync is due; `asked` when something asked for it.
    #[must_use = "its effects are to be carried"]
    pub fn sync(&mut self, asked: bool, now: u64) -> Vec<Effect> {
        let (Some(forge), Some(kept)) = (self.kept.forge, self.kept.issues.as_ref()) else {
            return Vec::new();
        };
        let wanted = kept.in_step_with(forge.name());
        let due = asked
            || self
                .kept
                .synced
                .as_ref()
                .is_none_or(|(at, _)| now.saturating_sub(*at) >= SYNC_GAP.as_secs());
        if self.kept.syncing && wanted && asked {
            self.again = true;
            return Vec::new();
        }
        if !wanted || !due || self.kept.syncing {
            return Vec::new();
        }
        self.kept.syncing = true;
        vec![Effect::Changed, Effect::Sync { forge, now }]
    }

    /// Send issue `number` to the forge, the only way anything written here
    /// reaches it; nothing without a forge, or while one is being sent.
    #[must_use = "its effects are to be carried"]
    pub fn publish(&mut self, number: u64) -> Vec<Effect> {
        let Some(forge) = self.kept.forge.filter(|_| self.kept.publishing.is_none()) else {
            return Vec::new();
        };
        self.kept.publishing = Some(number);
        vec![Effect::Changed, Effect::Publish { number, forge }]
    }

    /// What the front end reports back.
    #[must_use = "its effects are to be carried"]
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        match input {
            Input::Read {
                token,
                read,
                forge,
                templates,
                now,
            } => {
                if token != self.reading {
                    return Vec::new();
                }
                self.kept.forge = forge;
                self.kept.templates = templates;
                match read {
                    Ok(read) => {
                        self.pending.incoming(&mut self.kept.issues, read);
                        self.kept.failed = None;
                    }
                    Err(why) => self.kept.failed = Some(why),
                }
                let mut fx = vec![Effect::Changed];
                fx.extend(self.sync(false, now));
                fx
            }
            Input::Synced { done, now } => {
                self.kept.syncing = false;
                let came_to = match done {
                    Ok((kept, report)) => {
                        self.pending.incoming(&mut self.kept.issues, kept);
                        Ok(report)
                    }
                    Err(why) => Err(why),
                };
                self.kept.synced = Some((now, came_to));
                let mut fx = vec![Effect::Changed];
                if std::mem::take(&mut self.again) {
                    fx.extend(self.sync(true, now));
                }
                fx
            }
            Input::Written { of, kept, now } => {
                let done = kept.is_some();
                self.pending.land(&mut self.kept.issues, kept);
                let mut fx = vec![Effect::Changed];
                match (done, self.newest(of)) {
                    // Sent at once, so an edit reaches the forge without
                    // waiting for the gap.
                    (true, _) => fx.extend(self.sync(true, now)),
                    // The newest change refused: what is on disk is shown.
                    (false, true) => fx.extend(self.load()),
                    (false, false) => {}
                }
                fx
            }
            Input::Published { number, done: _ } => {
                if self.kept.publishing == Some(number) {
                    self.kept.publishing = None;
                }
                let mut fx = vec![Effect::Changed];
                fx.extend(self.load());
                fx
            }
        }
    }
}

/// Read `path`, the forge serving `root` among `connectors`, and the project's
/// templates: what [`Effect::Read`] does. Blocking.
pub fn read_blocking(
    root: &Path,
    path: &Path,
    connectors: &'static [&'static dyn Connector],
) -> (
    Result<Issues, String>,
    Option<&'static dyn Connector>,
    Vec<IssueTemplate>,
) {
    let forge = connector::serving(connectors, root)
        .ok()
        .map(|at| connectors[at]);
    let templates = template::for_project_blocking(root);
    (super::load_blocking(path), forge, templates)
}

/// Whether issue `issue` of `kept` may be sent to `forge`: a file kept in step
/// with it, an issue not there yet.
pub fn publishable(kept: &Issues, forge: Option<&dyn Connector>, issue: &LocalIssue) -> bool {
    forge.is_some_and(|forge| kept.in_step_with(forge.name())) && issue.link.is_none()
}

impl LocalIssue {
    /// The live session working this issue: the latest conversation its
    /// history names that is among `live`.
    pub fn working_in<'a>(&'a self, live: &[String]) -> Option<&'a str> {
        self.notes
            .iter()
            .rev()
            .filter_map(|note| note.session.as_deref())
            .find(|session| live.iter().any(|id| id == session))
    }
}

/// The writes still on their way, and what was read meanwhile. A change is
/// shown before its write lands, on a copy whose revision is the one it was
/// made on; a read or a sync landing before the write would be no older than
/// that copy, and would put the change back until its write landed. So while
/// any write is on its way what comes in is held, and once the last one
/// lands the newest of what came in and what the writes left is shown.
#[derive(Default)]
struct Pending {
    /// The newest change sent to be written: the one whose write decides
    /// what to show should it fail.
    sent: u64,
    /// How many writes have landed, done or refused.
    landed: u64,
    /// The newest copy that came in while a write was on its way.
    held: Option<Issues>,
}

impl Pending {
    /// Take a copy read or synced from the file.
    fn incoming(&mut self, shown: &mut Option<Issues>, copy: Issues) {
        if self.landed < self.sent && shown.is_some() {
            keep_newer(&mut self.held, copy);
        } else {
            keep_newer(shown, copy);
        }
    }

    /// A change is sent to be written: which one it is.
    fn send(&mut self) -> u64 {
        self.sent += 1;
        self.sent
    }

    fn newest(&self, of: u64) -> bool {
        of == self.sent
    }

    /// A write landed, with what it left on disk if it was done. Once none is
    /// on its way, the newest copy to come in since is what is shown.
    fn land(&mut self, shown: &mut Option<Issues>, kept: Option<Issues>) {
        self.landed += 1;
        if let Some(kept) = kept {
            keep_newer(&mut self.held, kept);
        }
        if self.landed >= self.sent {
            if let Some(held) = self.held.take() {
                keep_newer(shown, held);
            }
        }
    }
}

/// Put `incoming` on screen unless what is there was written later. Two reads
/// or writes finish in whatever order they finish, which is not always the
/// order they reached the disk in, and the older landing second would put an
/// edit back the way it was.
fn keep_newer(shown: &mut Option<Issues>, incoming: Issues) {
    if shown
        .as_ref()
        .is_none_or(|shown| incoming.revision() >= shown.revision())
    {
        *shown = Some(incoming);
    }
}

#[cfg(test)]
mod tests;
