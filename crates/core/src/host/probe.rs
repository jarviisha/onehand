//! Asking a host what it is and what it has, over the person's own ssh.

use std::process::{Command, Stdio};

use super::client::{HostAddr, SSH_OPTS};
use crate::config::HostOs;

/// The line each call prints first, so whatever the host's startup files
/// print before it is never read as the answer.
const MARK: &str = "__ONEHAND_PROBE__";

/// The programs asked after: the two a build cannot do without, then the
/// toolchains a person wants to know of.
const NEEDED: [&str; 2] = ["git", "tar"];

/// What the host is. One line that a POSIX shell and `cmd.exe` both run: a
/// host with no `uname` answers with `ver`.
const WHAT: &str = "echo __ONEHAND_PROBE__ && uname -sm || ver";

/// What a Linux host has, asked of its login shell so the answer is the one
/// a command run there gets: a toolchain put on `PATH` by a profile counts.
const HAS_POSIX: &str = r#""${SHELL:-/bin/sh}" -lc 'echo __ONEHAND_PROBE__; for t in git tar cargo rustc dotnet; do if command -v $t >/dev/null 2>&1; then echo "$t="; $t --version 2>/dev/null | head -n1; fi; done'"#;

/// What a Windows host has, and its architecture, which `ver` does not say.
const HAS_WINDOWS: &str = "echo __ONEHAND_PROBE__& echo %PROCESSOR_ARCHITECTURE%& for %t in (git tar cargo rustc dotnet) do @(where %t >nul 2>nul && (echo %t=& %t --version))";

/// How long a host is given to answer ssh at all, so a probe of a machine
/// that is off ends in seconds rather than at the network's own patience.
const CONNECT_WAIT: &str = "ConnectTimeout=10";

/// What a probe found of a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probed {
    pub os: HostOs,
    pub arch: String,
    /// Each program found, with the first line it says of its version, in
    /// the order asked.
    pub tools: Vec<(String, String)>,
}

impl Probed {
    pub fn has(&self, tool: &str) -> bool {
        self.tools.iter().any(|(name, _)| name == tool)
    }

    /// Which of the two programs a build needs is not there.
    pub fn missing(&self) -> Vec<&'static str> {
        NEEDED.into_iter().filter(|tool| !self.has(tool)).collect()
    }
}

/// Ask `addr` what it is and what it has, in two calls. A failure is in
/// words to show: ssh's own when the host was not reached.
pub fn probe_blocking(addr: &HostAddr) -> Result<Probed, String> {
    let (said, err) = run(addr, WHAT)?;
    let Some((os, arch)) = system(&said) else {
        let why = err.lines().next().unwrap_or("it printed nothing").trim();
        return Err(format!("{} did not answer the probe: {why}", addr.ssh));
    };
    let windows = os == HostOs::Windows;
    let (said, _) = run(addr, if windows { HAS_WINDOWS } else { HAS_POSIX })?;
    let (named, tools) = tools(&said, windows);
    Ok(Probed {
        os,
        arch: named.unwrap_or(arch),
        tools,
    })
}

/// Run `cmd` on the host to its end: what it printed, and what it said on
/// stderr. Only ssh's own failure is an error; the command's is its answer.
fn run(addr: &HostAddr, cmd: &str) -> Result<(String, String), String> {
    let mut command = if addr.local {
        let mut sh = Command::new("sh");
        sh.arg("-c");
        sh
    } else {
        let mut ssh = Command::new("ssh");
        ssh.args(SSH_OPTS)
            .args(["-o", CONNECT_WAIT, "--"])
            .arg(&addr.ssh);
        ssh
    };
    let out = command
        .arg(cmd)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ssh: {e}"))?;
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !addr.local && out.status.code() == Some(255) {
        return Err(if err.is_empty() {
            format!("{} could not be reached", addr.ssh)
        } else {
            err
        });
    }
    Ok((String::from_utf8_lossy(&out.stdout).into_owned(), err))
}

/// The lines after the last mark, trimmed, the empty ones left out.
fn answer(out: &str) -> Option<Vec<&str>> {
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let at = lines.iter().rposition(|line| *line == MARK)?;
    Some(
        lines[at + 1..]
            .iter()
            .copied()
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// The system and architecture the first call's answer names; `None` when
/// the host never answered. A Windows host's architecture comes with its
/// tools.
fn system(out: &str) -> Option<(HostOs, String)> {
    let lines = answer(out)?;
    let first = lines.first()?;
    if first.starts_with("Microsoft Windows") {
        return Some((HostOs::Windows, String::new()));
    }
    let (name, arch) = first.split_once(' ').unwrap_or((first, ""));
    let os = match name {
        "Linux" => HostOs::Linux,
        // `uname` there is Git's, or Cygwin's: the machine is Windows.
        n if ["MINGW", "MSYS", "CYGWIN"].iter().any(|p| n.starts_with(p)) => HostOs::Windows,
        other => HostOs::Other(other.to_string()),
    };
    Some((os, arch.trim().to_string()))
}

/// The architecture a Windows host named, and each program found with its
/// version: a `name=` line, then what the program said, if anything.
fn tools(out: &str, windows: bool) -> (Option<String>, Vec<(String, String)>) {
    let mut lines = answer(out).unwrap_or_default().into_iter().peekable();
    let arch = match windows {
        true => lines.next().map(str::to_string),
        false => None,
    };
    let mut tools = Vec::new();
    while let Some(line) = lines.next() {
        let Some(name) = line.strip_suffix('=') else {
            continue;
        };
        let version = lines.next_if(|next| !next.ends_with('=')).unwrap_or("");
        tools.push((name.to_string(), version.to_string()));
    }
    (arch, tools)
}

#[cfg(test)]
mod tests;
