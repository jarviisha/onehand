//! Plugins mode: the Claude Code plugins that reach a session started in this
//! project — installed globally, for the project, or for the project on this
//! machine alone — and the marketplaces' plugins that could.

// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity};
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Request, WorkbenchMode};
use std::path::Path;

mod cli;
mod view;
use view::PluginsView;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(PluginId::new("workbench.plugins"), "Plugins");

/// The Plugins mode: a view and nothing else.
pub struct Mode {
    view: Entity<PluginsView>,
}

impl Mode {
    pub fn new(cx: &mut App) -> Self {
        Self {
            view: PluginsView::new(cx),
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
            // Something other than this mode may have changed what is
            // installed — a terminal, or the agent itself — so the list is read
            // again the next time it is drawn. One local command, well under a
            // second, so there is nothing to save by waiting longer.
            Request::Shown => {
                self.view.update(cx, |view, cx| view.mark_stale(cx));
                true
            }
            _ => false,
        }
    }
}
