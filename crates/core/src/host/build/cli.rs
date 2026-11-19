//! A build asked from a command line, with no window: which project the
//! folder belongs to, what it brings back from the host named, and what the
//! process prints and exits with.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::{build_blocking, quoted, Built, Job};
use crate::config::{HostOs, HostSpec, WorkspaceConfig, WorkspaceLoad};
use crate::workspace::{BuildHost, Workspace};
use crate::worktree::worker::repo_of_blocking;

/// What a build was asked of, from a folder.
#[derive(Debug)]
pub struct Asked {
    /// The working folder's top: the project's own, or a worktree of it.
    pub top: PathBuf,
    /// Where under `top` the person stands, and so where the command runs.
    pub within: PathBuf,
    /// The project's name, for what is said of it.
    pub project: String,
    pub entry: BuildHost,
}

/// Why nothing builds on `host`, by the system it was declared with: only a
/// Linux host builds. One nobody declared is taken for Linux.
pub fn os_refusal(host: &str, declared: &[HostSpec]) -> Option<String> {
    let spec = declared.iter().find(|spec| spec.name.0 == host)?;
    match &spec.os {
        HostOs::Linux => None,
        HostOs::Windows => Some(format!(
            "{host} runs Windows; nothing builds on a Windows host yet"
        )),
        HostOs::Other(os) => Some(format!("{host} runs {os}; nothing builds on it yet")),
    }
}

/// The entry among `builds`, a project's build hosts, for `host`, or why
/// `project` cannot build there: it names no such host, or the host is one
/// nothing builds on yet.
pub fn entry_for(
    project: &str,
    builds: &[BuildHost],
    host: &str,
    declared: &[HostSpec],
) -> Result<BuildHost, String> {
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
    match os_refusal(host, declared) {
        Some(why) => Err(why),
        None => Ok(entry.clone()),
    }
}

/// Find the project `cwd` belongs to, as its own folder or a worktree of it,
/// among the workspaces kept under the config root `root`, and its entry for
/// `host`. A folder that is a project of several workspaces is read from the
/// first, by name, that names the host.
pub fn asked_blocking(
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
        let WorkspaceLoad::Found(cfg) = WorkspaceConfig::load_from(&listed.dir) else {
            continue;
        };
        let workspace = Workspace::from_config(cfg, listed.dir);
        found.extend(
            workspace
                .roots
                .into_iter()
                .filter(|project| {
                    project
                        .local_path()
                        .is_some_and(|path| path == repo.top || path == main)
                })
                .map(|project| (project.build_hosts().to_vec(), project.label))
                .map(|(builds, label)| (label, builds)),
        );
    }
    let Some((project, _)) = found.first().cloned() else {
        return Err(format!(
            "{} is not a project of any workspace",
            repo.top.display()
        ));
    };
    let builds: Vec<BuildHost> = found.into_iter().flat_map(|(_, builds)| builds).collect();
    let entry = entry_for(&project, &builds, host, declared)?;
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let within = cwd
        .strip_prefix(&repo.top)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Ok(Asked {
        top: repo.top,
        within,
        project,
        entry,
    })
}

/// The command a person gave after `--`: one word is a shell line, taken as
/// written; several are each a word of their own, quoted so and joined.
pub fn command_line(words: &[String]) -> Result<String, String> {
    let line = match words {
        [one] => one.trim().to_string(),
        many => many
            .iter()
            .map(|word| quoted(word))
            .collect::<Vec<_>>()
            .join(" "),
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
    build: impl FnOnce(&Job<'_>, &mut dyn FnMut(&str), &AtomicBool) -> Result<Built, String>,
) -> i32 {
    let mut refuse = |why: String| {
        let _ = writeln!(err, "onehand: {why}");
        1
    };
    let (asked, command) = match (asked, command_line(words)) {
        (Ok(asked), Ok(command)) => (asked, command),
        (Err(why), _) | (_, Err(why)) => return refuse(why),
    };
    let job = Job {
        top: &asked.top,
        within: &asked.within,
        host: &asked.entry,
        command: &command,
    };
    // Never set here: an interrupt at the terminal reaches this process and
    // its ssh alike.
    let cancel = AtomicBool::new(false);
    let built = build(
        &job,
        &mut |line| {
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        },
        &cancel,
    );
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
