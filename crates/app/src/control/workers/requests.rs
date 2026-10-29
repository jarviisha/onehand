//! What each lead asked `open_session` for, by the id it gave, and which
//! worker each lead holds.
//!
//! **A retry is not a second worker.** A lead that lost the answer to an open
//! calls again with the same request id: the open still running says so, one
//! that finished answers the same again, and only one that failed is
//! forgotten and opened afresh. The same id with other arguments is a
//! mistake, not a retry, and is refused.

use std::collections::HashMap;
use std::path::PathBuf;

/// The arguments a request is held to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::control) struct Asked {
    pub(in crate::control) project: String,
    pub(in crate::control) agent: String,
    pub(in crate::control) prompt: Option<String>,
    pub(in crate::control) dirs: Vec<PathBuf>,
    pub(in crate::control) turn: Option<u64>,
    pub(in crate::control) model: Option<String>,
}

/// What a request has come to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::control) enum Seen {
    /// Never seen: open it.
    New,
    /// An open of it is running, on this session once one has started.
    Opening(Option<u64>),
    /// It opened, and this was its answer.
    Answered(String),
}

/// Every lead's requests, for the run of the app.
#[derive(Default)]
pub(in crate::control) struct Requests {
    held: HashMap<(u64, String), (Asked, Seen)>,
}

impl Requests {
    /// What `lead`'s `request` has come to, recording it as opening when it
    /// is new.
    pub(in crate::control) fn see(
        &mut self,
        lead: u64,
        request: &str,
        asked: &Asked,
    ) -> Result<Seen, String> {
        let key = (lead, request.to_string());
        match self.held.get(&key) {
            Some((before, _)) if before != asked => {
                Err(format!("request {request} was made with other arguments"))
            }
            Some((_, seen)) => Ok(seen.clone()),
            None => {
                self.held.insert(key, (asked.clone(), Seen::Opening(None)));
                Ok(Seen::New)
            }
        }
    }

    /// The open of `request` started session `uid`.
    pub(in crate::control) fn started(&mut self, lead: u64, request: &str, uid: u64) {
        self.set(lead, request, Seen::Opening(Some(uid)));
    }

    /// The open of `request` finished with `answer`.
    pub(in crate::control) fn answered(&mut self, lead: u64, request: &str, answer: String) {
        self.set(lead, request, Seen::Answered(answer));
    }

    /// The open of `request` failed: the next call opens afresh.
    pub(in crate::control) fn failed(&mut self, lead: u64, request: &str) {
        self.held.remove(&(lead, request.to_string()));
    }

    /// `lead` closed: its requests go with it.
    pub(in crate::control) fn forget_lead(&mut self, lead: u64) {
        self.held.retain(|(held, _), _| *held != lead);
    }

    fn set(&mut self, lead: u64, request: &str, seen: Seen) {
        if let Some(entry) = self.held.get_mut(&(lead, request.to_string())) {
            entry.1 = seen;
        }
    }
}

/// A worker's lead, and what the lead last sent it: written once the open's
/// first prompt was accepted, never before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::control) struct Relation {
    pub(in crate::control) lead: u64,
    pub(in crate::control) turn: Option<u64>,
    pub(in crate::control) text: Option<String>,
    /// The worker's prompts sent once the lead's last went.
    pub(in crate::control) sent: usize,
}
