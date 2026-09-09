// Nothing here is `pub` unless the binary names it: a `pub` item in a
// library is reachable from outside the crate as far as rustc is concerned,
// so `dead_code` stops at one and a contribution that lost its last caller
// looks exactly like a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity, Window};
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Request, WorkbenchMode};
use std::path::Path;

mod mode;
mod view;
pub(crate) use mode::EditorView;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(PluginId::new("workbench.editor"), "Editor");

/// The Editor mode: a view and nothing else.
pub struct Mode {
    view: Entity<EditorView>,
}

impl Mode {
    pub fn new(cx: &mut App) -> Self {
        Self {
            view: EditorView::new(cx),
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

    fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut App) -> bool {
        self.view.update(cx, |view, cx| view.open(path, window, cx))
    }

    fn focus(&self, window: &mut Window, cx: &mut App) -> bool {
        let Some(caret) = self.view.read(cx).caret(cx) else {
            return false;
        };
        caret.focus(window, cx);
        true
    }

    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        match request {
            Request::Save => {
                self.view.update(cx, |view, cx| view.save_active(cx));
                true
            }
            _ => false,
        }
    }

    fn unsaved(&self, root: &Path, cx: &App) -> usize {
        self.view.read(cx).unsaved(root)
    }
}

#[cfg(test)]
mod tests {
    use crate::view::RootBuffers;
    use std::path::PathBuf;

    #[test]
    fn dirty_editor_state_is_reported() {
        let mut buffers = RootBuffers::default();
        buffers.tabs.open(PathBuf::from("file.rs"), None, 7);
        buffers.tabs.files[0].dirty = true;
        assert!(buffers.any_dirty());
    }
}
