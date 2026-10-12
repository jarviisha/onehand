//! The question the shell asks before it does something that cannot be taken
//! back. The modal itself is the plugin host's, so a built-in plugin asks the
//! same way; this names it as the app's call sites already do.

use super::Shell;
use gpui::{Context, Window};

pub(crate) use onehand_plugin_host::{Confirm as Ask, confirm_on as ask_on};

impl Shell {
    /// Ask `ask`, and run `then` on the shell only when it is answered yes.
    pub(super) fn ask(
        &self,
        ask: Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        ask_on(cx.entity(), ask, window, cx, then);
    }
}
