//! Building a folder on a host over the person's own ssh.
//!
//! **Five steps, none needing a service on the host.** The folder as it
//! stands, uncommitted work included and ignored files left out, is committed
//! aside as a snapshot that moves neither the branch nor the index; pushed to
//! a repository the host keeps for this folder; checked out there, which
//! removes what the snapshot dropped and leaves what git ignores, so a build
//! is incremental; the command run in it, each line it prints handed over as
//! it comes; and, when it exits 0, the paths the project names brought back.
//!
//! **Every word onehand writes into a shell line is quoted for the host's
//! shell** (`Shell::word`), or refused where that shell has no way to hold
//! it: a folder and an artifact reach it as one word each, and the command
//! reaches the shell that runs it as the person wrote it.
//!
//! Blocking throughout; a front end runs it off its UI.

pub mod cli;
mod shell;

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::client::HostAddr;
use super::offload::branch::{git_to_host, git_url};
use super::ssh::{self, said};
use crate::config::HostOs;
use crate::workspace::{inside_refusal, BuildHost, HostName};
use crate::worktree::git_message;
use shell::Shell;

/// How long a build may run before it is stopped.
pub(crate) const BUILD_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

/// The line the host prints once the snapshot is checked out and the command
/// is about to run: what comes before it is not the command's.
pub(super) const MARK: &str = "__ONEHAND_BUILD__";

/// The ref the snapshot is pushed to on the host.
const SNAP_REF: &str = "refs/onehand/snap";

/// One build to run.
pub struct Job<'a> {
    /// The working folder's top: the project's, or one of its worktrees'.
    pub top: &'a Path,
    /// Where under `top` the command runs; empty for the top itself.
    pub within: &'a Path,
    /// The project's entry for the host it builds on.
    pub entry: &'a BuildHost,
    /// The system the host was declared with, which says whose shell the
    /// steps are spelled for.
    pub os: HostOs,
    pub command: &'a str,
}

/// What a build came to.
#[derive(Debug, PartialEq, Eq)]
pub struct Built {
    /// The command's exit code; `None` when it was stopped or ran out of time.
    pub exit: Option<i32>,
    /// What came back, as paths under the working folder's top.
    pub brought: Vec<PathBuf>,
    /// Whose `PATH` lacks the program, after a command that was not found.
    pub hint: Option<String>,
}

/// Run `job`, handing each line the command prints to `line` as it comes,
/// until it ends or [`BUILD_LIMIT`] passes. `Err` is a step of onehand's own
/// that failed, in words naming the host; the command's own failure is `Ok`
/// with its exit code.
pub fn build_blocking(job: &Job<'_>, line: &mut dyn FnMut(&str)) -> Result<Built, String> {
    let home = dirs::home_dir().ok_or("This machine has no home folder to build in")?;
    // Nothing sets it: whoever asked ends the build by ending this.
    let never = AtomicBool::new(false);
    build_in(job, &home, super::client::laptop(), line, &never)
}

/// [`build_blocking`], with the folder the host `local` builds under, the
/// laptop's id, and a flag that stops it.
fn build_in(
    job: &Job<'_>,
    local_home: &Path,
    laptop: &str,
    line: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Built, String> {
    let host = &job.entry.host;
    // Checked here too, where every way in arrives: an entry written into
    // the workspace's file by hand has been through no form.
    let named = job.entry.artifacts.iter().chain(&job.entry.into);
    if let Some(why) = named
        .filter_map(|path| inside_refusal(&path.display().to_string()))
        .next()
    {
        return Err(why);
    }
    let _held = lock(job.top, &host.0).ok_or_else(|| {
        format!(
            "A build of {} is already running on {host}",
            folder_name(job.top)
        )
    })?;
    refuse_unfit(job.top)?;
    let snapshot = crate::task::marks::pin_blocking(job.top, &[])?;
    let place = Place::of(host, &job.os, &host_dir(laptop, job.top), local_home);

    place.receive(job.top, &snapshot)?;
    let (exit, hint) = place.run(job, &snapshot, line, cancel)?;
    let brought = match exit {
        Some(0) => place.send_back(job)?,
        Some(_) | None => Vec::new(),
    };
    Ok(Built {
        exit,
        brought,
        hint,
    })
}

/// Where a build happens: the host, how its shell is spoken to, and the
/// folder it keeps for the working folder being built.
struct Place<'a> {
    host: &'a HostName,
    addr: HostAddr,
    shell: Shell,
    /// The folder as the host's shell is told it: relative to the home ssh
    /// starts in, or whole for this machine.
    dir: String,
    /// The same folder as `git push` is told it.
    url: String,
}

