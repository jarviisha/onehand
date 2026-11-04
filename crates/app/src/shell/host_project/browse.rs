//! *Browse* on the add-a-project-on-a-host form: the folders of one folder on
//! the host, listed under *Folder*.

use super::HostProjectDraft;
use crate::dialogs::{browse_refusal, browse_rows};
use crate::hosts;
use crate::shell::Shell;
use gpui::{Context, Window};
use onehand_core::host::client::HostConn;
use onehand_core::tree::TreeEntry;
use onehand_core::workspace::HostName;
use std::path::PathBuf;

/// The folders of one folder on a host, as the form lists them.
#[derive(Clone)]
pub(crate) struct Browse {
    pub(crate) host: HostName,
    /// The folder listed; empty until the first listing lands.
    pub(crate) at: PathBuf,
    pub(crate) dirs: Vec<PathBuf>,
    /// How many folders the list leaves out.
    pub(crate) cut: usize,
    pub(crate) error: Option<String>,
    pub(crate) loading: bool,
}

impl HostProjectDraft {
    /// Let the folder list go: it was another host's. A wait it had put on
    /// the form goes with it, unless *Add* waits too.
    pub(super) fn drop_browse(&mut self) {
        self.browse = None;
        self._browsing = None;
        if self.attempt.is_none() {
            self.waiting = None;
        }
    }
}

/// Where *Browse* starts listing.
enum Start {
    /// A folder pressed in the list.
    At(PathBuf),
    /// What *Folder* names, if the host has it; its home if not.
    Folder(PathBuf),
    Home,
}

/// A folder listed, its entries and how many the host's bound left out; or
/// why it could not be.
type Listed = Result<(PathBuf, Vec<TreeEntry>, usize), String>;

/// List the folder `start` names over `conn`.
async fn list_from(conn: HostConn, start: Start, cx: &mut gpui::AsyncApp) -> Listed {
    let read = |at: PathBuf, cx: &mut gpui::AsyncApp| {
        let reading = cx.update(|cx| hosts::read_dir(cx, conn.clone(), at.clone()));
        async move { reading.await.map(|(entries, cut)| (at, entries, cut)) }
    };
    match start {
        Start::At(at) => return read(at, cx).await,
        Start::Folder(folder) => {
            if let Ok(listed) = read(folder, cx).await {
                return Ok(listed);
            }
        }
        Start::Home => {}
    }
    let home = cx.update(|cx| hosts::home(cx, conn.clone())).await?;
    read(home, cx).await
}

impl Shell {
    /// A folder pressed in the list: it becomes *Folder*, and is listed.
    pub(crate) fn browse_to(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.host_project.as_mut() else {
            return;
        };
        // Refused, *Folder* is left as it is: it only ever names what is listed.
        let host = draft.host.read(cx).value();
        if browse_refusal(&host, draft.busy(), draft.waiting.is_some()).is_some() {
            return;
        }
        draft.error = None;
        let value = path.display().to_string();
        draft
            .folder
            .update(cx, |input, cx| input.set_value(value, window, cx));
        self.browse_host_folders(Some(path), cx);
    }

    /// List folders on the host the form names: `at`, or with none the
    /// folder *Folder* names when the host has it, else the host's home.
    /// Attaches first, waiting out a busy host as *Add* does.
    pub(crate) fn browse_host_folders(&mut self, at: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(draft) = self.host_project.as_mut() else {
            return;
        };
        let host = draft.host.read(cx).value().trim().to_string();
        if browse_refusal(&host, draft.busy(), draft.waiting.is_some()).is_some() {
            return;
        }
        let host = HostName(host);
        let folder = PathBuf::from(draft.folder.read(cx).value().trim());
        let start = match at {
            Some(at) => Start::At(at),
            None if folder.is_absolute() => Start::Folder(folder),
            None => Start::Home,
        };
        match draft.browse.as_mut() {
            Some(browse) => {
                browse.loading = true;
                browse.error = None;
            }
            None => {
                draft.browse = Some(Browse {
                    host: host.clone(),
                    at: PathBuf::new(),
                    dirs: Vec::new(),
                    cut: 0,
                    error: None,
                    loading: true,
                })
            }
        }
        draft._browsing = Some(cx.spawn(async move |shell, cx| {
            let waiting = |_: String, cx: &mut gpui::AsyncApp| {
                let _ = shell.update(cx, |shell: &mut Self, cx| {
                    if let Some(draft) = shell.host_project.as_mut() {
                        draft.waiting = Some(host.clone());
                        cx.notify();
                    }
                });
            };
            let listed = match hosts::attach_waiting(&host, cx, waiting).await {
                Ok(conn) => list_from(conn, start, cx).await,
                Err(why) => Err(why),
            };
            let _ = shell.update(cx, |shell: &mut Self, cx| {
                shell.host_folders_listed(listed, cx)
            });
        }));
        cx.notify();
    }

    /// The listing *Browse* asked for has landed, or why it could not.
    fn host_folders_listed(&mut self, listed: Listed, cx: &mut Context<Self>) {
        let Some(draft) = self.host_project.as_mut() else {
            return;
        };
        if draft.attempt.is_none() {
            draft.waiting = None;
        }
        // One failure line at a time: the list's own, the newer, stands.
        if listed.is_err() {
            draft.error = None;
        }
        let Some(browse) = draft.browse.as_mut() else {
            return;
        };
        browse.loading = false;
        match listed {
            Ok((at, entries, cut)) => {
                (browse.dirs, browse.cut) = browse_rows(&at, &entries, cut);
                browse.at = at;
            }
            Err(why) => browse.error = Some(why),
        }
        cx.notify();
    }
}
