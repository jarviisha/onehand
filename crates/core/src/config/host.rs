//! A host as a person declares it: its name, and the system a probe found.

use serde::{Deserialize, Serialize};

use crate::workspace::HostName;

/// The system a host runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum HostOs {
    Linux,
    Windows,
    /// One this build does not know, kept as it was written so a save here
    /// does not lose what a newer build declared.
    Other(String),
}

/// Read whatever its case, since a person writing the file by hand writes
/// the name as the page shows it.
impl From<String> for HostOs {
    fn from(said: String) -> Self {
        match said.to_ascii_lowercase().as_str() {
            "linux" => Self::Linux,
            "windows" => Self::Windows,
            _ => Self::Other(said),
        }
    }
}

impl From<HostOs> for String {
    fn from(os: HostOs) -> Self {
        match os {
            HostOs::Linux => "linux".into(),
            HostOs::Windows => "windows".into(),
            HostOs::Other(said) => said,
        }
    }
}

fn linux() -> HostOs {
    HostOs::Linux
}

fn x86_64() -> String {
    "x86_64".into()
}

/// One declared host. An entry written with its name alone is what a host
/// nobody declared is taken for, so a hand-written one never costs the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSpec {
    pub name: HostName,
    #[serde(default = "linux")]
    pub os: HostOs,
    /// As the host names it: `x86_64`, `aarch64`, `AMD64`.
    #[serde(default = "x86_64")]
    pub arch: String,
}

impl HostSpec {
    /// Whether sessions can run on it: Linux on x86_64, the only machine the
    /// host service is built for. Every other host builds only.
    pub fn runs_sessions(&self) -> bool {
        self.os == HostOs::Linux && self.arch == "x86_64"
    }

    /// Whether a project can build on it: a Linux host and a Windows one,
    /// each asked in its own shell's words. A system this build does not know
    /// builds nothing.
    pub fn builds(&self) -> bool {
        match self.os {
            HostOs::Linux | HostOs::Windows => true,
            HostOs::Other(_) => false,
        }
    }

    /// What a project names without anyone having declared it: the only kind
    /// of host there was before hosts were declared.
    pub fn undeclared(name: HostName) -> Self {
        Self {
            name,
            os: linux(),
            arch: x86_64(),
        }
    }

    /// The system's name in a person's words: `Linux`, `Windows`, or what
    /// one this build does not know was written as.
    pub fn os_name(&self) -> &str {
        match &self.os {
            HostOs::Linux => "Linux",
            HostOs::Windows => "Windows",
            HostOs::Other(said) => said,
        }
    }

    /// The system in a person's words: `Linux x86_64`.
    pub fn system(&self) -> String {
        format!("{} {}", self.os_name(), self.arch)
            .trim()
            .to_string()
    }
}

/// The host of that name among the declared ones.
pub fn declared<'a>(hosts: &'a [HostSpec], name: &str) -> Option<&'a HostSpec> {
    hosts.iter().find(|host| host.name.0 == name.trim())
}

/// One change to the declared hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEdit {
    /// In place of the host of that name, or added at the end.
    Put(HostSpec),
    Remove(HostName),
}

impl HostEdit {
    pub fn apply(&self, hosts: &mut Vec<HostSpec>) {
        match self {
            Self::Put(spec) => match hosts.iter_mut().find(|h| h.name == spec.name) {
                Some(held) => *held = spec.clone(),
                None => hosts.push(spec.clone()),
            },
            Self::Remove(name) => hosts.retain(|h| h.name != *name),
        }
    }
}

/// The hosts to show: the declared ones, then each host a project names that
/// nobody declared, in order and once, each with whether it is declared.
pub fn listed(declared: &[HostSpec], named: &[HostName]) -> Vec<(HostSpec, bool)> {
    let mut all: Vec<(HostSpec, bool)> = declared.iter().cloned().map(|h| (h, true)).collect();
    for name in named {
        if all.iter().all(|(held, _)| held.name != *name) {
            all.push((HostSpec::undeclared(name.clone()), false));
        }
    }
    all
}

/// Why what was typed cannot be added as a host: it names none, or names
/// one already declared.
pub fn add_refusal(typed: &str, hosts: &[HostSpec]) -> Option<String> {
    let name = typed.trim();
    if name.is_empty() {
        return Some("Name the host".into());
    }
    declared(hosts, name).map(|_| format!("{name} is already declared"))
}

/// Why no session can open on `host`: it is declared, and as a host that
/// only builds. One nobody declared is not refused here; its attach says
/// what it is.
pub fn builds_only(host: &str, hosts: &[HostSpec]) -> Option<String> {
    declared(hosts, host)
        .filter(|spec| !spec.runs_sessions())
        .map(|_| format!("{} only builds; it runs no sessions", host.trim()))
}

/// Why `host` cannot be removed while `users`, its projects, name it.
pub fn removal_refusal(host: &HostName, users: &[String]) -> Option<String> {
    (!users.is_empty()).then(|| format!("{host} is used by {}", users.join(", ")))
}

#[cfg(test)]
mod tests;
