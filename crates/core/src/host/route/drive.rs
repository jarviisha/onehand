//! The machine's switch for builds on hosts, and what each host last did
//! when a routed run reached for it. The drive only answers; the carrier
//! writes the flag file.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex, MutexGuard};

use crate::workspace::HostName;

/// How a host answered the last routed run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Reached,
    NotAnswering,
}

/// What the carrier does for the drive.
#[derive(Debug, PartialEq, Eq)]
pub enum RouteEffect {
    WriteSwitch(bool),
}

#[derive(Debug)]
pub struct RouteDrive {
    hosts_on: bool,
    standing: BTreeMap<HostName, Standing>,
}

fn flag(dir: &Path) -> std::path::PathBuf {
    dir.join("builds").join("hosts-off")
}

/// Carries out `RouteEffect::WriteSwitch`: the flag file is there when off.
pub fn write_switch_blocking(dir: &Path, on: bool) -> std::io::Result<()> {
    let file = flag(dir);
    if on {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        std::fs::create_dir_all(file.parent().expect("flag has a parent"))?;
        std::fs::write(&file, b"")
    }
}

impl RouteDrive {
    pub fn load(dir: &Path) -> Self {
        Self {
            hosts_on: !flag(dir).exists(),
            standing: BTreeMap::new(),
        }
    }

    pub fn hosts_on(&self) -> bool {
        self.hosts_on
    }

    pub fn standing(&self, host: &HostName) -> Option<Standing> {
        self.standing.get(host).copied()
    }

    #[must_use]
    pub fn set_hosts_on(&mut self, on: bool) -> Vec<RouteEffect> {
        if self.hosts_on == on {
            return Vec::new();
        }
        self.hosts_on = on;
        vec![RouteEffect::WriteSwitch(on)]
    }

    // The app reads this once routed runs report back.
    #[allow(dead_code)]
    pub(crate) fn reached(&mut self, host: &HostName, answered: bool) {
        let s = if answered {
            Standing::Reached
        } else {
            Standing::NotAnswering
        };
        self.standing.insert(host.clone(), s);
    }
}

/// The process's one drive, loaded from the config directory on first use.
pub fn route_drive() -> MutexGuard<'static, RouteDrive> {
    static DRIVE: LazyLock<Mutex<RouteDrive>> =
        LazyLock::new(|| Mutex::new(RouteDrive::load(&crate::config::config_dir())));
    DRIVE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests;
