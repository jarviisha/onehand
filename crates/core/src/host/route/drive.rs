//! The machine's switch for builds on hosts, and what each host last did
//! when a routed run reached for it. The drive only answers; the carrier
//! writes the flag file.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::workspace::HostName;

/// How long a host that did not answer is taken as down without asking
/// again: a burst of routed commands then falls back at once, not after
/// ssh's wait each.
const DOWN_FOR: Duration = Duration::from_secs(30);

/// How a host answered the last routed run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Standing {
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
    /// Each host's standing, and when it was last set.
    standing: BTreeMap<HostName, (Standing, Instant)>,
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
    pub(crate) fn load(dir: &Path) -> Self {
        Self {
            hosts_on: !flag(dir).exists(),
            standing: BTreeMap::new(),
        }
    }

    pub fn hosts_on(&self) -> bool {
        self.hosts_on
    }

    pub(crate) fn standing(&self, host: &HostName) -> Option<Standing> {
        self.standing.get(host).map(|(s, _)| *s)
    }

    /// Whether `host` did not answer a routed run less than [`DOWN_FOR`]
    /// before `now`.
    pub(crate) fn recently_down(&self, host: &HostName, now: Instant) -> bool {
        matches!(self.standing.get(host), Some((Standing::NotAnswering, at)) if now.duration_since(*at) < DOWN_FOR)
    }

    #[must_use]
    pub fn set_hosts_on(&mut self, on: bool) -> Vec<RouteEffect> {
        if self.hosts_on == on {
            return Vec::new();
        }
        self.hosts_on = on;
        vec![RouteEffect::WriteSwitch(on)]
    }

    /// Record how `host` answered; whether its standing changed.
    pub(crate) fn reached(&mut self, host: &HostName, answered: bool) -> bool {
        self.reached_at(host, answered, Instant::now())
    }

    pub(crate) fn reached_at(&mut self, host: &HostName, answered: bool, now: Instant) -> bool {
        let s = if answered {
            Standing::Reached
        } else {
            Standing::NotAnswering
        };
        let had = self.standing.insert(host.clone(), (s, now));
        had.map(|(had, _)| had) != Some(s)
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