impl<'a> Place<'a> {
    /// The folder `rel` on `host`, a system `os`; under `local_home` when
    /// the host is this machine, which is always asked through `sh`.
    fn of(host: &'a HostName, os: &HostOs, rel: &str, local_home: &Path) -> Self {
        let addr = host.addr();
        let (shell, dir, url) = match (addr.local, Shell::of(os)) {
            (true, _) => {
                let dir = local_home.join(rel);
                (
                    Shell::Posix,
                    dir.display().to_string(),
                    git_url(&addr, &dir),
                )
            }
            (false, Shell::Posix) => (
                Shell::Posix,
                rel.to_string(),
                git_url(&addr, Path::new(&format!("/~/{rel}"))),
            ),
            // `cmd.exe` cannot take git's own quoting of the folder, so it
            // is named as scp names one, relative to the home.
            (false, Shell::Cmd) => (Shell::Cmd, rel.to_string(), format!("{}:{rel}", addr.ssh)),
        };
        Self {
            host,
            addr,
            shell,
            dir,
            url,
        }
    }

    /// `script` run on the host to its end. Only ssh's own failure is an error.
    fn ask(&self, script: &str) -> Result<Output, String> {
        let out = self
            .on_host(script)
            .output()
            .map_err(|e| format!("ssh: {e}"))?;
        ssh::unreached(&self.addr, &out.status, &out.stderr)?;
        Ok(out)
    }

    /// `script` as the host is to run it.
    fn on_host(&self, script: &str) -> Command {
        on_host(&self.addr, self.shell, script)
    }

    /// Make the folder and its repository on the host, refusing one with no
    /// `git` or `tar` before anything is sent, then push `snapshot` to it.
    fn receive(&self, top: &Path, snapshot: &str) -> Result<(), String> {
        let host = self.host;
        let made = self.ask(&self.shell.init(&self.dir)?)?;
        if let Some(why) = needs_refusal(host, made.status.code(), &made.stderr) {
            return Err(why);
        }
        if !made.status.success() {
            return Err(format!(
                "{host} could not make the folder to build in: {}",
                said(&made.stderr)
            ));
        }
        // On Windows the push is received through PowerShell, for the same
        // reason the folder is named as scp names it.
        let receive = match self.shell {
            Shell::Posix => None,
            Shell::Cmd => Some("--receive-pack=powershell -NoProfile git receive-pack"),
        };
        let pushed = git_to_host(top)
            .env("GIT_SSH_COMMAND", ssh::for_git())
            .args(["push", "-q", "--no-verify"])
            .args(receive)
            .arg(&self.url)
            .arg(format!("+{snapshot}:{SNAP_REF}"))
            .output()
            .map_err(|e| format!("git: {e}"))?;
        match pushed.status.success() {
            true => Ok(()),
            false => Err(format!(
                "The code did not reach {host}: {}",
                git_message(&pushed.stderr)
            )),
        }
    }

    /// Check `snapshot` out and run the job's command: its exit code, none
    /// when it was stopped, and whose `PATH` lacked its program when it was
    /// not found.
    fn run(
        &self,
        job: &Job<'_>,
        snapshot: &str,
        line: &mut dyn FnMut(&str),
        cancel: &AtomicBool,
    ) -> Result<(Option<i32>, Option<String>), String> {
        let host = self.host;
        let script = self
            .shell
            .run(&self.dir, snapshot, job.within, job.command)?;
        // The last lines printed are kept: they are where a shell says a
        // program was not there, and which.
        let mut last: Vec<String> = Vec::new();
        let ran = run_streaming(
            self.on_host(&script),
            &self.addr,
            &mut |text| {
                if !text.trim().is_empty() {
                    if last.len() == LAST_LINES {
                        last.remove(0);
                    }
                    last.push(text.to_string());
                }
                line(text);
            },
            cancel,
        )?;
        let exit = match ran {
            Ran::Exited(code) => Some(code),
            Ran::Stopped => None,
            Ran::NeverStarted(why) => {
                return Err(format!("{host} could not check the code out: {why}"));
            }
        };
        let hint = self
            .shell
            .not_found(exit, &last, job.command)
            .map(|program| format!("{program} is not on {host}'s PATH for ssh"));
        Ok((exit, hint))
    }

