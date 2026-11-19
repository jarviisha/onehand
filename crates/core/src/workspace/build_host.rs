//! One host a project builds on, as its workspace keeps it, and what may be
//! named as coming back from it.

use std::path::{Path, PathBuf};

use super::HostName;

/// One host a project builds on, and what comes back from it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildHost {
    pub host: HostName,
    /// What a build brings back: files or folders, as paths inside the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<PathBuf>,
    /// The folder in the project the artifacts land under; none for the same
    /// paths. Two hosts building one project would otherwise write over each
    /// other's output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<PathBuf>,
}

impl BuildHost {
    /// From a form's three fields: `artifacts` one path a line, blank lines
    /// dropped; a blank `into` is none.
    pub fn parse(host: &str, artifacts: &str, into: &str) -> Result<Self, String> {
        let host = host.trim();
        if host.is_empty() {
            return Err("Name the host".into());
        }
        let inside = |path: &str| match inside_refusal(path) {
            Some(why) => Err(why),
            None => Ok(PathBuf::from(path)),
        };
        let artifacts = artifacts
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(inside)
            .collect::<Result<Vec<_>, _>>()?;
        let into = match into.trim() {
            "" => None,
            into => Some(inside(into)?),
        };
        Ok(Self {
            host: HostName(host.to_string()),
            artifacts,
            into,
        })
    }
}

/// Why `path` cannot be an artifact, or the folder artifacts land under: it
/// must name one file or folder inside the project, never the project
/// itself, which would bring the host's whole checkout back over this one,
/// and never anything of git's own folder, which a host must not write.
pub(crate) fn inside_refusal(path: &str) -> Option<String> {
    use std::path::Component;
    let parts: Vec<Component<'_>> = Path::new(path).components().collect();
    let outside = parts
        .iter()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir));
    let names_something = parts
        .iter()
        .any(|part| matches!(part, Component::Normal(_)));
    let gits_own = parts
        .iter()
        .any(|part| matches!(part, Component::Normal(name) if *name == ".git"));
    if path.contains('\\') {
        // A path here is written as git writes one, on whichever system the
        // host runs: what comes back is matched by that spelling.
        Some(format!("{path} is written with \\; write it with /"))
    } else if outside || !names_something {
        Some(format!("{path} must be inside the project"))
    } else if gits_own {
        Some(format!(
            "{path} is git's own folder; it cannot be brought back"
        ))
    } else if path.contains(['*', '?', '[']) {
        Some(format!("{path} is a pattern; name a file or a folder"))
    } else {
        None
    }
}
