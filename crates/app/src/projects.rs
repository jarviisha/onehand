//! The projects' drive's carrier: what `ProjectsDrive` decides, carried out
//! for the whole process. A project is the same project in every window
//! holding it, so a change to one is told to each of them.
//!
//! **Every window is told later, never inside the change.** A change is
//! usually made from inside a window's own update (a check typed on its
//! page, a split from its dialog), and updating a window's shell while it is
//! being updated panics; so each window is reached after this update, as a
//! shell reaching into another window does.

use crate::state::Shared;
use gpui::{App, BorrowAppContext as _};
use gpui_component::WindowExt as _;
use onehand_core::project::drive::{Effect, Input, ProjectWrite, Signal};
use onehand_core::project::{MadeBy, Project, ProjectId, Worktree};
use onehand_core::workspace::HostName;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Carry out the projects' effects, in order, each once.
pub(crate) fn carry(fx: Vec<Effect>, cx: &mut App) {
    for effect in fx {
        match effect {
            Effect::Write(write) => {
                let ProjectWrite {
                    id,
                    pending,
                    project,
                    dir,
                } = *write;
                let written = Shared::global(cx).writer.write(move || {
                    onehand_core::project::file::save_to(&project, &dir)
                        .map_err(|why| why.to_string())
                });
                cx.spawn(async move |cx| {
                    let result = written.await;
                    cx.update(|cx| {
                        let fx = cx.update_global::<Shared, _>(|shared, _| {
                            shared.projects.handle(Input::Written {
                                id,
                                pending,
                                result,
                            })
                        });
                        carry(fx, cx);
                    });
                })
                .detach();
            }
            Effect::Arm { token, after } => {
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(after).await;
                    cx.update(|cx| {
                        let fx = cx.update_global::<Shared, _>(|shared, _| {
                            shared.projects.handle(Input::Elapsed { token })
                        });
                        carry(fx, cx);
                    });
                })
                .detach();
            }
            Effect::Changed(id) => tell(&id, cx),
            Effect::Signal(Signal::SaveFailed { id, name, why }) => {
                let what = format!("Saving {name}");
                for_each_holding(&id, cx, move |_, window, cx| {
                    window.push_notification(crate::toast::failed(&what, why.clone()), cx);
                });
            }
            // Said where the removal was asked, by `worktree_removal`.
            Effect::Signal(
                Signal::WorktreeRemoved { .. }
                | Signal::WorktreeStays(_)
                | Signal::WorktreeNotRemoved(_),
            ) => {}
        }
    }
}

/// Tell every window holding project `id` how it now stands.
fn tell(id: &ProjectId, cx: &mut App) {
    let Some(project) = Shared::global(cx).projects.get(id).cloned() else {
        return;
    };
    for_each_holding(id, cx, move |shell, window, cx| {
        shell.told_project(&project, window, cx);
    });
}

/// Run `act` in each window holding project `id`, after this update.
fn for_each_holding(
    id: &ProjectId,
    cx: &mut App,
    act: impl Fn(&mut crate::shell::Shell, &mut gpui::Window, &mut gpui::Context<crate::shell::Shell>)
    + Clone
    + 'static,
) {
    for (handle, shell) in crate::windows::live(cx) {
        let (act, id) = (act.clone(), id.clone());
        // Asked after this update too: the shell may be the one updating.
        cx.defer(move |cx| {
            if !shell.read(cx).holds_project(&id) {
                return;
            }
            let _ = handle.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| act(shell, window, cx));
            });
        });
    }
}

/// Place every row of `shell`'s workspace that names no project, a
/// worker's aside: the folder a launch seeded the workspace with, or an
/// entry nothing could place when the projects were brought in.
pub(crate) fn place_rows(
    handle: gpui::AnyWindowHandle,
    shell: gpui::Entity<crate::shell::Shell>,
    cx: &mut App,
) {
    for row in shell.read(cx).unplaced_rows() {
        place_row(handle, shell.downgrade(), row, cx);
    }
}

/// A row of no project, as its window had it: its folder, and the facts it
/// carried itself, which its project takes where it has none.
pub(crate) struct Unplaced {
    pub host: Option<HostName>,
    pub path: PathBuf,
    pub check: Option<String>,
    pub builds: Vec<onehand_core::workspace::BuildHost>,
    pub offload: Option<onehand_core::workspace::Offloaded>,
}

/// Place `row` of `shell`'s workspace in its project, and the project take
/// what the row carried where it has nothing of its own.
pub(crate) fn place_row(
    handle: gpui::AnyWindowHandle,
    shell: gpui::WeakEntity<crate::shell::Shell>,
    row: Unplaced,
    cx: &mut App,
) {
    let Unplaced {
        host,
        path,
        check,
        builds,
        offload,
    } = row;
    let (row_host, row_path) = (host.clone(), path.clone());
    place(host, path, MadeBy::Added, cx, move |project, cx| {
        let fx = cx.update_global::<Shared, _>(|shared, _| {
            shared
                .projects
                .take_facts(&project.id, &row_path, check, builds, offload)
        });
        carry(fx, cx);
        let project = Shared::global(cx)
            .projects
            .get(&project.id)
            .cloned()
            .unwrap_or(project);
        let _ = handle.update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.placed_row(row_host.as_ref(), &row_path, &project, window, cx)
            })
        });
    });
}

