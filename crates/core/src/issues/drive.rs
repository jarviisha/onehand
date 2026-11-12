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
use super::{Draft, Issues, LocalIssue};
use crate::chat::Now;
use crate::connector::{self, Connector};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a file kept in step waits between the syncs that only come round.
pub(crate) const SYNC_GAP: Duration = Duration::from_secs(60);

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

/// A change a person makes to an issues file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A new issue, as the form drafted it.
    Create(Draft),
    /// Issue `number` as the form redrafted it.
    Edit { number: u64, draft: Draft },
    /// Issue `number` opened or closed.
    SetOpen { number: u64, open: bool },
    /// Issue `number`'s conflict with its forge settled, this side's copy
    /// kept or the forge's.
    Resolve { number: u64, keep_mine: bool },
    /// The file kept in step with the forge so named, or with none.
    SyncWith(Option<String>),
    /// `text` added to what has been said about issue `number`.
    Note { number: u64, text: String },
    /// Said on issue `number`, in the words `text`, that the conversation
    /// `session` took it up.
    TakenUp {
        number: u64,
        text: String,
        session: String,
    },
    /// `label` taken off issue `number`, if it carries it.
    RemoveLabel { number: u64, label: String },
    /// A new issue, as drafted, unless an open one already has its title,
    /// case and surrounding spaces aside.
    CreateUnlessOpen(Draft),
}

/// What a change came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// It names no issue.
    Nothing,
    /// It changed the issue so numbered.
    Named(u64),
    /// It opened a new issue, so numbered.
    New(u64),
    /// An open issue, so numbered, already has the title: nothing changed.
    Already(u64),
}

impl Applied {
    /// The issue it names, for whoever made the change to select.
    pub fn issue(self) -> Option<u64> {
        match self {
            Self::Nothing => None,
            Self::Named(number) | Self::New(number) | Self::Already(number) => Some(number),
        }
    }
}

impl Change {
    /// Make the change on `kept` at `at`, seconds since the epoch, and say
    /// what it came to.
    pub fn apply(self, kept: &mut Issues, at: u64) -> Result<Applied, String> {
        match self {
            Self::Create(draft) => kept.create(draft, at).map(Applied::New),
            Self::Edit { number, draft } => kept
                .edit(number, draft, at)
                .map(|()| Applied::Named(number)),
            Self::SetOpen { number, open } => kept
                .set_open(number, open, at)
                .map(|()| Applied::Named(number)),
            Self::Resolve { number, keep_mine } => kept
                .resolve(number, keep_mine, at)
                .map(|()| Applied::Named(number)),
            Self::SyncWith(forge) => {
                kept.sync_with(forge);
                Ok(Applied::Nothing)
            }
            Self::Note { number, text } => kept
                .note(number, &text, at)
                .map(|()| Applied::Named(number)),
            Self::TakenUp {
                number,
                text,
                session,
            } => kept
                .taken_up(number, &text, session, at)
                .map(|()| Applied::Named(number)),
            Self::RemoveLabel { number, label } => kept
                .remove_label(number, &label, at)
                .map(|()| Applied::Named(number)),
            Self::CreateUnlessOpen(draft) => {
                let title = draft.title.trim().to_lowercase();
                match kept
                    .issues
                    .iter()
                    .find(|i| i.open && i.title.trim().to_lowercase() == title)
                {
                    Some(open) => Ok(Applied::Already(open.number)),
                    None => kept.create(draft, at).map(Applied::New),
                }
            }
        }
    }

    /// Whether it is shown before its write lands. A new issue unless one
    /// is open with its title is not: whether the title is taken is decided
    /// on disk alone, under the write's lock.
    fn shown_first(&self) -> bool {
        match self {
            Self::CreateUnlessOpen(_) => false,
            Self::Create(_)
            | Self::Edit { .. }
            | Self::SetOpen { .. }
            | Self::Resolve { .. }
            | Self::SyncWith(_)
            | Self::Note { .. }
            | Self::TakenUp { .. }
            | Self::RemoveLabel { .. } => true,
        }
    }
}

/// Who made a change: a view of the file, or a writer outside every view
/// (a tool, a session's note, a run's). Only a view's change can move a
/// view's selection, so only the views' changes count toward which is the
/// newest ([`FileDrive::newest`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maker {
    View,
    Outside,
}

/// Read `path`, make `change` on it at `at` and write it back, under the
/// file's lock: what is kept now, and what the change came to. A change
/// that changed nothing ([`Applied::Already`]) writes nothing.
pub fn write_blocking(path: &Path, change: Change, at: u64) -> Result<(Issues, Applied), String> {
    super::update_unless(
        path,
        |kept| change.apply(kept, at),
        |applied| matches!(applied, Applied::Already(_)),
    )
}

/// What a person is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// Change `of`'s write landed: what it came to, or why it was refused.
    /// Said to whoever made the change, whose form and selection it moves.
    Wrote {
        of: u64,
        done: Result<Applied, String>,
    },
    /// Issue `number` could not be sent to the forge: why.
    PublishFailed { number: u64, why: String },
}

/// What the front end carries out.
pub enum Effect {
    /// Read the file, make `change` on it at `at` and write it
    /// ([`write_blocking`]), off the UI loop and after every
    /// write before it, and report [`Input::Written`] with `of`. A late
    /// answer is **recorded**: the change is on disk or it is not, whatever
    /// changed since.
    Write {
        of: u64,
        change: Change,
        at: u64,
    },
    /// Read the file, the forge serving its project and its templates
    /// ([`read_blocking`]), off the UI loop, and report [`Input::Read`] with
    /// `token`. A late answer is **dropped** by its token: a newer read
    /// supersedes it.
    Read {
        token: u64,
    },
    /// Keep the file in step with `forge` ([`super::sync::sync_blocking`] at `now`),
    /// off the UI loop, and report [`Input::Synced`]. A late answer is
    /// **recorded**: what it wrote is on disk whatever changed since.
    Sync {
        forge: &'static dyn Connector,
        now: Now,
    },
    /// Send issue `number` to `forge` ([`super::sync::publish_blocking`]), off the UI
    /// loop, and report [`Input::Published`]. A late answer is **recorded**:
    /// the issue may be on the forge.
    Publish {
        number: u64,
        forge: &'static dyn Connector,
    },
    /// What the file holds changed: whatever draws it draws it again.
    Changed,
    Signal(Signal),
}

