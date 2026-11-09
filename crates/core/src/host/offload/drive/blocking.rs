//! What each of the offload drive's jobs does, off the UI loop: the copies
//! and git calls are core's blocking functions, and every call to the host
//! rides the attach the front end hands the job, polled on its runtime as a
//! host's disk is. A body that panics answers as a failure, so its folder
//! never stays claimed.

use super::super::branch as git;
use super::super::{self as offload, Back, Manifest, OffloadState};
use super::{Came, Input, Kept, Moving, Then, Went};
use crate::host::client::HostConn;
use crate::host::wire::{Holder, MirrorState};
use crate::host::HostFailure;
use crate::workspace::{stem_for, storage_for, HostName};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use tokio::runtime::Handle;

/// The attach a job is handed: `None` when it asked only for one already up
/// and there was none.
pub type Reached = Result<Option<HostConn>, HostFailure>;

/// Which attach a job is handed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// None: it does not call the host.
    Nothing,
    /// A person's attach, which takes the host.
    Take,
    /// One made by itself, which leaves a host another machine holds alone.
    Quiet,
    /// Only one already up.
    Live,
}

/// One of the drive's jobs, for the front end to run off its UI loop with
/// the attach it asks for.
pub struct Job {
    path: PathBuf,
    pub host: HostName,
    pub reach: Reach,
    work: Work,
}

#[derive(Debug)]
enum Work {
    Load,
    Attach(Then),
    Check,
    Offload { branch: bool },
    SendUp { sent: Manifest },
    CheckBack { sent: Manifest, poll: bool },
    TakeOver { sent: Manifest, into: PathBuf },
    Refresh { branch: String, poll: bool },
    UpdateHost { branch: String },
}

/// What a copy back brought: where the code settled, and for *Take the
/// host's code* whether the host said this machine held it before the copy
/// (`here`) and whether the backup folder is there (`backed`).
pub struct Brought {
    pub came: Came,
    pub here: bool,
    pub backed: bool,
}

impl Job {
    fn new(path: PathBuf, host: HostName, reach: Reach, work: Work) -> Self {
        Self {
            path,
            host,
            reach,
            work,
        }
    }

    pub(super) fn load(path: PathBuf, host: HostName) -> Self {
        Self::new(path, host, Reach::Nothing, Work::Load)
    }

    pub(super) fn attach(path: PathBuf, host: HostName, then: Then) -> Self {
        let reach = match then {
            Then::Poll => Reach::Quiet,
            Then::Again => Reach::Take,
        };
        Self::new(path, host, reach, Work::Attach(then))
    }

    pub(super) fn check(path: PathBuf, host: HostName) -> Self {
        Self::new(path, host, Reach::Nothing, Work::Check)
    }

    pub(super) fn offload(path: PathBuf, host: HostName, branch: bool) -> Self {
        Self::new(path, host, Reach::Take, Work::Offload { branch })
    }

    pub(super) fn send_up(path: PathBuf, host: HostName, sent: Manifest) -> Self {
        Self::new(path, host, Reach::Take, Work::SendUp { sent })
    }

    pub(super) fn check_back(path: PathBuf, host: HostName, sent: Manifest, poll: bool) -> Self {
        Self::new(path, host, Reach::Live, Work::CheckBack { sent, poll })
    }

    pub(super) fn take_over(path: PathBuf, host: HostName, sent: Manifest, into: PathBuf) -> Self {
        Self::new(path, host, Reach::Take, Work::TakeOver { sent, into })
    }

    pub(super) fn refresh(path: PathBuf, host: HostName, branch: String, poll: bool) -> Self {
        Self::new(path, host, Reach::Live, Work::Refresh { branch, poll })
    }

    pub(super) fn update_host(path: PathBuf, host: HostName, branch: String) -> Self {
        Self::new(path, host, Reach::Take, Work::UpdateHost { branch })
    }