/// Write every project change still waiting on its burst, now.
pub(crate) fn flush(cx: &mut App) {
    let fx = cx.update_global::<Shared, _>(|shared, _| shared.projects.flush());
    carry(fx, cx);
}

/// The project of `path` on `host`, made when none holds it: a host's folder
/// asks no git, so it is found or made at once.
pub(crate) fn of_host(host: HostName, path: PathBuf, cx: &mut App) -> Option<Project> {
    let fresh = mint(cx);
    let (id, fx) = cx.update_global::<Shared, _>(|shared, _| {
        shared.projects.place(
            Some(host),
            path,
            onehand_core::project::Placement::New,
            MadeBy::Added,
            fresh,
        )
    });
    carry(fx, cx);
    Shared::global(cx).projects.get(&id).cloned()
}

/// A fresh project id: one no project holds and no folder in the store is
/// named after.
pub(crate) fn mint(cx: &App) -> ProjectId {
    static MINTED: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    let pid = u64::from(std::process::id()) << 32;
    let store = onehand_core::project::file::store_dir();
    loop {
        let n = MINTED.fetch_add(1, Ordering::Relaxed);
        let id = ProjectId::mint(nanos ^ pid ^ n);
        if Shared::global(cx).projects.get(&id).is_none() && !store.join(id.as_str()).exists() {
            return id;
        }
    }
}

/// Find the project `folder` on `host` is a folder of, placing it when no
/// project holds it yet (asking git, off the UI loop), then hand it to
/// `then`, after this update. `made` says who made the folder, should it
/// become a worktree.
pub(crate) fn place(
    host: Option<HostName>,
    folder: PathBuf,
    made: MadeBy,
    cx: &mut App,
    then: impl FnOnce(Project, &mut App) + 'static,
) {
    if let Some((project, _)) = Shared::global(cx).projects.find(host.as_ref(), &folder) {
        let project = project.clone();
        cx.defer(move |cx| then(project, cx));
        return;
    }
    let known = (*Shared::global(cx).projects).clone();
    cx.spawn(async move |cx| {
        let (placement, folder) = cx
            .background_executor()
            .spawn({
                let host = host.clone();
                async move {
                    let folder = match host {
                        Some(_) => folder,
                        None => std::fs::canonicalize(&folder).unwrap_or(folder),
                    };
                    let placement =
                        onehand_core::project::place_blocking(&known, host.as_ref(), &folder);
                    (placement, folder)
                }
            })
            .await;
        cx.update(|cx| {
            let fresh = mint(cx);
            let (id, fx) = cx.update_global::<Shared, _>(|shared, _| {
                shared.projects.place(host, folder, placement, made, fresh)
            });
            carry(fx, cx);
            if let Some(project) = Shared::global(cx).projects.get(&id).cloned() {
                then(project, cx);
            }
        });
    })
    .detach();
}

/// How many of a project's worktrees a list draws (the page's box, the
/// menus offering *In <branch>*) before it says how many it left out.
pub(crate) const WORKTREES_SHOWN: usize = 20;

/// What a list cut at [`WORKTREES_SHOWN`] says of the `n` it left out.
pub(crate) fn more_not_shown(n: usize) -> Option<String> {
    let more = n.checked_sub(WORKTREES_SHOWN).filter(|more| *more > 0)?;
    Some(format!("{more} more not shown"))
}

/// The id of the project holding `of`, a folder here.
fn id_of(of: &Path, cx: &App) -> Option<ProjectId> {
    Shared::global(cx)
        .projects
        .find(None, of)
        .map(|(project, _)| project.id.clone())
}

/// Carry what project `of`'s drive answers to `change`; nothing when no
/// project holds `of`.
fn change_of(
    of: &Path,
    cx: &mut App,
    change: impl FnOnce(&mut onehand_core::project::ProjectsDrive, &ProjectId) -> Vec<Effect>,
) {
    let Some(id) = id_of(of, cx) else {
        return;
    };
    let fx = cx.update_global::<Shared, _>(|shared, _| change(&mut shared.projects, &id));
    carry(fx, cx);
}

/// Record `worktree` in the project holding `of`, a folder here, and answer
/// the project as it now is; none when no project holds `of`.
pub(crate) fn record(of: &Path, worktree: Worktree, cx: &mut App) -> Option<Project> {
    change_of(of, cx, |drive, id| drive.record_worktree(id, worktree));
    let id = id_of(of, cx)?;
    Shared::global(cx).projects.get(&id).cloned()
}

/// Take in `listed`, a checkout git lists, as a worktree of the project
/// holding `of`.
pub(crate) fn adopt_worktree(of: &Path, listed: onehand_core::worktree::Listed, cx: &mut App) {
    change_of(of, cx, |drive, id| drive.adopt_worktree(id, listed));
}

