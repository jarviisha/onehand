//! Issues mode: the project's own issues, listed on the left, the one being
//! read or written on the right.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity, Focusable as _, Window};
use onehand_core::connector::Connector;
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod view;
use view::IssuesView;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(PluginId::new("workbench.issues"), "Issues");

/// The Issues mode: a view and nothing else.
pub struct Mode {
    view: Entity<IssuesView>,
}

impl Mode {
    pub fn new(connectors: &'static [&'static dyn Connector], ask: Ask, cx: &mut App) -> Self {
        Self {
            view: IssuesView::new(connectors, ask, cx),
        }
    }
}

impl WorkbenchMode for Mode {
    fn spec(&self) -> WorkbenchModeSpec {
        SPEC
    }

    fn view(&self) -> AnyView {
        self.view.clone().into()
    }

    /// The caret goes to the mode itself, where its single-key shortcuts
    /// are read.
    fn focus(&self, window: &mut Window, cx: &mut App) -> bool {
        self.view.read(cx).focus_handle(cx).focus(window, cx);
        true
    }

    fn set_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.set_root(root, cx));
    }

    fn forget_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.forget_root(root, cx));
    }

    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        match request {
            Request::SetStorage(storage) => {
                self.view.update(cx, |view, cx| {
                    view.set_storage(storage.map(Path::to_path_buf), cx)
                });
                true
            }
            // Something other than this view may have written the file — a
            // run leaving a note — so it is read again the next time it is
            // drawn. One small file, so there is nothing to save by waiting
            // longer than that.
            Request::Rescan | Request::Shown => {
                self.view.update(cx, |view, cx| view.mark_stale(cx));
                true
            }
            Request::ShowIssue(number) => {
                self.view
                    .update(cx, |view, cx| view.show_issue(*number, cx));
                true
            }
            _ => false,
        }
    }
}
