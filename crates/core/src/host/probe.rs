//! Asking a host what it is and what it has, over the person's own ssh.

use std::io::Read as _;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::client::HostAddr;
use super::ssh;
use crate::config::HostOs;

/// The line each call prints first, so whatever the host's startup files
/// print before it is never read as the answer.
const MARK: &str = "__ONEHAND_PROBE__";

/// The two programs a build cannot do without.
const NEEDED: [&str; 2] = ["git", "tar"];

/// The toolchains a person wants to know of, asked after with the two above.
const TOOLCHAINS: [&str; 3] = ["cargo", "rustc", "dotnet"];

/// Every program asked after, in the order asked.
fn asked() -> String {
    NEEDED
        .into_iter()
        .chain(TOOLCHAINS)
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the host is. One line that a POSIX shell and `cmd.exe` both run: a
/// host with no `uname` answers with `ver`.
fn what() -> String {
    format!("echo {MARK} && uname -sm || ver")
}

/// What a Linux host has, asked of its login shell so the answer is the one
/// a command run there gets: a toolchain put on `PATH` by a profile counts.
fn has_posix() -> String {
    format!(
        r#""${{SHELL:-/bin/sh}}" -lc 'echo {MARK}; for t in {}; do if command -v $t >/dev/null 2>&1; then echo "$t="; $t --version 2>/dev/null | head -n1; fi; done'"#,
        asked()
    )
}

/// What a Windows host has, and its architecture, which `ver` does not say.
fn has_windows() -> String {
    format!(
        "echo {MARK}& echo %PROCESSOR_ARCHITECTURE%& for %t in ({}) do @(where %t >nul 2>nul && (echo %t=& %t --version))",
        asked()
    )
}

/// How long one call is given to answer once it runs: a login profile that
/// waits on something must not leave a probe running for good.
const ANSWER_WAIT: Duration = Duration::from_secs(30);

/// What a probe found of a host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probed {
    pub os: HostOs,
    pub arch: String,
    /// Each program found, in the order asked.
    pub tools: Vec<Tool>,
}

/// One program a host has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    /// The first line it says of its version; empty when it said nothing.
    pub said: String,
}

impl Tool {
    /// Whether a build cannot do without it, which is all that is asked of
    /// it: a toolchain is told with its version, this by its name.
    pub fn needed(&self) -> bool {
        NEEDED.contains(&self.name.as_str())
    }
}

impl Probed {
    pub fn has(&self, tool: &str) -> bool {
        self.tools.iter().any(|found| found.name == tool)
    }

    /// Which of the two programs a build needs is not there.
    pub fn missing(&self) -> Vec<&'static str> {
        NEEDED.into_iter().filter(|tool| !self.has(tool)).collect()
    }
}

/// Ask `addr` what it is and what it has, in two calls. A failure is in
/// words to show: ssh's own when the host was not reached.
pub fn probe_blocking(addr: &HostAddr) -> Result<Probed, String> {
    let (said, err) = run(addr, &what(), ANSWER_WAIT)?;
    let Some((os, arch)) = system(&said) else {
        let why = err.lines().next().unwrap_or("it printed nothing").trim();
        return Err(format!("{} did not answer the probe: {why}", addr.ssh));
    };
    let cmd = asks_cmd(&os, &arch);
    let has = if cmd { has_windows() } else { has_posix() };
    let (said, err) = run(addr, &has, ANSWER_WAIT)?;
    // Its shell could not run the question: said as that, never as a host
    // with nothing installed.
    let Some((named, tools)) = tools(&said, cmd) else {
        let why = err.lines().next().unwrap_or("it printed nothing").trim();
        return Err(format!("{} did not say what it has: {why}", addr.ssh));
    };
    Ok(Probed {
        os,
        arch: named.unwrap_or(arch),
        tools,
    })
}

/// Whether a host is asked what it has in `cmd.exe`'s words: a Windows one
/// that answered with `ver`, which names no architecture. One whose `uname`
/// answered runs a POSIX shell over ssh, and is asked in that shell's.
fn asks_cmd(os: &HostOs, arch: &str) -> bool {
    *os == HostOs::Windows && arch.is_empty()
}

/// Run `cmd` on the host to its end, or for `wait` at the most: what it
/// printed, and what it said on stderr. Only ssh's own failure, or no end
/// within `wait`, is an error; the command's own failure is its answer.
// ponytail: the answer is read once the command ends, so one printing more
// than a pipe holds waits out `wait`; read while waiting if a probe ever
// prints that much.
fn run(addr: &HostAddr, cmd: &str, wait: Duration) -> Result<(String, String), String> {
    let mut child = ssh::command(addr, cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| format!("ssh: {e}"))? {
            Some(status) => break status,
            None if started.elapsed() >= wait => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} did not answer within {}s",
                    addr.ssh,
                    wait.as_secs()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let (mut said, mut err) = (Vec::new(), Vec::new());
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut said);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut err);
    }
    let said = String::from_utf8_lossy(&said).into_owned();
    let err = String::from_utf8_lossy(&err).trim().to_string();
    ssh::unreached(addr, &status, err.as_bytes())?;
    Ok((said, err))
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
/// version: a `name=` line, then what the program said, if anything. `None`
/// when the call never answered.
fn tools(out: &str, windows: bool) -> Option<(Option<String>, Vec<Tool>)> {
    let mut lines = answer(out)?.into_iter().peekable();
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
        tools.push(Tool {
            name: name.to_string(),
            said: version.to_string(),
        });
    }
    Some((arch, tools))
}

#[cfg(test)]
mod tests;