/// Forget the worktree at `folder` in the project holding `of`, a folder
/// here.
pub(crate) fn forget_worktree(of: &Path, folder: &Path, cx: &mut App) {
    change_of(of, cx, |drive, id| drive.forget_worktree(id, folder));
}

/// A removal of the worktree at `folder` begins: false when one is under way
/// already. Every window's page draws it.
pub(crate) fn begin_removal(folder: &Path, cx: &mut App) -> bool {
    let fresh = cx.update_global::<Shared, _>(|shared, _| shared.projects.begin_removal(folder));
    redraw_pages(cx);
    fresh
}

/// The removal of `folder` waits on the person's answer, or was let go: it
/// is under way no more.
pub(crate) fn removal_asked(folder: &Path, cx: &mut App) {
    cx.update_global::<Shared, _>(|shared, _| shared.projects.removal_asked(folder));
    redraw_pages(cx);
}

/// Every window's project page drawn again: a removal under way is the
/// projects', whichever window asked for it. After this update, since the
/// window asking is being updated as this runs.
fn redraw_pages(cx: &mut App) {
    cx.defer(|cx| {
        for (_, shell) in crate::windows::live(cx) {
            let pane = shell.read(cx).chat_pane().clone();
            pane.update(cx, |_, cx| cx.notify());
        }
    });
}

/// Removing the worktree at `folder` of the project holding `of` came out as
/// `removal`: the drive's answer carried, whatever became of the window it
/// was asked in, and what it says answered as toasts for that window. One
/// whose project left meanwhile is under way no more.
pub(crate) fn worktree_removal(
    of: &Path,
    folder: &Path,
    removal: onehand_core::project::drive::Removal,
    cx: &mut App,
) -> Vec<gpui_component::notification::Notification> {
    let Some(id) = id_of(of, cx) else {
        removal_asked(folder, cx);
        return Vec::new();
    };
    let fx = cx.update_global::<Shared, _>(|shared, _| {
        shared.projects.worktree_removal(&id, folder, removal)
    });
    let toasts = fx
        .iter()
        .filter_map(|effect| match effect {
            Effect::Signal(Signal::WorktreeRemoved {
                folder,
                branch,
                kept,
            }) => {
                let at = folder.display();
                Some(match (branch, kept) {
                    (Some(branch), Some(why)) => crate::toast::success(format!(
                        "Removed {at}; its branch {branch} is kept: {why}"
                    )),
                    (Some(branch), None) => {
                        crate::toast::success(format!("Removed {at} and its branch {branch}"))
                    }
                    (None, _) => crate::toast::success(format!("Removed {at}")),
                })
            }
            Effect::Signal(Signal::WorktreeStays(why)) => Some(crate::toast::warning(format!(
                "The worktree stays. {}",
                why.join(" ")
            ))),
            Effect::Signal(Signal::WorktreeNotRemoved(why)) => {
                Some(crate::toast::failed("Removing the worktree", why.clone()))
            }
            Effect::Signal(Signal::SaveFailed { .. })
            | Effect::Write(_)
            | Effect::Arm { .. }
            | Effect::Changed(_) => None,
        })
        .collect();
    carry(fx, cx);
    redraw_pages(cx);
    toasts
}

/// Put the new worktrees of the project holding `of` in `parent`, or beside
/// the repository with none.
pub(crate) fn set_worktree_parent(of: &Path, parent: Option<PathBuf>, cx: &mut App) {
    change_of(of, cx, |drive, id| drive.set_worktree_parent(id, parent));
}

/// Answer the cards of a session sent from the page of the project holding
/// `of` at `cards`.
pub(crate) fn set_cards(of: &Path, cards: onehand_core::chat::Cards, cx: &mut App) {
    change_of(of, cx, |drive, id| drive.set_cards(id, cards));
}

/// The folder new worktrees of the project holding `of` go in; none for
/// beside the repository.
pub(crate) fn worktree_parent(of: &Path, cx: &App) -> Option<PathBuf> {
    self::of(of, cx).and_then(|project| project.worktree_parent)
}

/// Record the worktree a run's cut made, `dir` on `branch`, in the project
/// holding `of`, the folder it was cut from; the project as it now is.
pub(crate) fn record_cut(of: &Path, dir: &Path, branch: &str, cx: &mut App) -> Option<Project> {
    let worktree = Worktree {
        path: dir.to_path_buf(),
        branch: branch.to_string(),
        from: None,
        made_by: MadeBy::Run,
        offload: None,
    };
    record(of, worktree, cx)
}

/// Which conversations the row of `folder` here lists: its own, and, of a
/// project, every conversation of its project, in whichever of its folders
/// it ran.
pub(crate) fn scope_of(folder: &Path, cx: &App) -> onehand_core::chat::Scope {
    onehand_core::chat::Scope {
        folder: folder.to_path_buf(),
        project: Shared::global(cx)
            .projects
            .find(None, folder)
            .map(|(project, _)| project.id.clone()),
    }
}

/// The project holding `folder` here, as it now is.
pub(crate) fn of(folder: &Path, cx: &App) -> Option<Project> {
    Shared::global(cx)
        .projects
        .find(None, folder)
        .map(|(project, _)| project.clone())
}

#[cfg(test)]
mod tests;
