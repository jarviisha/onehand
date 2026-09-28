//! Running a program that nobody is watching, with a limit on how long it may
//! take.

use std::io::Read;
use std::process::{Command, Output, Stdio};
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
        }
    }
}

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
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait().map_err(Failure::Unstarted)? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                stop(&mut child);
                return Err(Failure::TimedOut(limit));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    // Whatever the child left running is ended too, so a pipe it was holding
    // closes and the drains below come back at once rather than at the deadline.
    stop(&mut child);
    let left = |at: Instant| at.saturating_duration_since(Instant::now());
    Ok(Output {
        status,
        stdout: out.recv_timeout(left(deadline)).unwrap_or_default(),
        stderr: err.recv_timeout(left(deadline)).unwrap_or_default(),
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
    {
        // ponytail: through `kill(1)` rather than a libc binding, for one call.
        // The `--` is load-bearing: procps' `kill` reads a bare `-<pid>` as an
        // option rather than as a process group, and does nothing.
        let _ = Command::new("kill")
            .args(["-KILL", "--"])
            .arg(format!("-{}", child.id()))
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
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
