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
//! **Every word onehand writes into a shell line is quoted** ([`quoted`]): a
//! folder, an artifact and the command itself reach the host's shell as one
//! word each, whatever they hold.
//!
//! Blocking throughout; a front end runs it off its UI.

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::client::{HostAddr, SSH_OPTS};
use super::offload::branch::{git_to_host, git_url};
use crate::workspace::BuildHost;
use crate::worktree::git_message;

/// How long a build may run before it is stopped.
pub const BUILD_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

/// The exit code a shell gives a command it could not find.
const NOT_FOUND: i32 = 127;

/// The line the host prints once the snapshot is checked out and the command
/// is about to run: what comes before it is not the command's.
const MARK: &str = "__ONEHAND_BUILD__";

/// The ref the snapshot is pushed to on the host.
const SNAP_REF: &str = "refs/onehand/snap";

/// One build to run.
pub struct Job<'a> {
    /// The working folder's top: the project's, or one of its worktrees'.
    pub top: &'a Path,
    /// Where under `top` the command runs; empty for the top itself.
    pub within: &'a Path,
    pub host: &'a BuildHost,
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
/// until it ends, `cancel` is set or [`BUILD_LIMIT`] passes. `Err` is a step
/// of onehand's own that failed, in words naming the host; the command's own
/// failure is `Ok` with its exit code.
pub fn build_blocking(
    job: &Job<'_>,
    line: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Built, String> {
    let home = dirs::home_dir().ok_or("This machine has no home folder to build in")?;
    build_in(job, &home, super::client::laptop(), line, cancel)
}

/// [`build_blocking`], with the folder the host `local` builds under and the
/// laptop's id given.
fn build_in(
    job: &Job<'_>,
    local_home: &Path,
    laptop: &str,
    line: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Built, String> {
    let host = &job.host.host;
    let addr = host.addr();
    let name = folder_name(job.top);
    let _held = lock(job.top, &host.0)
        .ok_or_else(|| format!("A build of {name} is already running on {host}"))?;
    refuse_unfit(job.top)?;
    let snapshot = crate::task::marks::pin_blocking(job.top, &[])?;

    // Where the folder is on the host: under its home for ssh, which starts
    // there, and under `local_home` for this machine.
    let rel = remote_dir(laptop, job.top);
    let (dir, url) = match addr.local {
        true => {
            let dir = local_home.join(&rel);
            (dir.display().to_string(), git_url(&addr, &dir))
        }
        false => (rel.clone(), git_url(&addr, Path::new(&format!("/~/{rel}")))),
    };
    let dir = quoted(&dir);

    let made = run(&addr, &format!("mkdir -p {dir} && git -C {dir} init -q"))?;
    if !made.status.success() {
        return Err(format!(
            "{host} could not make the folder to build in: {}",
            said(&made.stderr)
        ));
    }
    let pushed = git_to_host(job.top)
        .args(["push", "-q", "--no-verify", &url])
        .arg(format!("+{snapshot}:{SNAP_REF}"))
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !pushed.status.success() {
        return Err(format!(
            "The code did not reach {host}: {}",
            git_message(&pushed.stderr)
        ));
    }

    let within = match job.within.as_os_str().is_empty() {
        true => String::new(),
        false => format!("cd {} && ", quoted(&job.within.display().to_string())),
    };
    let script = format!(
        "cd {dir} && git checkout -q -f --detach {snapshot} && {within}echo {MARK} && \
         exec \"${{SHELL:-/bin/sh}}\" -lc {} 2>&1",
        quoted(job.command)
    );
    let exit = match run_streaming(&addr, &script, line, cancel)? {
        Ran::Exited(code) => Some(code),
        Ran::Stopped => None,
        Ran::NeverStarted(why) => {
            return Err(format!("{host} could not check the code out: {why}"));
        }
    };
    let hint = (exit == Some(NOT_FOUND)).then(|| {
        let program = job.command.split_whitespace().next().unwrap_or(job.command);
        format!("{program} is not on {host}'s PATH for ssh")
    });
    let brought = match exit {
        Some(0) => bring_back(&addr, &dir, job)?,
        Some(_) | None => Vec::new(),
    };
    Ok(Built {
        exit,
        brought,
        hint,
    })
}

/// `text` as one word of a POSIX shell line.
pub(crate) fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The folder on the host a working folder is built in, relative to the
/// host's home: by this laptop, then the folder's name and a hash of its
/// whole path, so two folders of one name never share one. The name is kept
/// to letters a shell and a URL both take as they are.
pub(crate) fn remote_dir(laptop: &str, top: &Path) -> String {
    let name: String = folder_name(top)
        .chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || matches!(c, '.' | '-') {
                true => c,
                false => '_',
            },
        )
        .collect();
    let hash = crate::chat::store::fnv1a(top.as_os_str().as_encoded_bytes());
    format!(".onehand-build/{laptop}/{name}-{hash:016x}")
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

/// `script` as the host runs it: over ssh, or through `sh` for `local`.
fn on_host(addr: &HostAddr, script: &str) -> Command {
    let mut cmd = match addr.local {
        true => {
            let mut sh = Command::new("sh");
            sh.arg("-c");
            sh
        }
        false => {
            let mut ssh = Command::new("ssh");
            ssh.args(SSH_OPTS).arg("--").arg(&addr.ssh);
            ssh
        }
    };
    cmd.arg(script).stdin(Stdio::null());
    cmd
}

/// Run `script` on the host to its end. Only ssh's own failure is an error.
fn run(addr: &HostAddr, script: &str) -> Result<std::process::Output, String> {
    let out = on_host(addr, script)
        .output()
        .map_err(|e| format!("ssh: {e}"))?;
    unreached(addr, &out.status, &out.stderr)?;
    Ok(out)
}

/// ssh's own failure, in its words: the host was not reached.
fn unreached(
    addr: &HostAddr,
    status: &std::process::ExitStatus,
    stderr: &[u8],
) -> Result<(), String> {
    if addr.local || status.code() != Some(255) {
        return Ok(());
    }
    let why = said(stderr);
    Err(match why.is_empty() {
        true => format!("{} could not be reached", addr.ssh),
        false => why,
    })
}

/// The last line of what a command said on stderr, which is the one that
/// says what went wrong.
fn said(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
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
    addr: &HostAddr,
    script: &str,
    line: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Ran, String> {
    let mut child = on_host(addr, script)
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
    unreached(addr, &status, &stderr)?;
    if !running {
        return Ok(Ran::NeverStarted(said(&stderr)));
    }
    Ok(match status.code() {
        Some(code) => Ran::Exited(code),
        None => Ran::Stopped,
    })
}

/// Bring the job's artifacts back from `dir` on the host, into the working
/// folder or the folder under it the entry names. All of them or none: one
/// the build did not make fails the lot before anything is written here.
fn bring_back(addr: &HostAddr, dir: &str, job: &Job<'_>) -> Result<Vec<PathBuf>, String> {
    let host = &job.host.host;
    if job.host.artifacts.is_empty() {
        return Ok(Vec::new());
    }
    let paths: Vec<String> = job
        .host
        .artifacts
        .iter()
        .map(|path| quoted(&path.display().to_string()))
        .collect();
    let paths = paths.join(" ");
    let script = format!(
        "cd {dir} && for p in {paths}; do test -e \"$p\" || {{ printf %s \"$p\" >&2; exit 3; }}; \
         done && tar -cf - -- {paths}"
    );
    let into = match &job.host.into {
        Some(into) => job.top.join(into),
        None => job.top.to_path_buf(),
    };
    std::fs::create_dir_all(&into).map_err(|e| format!("{}: {e}", into.display()))?;
    let mut sent = on_host(addr, &script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    let unpacked = Command::new("tar")
        .args(["-xf", "-", "-C"])
        .arg(&into)
        .stdin(Stdio::from(sent.stdout.take().expect("piped")))
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("tar: {e}"))?;
    let sent = sent.wait_with_output().map_err(|e| format!("ssh: {e}"))?;
    unreached(addr, &sent.status, &sent.stderr)?;
    match sent.status.code() {
        Some(0) => {}
        Some(3) => {
            return Err(format!(
                "{} was not built on {host}",
                String::from_utf8_lossy(&sent.stderr)
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
    Ok(job
        .host
        .artifacts
        .iter()
        .map(|path| match &job.host.into {
            Some(into) => into.join(path),
            None => path.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests;
