//! `onehand-host`: runs on a machine reached over SSH and keeps onehand's
//! sessions there alive while no laptop is attached.
//!
//! - `serve` runs the service (one per data directory).
//! - `attach` joins stdin and stdout to it, starting it when needed.
//! - `--build-id` prints the id this binary is known by.

mod attach;
mod files;
mod hub;
mod serve;

use std::path::PathBuf;
use std::process::ExitCode;

use onehand_core::host::wire::{build_id, Build};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("serve") => match own_build() {
            Ok(build) => serve::run(data_dir(), build).await,
            Err(e) => fail(&e),
        },
        Some("attach") => attach::run(data_dir()).await,
        Some("--build-id") => match own_build() {
            Ok(build) => {
                println!("{build}");
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        },
        _ => {
            eprintln!("usage: onehand-host serve | attach | --build-id");
            ExitCode::from(2)
        }
    }
}

/// `$ONEHAND_HOST_DIR`, or `~/.onehand-host/data`.
fn data_dir() -> PathBuf {
    match std::env::var_os("ONEHAND_HOST_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .unwrap_or_default()
            .join(".onehand-host")
            .join("data"),
    }
}

/// The build id of the binary this process runs.
fn own_build() -> Result<Build, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    Ok(build_id(&bytes))
}

fn fail(why: &str) -> ExitCode {
    eprintln!("onehand-host: {why}");
    ExitCode::FAILURE
}