    /// The folder it is about.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What it does, by name, for a test to compare.
    #[cfg(test)]
    pub(super) fn name(&self) -> &'static str {
        match &self.work {
            Work::Load => "Load",
            Work::Attach(Then::Poll) => "AttachQuietly",
            Work::Attach(Then::Again) => "Attach",
            Work::Check => "Check",
            Work::Offload { .. } => "Offload",
            Work::SendUp { .. } => "SendUp",
            Work::CheckBack { .. } => "CheckBack",
            Work::TakeOver { .. } => "TakeOver",
            Work::Refresh { .. } => "Refresh",
            Work::UpdateHost { .. } => "UpdateHost",
        }
    }

    /// Do it, over `conn`, calling the host on `rt`; blocking.
    pub fn run(self, conn: Reached, rt: &Handle) -> Input {
        let fallback = self.panicked();
        catch_unwind(AssertUnwindSafe(|| self.body(conn, rt))).unwrap_or_else(|_| {
            eprintln!("onehand: an offloaded project's job panicked");
            fallback
        })
    }

    /// What it answers when its body panicked.
    fn panicked(&self) -> Input {
        let why = || "Something went wrong here; try again".to_string();
        let path = self.path.clone();
        match &self.work {
            Work::Load => Input::Loaded {
                path,
                read: Err(why()),
            },
            Work::Attach(then) => Input::Attached {
                path,
                then: *then,
                result: Err(HostFailure::Other(why())),
            },
            Work::Check => Input::Checked {
                path,
                host: self.host.clone(),
                result: Err(why()),
            },
            Work::Offload { .. } => Input::Offloaded {
                path,
                host: self.host.clone(),
                result: Err(why()),
            },
            Work::SendUp { .. } => Input::SentUp {
                path,
                went: Err((why(), None)),
            },
            Work::CheckBack { poll, .. } => Input::BroughtBack {
                path,
                poll: *poll,
                took: None,
                brought: Brought {
                    came: Err((why(), None)),
                    here: false,
                    backed: false,
                },
            },
            Work::TakeOver { into, .. } => Input::BroughtBack {
                path,
                poll: false,
                took: Some(into.clone()),
                brought: Brought {
                    came: Err((why(), None)),
                    here: false,
                    backed: into.is_dir(),
                },
            },
            Work::Refresh { poll, .. } => Input::Counted {
                path,
                by: Moving::Back,
                poll: *poll,
                counted: Err(why()),
            },
            Work::UpdateHost { .. } => Input::Counted {
                path,
                by: Moving::Up,
                poll: false,
                counted: Err(why()),
            },
        }
    }

    fn body(self, conn: Reached, rt: &Handle) -> Input {
        let Job {
            path, host, work, ..
        } = self;
        match work {
            Work::Load => Input::Loaded {
                read: OffloadState::load(&storage_for(&path)),
                path,
            },
            Work::Attach(then) => Input::Attached {
                path,
                then,
                result: conn.map(drop),
            },
            Work::Check => Input::Checked {
                result: offload::rsync_present_blocking(&host.addr()),
                path,
                host,
            },
            Work::Offload { branch } => {
                let result = match branch {
                    true => offload_branch(conn, rt, &path, &host),
                    false => offload_mirror(conn, rt, &path, &host),
                };
                Input::Offloaded { path, host, result }
            }
            Work::SendUp { sent } => Input::SentUp {
                went: send_up(conn, rt, &path, &host, sent),
                path,
            },
            Work::CheckBack { sent, poll } => {
                let came = match conn {
                    Ok(Some(conn)) => bring_back(rt, &conn, &path, &host, sent, None),
                    Ok(None) | Err(_) => Ok(None),
                };
                Input::BroughtBack {
                    path,
                    poll,
                    took: None,
                    brought: Brought {
                        came,
                        here: false,
                        backed: false,
                    },
                }
            }
            Work::TakeOver { sent, into } => {
                // Asked first, so a host that hands the code back without a
                // copy is told apart from one whose code came.
                let (came, here) = match attached(conn, &host) {
                    Ok(conn) => match mirror_of(rt, &conn, &stem_for(&path), None, &host) {
                        Ok(mirror) => (
                            bring_back(rt, &conn, &path, &host, sent, Some(&into)),
                            mirror.key == Holder::Laptop,
                        ),
                        Err(why) => (Err((why, None)), false),
                    },
                    Err(why) => (Err((why, None)), false),
                };
                let backed = into.is_dir();
                Input::BroughtBack {
                    path,
                    poll: false,
                    took: Some(into),
                    brought: Brought { came, here, backed },
                }
            }
            Work::Refresh { branch, poll } => {
                let state = match conn {
                    Ok(Some(conn)) => match branch_mirror(rt, &conn, &path, &branch, &host) {
                        Ok(state) => Some(state),
                        Err(why) => {
                            return Input::Counted {
                                path,
                                by: Moving::Back,
                                poll,
                                counted: Err(why),
                            };
                        }
                    },
                    Ok(None) | Err(_) => None,
                };
                let counted = fetch_and_count(&path, &host, &branch, state.as_ref());
                Input::Counted {
                    path,
                    by: Moving::Back,
                    poll,
                    counted,
                }
            }
            Work::UpdateHost { branch } => Input::Counted {
                counted: update_host(conn, rt, &path, &host, &branch),
                path,
                by: Moving::Up,
                poll: false,
            },
        }
    }
}

