//! What a change shown at once and saved after it needs: a thread that writes
//! in the order asked, and the value on screen held beside the value last
//! known saved, so a failed save puts back the one change it carried.
//!
//! **The rule, once**: a change shows in the frame it is made; it is written
//! off the UI thread, after every change made before it; and a write that
//! fails puts the value back to the last one known saved, said once. A write
//! a newer change has already superseded failing changes nothing, since the
//! newer write carries the value on screen.

use futures::channel::oneshot;
use std::sync::mpsc;

/// One write: whatever lands a value on disk, and why it could not.
type Job = Box<dyn FnOnce() -> Result<(), String> + Send>;

/// A thread that runs writes one at a time, in the order sent. A clone hands
/// work to the same thread, which ends with the last clone.
#[derive(Clone)]
pub struct OrderedWriter {
    tx: mpsc::Sender<(Job, oneshot::Sender<Result<(), String>>)>,
}

impl OrderedWriter {
    /// A writer on a thread of its own, named `name`.
    pub fn spawn(name: &str) -> Self {
        let (tx, rx) = mpsc::channel::<(Job, oneshot::Sender<Result<(), String>>)>();
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                for (job, done) in rx {
                    let _ = done.send(job());
                }
            })
            .inspect_err(|why| eprintln!("onehand: no thread to write with: {why}"))
            .ok();
        Self { tx }
    }

    /// Run `write` after every write sent before it, and answer how it went.
    /// The answer is a future any executor can wait on; a writer whose thread
    /// is gone answers that it is.
    pub fn write<W>(
        &self,
        write: W,
    ) -> impl std::future::Future<Output = Result<(), String>> + use<W>
    where
        W: FnOnce() -> Result<(), String> + Send + 'static,
    {
        let (done, answer) = oneshot::channel();
        let sent = self.tx.send((Box::new(write), done)).is_ok();
        async move {
            match sent {
                true => answer
                    .await
                    .unwrap_or_else(|_| Err("the writer stopped".to_string())),
                false => Err("nothing is left to write with".to_string()),
            }
        }
    }
}

/// A value shown at once and saved after: what is on screen, what is known
/// to be on disk, and which change is the newest.
#[derive(Clone, Debug)]
pub struct Saving<T> {
    shown: T,
    saved: T,
    newest: u64,
}

impl<T: Clone> Saving<T> {
    /// A value as read, so shown and saved alike.
    pub fn new(value: T) -> Self {
        Self {
            shown: value.clone(),
            saved: value,
            newest: 0,
        }
    }

    /// What is on screen.
    pub fn shown(&self) -> &T {
        &self.shown
    }

    /// Show `value` at once, and say which change it is, for its write.
    pub fn change(&mut self, value: T) -> u64 {
        self.shown = value;
        self.newest += 1;
        self.newest
    }

    /// Take the write of change `of`, which wrote `value`. It landing makes
    /// `value` the one known saved. It failing as the newest change puts the
    /// last saved value back on screen and answers why, to be said once; it
    /// failing behind a newer change answers nothing, since the newer write
    /// carries what is shown.
    pub fn landed(&mut self, of: u64, value: T, result: Result<(), String>) -> Option<String> {
        match result {
            Ok(()) => {
                self.saved = value;
                None
            }
            Err(why) if of == self.newest => {
                self.shown = self.saved.clone();
                Some(why)
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests;
