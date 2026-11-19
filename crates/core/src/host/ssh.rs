//! One line run on a host over the person's own ssh, or through `sh` for the
//! host `local`: what the probe of a host and a build on one both do.

use std::process::{Command, ExitStatus, Stdio};

use super::client::{HostAddr, SSH_OPTS};

/// How long a host is given to answer ssh at all, so what is asked of a
/// machine that is off ends in seconds rather than at the network's own
/// patience.
const CONNECT_WAIT: &str = "ConnectTimeout=10";

/// How `cmd.exe` ends what it says of a program it could not find. Its exit
/// code is 1, as any failure's, so its words are all there is to go by; they
/// are English on an English Windows only.
pub(crate) const CMD_NOT_FOUND: &str = "operable program or batch file";

/// `line` as the host is to run it, its stdin closed. Over ssh it is handed
/// to the account's own shell; `local` runs it through `sh`.
pub(crate) fn command(addr: &HostAddr, line: &str) -> Command {
    let mut cmd = match addr.local {
        true => {
            let mut sh = Command::new("sh");
            sh.arg("-c");
            sh
        }
        false => {
            let mut ssh = Command::new("ssh");
            ssh.args(SSH_OPTS)
                .args(["-o", CONNECT_WAIT, "--"])
                .arg(&addr.ssh);
            ssh
        }
    };
    cmd.arg(line).stdin(Stdio::null());
    cmd
}

/// The `ssh` git is to reach a host with: the same options, and the same
/// wait to connect.
pub(crate) fn for_git() -> String {
    format!("ssh {} -o {CONNECT_WAIT}", SSH_OPTS.join(" "))
}

/// ssh's own failure, in its words: the host was not reached.
pub(crate) fn unreached(addr: &HostAddr, status: &ExitStatus, stderr: &[u8]) -> Result<(), String> {
    if addr.local || status.code() != Some(255) {
        return Ok(());
    }
    let why = said(stderr);
    Err(match why.is_empty() {
        true => format!("{} could not be reached", addr.ssh),
        false => why,
    })
}

/// The last line of what a command said on stderr, which is the one that
/// says what went wrong; with the line before it when `cmd.exe` is saying a
/// program is not there, which it says over two.
pub(crate) fn said(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev();
    let last = lines.next().unwrap_or_default();
    match (last.contains(CMD_NOT_FOUND), lines.next()) {
        (true, Some(before)) => format!("{before} {last}"),
        _ => last.to_string(),
    }
}
