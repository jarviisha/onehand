//! An offloaded project's code on its way to a host's mirror and back.
//!
//! **What travels** is what git does not ignore, plus `.git` itself, so the
//! mirror works for a repository with no remote and an ignored secret never
//! leaves on its own. It goes with `rsync` over the person's own ssh.
//!
//! **What was sent is remembered** as a manifest: each file's size and
//! modification time (rsync keeps them), `HEAD` and the branch and tag refs.
//! Before the code comes back the folder is held against it, and anything
//! changed meanwhile stops the copy: no change made outside onehand is ever
//! written over without the person saying so.
//!
//! Blocking throughout; a front end runs these off its UI.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::client::{HostAddr, SSH_OPTS};
use super::wire::Holder;
use crate::worktree::git_message;

/// The repository's own folder, which travels whole beside the listed files.
const GIT_DIR: &str = ".git";

/// What the copy carries of `folder`, relative to it: every file git does not
/// ignore, tracked or not, then `.git`. A name that is not UTF-8 is refused,
/// since it could not be kept or said.
///
/// **Refused unless `folder` is a repository's top with a `.git` folder of its
/// own**, and when it holds another repository (a submodule, or a checkout
/// nobody added): git lists that as one folder, which would travel whole,
/// ignored files and all, and whose changes no manifest would see.
pub fn listed_blocking(folder: &Path) -> Result<Vec<PathBuf>, String> {
    if !folder.join(GIT_DIR).is_dir() {
        return Err(format!(
            "{} is not the top of a git repository with a .git folder of its own",
            folder.display()
        ));
    }
    let out = git(folder)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    let mut files = BTreeSet::new();
    for name in out.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = std::str::from_utf8(name).map_err(|_| {
            format!(
                "{} has a name that is not UTF-8",
                String::from_utf8_lossy(name)
            )
        })?;
        let nested = name.ends_with('/')
            || std::fs::symlink_metadata(folder.join(name)).is_ok_and(|m| m.is_dir());
        if nested {
            return Err(format!(
                "{} is a repository inside this one; a project holding one cannot be offloaded yet",
                name.trim_end_matches('/')
            ));
        }
        files.insert(PathBuf::from(name));
    }
    let mut files: Vec<PathBuf> = files.into_iter().collect();
    files.push(PathBuf::from(GIT_DIR));
    Ok(files)
}

/// The code as it stood when it last went up or came back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Each listed file there was, by its size and modification time.
    files: BTreeMap<PathBuf, (u64, i128)>,
    /// What `HEAD` names and the commit it is, `refs` every ref.
    head: String,
    refs: BTreeMap<String, String>,
    /// Digests of what is staged and of the repository's own settings, which
    /// the copy back writes over as it writes over a file.
    #[serde(default)]
    staged: u64,
    #[serde(default)]
    config: u64,
}

/// What changed in a folder since its manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    pub files: Vec<PathBuf>,
    /// `HEAD`, a ref by its full name, `index` (what is staged) or `config`
    /// (the repository's settings).
    pub refs: Vec<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.refs.is_empty()
    }
}

impl Manifest {
    /// `folder` as it is now.
    pub fn read_blocking(folder: &Path) -> Result<Manifest, String> {
        let mut files = BTreeMap::new();
        for rel in listed_blocking(folder)? {
            if rel.as_os_str() == GIT_DIR {
                continue;
            }
            // A file listed and gone (deleted, not yet staged) is not there.
            if let Ok(meta) = std::fs::symlink_metadata(folder.join(&rel)) {
                files.insert(rel, stamp(&meta));
            }
        }
        // A repository with no commit yet has no HEAD to read.
        let symbolic = git_out(folder, &["symbolic-ref", "-q", "HEAD"]).unwrap_or_default();
        let commit = git_out(folder, &["rev-parse", "-q", "--verify", "HEAD"]).unwrap_or_default();
        let listed = git_out(
            folder,
            &["for-each-ref", "--format=%(refname) %(objectname)"],
        )?;
        let staged = git_out(folder, &["ls-files", "--stage", "-z"])?;
        let config = std::fs::read(folder.join(GIT_DIR).join("config")).unwrap_or_default();
        let refs = listed
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(name, id)| (name.to_string(), id.to_string()))
            .collect();
        Ok(Manifest {
            files,
            head: format!("{symbolic} {commit}"),
            refs,
            staged: crate::chat::store::fnv1a(staged.as_bytes()),
            config: crate::chat::store::fnv1a(&config),
        })
    }

    /// What differs in `now` from this: files changed, added or gone, and
    /// refs moved, added or gone. Objects git adds or packs on its own are
    /// not looked at, so they never stop a copy.
    pub fn changes(&self, now: &Manifest) -> Changes {
        let files = differing(&self.files, &now.files);
        let mut refs = differing(&self.refs, &now.refs);
        if self.head != now.head {
            refs.insert(0, "HEAD".to_string());
        }
        if self.staged != now.staged {
            refs.push("index".to_string());
        }
        if self.config != now.config {
            refs.push("config".to_string());
        }
        Changes { files, refs }
    }

    /// The files it lists and `now` does not: gone since.
    fn gone(&self, now: &[PathBuf]) -> Vec<PathBuf> {
        let now: BTreeSet<&PathBuf> = now.iter().collect();
        self.files
            .keys()
            .filter(|f| !now.contains(f))
            .cloned()
            .collect()
    }
}

