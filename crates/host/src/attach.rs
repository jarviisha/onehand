//! What a laptop runs over SSH: this process's stdin and stdout, joined to the
//! service's socket, with the service started first when none is running.

use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use onehand_core::instance::socket_path;
use tokio::io::AsyncWriteExt as _;
use tokio::net::UnixStream;

/// How long a service just started gets to listen, and how often it is tried.
const START_WAIT: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_millis(100);

pub(crate) async fn run(dir: PathBuf) -> ExitCode {
    let socket = socket_path(&dir);
    let stream = match UnixStream::connect(&socket).await {
        Ok(stream) => stream,
        Err(_) => match start_and_connect(&dir, &socket).await {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("onehand-host: no service to attach to: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let (mut from, mut to) = stream.into_split();
    // Stdin closing half-closes the socket, and the service then closes its
    // side once it has said what it had queued, which ends the copy below.
    tokio::spawn(async move {
        let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut to).await;
        let _ = to.shutdown().await;
    });
    let mut stdout = tokio::io::stdout();
    let _ = tokio::io::copy(&mut from, &mut stdout).await;
    let _ = stdout.flush().await;
    ExitCode::SUCCESS
}

/// Start `serve` in a process group of its own, so it outlives this SSH
/// session, then connect once it listens.
async fn start_and_connect(dir: &Path, socket: &Path) -> std::io::Result<UnixStream> {
    Command::new(std::env::current_exe()?)
        .arg("serve")
        .env("ONEHAND_HOST_DIR", dir)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut waited = Duration::ZERO;
    loop {
        tokio::time::sleep(RETRY).await;
        waited += RETRY;
        match UnixStream::connect(socket).await {
            Ok(stream) => return Ok(stream),
            Err(e) if waited >= START_WAIT => return Err(e),
            Err(_) => {}
        }
    }
}