/// The attach a job that needs one was handed, or why not in a person's
/// words.
fn attached(conn: Reached, host: &HostName) -> Result<HostConn, String> {
    match conn {
        Ok(Some(conn)) => Ok(conn),
        Ok(None) => Err(format!("Not connected to {host}")),
        Err(failure) => Err(failure.said()),
    }
}

/// The mirror named `name` (on `branch`, for a branch mirror) on the host
/// over `conn`, as the host has it now.
fn mirror_of(
    rt: &Handle,
    conn: &HostConn,
    name: &str,
    branch: Option<&str>,
    host: &HostName,
) -> Result<MirrorState, String> {
    rt.block_on(conn.mirror(name, branch))
        .map_err(|e| format!("Could not ask {host} about its copy: {e}"))
}

fn hand_key(
    rt: &Handle,
    conn: &HostConn,
    mirror: &Path,
    to: Holder,
    generation: u64,
) -> Result<MirrorState, String> {
    rt.block_on(conn.mirror_key(mirror, to, generation))
        .map_err(|e| e.to_string())
}

fn now() -> u64 {
    crate::chat::now_secs()
}

/// Offload `path` to `host` into a mirror: the mirror asked for, the code
/// sent up and what went kept. The key stays here: the first prompt hands it
/// over. A mirror whose code the host holds already (offloaded before, never
/// brought back) is not sent over: its code is newer, and it comes back as
/// any does.
fn offload_mirror(
    conn: Reached,
    rt: &Handle,
    path: &Path,
    host: &HostName,
) -> Result<(PathBuf, Kept), String> {
    let conn = attached(conn, host)?;
    let mirror = mirror_of(rt, &conn, &stem_for(path), None, host)?;
    let dir = storage_for(path);
    let kept = OffloadState::load(&dir)?;
    if mirror.key == Holder::Host {
        let state = OffloadState {
            key: Holder::Host,
            ..kept.unwrap_or_else(|| super::fresh(Holder::Host))
        };
        state.save(&dir)?;
        return Ok((mirror.path, Kept::Mirror(state)));
    }
    let sent = kept.map(|kept| kept.manifest).unwrap_or_default();
    let manifest = offload::send_up_blocking(&host.addr(), path, &mirror.path, &sent)?;
    let state = OffloadState {
        key: Holder::Laptop,
        manifest,
        moved: now(),
        stopped: None,
    };
    state.save(&dir)?;
    Ok((mirror.path, Kept::Mirror(state)))
}