/// What the front end reports back.
pub enum Input {
    /// What read `token` found, landing at `now`.
    Read {
        token: u64,
        read: Result<Issues, String>,
        forge: Option<&'static dyn Connector>,
        templates: Vec<IssueTemplate>,
        now: Now,
    },
    /// What the sync came to, at `now`.
    Synced {
        done: Result<(Issues, Report), String>,
        now: Now,
    },
    /// Change `of`'s write landed: what it left on disk and what the change
    /// came to, or why it was refused.
    Written {
        of: u64,
        done: Result<(Issues, Applied), String>,
        now: Now,
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
    pub fn place(&self) -> (PathBuf, PathBuf) {
        (self.root.clone(), self.path.clone())
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

    /// Show `change` at once, at `now`, before its write; a change that does
    /// not apply to what is shown is left for its write to refuse.
    #[must_use = "its effects are to be carried"]
    pub fn show(&mut self, change: &Change, now: Now) -> Vec<Effect> {
        if !change.shown_first() {
            return Vec::new();
        }
        let shown = self.kept.issues.as_mut();
        match shown.map(|kept| change.clone().apply(kept, now.secs)) {
            Some(Ok(_)) => vec![Effect::Changed],
            Some(Err(_)) | None => Vec::new(),
        }
    }

    /// Write `change`, made by `maker`, after every change before it: which
    /// change it is, and the write.
    #[must_use = "its effects are to be carried"]
    pub fn write(&mut self, change: Change, maker: Maker, now: Now) -> (u64, Effect) {
        let of = self.pending.send(maker);
        let at = now.secs;
        (of, Effect::Write { of, change, at })
    }

    /// Whether change `of` is the newest one a view made.
    pub fn newest(&self, of: u64) -> bool {
        self.pending.newest(of)
    }

    /// Keep the file in step with its forge, at `now`, if it is kept in step
    /// with one and a sync is due; `asked` when something asked for it.
    #[must_use = "its effects are to be carried"]
    pub fn sync(&mut self, asked: bool, now: Now) -> Vec<Effect> {
        let (Some(forge), Some(kept)) = (self.kept.forge, self.kept.issues.as_ref()) else {
            return Vec::new();
        };
        let wanted = kept.in_step_with(forge.name());
        let due = asked
            || self
                .kept
                .synced
                .as_ref()
                .is_none_or(|(at, _)| now.secs.saturating_sub(*at) >= SYNC_GAP.as_secs());
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
                self.kept.synced = Some((now.secs, came_to));
                let mut fx = vec![Effect::Changed];
                if std::mem::take(&mut self.again) {
                    fx.extend(self.sync(true, now));
                }
                fx
            }
            Input::Written { of, done, now } => {
                let (kept, said) = match done {
                    Ok((kept, number)) => (Some(kept), Ok(number)),
                    Err(why) => (None, Err(why)),
                };
                let landed = kept.is_some();
                self.pending.land(&mut self.kept.issues, kept);
                let mut fx = vec![
                    Effect::Changed,
                    Effect::Signal(Signal::Wrote { of, done: said }),
                ];
                match (landed, self.pending.last(of)) {
                    // Sent at once, so an edit reaches the forge without
                    // waiting for the gap.
                    (true, _) => fx.extend(self.sync(true, now)),
                    // The newest change of anyone's refused: what is on disk
                    // is shown.
                    (false, true) => fx.extend(self.load()),
                    (false, false) => {}
                }
                fx
            }
            Input::Published { number, done } => {
                if self.kept.publishing == Some(number) {
                    self.kept.publishing = None;
                }
                let mut fx = vec![Effect::Changed];
                if let Err(why) = done {
                    fx.push(Effect::Signal(Signal::PublishFailed { number, why }));
                }
                fx.extend(self.load());
                fx
            }
        }
    }
}

/// Where a view's selection goes once a change it made lands as `done`,
/// naming the issue it wrote: to that issue only for the newest change
/// (`newest`), only while what is selected is what was selected when the
/// change was made (`picked_then`, `picked_now`), and only with no form open,
/// so an older write landing never takes the person back. `None` leaves it.
pub fn moves_selection(
    newest: bool,
    done: Option<u64>,
    picked_then: Option<u64>,
    picked_now: Option<u64>,
    form_open: bool,
) -> Option<u64> {
    done.filter(|_| newest && picked_now == picked_then && !form_open)
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
    /// The newest change sent to be written, by anyone: the one whose write
    /// decides what to show should it fail.
    sent: u64,
    /// The newest change a view sent: the one whose write may move a
    /// view's selection.
    viewed: u64,
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

    /// A change made by `maker` is sent to be written: which one it is.
    fn send(&mut self, maker: Maker) -> u64 {
        self.sent += 1;
        match maker {
            Maker::View => self.viewed = self.sent,
            Maker::Outside => {}
        }
        self.sent
    }

    /// Whether `of` is the newest change a view made.
    fn newest(&self, of: u64) -> bool {
        of == self.viewed
    }

    /// Whether `of` is the newest change anyone made.
    fn last(&self, of: u64) -> bool {
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
