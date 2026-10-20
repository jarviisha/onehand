//! The question the shell asks before it does something that cannot be taken
//! back. The modal itself is the plugin host's, so a built-in plugin asks the
//! same way.

use super::Shell;
use gpui::{Context, Window};

pub(crate) use onehand_ui::{Confirm, confirm_on};

impl Shell {
    /// Ask `ask`, and run `then` on the shell only when it is answered yes.
    pub(super) fn confirm(
        &self,
        ask: Confirm,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        confirm_on(cx.entity(), ask, window, cx, then);
    }
}