/// Before a prompt to a session of `path`: send the code up and hand the key
/// to the host. `Ok` once the host holds it, which it may have already;
/// `Err` why not, the key left here, with what to keep then.
fn send_up(conn: Reached, rt: &Handle, path: &Path, host: &HostName, sent: Manifest) -> Went {
    let dir = storage_for(path);
    let conn = attached(conn, host).map_err(|why| (why, None))?;
    let mirror = mirror_of(rt, &conn, &stem_for(path), None, host).map_err(|why| (why, None))?;
    if mirror.key == Holder::Host {
        let state = OffloadState {
            key: Holder::Host,
            manifest: sent,
            moved: now(),
            stopped: None,
        };
        return match state.save(&dir) {
            Ok(()) => Ok(state),
            Err(why) => Err((why, Some(Box::new(state)))),
        };
    }
    let manifest = offload::send_up_blocking(&host.addr(), path, &mirror.path, &sent)
        .map_err(|why| (why, None))?;
    // Kept whatever the key's hand-over says: this is what the mirror holds.
    let handed = hand_key(rt, &conn, &mirror.path, Holder::Host, mirror.generation);
    let state = OffloadState {
        key: match handed {
            Ok(_) => Holder::Host,
            Err(_) => Holder::Laptop,
        },
        manifest,
        moved: now(),
        stopped: None,
    };
    let kept = state.save(&dir);
    match (handed, kept) {
        (Ok(_), Ok(())) => Ok(state),
        (Err(why), _) => Err((
            format!("{host} did not take the code: {why}"),
            Some(Box::new(state)),
        )),
        (Ok(_), Err(why)) => Err((why, Some(Box::new(state)))),
    }
}

/// The copy back: the state to keep after it, `None` when the host still
/// works on the code; or why it failed, with what to keep then.
///
/// With `backup`, the person said to take the host's code over what changed
/// here: what changed is copied there first, and the code comes over it.
fn bring_back(
    rt: &Handle,
    conn: &HostConn,
    path: &Path,
    host: &HostName,
    sent: Manifest,
    backup: Option<&Path>,
) -> Came {
    let dir = storage_for(path);
    let keep = |state: OffloadState| match state.save(&dir) {
        Ok(()) => Ok(state),
        Err(why) => Err((why, Some(Box::new(state)))),
    };
    let mirror = mirror_of(rt, conn, &stem_for(path), None, host).map_err(|why| (why, None))?;
    // Only this machine hands the key back to itself, after the copy back:
    // a host that says this machine holds it saw that hand-back land, which
    // was not kept here.
    if mirror.key == Holder::Laptop {
        return keep(OffloadState {
            key: Holder::Laptop,
            manifest: sent,
            moved: now(),
            stopped: None,
        })
        .map(Some);
    }
    if mirror.working {
        return Ok(None);
    }
    let listed = rt
        .block_on(conn.mirror_files(&mirror.path))
        .map_err(|e| (format!("Could not list the copy on {host}: {e}"), None))?;
    let addr = host.addr();
    // What the copy writes over waits here until it is known not to be an
    // edit made meanwhile.
    let coming = dir.join("coming-back");
    let back = match (
        offload::bring_back_blocking(&addr, path, &mirror.path, &listed, &sent, &coming)
            .map_err(|why| (why, None))?,
        backup,
    ) {
        (Back::Stopped(changes), Some(into)) => offload::take_back_blocking(
            &addr,
            path,
            &mirror.path,
            &listed,
            &sent,
            &changes,
            into,
            &coming,
        )
        .map_err(|why| (why, None))?,
        (back, _) => back,
    };
    let state = match came_to(back, sent, now()) {
        Settled::Keep(state) => state,
        Settled::HandBack(came) => {
            // The folder holds this code now, so it is kept whatever the
            // hand-back says; a refused one is copied again next time.
            let came = keep(came)?;
            match hand_key(rt, conn, &mirror.path, Holder::Laptop, mirror.generation) {
                Ok(_) => OffloadState {
                    key: Holder::Laptop,
                    ..came
                },
                // A prompt went there while the code came: the host keeps it.
                Err(_) => return Ok(Some(came)),
            }
        }
    };
    keep(state).map(Some)
}

