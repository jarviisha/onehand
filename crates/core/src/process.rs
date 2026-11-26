//! Running a program that nobody is watching, with a limit on how long it may
//! take.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Why a bounded command gave no output.
///
/// Kept apart rather than flattened to text, because the two mean different
/// things to a caller: a program that is not installed is a fact about the
/// machine, and one that ran too long is a fact about right now.
#[derive(Debug)]
pub enum Failure {
    /// There is no such program to run.
    Missing,
    /// It could not be started, for another reason.
    Unstarted(std::io::Error),
    /// It ran past its limit and was stopped.
    TimedOut(Duration),
    /// It was called off before it finished, and was stopped.
    Cancelled,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("is not installed"),
            Self::Unstarted(err) => write!(f, "could not be started: {err}"),
            Self::TimedOut(limit) => write!(
                f,
                "did not finish within {}s and was stopped",
                limit.as_secs()
            ),
            Self::Cancelled => f.write_str("was called off and stopped"),
        }
    }
}

/// How long a command that has exited is given to hand over its output.
const DRAIN_FLOOR: Duration = Duration::from_secs(1);

/// How long what a command printed is waited for once it has exited, when it
/// ran with no deadline: long enough for its last output to land, bounded so
/// a pipe something else still holds cannot keep it.
// ponytail: output landing later than this after the exit is dropped unsaid;
// say the cut in the result if a slow-flushing command ever needs it.
const UNBOUNDED_DRAIN: Duration = Duration::from_secs(10);

/// Run `cmd` to completion, or stop it once it has run for `limit`.
///
/// **A command nobody is watching must not be able to hang its caller.** A
/// call to a forge waiting on a network that went away, or a `git` waiting on a
/// credential prompt nobody will answer, would otherwise hold its caller for
/// good. Standard input is closed for the same reason: a program that asks its
/// terminal a question gets an end of file rather than a wait.
///
/// The limit covers the output as well as the exit. Both pipes are drained on
/// threads of their own, because a child that fills a pipe nobody reads blocks
/// just as surely — and waiting for those threads is bounded too, because a
/// program can exit while something it started (an ssh connection kept open
/// for reuse, a credential helper) still holds the pipe and never closes it.
/// Whatever is read by the deadline is what is handed back.
///
/// On Unix the command runs in a process group of its own, and stopping it
/// stops the whole group, so what it started goes with it.
pub fn output_within(cmd: &mut Command, limit: Duration) -> Result<Output, Failure> {
    output_until(cmd, Some(limit), &AtomicBool::new(false))
}

/// [`output_within`], also stopped as soon as `cancel` is set, and with no
/// deadline when `limit` is `None`. Only once the
/// command and its process group have been stopped does it return, so a
/// caller that hears [`Failure::Cancelled`] knows nothing of it is left
/// running.
pub fn output_until(
    cmd: &mut Command,
    limit: Option<Duration>,
    cancel: &AtomicBool,
) -> Result<Output, Failure> {
    // Called off already: never started, so it cannot touch the work in the
    // moment before the first look at the flag below.
    if cancel.load(Ordering::SeqCst) {
        return Err(Failure::Cancelled);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => Failure::Missing,
            _ => Failure::Unstarted(err),
        })?;
    // Both are piped just above, so both are there to take.
    let out = drain(child.stdout.take().expect("stdout was piped"));
    let err = drain(child.stderr.take().expect("stderr was piped"));
    let deadline = limit.map(|limit| Instant::now() + limit);
    // From a millisecond, doubling to a twentieth of a second: most commands
    // run here (git asked one thing) end in a few milliseconds, and a first
    // look only after 50ms made each of them cost that much.
    let mut wait = Duration::from_millis(1);
    let status = loop {
        match child.try_wait().map_err(Failure::Unstarted)? {
            Some(status) => break status,
            None if deadline.is_some_and(|deadline| Instant::now() >= deadline) => {
                stop(&mut child);
                return Err(Failure::TimedOut(limit.unwrap_or_default()));
            }
            None if cancel.load(Ordering::SeqCst) => {
                stop(&mut child);
                return Err(Failure::Cancelled);
            }
            None => {
                std::thread::sleep(wait);
                wait = (wait * 2).min(Duration::from_millis(50));
            }
        }
    };
    // Whatever the child left running is ended too, so a pipe it was holding
    // closes and the drains below come back at once rather than at the deadline.
    stop(&mut child);
    // At least a moment, whatever the deadline left: a command that exited in
    // the last poll before it would otherwise come back successful and silent,
    // and a caller reading its stdout (the number of an issue it just made)
    // would take the silence for an answer.
    // With no deadline, what is still held open past a short while is let
    // go: the command has exited and its group is gone.
    let left = || {
        deadline
            .map_or(UNBOUNDED_DRAIN, |at| {
                at.saturating_duration_since(Instant::now())
            })
            .max(DRAIN_FLOOR)
    };
    Ok(Output {
        status,
        stdout: out.recv_timeout(left()).unwrap_or_default(),
        stderr: err.recv_timeout(left()).unwrap_or_default(),
    })
}

