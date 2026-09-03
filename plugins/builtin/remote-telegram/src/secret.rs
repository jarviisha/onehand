//! Where a channel's credential comes from, and where it must never be.
//!
//! **A bot token is not a setting.** It is a bearer credential: anyone holding
//! it can read every message the bot receives and send as it, with no second
//! factor and no per-device revocation. The app's own config file is the wrong
//! place for one and would stay the wrong place even if nothing else changed,
//! for three reasons that are all about what that file *is* rather than about
//! how it happens to be written today. It is rewritten whole by the settings
//! dialog and by the agent manager, so a value put in it is a value the app
//! prints back out on a schedule nobody chose. It is the file people paste into
//! a bug report, because it holds the agent list. And it is world-readable by
//! default, because everything else in it is a preference.
//!
//! So the token is read and never written. Two sources, tried in order, and the
//! second exists because the first cannot cover how the app is actually started:
//!
//! 1. **An environment variable**, named by the config or defaulting to
//!    [`DEFAULT_TOKEN_ENV`]. This is the right answer whenever something else is
//!    already managing secrets — a service manager's environment file, a shell
//!    profile that is not in a repository, a password manager's shim.
//! 2. **A file of its own**, [`token_file`], holding the token and nothing else.
//!    This exists because a desktop application is usually launched by clicking
//!    an icon, and there is no shell in that path to have set a variable in. A
//!    separate file is still a real improvement over the config: it is never
//!    rewritten, it is never quoted into a bug report, and its permissions can
//!    be checked, which is the one thing this module then does.
//!
//! The trade being made is stated rather than hidden: the token still sits in
//! plaintext on the disk in case 2, and this is not a keyring. What it buys is
//! that the plaintext is in exactly one file, that file's only content is the
//! secret, and the app reads it without ever being in a position to copy it
//! somewhere else.

use onehand_core::config::{TelegramConfig, config_dir};
use std::path::{Path, PathBuf};

/// The environment variable a Telegram token is read from unless the config
/// names another.
pub const DEFAULT_TOKEN_ENV: &str = "ONEHAND_TELEGRAM_TOKEN";

/// The file the token is read from when no environment variable holds it.
pub fn token_file() -> PathBuf {
    config_dir().join("telegram.token")
}

/// The token, from whichever source has one.
///
/// `None` means the bridge cannot start, which the caller reports once rather
/// than retrying: a missing credential is not a condition that clears itself.
pub fn telegram_token(cfg: &TelegramConfig) -> Option<String> {
    let env_name = cfg.token_env.as_deref().unwrap_or(DEFAULT_TOKEN_ENV);
    let from_env = std::env::var(env_name).ok();
    let path = token_file();
    let from_file = std::fs::read_to_string(&path).ok();

    if from_env.is_none() && from_file.is_some() {
        warn_if_shared(&path);
    }
    pick_token(from_env.as_deref(), from_file.as_deref())
}

/// Which of the two sources answers, and what counts as an answer.
///
/// Split out from the reading so the rule is testable without a filesystem or a
/// process environment. Whitespace is trimmed and an empty source is *not* an
/// answer: an environment variable set to nothing is how a service manager says
/// "unset" when the file it read was empty, and a token file ending in the
/// newline every editor adds is the ordinary case rather than the exception.
pub fn pick_token(from_env: Option<&str>, from_file: Option<&str>) -> Option<String> {
    [from_env, from_file]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|token| !token.is_empty())
        .map(str::to_string)
}

/// Whether a Unix mode keeps a file to its owner.
///
/// The bits that matter are the six for group and other. A token readable by
/// anyone with an account on the machine is a token that is not really a secret,
/// and on a shared or multi-user box that is the difference between a
/// credential and a published one.
pub fn mode_is_private(mode: u32) -> bool {
    mode & 0o077 == 0
}

/// Say so on stderr when the token file is readable by more than its owner.
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
             It holds a bot token; run: chmod 600 {}",
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
mod tests {
    use super::*;

    #[test]
    fn the_environment_wins_over_the_file() {
        assert_eq!(
            pick_token(Some("from-env"), Some("from-file")).as_deref(),
            Some("from-env")
        );
    }

    #[test]
    fn a_file_answers_when_nothing_is_in_the_environment() {
        assert_eq!(
            pick_token(None, Some("from-file")).as_deref(),
            Some("from-file")
        );
        assert_eq!(pick_token(None, None), None);
    }

    /// An empty or blank source is not an answer, and must fall through rather
    /// than shadow the source that has one. A service manager reading an empty
    /// file sets the variable to the empty string, which is how the environment
    /// ends up saying "" and meaning "nothing here".
    #[test]
    fn a_blank_source_falls_through() {
        assert_eq!(
            pick_token(Some(""), Some("from-file")).as_deref(),
            Some("from-file")
        );
        assert_eq!(
            pick_token(Some("   \n"), Some("from-file")).as_deref(),
            Some("from-file")
        );
        assert_eq!(pick_token(Some(""), Some("  ")), None);
    }

    /// The newline every editor puts at the end of a file is not part of the
    /// token, and a token sent with one is rejected by the far side with an
    /// error that says nothing about a newline.
    #[test]
    fn the_trailing_newline_of_a_token_file_is_not_the_token() {
        assert_eq!(
            pick_token(None, Some("123:abc\n")).as_deref(),
            Some("123:abc")
        );
    }

    #[test]
    fn only_an_owner_only_mode_is_private() {
        assert!(mode_is_private(0o600));
        assert!(mode_is_private(0o400));
        // The two that matter: the group and the rest of the machine.
        assert!(!mode_is_private(0o640));
        assert!(!mode_is_private(0o604));
        assert!(!mode_is_private(0o644));
        // The file type bits ride in the same number and say nothing about who
        // can read it.
        assert!(mode_is_private(0o100_600));
    }
}