/// Where a copy back that ran leaves the code.
#[derive(Debug)]
pub(super) enum Settled {
    /// Kept so, the key with the host.
    Keep(OffloadState),
    /// Come, and kept so before the key is handed back here.
    HandBack(OffloadState),
}

/// What `back`, a copy back held against `sent` that ran at `now`, comes to.
pub(super) fn came_to(back: Back, sent: Manifest, now: u64) -> Settled {
    match back {
        // The key stays with the host until the person settles it.
        Back::Stopped(changes) => Settled::Keep(OffloadState {
            key: Holder::Host,
            manifest: sent,
            moved: now,
            stopped: Some(changes),
        }),
        // The host's code is here but for what changed here as it came: held
        // against that code, those stop it as any change here does.
        Back::Raced(manifest, changes) => Settled::Keep(OffloadState {
            key: Holder::Host,
            manifest,
            moved: now,
            stopped: Some(changes),
        }),
        Back::Came(manifest) => Settled::HandBack(OffloadState {
            key: Holder::Host,
            manifest,
            moved: now,
            stopped: None,
        }),
    }
}

/// The branch mirror of `path` on the host over `conn`, as the host has it:
/// named beside the mirror a project with a key would have, so switching
/// modes never touches the other's code.
fn branch_mirror(
    rt: &Handle,
    conn: &HostConn,
    path: &Path,
    branch: &str,
    host: &HostName,
) -> Result<MirrorState, String> {
    let name = format!("{}.branch", stem_for(path));
    mirror_of(rt, conn, &name, Some(branch), host)
}

/// Push this folder's `HEAD` into the mirror and have the host bring it onto
/// its branch: started from it, or merged in.
fn send_commit(
    rt: &Handle,
    conn: &HostConn,
    path: &Path,
    host: &HostName,
    mirror: &Path,
) -> Result<(), String> {
    let commit = git::push_head_blocking(&host.addr(), path, mirror)?;
    rt.block_on(conn.mirror_update(mirror, &commit))
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Fetch the host's branch when it is not where it was last fetched, then
/// count what it has to merge.
fn fetch_and_count(
    path: &Path,
    host: &HostName,
    branch: &str,
    state: Option<&MirrorState>,
) -> Result<usize, String> {
    if let Some((head, mirror)) = state.and_then(|s| Some((s.head.as_ref()?, &s.path))) {
        if git::local_head_blocking(path, branch).as_ref() != Some(head) {
            git::fetch_blocking(&host.addr(), path, mirror, branch)?;
        }
    }
    git::ahead_blocking(path, branch)
}

/// Offload `path` to `host` on a branch: the mirror asked for, and, unless
/// its branch started already (offloaded before), this folder's commit sent
/// to start it. Answers the mirror and the branch.
fn offload_branch(
    conn: Reached,
    rt: &Handle,
    path: &Path,
    host: &HostName,
) -> Result<(PathBuf, Kept), String> {
    let branch = git::branch_for(host);
    let conn = attached(conn, host)?;
    let mut state = branch_mirror(rt, &conn, path, &branch, host)?;
    if state.head.is_none() {
        send_commit(rt, &conn, path, host, &state.path)?;
        state = branch_mirror(rt, &conn, path, &branch, host)?;
    }
    fetch_and_count(path, host, &branch, Some(&state))?;
    Ok((state.path, Kept::Branch(branch)))
}

/// *Update host from laptop*: this folder's commit merged into the host's
/// branch, once nothing works there, then the branch fetched back.
fn update_host(
    conn: Reached,
    rt: &Handle,
    path: &Path,
    host: &HostName,
    branch: &str,
) -> Result<usize, String> {
    let conn = attached(conn, host)?;
    let state = branch_mirror(rt, &conn, path, branch, host)?;
    if state.working {
        return Err(format!("Wait for the sessions on {host} to finish"));
    }
    send_commit(rt, &conn, path, host, &state.path)?;
    let state = branch_mirror(rt, &conn, path, branch, host)?;
    fetch_and_count(path, host, branch, Some(&state))
}