/// Read `pipe` to its end on a thread of its own, handing the bytes back
/// through a channel so the wait for them can be bounded.
fn drain(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    rx
}

/// End `child` and, on Unix, every process in its group.
fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    kill_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

/// Kill every process in the group `leader` leads, without asking first.
#[cfg(unix)]
pub(crate) fn kill_group(leader: u32) {
    // ponytail: through `kill(1)` rather than a libc binding, for one call.
    // The `--` is load-bearing: procps' `kill` reads a bare `-<pid>` as an
    // option rather than as a process group, and does nothing.
    let _ = Command::new("kill")
        .args(["-KILL", "--"])
        .arg(format!("-{leader}"))
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_that_outlives_its_limit_is_stopped_and_said() {
        let started = Instant::now();
        let out = output_within(Command::new("sleep").arg("5"), Duration::from_millis(200));
        assert!(out.unwrap_err().to_string().contains("did not finish"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn a_called_off_command_returns_only_once_its_whole_group_is_gone() {
        let marker = std::env::temp_dir().join(format!("onehand-cancel-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let calling_off = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            calling_off.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        // A child that would write to the work a second later, the way a
        // build left running by a stopped run would.
        let script = format!("(sleep 1; touch {}) & sleep 30", marker.display());
        let out = output_until(
            Command::new("sh").args(["-c", &script]),
            Some(Duration::from_secs(60)),
            &cancel,
        );
        assert!(matches!(out, Err(Failure::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!marker.exists(), "a child of the stopped command still ran");
    }

    /// With no limit, a command runs until it exits or is called off; being
    /// called off still stops it at once.
    #[test]
    fn no_limit_runs_until_called_off() {
        let finished = output_until(
            Command::new("sh").args(["-c", "sleep 0.2; echo done"]),
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&finished.stdout).contains("done"));
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let calling_off = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            calling_off.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let out = output_until(Command::new("sleep").arg("5"), None, &cancel);
        assert!(matches!(out, Err(Failure::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    /// Called off before it began: it never starts, so it cannot touch the
    /// work in the moment before the first look at the flag.
    #[test]
    fn a_command_called_off_before_it_starts_never_runs() {
        let marker = std::env::temp_dir().join(format!("onehand-unstarted-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let cancel = AtomicBool::new(true);
        let out = output_until(
            Command::new("touch").arg(&marker),
            Some(Duration::from_secs(5)),
            &cancel,
        );
        assert!(matches!(out, Err(Failure::Cancelled)));
        assert!(!marker.exists(), "the called-off command ran");
    }

    #[test]
    fn a_grandchild_holding_the_pipe_open_cannot_hang_it() {
        // The shell exits at once; the `sleep` it leaves behind keeps stdout
        // open, which is what an ssh connection kept for reuse does under a
        // `git fetch`.
        let started = Instant::now();
        let out = output_within(
            Command::new("sh").args(["-c", "echo hi; sleep 5 &"]),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hi"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_program_that_is_not_there_is_told_apart_from_one_that_ran_long() {
        let missing = output_within(
            &mut Command::new("onehand-no-such-program"),
            Duration::from_secs(1),
        );
        assert!(matches!(missing, Err(Failure::Missing)));
        let long = output_within(Command::new("sleep").arg("5"), Duration::from_millis(100));
        assert!(matches!(long, Err(Failure::TimedOut(_))));
    }

    #[test]
    fn a_command_that_finishes_hands_back_what_it_printed() {
        let out = output_within(
            Command::new("sh").args(["-c", "echo out; echo err >&2"]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "err");
    }
}
