//! Which processes of the person's own work inside a folder, found by
//! looking through every one of them rather than a list of known pids.
//!
//! **An earlier run's worker is what this is for.** After onehand crashes, a
//! worker's agent may still be running in its worktree, and nothing in the
//! new run knows its pid. So the whole process table is read, and a process
//! whose folder cannot be read for an unexpected reason is a refusal rather
//! than a pass: "nothing was found" is only an answer when everything was
//! looked at. Two reasons are expected and pass. A process gone between being
//! listed and being read is simply gone. And the system refuses its own user
//! the folder of a process that user may not trace (`systemd --user`,
//! `ssh-agent`, anything holding capabilities): every desktop has some, and
//! an agent, an ordinary process of the user's, is never one.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// What works inside a folder, each as `<pid> <command>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Occupants {
    None,
    Found(Vec<String>),
    /// Processes whose folder could not be read, or why the table could not
    /// be read at all.
    Unknown(Vec<String>),
}

/// One process, as the system describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    pid: u32,
    /// Its parent's pid, where the system says.
    parent: Option<u32>,
    command: String,
    cwd: Result<PathBuf, ErrorKind>,
}

/// Every process of this user working inside `folder`, but those in `skip`
/// and the children of `onehand`, this process: what it starts itself (its
/// agents, a `git status`, an editor) is its own, never an earlier run's
/// worker, and its agents' sessions are the caller's to check by folder.
/// Blocking.
pub fn occupants_blocking(folder: &Path, skip: &[u32], onehand: u32) -> Occupants {
    match table() {
        Ok(table) => judge(folder, table, skip, onehand),
        Err(why) => Occupants::Unknown(vec![why]),
    }
}

/// The rule, over a table however it was read.
fn judge(
    folder: &Path,
    table: impl IntoIterator<Item = Entry>,
    skip: &[u32],
    onehand: u32,
) -> Occupants {
    let (mut found, mut unknown) = (Vec::new(), Vec::new());
    let own = |entry: &Entry| skip.contains(&entry.pid) || entry.parent == Some(onehand);
    for entry in table.into_iter().filter(|entry| !own(entry)) {
        let named = format!("{} {}", entry.pid, entry.command);
        match entry.cwd {
            Ok(cwd) if cwd.starts_with(folder) => found.push(named),
            // Gone meanwhile, or one the system will not let its own user
            // trace: never an agent, whose folder is always readable.
            Ok(_) | Err(ErrorKind::NotFound | ErrorKind::PermissionDenied) => {}
            Err(_) => unknown.push(named),
        }
    }
    // Both refuse; the processes found are the ones a person can stop.
    match (found.is_empty(), unknown.is_empty()) {
        (false, _) => Occupants::Found(found),
        (true, false) => Occupants::Unknown(unknown),
        (true, true) => Occupants::None,
    }
}

/// Every process of this user: `/proc` where there is one.
#[cfg(target_os = "linux")]
fn table() -> Result<Vec<Entry>, String> {
    use std::os::unix::fs::MetadataExt as _;
    let me = std::fs::metadata("/proc/self")
        .map_err(|why| format!("/proc: {why}"))?
        .uid();
    let listed = std::fs::read_dir("/proc").map_err(|why| format!("/proc: {why}"))?;
    let mut table = Vec::new();
    for item in listed.flatten() {
        let Some(pid) = item
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let dir = item.path();
        match std::fs::metadata(&dir) {
            Ok(meta) if meta.uid() != me => continue,
            Ok(_) => {}
            Err(why) if why.kind() == ErrorKind::NotFound => continue,
            Err(why) => {
                table.push(Entry {
                    pid,
                    parent: None,
                    command: "?".to_string(),
                    cwd: Err(why.kind()),
                });
                continue;
            }
        }
        let command = std::fs::read_to_string(dir.join("comm"))
            .map(|comm| comm.trim().to_string())
            .unwrap_or_else(|_| "?".to_string());
        let cwd = std::fs::read_link(dir.join("cwd")).map_err(|why| why.kind());
        let parent = std::fs::read_to_string(dir.join("stat"))
            .ok()
            .and_then(|stat| parent_of_stat(&stat));
        table.push(Entry {
            pid,
            parent,
            command,
            cwd,
        });
    }
    Ok(table)
}

/// Every process of this user: `lsof`, where there is no `/proc`.
#[cfg(not(target_os = "linux"))]
fn table() -> Result<Vec<Entry>, String> {
    use super::LOCAL_LIMIT;
    use crate::process::output_within;
    let user = std::env::var("USER").map_err(|_| "lsof: no USER to ask for".to_string())?;
    let mut lsof = std::process::Command::new("lsof");
    lsof.args(["-a", "-d", "cwd", "-u", &user, "-FpRcn"]);
    let out = output_within(&mut lsof, LOCAL_LIMIT).map_err(|why| format!("lsof: {why}"))?;
    Ok(parse_lsof(&String::from_utf8_lossy(&out.stdout)))
}

/// `lsof -FpRcn`'s fields: `p<pid>` opens a process, `R<parent>`,
/// `c<command>` and `n<cwd>` follow it. A process with no `n` line had its
/// folder hidden.
#[cfg(any(not(target_os = "linux"), test))]
fn parse_lsof(out: &str) -> Vec<Entry> {
    let mut table: Vec<Entry> = Vec::new();
    for line in out.lines() {
        let (tag, rest) = line.split_at(line.len().min(1));
        match (tag, table.last_mut()) {
            ("p", _) => {
                if let Ok(pid) = rest.parse() {
                    table.push(Entry {
                        pid,
                        parent: None,
                        command: "?".to_string(),
                        cwd: Err(ErrorKind::PermissionDenied),
                    });
                }
            }
            ("R", Some(entry)) => entry.parent = rest.parse().ok(),
            ("c", Some(entry)) => entry.command = rest.to_string(),
            ("n", Some(entry)) => entry.cwd = Ok(PathBuf::from(rest)),
            _ => {}
        }
    }
    table
}

/// A process's parent pid from its `/proc/<pid>/stat` line: the field after
/// the state, which follows the command's closing parenthesis (a command may
/// hold parentheses and spaces of its own).
#[cfg(any(target_os = "linux", test))]
fn parent_of_stat(stat: &str) -> Option<u32> {
    let (_, after) = stat.rsplit_once(')')?;
    after.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests;
