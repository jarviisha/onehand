//! Reading a credential: a bearer secret such as a bot token, which never
//! sits in `onehand.toml` — settings write into that file and people paste it
//! into reports — and is read rather than ever written.
//!
//! Two sources, tried in order, and the second exists because the first cannot
//! cover how the app is actually started:
//!
//! 1. **An environment variable.** The right answer whenever something else is
//!    already managing secrets: a service manager's environment file, a shell
//!    profile that is not in a repository, a password manager's shim.
//! 2. **A file of its own**, holding the secret and nothing else. A desktop app
//!    is usually launched by clicking an icon, with no shell in that path to
//!    have set a variable in. A separate file is never rewritten, never quoted
//!    into a bug report, and its permissions can be checked, which is the one
//!    thing this module then does.
//!
//! The trade is stated rather than hidden: in case 2 the secret still sits in
//! plaintext on the disk, and this is not a keyring. What it buys is that the
//! plaintext is in exactly one file, that file's only content is the secret,
//! and the app reads it without ever being in a position to copy it elsewhere.

use std::path::Path;

/// The secret in `$env_var`, else in `file`, or `None` when neither holds one.
pub fn secret(env_var: &str, file: &Path) -> Option<String> {
    let from_env = std::env::var(env_var).ok();
    let from_file = std::fs::read_to_string(file).ok();
    if from_env.is_none() && from_file.is_some() {
        warn_if_shared(file);
    }
    pick(from_env.as_deref(), from_file.as_deref())
}

/// Write `value` to `file` as a secret: owner-only from its first byte,
/// trimmed, ending in a newline, and whole or not at all.
///
/// Written to a file beside it and renamed over it, so a write cut short
/// leaves the old secret rather than half of a new one.
pub fn write_secret(file: &Path, value: &str) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut name = file.as_os_str().to_owned();
    name.push(".tmp");
    let temp = std::path::PathBuf::from(name);
    let _ = std::fs::remove_file(&temp);
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        open.mode(0o600);
    }
    let written = (|| {
        use std::io::Write as _;
        let mut out = open.open(&temp)?;
        out.write_all(format!("{}\n", value.trim()).as_bytes())?;
        out.sync_all()
    })();
    match written.and_then(|()| std::fs::rename(&temp, file)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            Err(e)
        }
    }
}

/// Remove the secret in `file`; one that is not there is already removed.
pub fn remove_secret(file: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Which of the two sources answers, and what counts as an answer.
///
/// Split out from the reading so the rule is testable without a filesystem or a
/// process environment. Whitespace is trimmed and an empty source is *not* an
/// answer: an environment variable set to nothing is how a service manager says
/// "unset" when the file it read was empty, and a file ending in the newline
/// every editor adds is the ordinary case rather than the exception.
fn pick(from_env: Option<&str>, from_file: Option<&str>) -> Option<String> {
    [from_env, from_file]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|secret| !secret.is_empty())
        .map(str::to_string)
}

/// Whether a Unix mode keeps a file to its owner.
///
/// The bits that matter are the six for group and other. A secret readable by
/// anyone with an account on the machine is not really a secret, and on a
/// shared or multi-user box that is the difference between a credential and a
/// published one.
#[cfg_attr(not(unix), allow(dead_code))]
fn mode_is_private(mode: u32) -> bool {
    mode & 0o077 == 0
}

/// Say so on stderr when the file is readable by more than its owner.
///
/// A warning and not a refusal. Refusing would mean an app that silently does
/// not work because of a permission the user cannot see, and the person who has
/// to fix it is the same person who is already reading this line — telling them
/// what to run is worth more than withholding the feature.
#[cfg(unix)]
fn warn_if_shared(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let mode = meta.permissions().mode();
    if !mode_is_private(mode) {
        eprintln!(
            "onehand: {} is readable by more than its owner (mode {:o}). \
             It holds a secret; run: chmod 600 {}",
            path.display(),
            mode & 0o777,
            path.display()
        );
    }
}

/// Nothing to check: the platform's file permissions are not a Unix mode, and
/// inventing an answer for one would be worse than saying nothing.
#[cfg(not(unix))]
fn warn_if_shared(_path: &Path) {}

#[cfg(test)]
mod tests;
