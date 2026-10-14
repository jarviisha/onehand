//! A change to `onehand.toml` shown at once and written after it: off the UI
//! thread, in the order the changes were made, and put back to its last value
//! known saved if its write fails while no later write carries it, said once.
//!
//! The ordering and the rule are core's ([`onehand_core::saving`]); what is
//! here is the spawn and the toast.

use crate::state::Shared;
use gpui::{App, BorrowAppContext as _, Window};
use gpui_component::WindowExt as _;
use onehand_core::config::AppConfig;
use std::rc::Rc;

/// What puts one setting's value back on screen.
pub(crate) type Put = Rc<dyn Fn(&mut Window, &mut App)>;

/// A change a failed write puts back: the value it replaced, the value it
/// shows, and what puts either on screen.
pub(crate) struct Change<V> {
    pub before: V,
    pub after: V,
    pub put: fn(V, &mut Window, &mut App),
}

/// Write `edit` into `onehand.toml` after every write before it. `kind` names
/// the setting it edits, each put back on its own; `doing` names it in the
/// toast. A `change` of `None` is never put back, and its failure is still
/// said once.
pub(crate) fn write_config<V: Clone + 'static>(
    kind: &'static str,
    doing: &'static str,
    edit: impl FnOnce(&mut AppConfig) + Send + 'static,
    change: Option<Change<V>>,
    window: &mut Window,
    cx: &mut App,
) {
    let path = Shared::global(cx).config_path.clone();
    let (before, after): (Put, Put) = match change {
        Some(change) => {
            let (before, after, put) = (change.before, change.after, change.put);
            (
                Rc::new(move |window, cx| put(before.clone(), window, cx)),
                Rc::new(move |window, cx| put(after.clone(), window, cx)),
            )
        }
        None => (Rc::new(|_, _| {}), Rc::new(|_, _| {})),
    };
    let pending = cx.update_global::<Shared, _>(|shared, _| {
        shared.config_saves.change(kind, before, after);
        shared.config_saves.write_of([kind])
    });
    let written = Shared::global(cx)
        .writer
        .write(move || AppConfig::update_in_place(&path, edit));
    window
        .spawn(cx, async move |cx| {
            let result = written.await;
            let _ = cx.update(|window, cx| {
                let said = cx.update_global::<Shared, _>(|shared, _| {
                    shared.config_saves.landed(pending, result)
                });
                let Some((why, back)) = said else {
                    return;
                };
                for put in back {
                    put(window, cx);
                }
                window.push_notification(crate::toast::failed(doing, why), cx);
            });
        })
        .detach();
}