    /// Bring the job's artifacts back from the host, into the working folder
    /// or the folder under it the entry names. All of them or none: one the
    /// build did not make fails the lot before anything is written here.
    fn send_back(&self, job: &Job<'_>) -> Result<Vec<PathBuf>, String> {
        let host = self.host;
        let entry = job.entry;
        if entry.artifacts.is_empty() {
            return Ok(Vec::new());
        }
        let script = self.shell.send(&self.dir, &entry.artifacts)?;
        let into = match &entry.into {
            Some(into) => job.top.join(into),
            None => job.top.to_path_buf(),
        };
        std::fs::create_dir_all(&into).map_err(|e| format!("{}: {e}", into.display()))?;
        let mut sent = self
            .on_host(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ssh: {e}"))?;
        let unpacked = unpack(&into, &entry.artifacts)
            .stdin(Stdio::from(sent.stdout.take().expect("piped")))
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("tar: {e}"))?;
        let sent = sent.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
        ssh::unreached(&self.addr, &sent.status, &sent.stderr)?;
        match sent.status.code() {
            Some(0) => {}
            Some(shell::NOT_BUILT) => {
                return Err(format!(
                    "{} was not built on {host}",
                    String::from_utf8_lossy(&sent.stderr).trim()
                ));
            }
            _ => {
                return Err(format!(
                    "{host} could not send what it built: {}",
                    said(&sent.stderr)
                ));
            }
        }
        if !unpacked.status.success() {
            return Err(format!(
                "What {host} built could not be written here: {}",
                said(&unpacked.stderr)
            ));
        }
        Ok(entry
            .artifacts
            .iter()
            .map(|path| match &entry.into {
                Some(into) => into.join(path),
                None => path.clone(),
            })
            .collect())
    }
}

/// How many of the last lines a command printed are kept to read what its
/// shell said of a program it could not find: `cmd.exe` says it over two.
const LAST_LINES: usize = 2;

/// Why nothing builds on `host`, when making its folder ended saying which
/// of the programs a build needs it lacks.
fn needs_refusal(host: &HostName, exit: Option<i32>, stderr: &[u8]) -> Option<String> {
    if exit != Some(shell::NEEDS) {
        return None;
    }
    let text = String::from_utf8_lossy(stderr);
    let lacking: Vec<&str> = text.split_whitespace().collect();
    Some(match lacking.as_slice() {
        [one] => format!("{host} has no {one}; a build needs git and tar"),
        many => format!("{host} has no {}; a build needs both", many.join(" or ")),
    })
}

/// How many of the last lines a command printed a tool's answer carries.
pub(crate) const TOOL_LINES: usize = 200;

/// The last [`TOOL_LINES`] lines a command printed, and how many came before
/// them: what a build of any length leaves for an answer.
#[derive(Default)]
pub struct Tail {
    lines: std::collections::VecDeque<String>,
    earlier: usize,
}

impl Tail {
    pub fn push(&mut self, line: &str) {
        if self.lines.len() == TOOL_LINES {
            self.lines.pop_front();
            self.earlier += 1;
        }
        self.lines.push_back(line.to_string());
    }
}

/// What a tool answers of a build: how the command ended, whose `PATH`
/// lacked it, what came back, then the last lines it printed, saying how
/// many came before them.
pub fn answer(built: &Built, tail: &Tail) -> String {
    let mut said = vec![match built.exit {
        Some(code) => format!("exit {code}"),
        None => "did not finish".to_string(),
    }];
    said.extend(built.hint.clone());
    if !built.brought.is_empty() {
        let brought: Vec<String> = built
            .brought
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        said.push(format!("brought back: {}", brought.join(", ")));
    }
    if tail.earlier > 0 {
        said.push(format!("… {} earlier lines not shown", tail.earlier));
    }
    said.extend(tail.lines.iter().cloned());
    said.join("\n")
}

/// The folder on the host a working folder is built in, relative to the
/// host's home: by this laptop, then the folder's name and a hash of its
/// whole path, so two folders of one name never share one. The name is kept
/// to letters a shell and a URL both take as they are.
pub(crate) fn host_dir(laptop: &str, top: &Path) -> String {
    let name = folder_name(top);
    let hash = crate::chat::store::fnv1a(top.as_os_str().as_encoded_bytes());
    // The laptop's id is read from a file, and goes into a URL and a shell
    // line as the name does.
    format!(
        ".onehand-build/{}/{}-{hash:016x}",
        safe(laptop),
        safe(&name)
    )
}

/// `text` kept to the letters a shell and a URL both take as they are.
fn safe(text: &str) -> String {
    text.chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || matches!(c, '.' | '-') {
                true => c,
                false => '_',
            },
        )
        .collect()
}

fn folder_name(top: &Path) -> String {
    top.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| top.display().to_string())
}

