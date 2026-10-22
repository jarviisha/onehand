//! The adapter as the serve loop reaches it: its pipes, its stderr tail,
//! and the process group that goes with it.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Anything the serve loop needs from the far end: the two pipes, and the news
/// that it will not be answering again.
///
/// **The seam is the byte stream, not the process.** A child is one thing that
/// satisfies it and the only one in production; what the split buys is that the
/// loop above it can be driven by a test at all. Boxed rather than generic so
/// the functions that write into it keep their signatures free of a type
/// parameter that says nothing about what they do.
pub(super) struct Transport {
    /// Lines the far end sends.
    pub(super) incoming: Pin<Box<dyn tokio::io::AsyncBufRead + Send>>,
    /// Where messages to the far end go.
    pub(super) outgoing: Outgoing,
    /// Resolves, with a sentence, once the far end can no longer answer.
    ///
    /// **Owns whatever it is waiting on**, which for a child process is the
    /// child itself — so dropping this future is what kills the adapter, and the
    /// stream owning it is what ties the agent's life to the subscription.
    pub(super) ended: Pin<Box<dyn std::future::Future<Output = String> + Send>>,
    /// The last lines the adapter wrote to stderr.
    ///
    /// **An adapter that dies explains itself on stderr and nowhere else.**
    /// What reaches the protocol is silence — stdout closes without a further
    /// message — so every distinct failure arrives at the reader as the same
    /// sentence: the adapter stopped. The reason was printed a moment earlier
    /// and, without this, went only to *our* stderr, which nobody reads when the
    /// app was started from a desktop icon.
    ///
    /// **Kept for any death, not only a failure to start.** A start that never
    /// happens is the case this was built for, but the tail costs the same at
    /// every other one, and the alternative is deciding — at the moment
    /// something has already gone wrong — that the reader may not see what was
    /// said.
    ///
    /// Shared with the draining task rather than read off the pipe at failure
    /// time, because by then the pipe is closed and the process is gone.
    pub(super) stderr: StderrTail,
    /// The task filling [`Self::stderr`], so a failure can let it catch up.
    ///
    /// **Nothing else orders the two.** The serve loop gives up the moment
    /// stdout ends, while the reason is carried by a separate task that may not
    /// have been polled yet — and a child is free to close stdout *before* it
    /// writes why, which puts the reason strictly after the event that ends the
    /// loop. Read without waiting, the tail is then empty exactly when it
    /// matters, and intermittently, by thread scheduling.
    pub(super) drain: Option<tokio::task::JoinHandle<()>>,
}

/// The adapter's own last words, kept for whoever has to explain its death.
pub(super) type StderrTail = std::sync::Arc<std::sync::Mutex<LastWords>>;

/// A bounded tail of adapter stderr, and how much of it fell off the front.
///
/// **The count is the half that keeps the bound honest.** Twenty lines is
/// plenty for one tool's error block and nothing at all for its output, so a
/// chatty adapter silently reduces to its least interesting lines. Saying how
/// many were dropped is what stops the survivors from reading as everything
/// that was said — the same reason the terminal extension reports a capped
/// buffer rather than quietly shortening it.
#[derive(Default)]
pub(super) struct LastWords {
    pub(super) kept: VecDeque<String>,
    pub(super) dropped: usize,
}

/// How many lines of adapter stderr are kept, and how wide each may be.
///
/// **Room for the block plus whatever it arrived behind.** The failures this
/// exists for announce themselves in a handful of lines — `npx` naming a version
/// it cannot resolve, a runtime naming a module it cannot find — but the line
/// that names the fault class comes *first* in such a block, so a tail sized to
/// the block exactly loses that line to any warning printed after it. The npm
/// error this was written for is five lines on its own; at five, one
/// `npm warn exec` ahead of it is enough to evict the reason.
///
/// The width is set by paths: a module or package a runtime cannot find is
/// named with its full path, and cutting mid-path leaves the half that is the
/// same for every adapter.
///
/// Twenty lines at two hundred characters is four kilobytes per session, which
/// is the whole cost of leaving this on. What it does *not* bound is one
/// enormous line: the reader assembles a line before this ever sees it, so an
/// adapter that writes a megabyte without a newline still spends it.
pub(super) const STDERR_TAIL_LINES: usize = 20;
pub(super) const STDERR_LINE_CAP: usize = 200;

/// How long a failed adapter is given to finish saying why.
///
/// **Bounded because the pipe can outlive the process we spawned.** Waiting for
/// the drain to reach end-of-file is the exact wait wanted — it is how the tail
/// is known to be complete — but the write end belongs to whoever holds it, and
/// the adapter is a chain (`npx` runs `node` runs the agent) whose middle can
/// die while a grandchild keeps stderr open. Unbounded, that is a hang on the
/// one path where the app is already trying to report a failure.
///
/// Paid only on the way to a `Disconnected`, where nothing is watching for a
/// quarter second.
pub(super) const STDERR_GRACE: Duration = Duration::from_millis(250);

/// The write half of a [`Transport`], as every message-writing function takes
/// it.
pub(super) type Outgoing = Pin<Box<dyn tokio::io::AsyncWrite + Send>>;

