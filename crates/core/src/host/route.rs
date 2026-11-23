//! Whether an agent's command goes to its project's build host: a pure
//! decision over the project found for the folder and what the machine is.

pub mod call;
pub mod drive;

use std::path::PathBuf;

use super::build::cli::{command_line, Found};
use crate::config::{HostOs, HostSpec};
use crate::workspace::{BuildHost, HostName};

/// A command an agent asked to run, and the folder it asked from.
#[derive(Debug)]
pub struct Ask {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// Where the command runs.
#[derive(Debug)]
pub(crate) enum Decision {
    /// On this machine, as the agent asked.
    Here,
    Host(Routed),
}

/// A command handed to the build host.
#[derive(Debug)]
pub(crate) struct Routed {
    pub host: HostName,
    pub top: PathBuf,
    pub within: PathBuf,
    /// The project's entry for the host, with this route's artifacts.
    pub entry: BuildHost,
    /// The whole line, for the host's POSIX shell.
    pub command: String,
}

/// Where `ask` runs, given the project found for its folder. Anything short
/// of a clear route, and anything the host cannot take, stays here.
pub(crate) fn decide(
    found: Result<Found, String>,
    hosts_on: bool,
    laptop_os: &str,
    declared: &[HostSpec],
    ask: &Ask,
) -> Decision {
    let Ok(Found {
        project,
        top,
        within,
    }) = found
    else {
        return Decision::Here;
    };
    if project.host.is_some() || project.offload.is_some() || !hosts_on {
        return Decision::Here;
    }
    let Some(host) = project
        .builds_on
        .filter(|on| project.builds.iter().any(|entry| entry.host == *on))
    else {
        return Decision::Here;
    };
    let Some(route) = project
        .routes
        .iter()
        .find(|route| route.matches(&ask.program, &ask.args))
    else {
        return Decision::Here;
    };
    // A host nobody declared is taken for Linux.
    let linux =
        crate::config::declared(declared, &host.0).is_none_or(|spec| spec.os == HostOs::Linux);
    if !linux || laptop_os != "linux" {
        return Decision::Here;
    }
    let words: Vec<String> = std::iter::once(ask.program.clone())
        .chain(ask.args.iter().cloned())
        .collect();
    let Ok(command) = command_line(&words, &HostOs::Linux) else {
        return Decision::Here;
    };
    Decision::Host(Routed {
        entry: BuildHost {
            host: host.clone(),
            artifacts: route.artifacts.clone(),
            into: None,
        },
        host,
        top,
        within,
        command,
    })
}

#[cfg(test)]
mod tests;
