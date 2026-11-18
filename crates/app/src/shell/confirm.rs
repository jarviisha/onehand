//! The question the shell asks before it does something that cannot be taken
//! back. The modal itself is the UI crate's, so a feature crate asks the
//! same way.

use super::Shell;
use gpui::{Context, Window};

pub(crate) use onehand_ui::{Answer, Confirm, confirm_either_on, confirm_on};

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

    /// Ask `ask`, which has a second way ahead, and run `then` on the shell
    /// with the way it was answered, unless it was answered *Keep*.
    pub(super) fn confirm_either(
        &self,
        ask: Confirm,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, Answer, &mut Window, &mut Context<Self>) + 'static,
    ) {
        confirm_either_on(cx.entity(), ask, window, cx, then);
    }
}
