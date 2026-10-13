//! A change to `onehand.toml` shown at once and written after it: off the UI
//! thread, in the order the changes were made, and put back if its write
//! fails as the newest of its kind, said once.
//!
//! The ordering and the rule are core's ([`onehand_core::saving`]); what is
//! here is the spawn and the toast.

use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Window};
use gpui_component::WindowExt as _;
use onehand_core::config::AppConfig;

/// What puts one change back when its write fails.
pub(crate) type Undo = Box<dyn FnOnce(&mut Window, &mut App)>;

/// Write `edit` into `onehand.toml` after every write before it. `kind` names
/// the change, so only the newest change of a kind is put back by `undo` when
/// its write fails; `doing` names it in the toast.
pub(crate) fn write_config(
    kind: &'static str,
    doing: &'static str,
    edit: impl FnOnce(&mut AppConfig) + Send + 'static,
    undo: Option<Undo>,
    window: &mut Window,
    cx: &mut App,
) {
    let path = Shared::global(cx).config_path.clone();
    let this = cx.update_global::<Shared, _>(|shared, _| {
        let newest = shared.config_writes.entry(kind).or_default();
        *newest += 1;
        *newest
    });
    let written = Shared::global(cx)
        .writer
        .write(move || AppConfig::update_in_place(&path, edit));
    window
        .spawn(cx, async move |cx| {
            let Err(why) = written.await else {
                return;
            };
            let _ = cx.update(|window, cx| {
                if Shared::global(cx).config_writes.get(kind) != Some(&this) {
                    return;
                }
                if let Some(undo) = undo {
                    undo(window, cx);
                }
                window.push_notification(crate::toast::failed(doing, why), cx);
            });
        })
        .detach();
}
