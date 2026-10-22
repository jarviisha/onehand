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
//! So the token is read and never written, by the rule every credential is
//! read by ([`onehand_core::config::secret`]): `$ONEHAND_TELEGRAM_TOKEN` (or
//! the variable `token_env` names) first, then [`token_file`].

use onehand_core::config::{TelegramConfig, config_dir};
use std::path::PathBuf;

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
    onehand_core::config::secret(env_name, &token_file())
}
