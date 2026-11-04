//! Putting the service's binary on a host: ask what the host is and whether
//! this build is already there, and copy it over only when it is not.

use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncWriteExt as _;

use super::{command, HostAddr, HostError};
use crate::host::wire::build_id;

/// The only build shipped.
const SUPPORTED: &str = "Linux x86_64";

/// Run `cmd` to its end with `input` on its stdin; what it printed.
async fn run(addr: &HostAddr, cmd: &str, input: &[u8]) -> Result<String, HostError> {
    let mut child = command(addr, cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| HostError::Unreachable(format!("ssh: {e}")))?;
    let written = match child.stdin.take() {
        // Dropped once written, so the command reads its end.
        Some(mut stdin) => stdin.write_all(input).await,
        None => Ok(()),
    };
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| HostError::Unreachable(e.to_string()))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(HostError::Unreachable(if why.is_empty() {
            format!("{} ended with {}", addr.ssh, out.status)
        } else {
            why
        }));
    }
    if let Err(e) = written {
        return Err(HostError::Unreachable(format!(
            "the copy to {} was cut short: {e}",
            addr.ssh
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Copy `binary` to the host unless that build is already there; return its remote path
/// (`~/.onehand-host/<build>/onehand-host`). Refuses a host whose `uname -sm` is not `Linux x86_64`
/// (the only build shipped).
pub async fn install(addr: &HostAddr, binary: &Path) -> Result<String, HostError> {
    let bytes = tokio::fs::read(binary)
        .await
        .map_err(|e| HostError::Protocol(format!("{}: {e}", binary.display())))?;
    let build = build_id(&bytes);
    let dir = format!("~/.onehand-host/{build}");
    let path = format!("{dir}/onehand-host");
    let probe = run(
        addr,
        &format!("echo {PROBE} && uname -sm && if test -x {path}; then echo present; fi"),
        &[],
    )
    .await?;
    let (system, present) = probed(&probe);
    if system != SUPPORTED {
        return Err(HostError::Unsupported(system.to_string()));
    }
    if present {
        return Ok(path);
    }
    run(addr, &copy_command(&dir, &build.0), &bytes).await?;
    Ok(path)
}

/// The line the probe prints first, so whatever the host's startup files print
/// before it cannot be read as the answer.
const PROBE: &str = "__ONEHAND_PROBE__";

/// The probe's answer: `uname -sm` as the host said it, and whether that build is there.
fn probed(out: &str) -> (&str, bool) {
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    let Some(at) = lines.iter().rposition(|l| *l == PROBE) else {
        return ("", false);
    };
    let system = lines.get(at + 1).copied().unwrap_or("");
    (system, lines.get(at + 2) == Some(&"present"))
}

/// What writes the binary on stdin into `dir`. It is written beside its place
/// and moved there only once it names itself `build`: ssh dying mid-copy
/// leaves `cat` a short file and a clean exit, and a short file left in place
/// would pass for that build from then on. A copy that does not run at all
/// (a C library older than the one it was built against) says what the host
/// said of it instead.
///
/// Only what the copy prints on stdout is its build: the loader may warn on
/// stderr of a symbol version the host lacks but the binary only weakly
/// needs, and a copy that runs and names its build is the one sent.
fn copy_command(dir: &str, build: &str) -> String {
    let path = format!("{dir}/onehand-host");
    let tmp = format!("{path}.tmp");
    let err = format!("{tmp}.err");
    format!(
        "mkdir -p {dir} && cat > {tmp} && chmod 755 {tmp} && \
         if said=$({tmp} --build-id 2>{err}); then \
           if test \"$said\" = {build}; then rm -f {err}; mv {tmp} {path}; \
           else warned=$(cat {err}); rm -f {tmp} {err}; \
             echo \"the copy arrived cut short: it names itself $said $warned\" >&2; exit 1; fi; \
         else ended=$?; said=\"$said $(cat {err})\"; rm -f {tmp} {err}; \
           echo \"onehand-host does not run on this host (exit $ended): $said\" >&2; exit 1; fi"
    )
}

#[cfg(test)]
mod tests;
