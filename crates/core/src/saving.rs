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
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::mpsc;
use std::time::Duration;

/// One write, which answers on a channel of its own.
type Job = Box<dyn FnOnce() + Send>;

/// A thread that runs writes one at a time, in the order sent. A clone hands
/// work to the same thread, which ends with the last clone.
#[derive(Clone)]
pub struct OrderedWriter {
    tx: mpsc::Sender<Job>,
}

impl OrderedWriter {
    /// A writer on a thread of its own, named `name`.
    pub fn spawn(name: &str) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                for job in rx {
                    job();
                }
            })
            .inspect_err(|why| eprintln!("onehand: no thread to write with: {why}"))
            .ok();
        Self { tx }
    }

    /// Run `write` after every write sent before it, and answer what it came
    /// to. The answer is a future any executor can wait on; a writer whose
    /// thread is gone answers that it is.
    pub fn write<W, T>(
        &self,
        write: W,
    ) -> impl std::future::Future<Output = Result<T, String>> + use<W, T>
    where
        W: FnOnce() -> Result<T, String> + Send + 'static,
        T: Send + 'static,
    {
        let (done, answer) = oneshot::channel();
        let job: Job = Box::new(move || {
            let _ = done.send(write());
        });
        let sent = self.tx.send(job).is_ok();
        async move {
            match sent {
                true => answer
                    .await
                    .unwrap_or_else(|_| Err("the writer stopped".to_string())),
                false => Err("nothing is left to write with".to_string()),
            }
        }
    }

    /// Run `write` after every write sent before it, and wait up to `within`
    /// for what it came to: what a window going away does last, when there is
    /// no later to hear the answer in. Blocking.
    pub fn write_within<W, T>(&self, write: W, within: Duration) -> Result<T, String>
    where
        W: FnOnce() -> Result<T, String> + Send + 'static,
        T: Send + 'static,
    {
        let (done, answer) = mpsc::channel();
        let job: Job = Box::new(move || {
            let _ = done.send(write());
        });
        if self.tx.send(job).is_err() {
            return Err("nothing is left to write with".to_string());
        }
        answer
            .recv_timeout(within)
            .unwrap_or_else(|_| Err("the write did not land in time".to_string()))
    }

    /// Wait up to `within` for every write sent so far to land: what a
    /// process about to exit does last. Whether they all landed. Blocking.
    pub fn flush(&self, within: Duration) -> bool {
        let (mark, landed) = mpsc::channel();
        let job: Job = Box::new(move || {
            let _ = mark.send(());
        });
        self.tx.send(job).is_ok() && landed.recv_timeout(within).is_ok()
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

/// Several things saved in one file, each shown at once and each put back
/// alone: a theme and a shortcut in one config, a pin and an order in one
/// workspace.
///
/// One write can carry the changes of several things (a whole snapshot
/// carries all of them), so what decides a failure is per thing: a failed
/// write puts a thing back only when no later write carries it, and then to
/// that thing's last value known saved, never to the value before its newest
/// change. `V` is whatever puts a thing's value back on screen.
pub struct Saves<K, V> {
    things: HashMap<K, Thing<V>>,
    writes: u64,
}

/// One thing's state: the value known saved and the change it came from, the
/// newest change, and the newest write that carries it.
struct Thing<V> {
    saved: V,
    saved_change: u64,
    newest: V,
    newest_change: u64,
    carried_by: u64,
}

/// A write on its way: which it is, and each thing it carries with the
/// change and the value it carries.
pub struct Pending<K, V> {
    write: u64,
    carries: Vec<(K, u64, V)>,
}

impl<K, V> Default for Saves<K, V> {
    fn default() -> Self {
        Self {
            things: HashMap::new(),
            writes: 0,
        }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Saves<K, V> {
    /// Show `after` for `key` at once. `before` is what was shown until now,
    /// which is the value known saved when `key` has not changed before.
    pub fn change(&mut self, key: K, before: V, after: V) {
        let thing = self.things.entry(key).or_insert(Thing {
            saved: before,
            saved_change: 0,
            newest: after.clone(),
            newest_change: 0,
            carried_by: 0,
        });
        thing.newest = after;
        thing.newest_change += 1;
    }

    /// A write of the whole file, carrying every thing as it now stands.
    pub fn write_all(&mut self) -> Pending<K, V> {
        let keys: Vec<K> = self.things.keys().cloned().collect();
        self.write_of(keys)
    }

    /// A write of `keys` alone, as a write that edits one field in place is.
    pub fn write_of(&mut self, keys: impl IntoIterator<Item = K>) -> Pending<K, V> {
        self.writes += 1;
        let write = self.writes;
        let carries = keys
            .into_iter()
            .filter_map(|key| {
                let thing = self.things.get_mut(&key)?;
                thing.carried_by = write;
                Some((key, thing.newest_change, thing.newest.clone()))
            })
            .collect();
        Pending { write, carries }
    }

    /// Take what `pending` came to; writes land in the order they were made.
    /// Landing makes each value it carried the one known saved. Failing
    /// answers why and the values that put back each thing no later write
    /// carries and no later change waits on, to be said once; it answers nothing when every thing it
    /// carried rides a later write and a later write was made, since that
    /// write's answer decides. A failure with nothing to put back is still
    /// said while its write is the newest, so a write of something never put
    /// back (a dragged size) is not lost in silence.
    pub fn landed(
        &mut self,
        pending: Pending<K, V>,
        result: Result<(), String>,
    ) -> Option<(String, Vec<V>)> {
        let why = match result {
            Ok(()) => {
                for (key, change, value) in pending.carries {
                    // An answer taken out of turn never moves what is known
                    // saved back past a newer one.
                    if let Some(thing) = self
                        .things
                        .get_mut(&key)
                        .filter(|thing| change >= thing.saved_change)
                    {
                        thing.saved = value;
                        thing.saved_change = change;
                    }
                }
                return None;
            }
            Err(why) => why,
        };
        let mut newest_of_some = false;
        let mut back = Vec::new();
        for (key, change, _) in pending.carries {
            let Some(thing) = self.things.get_mut(&key) else {
                continue;
            };
            // A later write carries it, or a later change waits for its own
            // write: either way that one decides, not this failure.
            if thing.carried_by != pending.write || change != thing.newest_change {
                continue;
            }
            newest_of_some = true;
            if thing.newest_change != thing.saved_change {
                thing.newest = thing.saved.clone();
                thing.saved_change = thing.newest_change;
                back.push(thing.saved.clone());
            }
        }
        (newest_of_some || pending.write == self.writes).then_some((why, back))
    }
}

#[cfg(test)]
mod tests;