fn differing<K: Ord + Clone, V: PartialEq>(was: &BTreeMap<K, V>, now: &BTreeMap<K, V>) -> Vec<K> {
    let keys: BTreeSet<&K> = was.keys().chain(now.keys()).collect();
    keys.into_iter()
        .filter(|k| was.get(k) != now.get(k))
        .cloned()
        .collect()
}

fn stamp(meta: &std::fs::Metadata) -> (u64, i128) {
    let nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i128);
    (meta.len(), nanos)
}

/// Send `folder`'s code to `mirror` on the host, deleting there what is gone
/// here since `sent`; what was sent, read before it went, so an edit made
/// while it goes reads as a change later rather than as sent.
pub fn send_up_blocking(
    addr: &HostAddr,
    folder: &Path,
    mirror: &Path,
    sent: &Manifest,
) -> Result<Manifest, String> {
    let manifest = Manifest::read_blocking(folder)?;
    let mut list = listed_blocking(folder)?;
    list.extend(sent.gone(&list));
    // `--delete` reaches only the folders sent whole, which is `.git` alone:
    // the mirror's repository is the laptop's.
    rsync(
        addr,
        &["--delete"],
        &list,
        &local_src(folder),
        &remote(addr, mirror),
    )?;
    Ok(manifest)
}

/// What came of bringing the code back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Back {
    /// Something changed in the folder since `sent`; nothing was copied.
    Stopped(Changes),
    /// The code came; the folder as it is now. Kept as the manifest whatever
    /// the key's hand-back says after: the folder holds this code now.
    Came(Manifest),
}

/// Bring the mirror's code back into `folder`, unless the folder changed
/// since `sent`: `listed` is what the mirror carries, and what `sent` had
/// that it no longer does is deleted here. A file the mirror carries that is
/// here and `sent` never had (one this machine's git ignores and the host's
/// does not) counts as changed, so it is never written over unasked. `.git`
/// comes back without deleting, so a commit only this folder had keeps its
/// objects.
// ponytail: an edit landing between the check and rsync writing that file is
// written over; `--backup-dir` on the way back would keep it.
pub fn bring_back_blocking(
    addr: &HostAddr,
    folder: &Path,
    mirror: &Path,
    listed: &[PathBuf],
    sent: &Manifest,
) -> Result<Back, String> {
    let mut changes = sent.changes(&Manifest::read_blocking(folder)?);
    for rel in listed {
        let unknown = rel.as_os_str() != GIT_DIR
            && !sent.files.contains_key(rel)
            && !changes.files.contains(rel)
            && std::fs::symlink_metadata(folder.join(rel)).is_ok();
        if unknown {
            changes.files.push(rel.clone());
        }
    }
    if !changes.is_empty() {
        changes.files.sort();
        return Ok(Back::Stopped(changes));
    }
    take_back_blocking(addr, folder, mirror, listed, sent).map(Back::Came)
}

/// Bring the mirror's code back over whatever changed here, once the person
/// said so and what changed was backed up.
pub fn take_back_blocking(
    addr: &HostAddr,
    folder: &Path,
    mirror: &Path,
    listed: &[PathBuf],
    sent: &Manifest,
) -> Result<Manifest, String> {
    let mut list = listed.to_vec();
    list.extend(sent.gone(listed));
    rsync(addr, &[], &list, &remote(addr, mirror), &local_src(folder))?;
    Manifest::read_blocking(folder)
}

/// Copy `rel`, a file or a folder with all under it, ignored or not, from
/// `folder` to the same place in `mirror`.
pub fn send_path_blocking(
    addr: &HostAddr,
    folder: &Path,
    mirror: &Path,
    rel: &Path,
) -> Result<(), String> {
    let plain = rel.components().next().is_some()
        && rel
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
    if !plain {
        return Err(format!(
            "{} is not a path inside the project",
            rel.display()
        ));
    }
    let src = folder.join(".").join(rel);
    let out = rsync_command(addr)
        .arg("--relative")
        .arg(&src)
        .arg(remote(addr, mirror))
        .output()
        .map_err(|e| format!("rsync: {e}"))?;
    said(out)
}

