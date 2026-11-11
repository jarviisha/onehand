//! Why a host could not be attached, as what follows from it is decided: a
//! host that may answer later, one that never can, one another machine holds,
//! and one waiting on its own busy sessions. Each carries the sentence a
//! person reads, naming the host, since the same words stand in a session's
//! transcript, on a project's page and in a toast.

use super::client::HostError;
use super::wire::HostSession;
use crate::workspace::HostName;

/// Why a host could not be attached.
#[derive(Clone, Debug)]
pub enum HostFailure {
    /// The host runs another build, with these sessions mid-turn and
    /// `terms` terminals open; it is left alone and waited on until they are
    /// done and closed, or the person agrees to stop them.
    Busy {
        host: HostName,
        busy: Vec<HostSession>,
        terms: usize,
    },
    /// A sentence a person reads, naming the host.
    Other(String),
    /// The host is not one this build can attach to (a machine it does not
    /// run on), or this build cannot attach anywhere (its service binary
    /// missing): a sentence naming why. Never tried again by itself.
    Refused(String),
    /// An attach made by itself found the host held by another machine's,
    /// and left it so: a sentence naming the host. Only a person attaches
    /// to it again.
    Taken(String),
}

impl HostFailure {
    /// What `e`, an attach to `host` that failed, comes to.
    pub fn of(host: &HostName, e: HostError) -> Self {
        match e {
            e @ (HostError::Unreachable(_) | HostError::Protocol(_)) => {
                Self::Other(format!("Could not attach to {host}: {e}"))
            }
            HostError::Unsupported(uname) => Self::Refused(format!(
                "{host} is not a machine this build runs on: {uname}"
            )),
            HostError::Busy { busy, terms, .. } => Self::Busy {
                host: host.clone(),
                busy,
                terms,
            },
            HostError::Held => Self::Taken(taken_line(host)),
        }
    }

    /// What it comes to for what waits on its host to attach (the terminal
    /// dock, the Workbench, the folder list): a busy host is waited out,
    /// saying so, and attached again once its sessions are done; anything
    /// else gives up, on why.
    pub fn after_attach(self) -> AfterAttach {
        match self {
            Self::Busy { host, busy, terms } => {
                AfterAttach::Wait(waiting_line(&host, busy.len(), terms))
            }
            Self::Other(why) | Self::Refused(why) | Self::Taken(why) => AfterAttach::Failed(why),
        }
    }

    /// The sentence it reads as where nothing waits on a busy host: the
    /// waiting line itself.
    pub fn said(self) -> String {
        match self {
            Self::Other(why) | Self::Refused(why) | Self::Taken(why) => why,
            Self::Busy { host, busy, terms } => waiting_line(&host, busy.len(), terms),
        }
    }
}

/// What an attach that failed comes to for what waits on its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterAttach {
    /// Say this, wait for the host's busy sessions, and attach again.
    Wait(String),
    /// Give up, on why.
    Failed(String),
}

/// What is said wherever something waits on `host`'s `n` busy sessions and
/// its `terms` open terminals.
pub fn waiting_line(host: &HostName, n: usize, terms: usize) -> String {
    let plural = |n: usize, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let what = match (n, terms) {
        (n, 0) => format!("{} on {host} to finish", plural(n, "session", "sessions")),
        (0, t) => format!("{} on {host} to close", plural(t, "terminal", "terminals")),
        (n, t) => format!(
            "{} to finish and {} to close on {host}",
            plural(n, "session", "sessions"),
            plural(t, "terminal", "terminals")
        ),
    };
    format!("Waiting for {what} before restarting its host service onto this build")
}

/// What is said of `host` once another machine holds it.
pub(crate) fn taken_line(host: &HostName) -> String {
    format!("{host} was attached from another machine")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A busy host is waited out, saying so; anything else gives up on why.
    #[test]
    fn only_a_busy_host_is_waited_out() {
        let busy = HostFailure::Busy {
            host: HostName("vps".into()),
            busy: vec![HostSession("h".into())],
            terms: 0,
        };
        assert_eq!(
            busy.after_attach(),
            AfterAttach::Wait(waiting_line(&HostName("vps".into()), 1, 0))
        );
        for failure in [
            HostFailure::Other("away".into()),
            HostFailure::Refused("no".into()),
            HostFailure::Taken("held".into()),
        ] {
            assert!(matches!(failure.after_attach(), AfterAttach::Failed(_)));
        }
    }
}