impl LastWords {
    /// Keep one stderr line, dropping the oldest once the tail is full.
    ///
    /// Blank lines are skipped because tools pad their error blocks with them,
    /// and a kept blank is one of the few slots not spent on a reason. A
    /// skipped blank is not a drop: nothing was lost to report.
    ///
    /// So is an `npm notice` line: npm narrating its own start, never the
    /// reason an adapter died, and each one kept costs a slot a reason needs.
    pub(super) fn remember(&mut self, line: &str) {
        let line = line.trim();
        let notice = line
            .get(..10)
            .is_some_and(|head| head.eq_ignore_ascii_case("npm notice"));
        if line.is_empty() || notice {
            return;
        }
        if self.kept.len() == STDERR_TAIL_LINES {
            self.kept.pop_front();
            self.dropped += 1;
        }
        // Cut on a character boundary — an adapter is free to print a path, and
        // slicing a multi-byte character in half panics rather than shortens.
        let end = line
            .char_indices()
            .nth(STDERR_LINE_CAP)
            .map_or(line.len(), |(at, _)| at);
        self.kept.push_back(line[..end].to_string());
    }

    /// What the adapter said, as one line, or nothing if it said nothing.
    ///
    /// Joined rather than kept as separate lines because the caller folds this
    /// into a single error string, and what draws that string decides for
    /// itself where to wrap.
    pub(super) fn say(&self) -> Option<String> {
        if self.kept.is_empty() {
            return None;
        }
        let mut said = self.kept.iter().cloned().collect::<Vec<_>>();
        if self.dropped > 0 {
            said.insert(0, format!("(+{} earlier lines)", self.dropped));
        }
        Some(said.join(" · "))
    }
}

/// Put the adapter's last words behind the sentence describing its death.
///
/// The protocol-level sentence stays in front because it is what happened —
/// the tail is evidence, and evidence that pushed the finding off the start of
/// the line would be worse than none.
pub(super) fn explain(what: String, tail: &StderrTail) -> String {
    // A poisoned lock means the drain task panicked mid-write, which leaves the
    // tail readable and every line before the panic still in it. Taking it
    // anyway is what stops one panic from costing the reader the reason; the
    // alternative degrades to silence exactly when there is most to say.
    let said = tail
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .say();
    match said {
        Some(said) => format!("{what}: {said}"),
        None => what,
    }
}

/// How long an adapter's group is given to leave on its own before what is
/// left of it is killed.
pub(super) const ADAPTER_GRACE: Duration = Duration::from_secs(2);

/// The adapter's process group, killed a grace after this is dropped: with
/// the transport, whichever way the stream ends.
///
/// **Not at once.** The child itself goes on drop, but the agent under it
/// learns the session is over from its stdin closing, and may still have its
/// own session to write down for a resume; a group killed on the spot cut that
/// short. What is left after the grace is what would otherwise outlive it.
pub(super) struct Group(Option<u32>);

impl Drop for Group {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(leader) = self.0 {
            // ponytail: a thread per ended adapter, asleep for the grace; a
            // reaper shared by every adapter if these ever come in numbers.
            std::thread::spawn(move || {
                std::thread::sleep(ADAPTER_GRACE);
                crate::process::kill_group(leader);
            });
        }
    }
}

impl Transport {
    /// Run `command` as a child process and talk to it over its stdio.
    pub(super) fn spawn(
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: PathBuf,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(&command);
        cmd.args(&args)
            .envs(env)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // A group of its own, so the adapter can be ended with everything it
        // started: the child is often `npx`, with the agent and its tools under it.
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| format!("spawn {command}: {e}"))?;
        let group = Group(child.id());

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;

        // Drain stderr so the adapter never blocks on a full pipe; echo for
        // debug, and keep the tail so a death can be explained afterwards.
        let tail: StderrTail = Default::default();
        let sink = tail.clone();
        let drain = tokio::spawn(async move {
            // Bytes, decoded lossily, rather than lines of text: reading as
            // text makes the first line that is not UTF-8 an error, the loop
            // stops on it, and nothing empties the pipe after that — the reason
            // written next is lost, and an adapter that keeps writing blocks
            // once the pipe is full. Only a closed or broken pipe ends this.
            let mut reader = BufReader::new(stderr);
            let mut raw = Vec::new();
            loop {
                raw.clear();
                match reader.read_until(b'\n', &mut raw).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&raw);
                let line = text.trim_end_matches(['\n', '\r']);
                eprintln!("[acp:stderr] {line}");
                // Taken through a poison rather than around it: a tail that
                // stopped recording at the first panic would be empty at the
                // one moment it is read.
                sink.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remember(line);
            }
        });

        Ok(Self {
            stderr: tail,
            drain: Some(drain),
            incoming: Box::pin(BufReader::new(stdout)),
            outgoing: Box::pin(stdin),
            // The child moves in here, which is what keeps `kill_on_drop`
            // meaning what it says: this future is dropped with the transport,
            // the transport with the stream, and the stream with the caller's
            // interest in the agent.
            ended: Box::pin(async move {
                let _group = group;
                match child.wait().await {
                    Ok(status) => format!("adapter exited ({status})"),
                    Err(e) => format!("adapter wait failed: {e}"),
                }
            }),
        })
    }

    /// A transport with a test on the other end of it.
    ///
    /// Bidirectional like a socket pair: what the client writes is what the
    /// returned half reads, and the other way round. Nothing ends it, so a test
    /// that wants a dead adapter closes its half.
    #[cfg(test)]
    pub(super) fn scripted() -> (Self, tokio::io::DuplexStream) {
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        let (read, write) = tokio::io::split(ours);
        (
            Self {
                incoming: Box::pin(BufReader::new(read)),
                outgoing: Box::pin(write),
                ended: Box::pin(std::future::pending()),
                // No child, so nothing ever writes to it and nothing drains it.
                stderr: Default::default(),
                drain: None,
            },
            theirs,
        )
    }
}
