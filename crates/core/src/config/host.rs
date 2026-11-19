//! A host as a person declares it: its name, and the system a probe found.

use serde::{Deserialize, Serialize};

use crate::workspace::HostName;

/// The system a host runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostOs {
    Linux,
    Windows,
    /// One this build does not know, kept as it was written so a save here
    /// does not lose what a newer build declared.
    #[serde(untagged)]
    Other(String),
}

/// One declared host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSpec {
    pub name: HostName,
    pub os: HostOs,
    /// As the host names it: `x86_64`, `aarch64`, `AMD64`.
    pub arch: String,
}

impl HostSpec {
    /// Whether sessions can run on it: Linux on x86_64, the only machine the
    /// host service is built for. Every other host builds only.
    pub fn runs_sessions(&self) -> bool {
        self.os == HostOs::Linux && self.arch == "x86_64"
    }

    /// What a project names without anyone having declared it: the only kind
    /// of host there was before hosts were declared.
    pub fn undeclared(name: HostName) -> Self {
        Self {
            name,
            os: HostOs::Linux,
            arch: "x86_64".into(),
        }
    }

    /// The system in a person's words: `Linux x86_64`.
    pub fn system(&self) -> String {
        let os = match &self.os {
            HostOs::Linux => "Linux",
            HostOs::Windows => "Windows",
            HostOs::Other(said) => said,
        };
        format!("{os} {}", self.arch).trim().to_string()
    }
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

/// Why `host` cannot be removed while `users`, its projects, name it.
pub fn removal_refusal(host: &HostName, users: &[String]) -> Option<String> {
    (!users.is_empty()).then(|| format!("{host} is used by {}", users.join(", ")))
}

#[cfg(test)]
mod tests;
