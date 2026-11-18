//! The carrier for who each connector acts as: the check run off the UI
//! loop, what it found handed to the drive, and the reads Settings and the
//! preflight draw from. What is known, and when to ask, is the drive's.

use gpui::{App, Global};
use onehand_core::connector::accounts::{self, AccountsDrive, Said};

#[derive(Default)]
struct Accounts(AccountsDrive);

impl Global for Accounts {}

/// Ask every connector who it acts as, unless a check is under way, and
/// redraw every window once it lands.
pub(crate) fn check(cx: &mut App) {
    if !cx.default_global::<Accounts>().0.check() {
        return;
    }
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let said = cx
            .background_executor()
            .spawn(async { accounts::check_blocking(crate::features::connectors()) })
            .await;
        cx.update(|cx| {
            cx.default_global::<Accounts>()
                .0
                .checked(said, onehand_core::Now::read());
            cx.refresh_windows();
        });
    })
    .detach();
}

/// What the connector named `name` said of its account, `None` before it
/// answered.
pub(crate) fn of(name: &str, cx: &App) -> Option<Said> {
    cx.try_global::<Accounts>()?.0.of(name)
}

/// Whether a check is under way.
pub(crate) fn checking(cx: &App) -> bool {
    cx.try_global::<Accounts>().is_some_and(|a| a.0.checking())
}

/// When the last check landed, in seconds past the epoch.
pub(crate) fn checked_at(cx: &App) -> Option<u64> {
    cx.try_global::<Accounts>()?.0.checked_at()
}
