// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity, Window};
use onehand_plugin_api::{PluginId, WorkbenchModeSpec};
use onehand_plugin_host::{Ask, Request, WorkbenchMode};
use std::path::Path;

mod buffers;
mod split;
mod view;
pub(crate) use split::CodeView;
pub(crate) use view::EditorView;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(PluginId::new("workbench.editor"), "Editor");

/// The Editor mode: the project's file tree, and the buffers opened out of it.
///
/// The tree is the Files plugin unchanged, held here as the mode it already is
/// rather than copied in: every request this one does not answer itself is put
/// to it, and what is new is the view pairing the two. They shared the panel by
/// taking turns on the strip before, which charged a mode switch for every file
/// picked — the one thing this panel is opened to do.
pub struct Mode {
    view: Entity<EditorView>,
    files: onehand_workbench_files::Mode,
    /// The pair, drawn side by side. Built once, because a view rebuilt per
    /// frame is a divider that forgets where it was dragged to.
    split: Entity<CodeView>,
}

impl Mode {
    pub fn new(ask: Ask, cx: &mut App) -> Self {
        let view = EditorView::new(cx);
        let files = onehand_workbench_files::Mode::new(ask, cx);
        let split = CodeView::new(files.view(), view.clone(), cx);
        Self { view, files, split }
    }
}

impl WorkbenchMode for Mode {
    fn spec(&self) -> WorkbenchModeSpec {
        SPEC
    }

    fn view(&self) -> AnyView {
        self.split.clone().into()
    }

    fn set_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.set_root(root, cx));
        self.files.set_root(root, cx);
    }

    fn forget_root(&mut self, root: &Path, cx: &mut App) {
        self.view.update(cx, |view, cx| view.forget_root(root, cx));
        self.files.forget_root(root, cx);
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

    /// Answered here where the buffers are the half that knows, and passed on
    /// otherwise — the git badges and the rescan are the tree's, and this mode
    /// has no business knowing which of the two took either.
    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        if let Request::Save = request {
            self.view.update(cx, |view, cx| view.save_active(cx));
            return true;
        }
        self.files.handle(request, cx)
    }

    fn unsaved(&self, root: &Path, cx: &App) -> usize {
        self.view.read(cx).unsaved(root)
    }
}

#[cfg(test)]
mod tests {
    use crate::buffers::RootBuffers;
    use std::path::PathBuf;

    #[test]
    fn dirty_editor_state_is_reported() {
        let mut buffers = RootBuffers::default();
        buffers.tabs.open(PathBuf::from("file.rs"), None, 7);
        buffers.tabs.files[0].dirty = true;
        assert!(buffers.any_dirty());
    }
}
