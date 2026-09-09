// Nothing here is `pub` unless the binary names it: a `pub` item in a
// library is reachable from outside the crate as far as rustc is concerned,
// so `dead_code` stops at one and a contribution that lost its last caller
// looks exactly like a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity};
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod view;
pub(crate) use view::*;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(PluginId::new("workbench.files"), "Files");

/// The Files mode: a view and nothing else.
///
/// Everything this mode works on lives in the view, so what is left here is the
/// declaration and the forwarding — which is the whole point of the shape.
pub struct Mode {
    view: Entity<FilesView>,
}

impl Mode {
    pub fn new(ask: Ask, cx: &mut App) -> Self {
        Self {
            view: FilesView::new(ask, cx),
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
            Request::SetGit(git) => {
                self.view.update(cx, |view, cx| view.set_git(git, cx));
                true
            }
            Request::Rescan => {
                self.view.update(cx, |view, cx| view.rescan(cx));
                true
            }
            _ => false,
        }
    }
}