/// Copy each of `changes`' files as it is in `folder` now into `into`, at its
/// place there, and the refs that moved as `refs.txt`; `into` once done.
/// A file gone from `folder` has nothing to keep.
pub fn backup_blocking(folder: &Path, changes: &Changes, into: &Path) -> Result<PathBuf, String> {
    let fail = |e: std::io::Error| format!("Could not back up to {}: {e}", into.display());
    std::fs::create_dir_all(into).map_err(fail)?;
    for rel in &changes.files {
        let from = folder.join(rel);
        if !from.is_file() {
            continue;
        }
        let to = into.join(rel);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        std::fs::copy(&from, &to).map_err(fail)?;
    }
    if !changes.refs.is_empty() {
        let now = Manifest::read_blocking(folder)?;
        let mut text = format!("HEAD {}\n", now.head);
        for (name, id) in &now.refs {
            text.push_str(&format!("{name} {id}\n"));
        }
        std::fs::write(into.join("refs.txt"), text).map_err(fail)?;
    }
    Ok(into.to_path_buf())
}

/// Refused, naming the side, when `rsync` is missing here or on the host.
pub fn rsync_present_blocking(addr: &HostAddr) -> Result<(), String> {
    let here = Command::new("rsync")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !here {
        return Err("rsync is not installed on this machine".into());
    }
    if addr.local {
        return Ok(());
    }
    let there = Command::new("ssh")
        .args(SSH_OPTS)
        .arg("--")
        .arg(&addr.ssh)
        .arg("command -v rsync")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("ssh: {e}"))?;
    match there.status.code() {
        Some(0) => Ok(()),
        // ssh's own failure: the host was not reached, so nothing is known of rsync.
        Some(255) => Err(String::from_utf8_lossy(&there.stderr).trim().to_string()),
        _ => Err(format!("rsync is not installed on {}", addr.ssh)),
    }
}

/// `rsync` reaching the host over the attach's ssh options; archive mode,
/// names passed as they are, files told apart by their contents.
///
/// **By contents, not by size and time:** rsync's own check sees whole
/// seconds, so a file of the same size changed within the second it was
/// copied (a ref git moved right after the code went up) would be left as
/// it was, and the code would come back without it.
// ponytail: every file is read on both ends at each copy; skip `.git/objects`
// from the checksum (its names are their hashes) if big repositories feel it.
fn rsync_command(addr: &HostAddr) -> Command {
    let mut cmd = Command::new("rsync");
    cmd.args(["-a", "-s", "--checksum"]);
    if !addr.local {
        cmd.arg("-e").arg(format!("ssh {}", SSH_OPTS.join(" ")));
    }
    cmd.stdin(Stdio::null());
    cmd
}

/// Copy `list` from `src` to `dst`, deleting at `dst` what is listed and
/// missing at `src`.
fn rsync(
    addr: &HostAddr,
    extra: &[&str],
    list: &[PathBuf],
    src: &str,
    dst: &str,
) -> Result<(), String> {
    let mut child = rsync_command(addr)
        .args(["-r", "--from0", "--files-from=-", "--delete-missing-args"])
        .args(extra)
        .arg(src)
        .arg(dst)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("rsync: {e}"))?;
    let mut names = Vec::new();
    for rel in list {
        names.extend_from_slice(rel.as_os_str().as_encoded_bytes());
        names.push(0);
    }
    // Written from a thread: a long list fills the pipe before rsync reads it all.
    let mut stdin = child.stdin.take().expect("piped");
    let writer = std::thread::spawn(move || stdin.write_all(&names));
    // ponytail: no deadline; a first copy of a big repository takes as long as it takes.
    let out = child
        .wait_with_output()
        .map_err(|e| format!("rsync: {e}"))?;
    let _ = writer.join();
    said(out)
}

fn said(out: std::process::Output) -> Result<(), String> {
    if out.status.success() {
        return Ok(());
    }
    let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if why.is_empty() {
        format!("rsync failed ({})", out.status)
    } else {
        why
    })
}

/// A folder here, as rsync reads a folder's contents.
fn local_src(folder: &Path) -> String {
    format!("{}/", folder.display())
}

/// The mirror, as rsync names a folder on the host.
fn remote(addr: &HostAddr, mirror: &Path) -> String {
    match addr.local {
        true => format!("{}/", mirror.display()),
        false => format!("{}:{}/", addr.ssh, mirror.display()),
    }
}

fn git(folder: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(folder)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    cmd
}

fn git_out(folder: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(folder)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(git_message(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What the laptop keeps of an offloaded project beside the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffloadState {
    /// Who holds the key, as the host last said.
    pub key: Holder,
    /// What last went up or came back.
    pub manifest: Manifest,
    /// When the code last moved, in unix seconds.
    pub moved: u64,
    /// What stopped the last copy back, while it stands stopped.
    #[serde(default)]
    pub stopped: Option<Changes>,
}

impl OffloadState {
    const FILE: &str = "offload.json";

    /// Kept in `dir`, the project's own storage; none when nothing is kept.
    pub fn load(dir: &Path) -> Result<Option<OffloadState>, String> {
        match std::fs::read(dir.join(Self::FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| format!("{}: {e}", Self::FILE)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", Self::FILE)),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        crate::config::write_atomic(&dir.join(Self::FILE), &text)
            .map_err(|e| format!("Could not keep the offload state: {e}"))
    }
}

#[cfg(test)]
mod tests;
