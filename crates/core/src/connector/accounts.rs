//! Who each connector acts as, as last asked: what the preflight judges a
//! forge step's start on, and what Settings shows.
//!
//! Asking is a call over the network per connector, so it is made once at
//! launch and again when a person asks, never on a frame; a check already
//! under way is not asked twice.

use super::Connector;
use crate::Now;

/// What a connector said of its account: who it acts as, or why it cannot.
pub type Said = Result<String, String>;

/// What each connector said of its account, as last asked.
#[derive(Debug, Default)]
pub struct AccountsDrive {
    /// Each connector by name, beside what it said; `None` before the first
    /// answer.
    said: Option<Vec<(String, Said)>>,
    /// When the last answer landed, in seconds past the epoch.
    at: Option<u64>,
    checking: bool,
}

impl AccountsDrive {
    /// Ask every connector again: whether to carry out
    /// [`check_blocking`] now, which is not while a check is under way.
    #[must_use = "a true answer is a check to carry out"]
    pub fn check(&mut self) -> bool {
        if self.checking {
            return false;
        }
        self.checking = true;
        true
    }

    /// What [`check_blocking`] found, landed at `now`.
    pub fn checked(&mut self, said: Vec<(String, Said)>, now: Now) {
        self.said = Some(said);
        self.at = Some(now.secs);
        self.checking = false;
    }

    /// What the connector named `name` said, `None` before it answered.
    pub fn of(&self, name: &str) -> Option<Said> {
        self.said
            .as_ref()?
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, said)| said.clone())
    }

    /// Whether a check is under way.
    pub fn checking(&self) -> bool {
        self.checking
    }

    /// When the last answer landed, in seconds past the epoch.
    pub fn checked_at(&self) -> Option<u64> {
        self.at
    }
}

/// Ask each of `connectors` who it acts as. Blocking: each is a call over
/// the network.
pub fn check_blocking(connectors: &[&'static dyn Connector]) -> Vec<(String, Said)> {
    connectors
        .iter()
        .map(|connector| (connector.name().to_string(), connector.account_blocking()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::fake::Fake;

    static SIGNED_OUT: Fake = Fake {
        name: "Elsewhere",
        account: Err("not signed in"),
        ..Fake::SERVING
    };

    /// One check at a time; what it found is read by name, and nothing is
    /// known before it lands.
    #[test]
    fn accounts_are_checked_once_at_a_time_and_read_by_name() {
        let mut drive = AccountsDrive::default();
        assert!(drive.check());
        assert!(!drive.check(), "a check under way is not asked twice");
        assert_eq!(drive.of("Forge"), None);
        let said = check_blocking(&[&Fake::SERVING, &SIGNED_OUT]);
        drive.checked(said, Now::read());
        assert!(!drive.checking() && drive.checked_at().is_some());
        assert_eq!(drive.of("Forge"), Some(Ok("Signed in as t".into())));
        assert_eq!(drive.of("Elsewhere"), Some(Err("not signed in".into())));
        assert_eq!(drive.of("Nobody"), None);
        assert!(drive.check(), "asked again once it landed");
    }
}
