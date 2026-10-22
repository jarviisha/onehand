//! One onehand per config directory. The first process to start takes a lock
//! on `<dir>/instance.lock` and keeps it until it exits; the kernel lets go
//! of it if the process dies. Whatever must only ever run once at a time —
//! moving the config directory's files at start — runs behind it.

use std::fs::{File, TryLockError};
use std::path::Path;
use std::sync::OnceLock;

/// Another process holds the lock on this config directory.
#[derive(Debug)]
pub struct Held;

/// The lock, kept for as long as the process lives.
static LOCK: OnceLock<File> = OnceLock::new();

/// Take the lock on `dir` for the rest of this process, or say another
/// process has it. A lock that cannot be taken at all (a read-only
/// directory, say) is reported and let go: it must not keep the app from
/// starting.
pub fn hold_lock(dir: &Path) -> Result<(), Held> {
    match try_lock(dir) {
        Ok(file) => {
            let _ = LOCK.set(file);
            Ok(())
        }
        Err(TryLockError::WouldBlock) => Err(Held),
        Err(TryLockError::Error(why)) => {
            eprintln!("onehand: no lock on {}: {why}", dir.display());
            Ok(())
        }
    }
}

fn try_lock(dir: &Path) -> Result<File, TryLockError> {
    std::fs::create_dir_all(dir).map_err(TryLockError::Error)?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("instance.lock"))
        .map_err(TryLockError::Error)?;
    file.try_lock()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_on_the_same_directory_is_refused_until_the_first_goes() {
        let dir = std::env::temp_dir().join(format!("onehand-instance-{}", std::process::id()));
        let first = try_lock(&dir).unwrap();
        assert!(matches!(try_lock(&dir), Err(TryLockError::WouldBlock)));
        drop(first);
        // A lock goes with the open file, and a child another test forks
        // meanwhile holds a copy of it until it execs: let go is let go
        // within a moment, not at the instant of the drop.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while try_lock(&dir).is_err() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(try_lock(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
