//! Whether an agent's command goes to its project's build host: a pure
//! decision over the project found for the folder and what the machine is.

pub mod call;
pub mod drive;
pub mod shim;

use std::path::PathBuf;

use super::build::cli::{command_line, Found};
use crate::config::{HostOs, HostSpec};
use crate::project::Project;
use crate::workspace::{BuildHost, HostName};
use drive::{RouteDrive, Standing};

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

/// The machine a command is asked from: its system, and the hosts it
/// declared.
#[derive(Debug)]
pub struct Machine<'a> {
    pub declared: &'a [HostSpec],
    pub os: HostOs,
}

impl<'a> Machine<'a> {
    /// The machine onehand runs on, with the hosts it declared.
    pub fn this(declared: &'a [HostSpec]) -> Self {
        Self {
            declared,
            os: crate::config::this_machine_os(),
        }
    }
}

/// Where `ask` runs, given the project found for its folder. Anything short
/// of a clear route, and anything the host cannot take, stays here.
pub(crate) fn decide(
    found: Result<Found, String>,
    hosts_on: bool,
    machine: &Machine,
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
    if !hosts_on {
        return Decision::Here;
    }
    let Some(host) = picked(&project, machine) else {
        return Decision::Here;
    };
    let Some(route) = project
        .routes
        .iter()
        .find(|route| route.matches(&ask.program, &ask.args))
    else {
        return Decision::Here;
    };
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

/// The build hosts of `project` a command of it can be routed to: a Linux
/// host (one nobody declared is taken for one) from a Linux machine alone,
/// and none for a project whose sessions run on a host, which builds there
/// already.
pub fn routable_hosts(project: &Project, machine: &Machine) -> Vec<HostName> {
    if project.host.is_some() || project.offload.is_some() || machine.os != HostOs::Linux {
        return Vec::new();
    }
    project
        .builds
        .iter()
        .map(|entry| entry.host.clone())
        .filter(|host| {
            crate::config::declared(machine.declared, &host.0)
                .is_none_or(|spec| spec.os == HostOs::Linux)
        })
        .collect()
}

/// The project's pick, while it is one of its routable hosts; any other
/// pick is taken as none.
fn picked(project: &Project, machine: &Machine) -> Option<HostName> {
    project
        .builds_on
        .clone()
        .filter(|on| routable_hosts(project, machine).contains(on))
}

/// Where the project's next routed command goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Builds {
    /// Nothing picked: this machine.
    Here,
    On(HostName),
    /// A host picked, and builds on hosts switched off for the machine.
    HostsOff,
    /// A host picked that did not answer the last routed run.
    NotAnswering(HostName),
}

/// Where `project`'s next routed command goes, as the switch and the picked
/// host's standing stand now.
pub fn builds(project: &Project, drive: &RouteDrive, machine: &Machine) -> Builds {
    let Some(host) = picked(project, machine) else {
        return Builds::Here;
    };
    if !drive.hosts_on() {
        return Builds::HostsOff;
    }
    match drive.standing(&host) {
        Some(Standing::NotAnswering) => Builds::NotAnswering(host),
        Some(Standing::Reached) | None => Builds::On(host),
    }
}

#[cfg(test)]
mod tests;
