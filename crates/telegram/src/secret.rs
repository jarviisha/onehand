//! Where a channel's credential comes from, and where it must never be.
//!
//! **A bot token is not a setting.** It is a bearer credential: anyone holding
//! it can read every message the bot receives and send as it, with no second
//! factor and no per-device revocation. The app's own config file is the wrong
//! place for one and would stay the wrong place even if nothing else changed,
//! for three reasons that are all about what that file *is* rather than about
//! how it happens to be written today. Settings writes into it, and its agent
//! page rewrites the agent list whole, so a value put in it is a value the app
//! handles on a schedule nobody chose. It is the file people paste into
//! a bug report, because it holds the agent list. And it is world-readable by
//! default, because everything else in it is a preference.
//!
//! So the token is read by the rule every credential is read by
//! ([`onehand_core::config::secret`]): `$ONEHAND_TELEGRAM_TOKEN` (or the
//! variable `token_env` names) first, then [`token_file`]. **The one place it
//! is written is that file**, owner-only, when Settings saves it
//! ([`onehand_core::config::write_secret`]); it is never written to the
//! config, never logged and never shown back.

use onehand_core::config::{TelegramConfig, config_dir};
use onehand_core::remote::TokenSource;
use std::path::{Path, PathBuf};

/// The environment variable a Telegram token is read from unless the config
/// names another.
pub const DEFAULT_TOKEN_ENV: &str = "ONEHAND_TELEGRAM_TOKEN";

/// The file the token is read from when no environment variable holds it.
pub fn token_file() -> PathBuf {
    config_dir().join("telegram.token")
}

/// The token, and where it was found.
///
/// `None` means the bridge cannot start, which is said rather than retried: a
/// missing credential is not a condition that clears itself. **Never the
/// token's value in the source**: an environment variable is named, not read
/// back.
pub fn token_source(cfg: &TelegramConfig) -> (TokenSource, Option<String>) {
    source_at(cfg, &token_file())
}

fn source_at(cfg: &TelegramConfig, file: &Path) -> (TokenSource, Option<String>) {
    let env_name = cfg.token_env.as_deref().unwrap_or(DEFAULT_TOKEN_ENV);
    let from_env = std::env::var(env_name)
        .ok()
        .filter(|value| !value.trim().is_empty());
    // One rule for what counts as a token, the same as every credential's.
    let token = onehand_core::config::secret(env_name, file);
    let source = match (&token, from_env) {
        (None, _) => TokenSource::Missing,
        (Some(_), Some(_)) => TokenSource::Env(env_name.to_string()),
        (Some(_), None) => TokenSource::File(file.to_path_buf()),
    };
    (source, token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(env: &str) -> TelegramConfig {
        TelegramConfig {
            token_env: Some(env.into()),
            ..TelegramConfig::default()
        }
    }

    /// A file ending in the newline every editor adds is still a token, and is
    /// said to come from that file.
    #[test]
    fn a_token_in_the_file_is_found_there() {
        let file = std::env::temp_dir().join(format!("onehand-tg-{}", std::process::id()));
        std::fs::write(&file, "tok\n").unwrap();
        let found = source_at(&cfg("ONEHAND_TEST_TG_UNSET_A"), &file);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(found, (TokenSource::File(file), Some("tok".into())));
    }

    #[test]
    fn no_token_anywhere_is_missing() {
        let file = std::env::temp_dir().join("onehand-tg-never-written");
        assert_eq!(
            source_at(&cfg("ONEHAND_TEST_TG_UNSET_B"), &file),
            (TokenSource::Missing, None)
        );
    }
}
