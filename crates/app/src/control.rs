//! onehand's tools, carried out.
//!
//! What a tool may do and how a message is answered is core's `control`; the
//! socket's side of the conversation runs on the listener's runtime and sends
//! only a [`Call`] across, which [`perform`] carries out on the window in
//! front and answers with the text the caller reads.

use gpui::{App, Window};
use onehand_core::config::ControlConfig;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a program outside onehand may use the tools. Read by the
/// listener on every message it answers, so it is shared with that runtime
/// rather than kept in a global only the UI thread reaches.
static OUTSIDE: AtomicBool = AtomicBool::new(false);

/// Put the config's answer in force, before anything listens.
pub(crate) fn boot(cfg: &ControlConfig) {
    OUTSIDE.store(cfg.outside, Ordering::Relaxed);
}

/// Whether outside control is on.
pub(crate) fn outside() -> bool {
    OUTSIDE.load(Ordering::Relaxed)
}

/// Turn outside control on or off, at once and for the next message of a
/// client already connected, then write it; a write that fails puts it back.
pub(crate) fn set_outside(on: bool, window: &mut Window, cx: &mut App) {
    let before = outside();
    if before == on {
        return;
    }
    put_outside(on, window, cx);
    crate::saves::write_config(
        "control-outside",
        "Saving outside control",
        move |cfg| cfg.control.outside = on,
        Some(crate::saves::Change {
            before,
            after: on,
            put: put_outside,
        }),
        window,
        cx,
    );
}

fn put_outside(on: bool, _: &mut Window, cx: &mut App) {
    OUTSIDE.store(on, Ordering::Relaxed);
    cx.refresh_windows();
}
