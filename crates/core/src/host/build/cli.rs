//! A build asked from a command line, with no window: which project the
//! folder belongs to, what it brings back from the host named, and what the
//! process prints and exits with.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::shell::Shell;
use super::{build_blocking, Built, Job};
use crate::config::{HostOs, HostSpec, WorkspaceConfig, WorkspaceLoad};
use crate::workspace::{BuildHost, Workspace};
use crate::worktree::worker::repo_of_blocking;

/// What a build was asked of, from a folder.
#[derive(Debug)]
pub(crate) struct Asked {
    /// The working folder's top: the project's own, or a worktree of it.
    pub top: PathBuf,
    /// Where under `top` the person stands, and so where the command runs.
    pub within: PathBuf,
    pub entry: BuildHost,
    /// The system the host was declared with.
    pub os: HostOs,
}

/// The system `host` was declared with, or why nothing builds on it: a
/// Linux host and a Windows one build, each asked in its own shell's words,
/// and a system this build does not know does not. One nobody declared is
/// taken for Linux.
fn os_of(host: &str, declared: &[HostSpec]) -> Result<HostOs, String> {
    let Some(spec) = crate::config::declared(declared, host) else {
        return Ok(HostOs::Linux);
    };
    match spec.builds() {
        true => Ok(spec.os.clone()),
        false => Err(format!(
            "{host} runs {}; nothing builds on it yet",
            spec.os_name()
        )),
    }
}

/// The entry among `builds`, a project's build hosts, for `host`, with the
/// host's system, or why `project` cannot build there: it names no such
/// host, or the host is one nothing builds on yet.
pub fn entry_for(
    project: &str,
    builds: &[BuildHost],
    host: &str,
    declared: &[HostSpec],
) -> Result<(BuildHost, HostOs), String> {
    let Some(entry) = builds.iter().find(|entry| entry.host.0 == host) else {
        let mut names: Vec<&str> = Vec::new();
        for entry in builds {
            if !names.contains(&entry.host.0.as_str()) {
                names.push(&entry.host.0);
            }
        }
        return Err(match names.is_empty() {
            true => format!("{project} builds on no host; add one on its page"),
            false => format!(
                "{project} does not build on {host}; it builds on {}",
                names.join(", ")
            ),
        });
    };
    Ok((entry.clone(), os_of(host, declared)?))
}

/// Find the project `cwd` belongs to, as its own folder or a worktree of it,
/// among the workspaces kept under the config root `root`, and its entry for
/// `host`. A folder that is a project of several workspaces is read from the
/// first, by name, that names the host.
pub(crate) fn asked_blocking(
    root: &Path,
    declared: &[HostSpec],
    cwd: &Path,
    host: &str,
) -> Result<Asked, String> {
    let repo = repo_of_blocking(cwd)?
        .ok_or_else(|| format!("{} is not inside a git working tree", cwd.display()))?;
    // The checkout every worktree of the repository hangs off.
    let main = match repo.common.file_name() {
        Some(name) if name == ".git" => repo.common.parent().unwrap_or(&repo.common),
        _ => &repo.common,
    };
    let mut kept = crate::config::recents::list_blocking(&crate::config::workspaces_in(root));
    kept.sort_by(|a, b| (&a.name, &a.dir).cmp(&(&b.name, &b.dir)));
    // Every workspace's entry for this folder, as (the project's name, its build hosts).
    let mut found: Vec<(String, Vec<BuildHost>)> = Vec::new();
    for listed in kept {
        let cfg = match WorkspaceConfig::load_from(&listed.dir) {
            WorkspaceLoad::Found(cfg) => cfg,
            // One that cannot be read holds no project to build.
            WorkspaceLoad::Missing | WorkspaceLoad::Unreadable(_) => continue,
        };
        let workspace = Workspace::from_config(cfg, listed.dir, &Default::default());
        found.extend(
            workspace
                .roots
                .into_iter()
                .filter(|project| {
                    project
                        .local_path()
                        .is_some_and(|path| path == repo.top || path == main)
                })
                .map(|project| (project.label.clone(), project.build_hosts().to_vec())),
        );
    }
    let Some((project, _)) = found.first().cloned() else {
        return Err(format!(
            "{} is not a project of any workspace",
            repo.top.display()
        ));
    };
    let builds: Vec<BuildHost> = found.into_iter().flat_map(|(_, builds)| builds).collect();
    let (entry, os) = entry_for(&project, &builds, host, declared)?;
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let within = cwd
        .strip_prefix(&repo.top)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Ok(Asked {
        top: repo.top,
        within,
        entry,
        os,
    })
}

/// The command a person gave after `--`, for a host of system `os`: one
/// word is a line for that host's shell, taken as written; several are each
/// a word of their own, joined as that shell reads words.
pub(crate) fn command_line(words: &[String], os: &HostOs) -> Result<String, String> {
    let line = match words {
        [one] => one.trim().to_string(),
        many => Shell::of(os).join(many)?,
    };
    match line.is_empty() {
        true => Err("Give the command after --".into()),
        false => Ok(line),
    }
}

/// The exit code of a build that was stopped, as a shell says of one ended
/// by an interrupt.
const STOPPED: i32 = 130;

/// The whole flag, from the folder `cwd`: print the build's lines on `out`
/// as they come and what onehand has to say on `err`, and answer the
/// process's exit code: the command's own, 1 for a refusal or a step that
/// failed.
pub fn run(
    host: &str,
    words: &[String],
    cwd: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let declared = crate::config::AppConfig::load_resolved().0.hosts;
    let asked = asked_blocking(&crate::config::config_dir(), &declared, cwd, host);
    run_with(asked, words, out, err, build_blocking)
}

/// [`run`], with the project already looked for and the build given.
pub(crate) fn run_with(
    asked: Result<Asked, String>,
    words: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
    build: impl FnOnce(&Job<'_>, &mut dyn FnMut(&str)) -> Result<Built, String>,
) -> i32 {
    let mut refuse = |why: String| {
        let _ = writeln!(err, "onehand: {why}");
        1
    };
    let asked = match asked {
        Ok(asked) => asked,
        Err(why) => return refuse(why),
    };
    let command = match command_line(words, &asked.os) {
        Ok(command) => command,
        Err(why) => return refuse(why),
    };
    let job = Job {
        top: &asked.top,
        within: &asked.within,
        entry: &asked.entry,
        os: asked.os.clone(),
        command: &command,
    };
    // An interrupt at the terminal reaches this process and its ssh alike.
    let built = build(&job, &mut |line| {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    });
    let built = match built {
        Ok(built) => built,
        Err(why) => return refuse(why),
    };
    if let Some(hint) = &built.hint {
        let _ = writeln!(err, "onehand: {hint}");
    }
    if !built.brought.is_empty() {
        let brought: Vec<String> = built
            .brought
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        let _ = writeln!(err, "onehand: brought back {}", brought.join(", "));
    }
    match built.exit {
        Some(code) => code,
        None => {
            let _ = writeln!(err, "onehand: the build did not finish");
            STOPPED
        }
    }
}