/// A folder's build on a host, held: let go when dropped.
struct Held(std::fs::File);

impl Drop for Held {
    /// Unlocked in so many words, not left to the file closing: a process
    /// another thread forked while this was held carries a copy of the file
    /// until it execs, and a lock let go only by closing would outlive this
    /// for that moment, refusing the build that follows at once.
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// The lock that keeps a folder to one build on a host at a time; `None`
/// while another holds it.
fn lock(top: &Path, host: &str) -> Option<Held> {
    let dir = crate::config::config_dir().join("builds");
    std::fs::create_dir_all(&dir).ok()?;
    let key = format!("{}\0{host}", top.display());
    let path = dir.join(format!(
        "{:016x}.lock",
        crate::chat::store::fnv1a(key.as_bytes())
    ));
    let file = std::fs::File::create(path).ok()?;
    file.try_lock().ok()?;
    Some(Held(file))
}

/// Refuse a folder a snapshot cannot carry whole: one that is not a working
/// tree's top, or that holds another repository, which git lists as one
/// folder and commits as a pointer to it.
fn refuse_unfit(top: &Path) -> Result<(), String> {
    let is_top = crate::worktree::repo_top_blocking(top)
        .is_some_and(|found| top.canonicalize().is_ok_and(|top| top == found));
    if !is_top {
        return Err(format!(
            "{} is not the top of a git working tree",
            top.display()
        ));
    }
    let out = crate::worktree::git(top)
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
    for name in out.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name);
        let nested = name.ends_with('/')
            || std::fs::symlink_metadata(top.join(&*name)).is_ok_and(|m| m.is_dir());
        if nested {
            return Err(format!(
                "{} is a repository inside this one; a project holding one cannot be built on a host yet",
                name.trim_end_matches('/')
            ));
        }
    }
    Ok(())
}

/// `script` as the host runs it: as it is for this machine, and for another
/// as its shell is handed a line over ssh.
fn on_host(addr: &HostAddr, shell: Shell, script: &str) -> Command {
    match addr.local {
        true => ssh::command(addr, script),
        false => ssh::command(addr, &shell.over_ssh(script)),
    }
}

/// How the command's run ended.
enum Ran {
    Exited(i32),
    /// Stopped by the caller, or out of time.
    Stopped,
    /// The host never reached the command, for why.
    NeverStarted(String),
}

/// Run `script`, handing `line` each line printed after [`MARK`], until it
/// ends, `cancel` is set or [`BUILD_LIMIT`] passes.
///
/// A stopped run kills its ssh. Whether the command on the host dies with it
/// is the host's to say: nothing here can reach it once the connection goes.
fn run_streaming(
    mut on_host: Command,
    addr: &HostAddr,
    line: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Ran, String> {
    let mut child = on_host
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    let stdout = child.stdout.take().expect("piped");
    let (tx, rx) = mpsc::channel::<String>();
    // Never joined: a command that outlives a stopped run keeps the pipe
    // open, and the reader ends with it.
    std::thread::spawn(move || {
        for text in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
            let text = String::from_utf8_lossy(&text)
                .trim_end_matches('\r')
                .to_string();
            if tx.send(text).is_err() {
                break;
            }
        }
    });
    let started = Instant::now();
    let mut running = false;
    let mut take = |text: String, running: &mut bool| match *running {
        true => line(&text),
        false => *running = text.trim() == MARK,
    };
    let status = loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(text) => take(text, &mut running),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // The pipe closed: the command is over, or about to be.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break child.wait().map_err(|e| format!("ssh: {e}"))?;
            }
        }
        if cancel.load(Ordering::Relaxed) || started.elapsed() >= BUILD_LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Ran::Stopped);
        }
    };
    for text in rx.try_iter() {
        take(text, &mut running);
    }
    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = std::io::Read::read_to_end(&mut pipe, &mut stderr);
    }
    ssh::unreached(addr, &status, &stderr)?;
    if !running {
        return Ok(Ran::NeverStarted(said(&stderr)));
    }
    Ok(match status.code() {
        Some(code) => Ran::Exited(code),
        None => Ran::Stopped,
    })
}

/// `tar` unpacking, into `into`, only `paths` of what it is fed: what a host
/// sends is the host's to choose, and nothing but what was asked for is
/// written here, whatever else its answer holds.
fn unpack(into: &Path, paths: &[PathBuf]) -> Command {
    let mut tar = Command::new("tar");
    tar.args(["-xf", "-", "-C"]).arg(into).arg("--").args(paths);
    tar
}

#[cfg(test)]
mod tests;
