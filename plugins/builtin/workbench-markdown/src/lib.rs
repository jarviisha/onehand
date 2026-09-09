//! Markdown mode: the project's `.md` files, and the one being read, rendered.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity};
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod document;
mod index;
mod view;
pub(crate) use view::MarkdownView;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(PluginId::new("workbench.markdown"), "Markdown");

/// The Markdown mode: a view and nothing else.
pub struct Mode {
    view: Entity<MarkdownView>,
}

impl Mode {
    pub fn new(ask: Ask, cx: &mut App) -> Self {
        Self {
            view: MarkdownView::new(ask, cx),
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

    fn set_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.set_root(root, cx));
    }

    fn forget_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.forget_root(root, cx));
    }

    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        match request {
            // The walk is the whole project, so what these two do is note that
            // it is out of date. It is run when the mode is next drawn, which
            // is the moment that is known not to be a project nobody has
            // opened this mode on.
            Request::Rescan | Request::Shown => {
                self.view.update(cx, |view, cx| view.mark_stale(cx));
                true
            }
            _ => false,
        }
    }
}
