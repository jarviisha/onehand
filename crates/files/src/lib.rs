// Nothing here is `pub` unless the binary names it: `dead_code` stops at a
// `pub` item in a library, so one that lost its last caller looks exactly like
// a working feature.
#![warn(unreachable_pub)]

use gpui::{AnyView, App, Entity};
use onehand_workbench::{Asker, Disk, ModeId, Request, WorkbenchMode, WorkbenchModeSpec};
use std::path::Path;

mod view;
pub(crate) use view::*;

/// What this mode declares about itself, which is what the panel reads
/// instead of matching the ID against a list it has to know by heart.
pub const SPEC: WorkbenchModeSpec =
    WorkbenchModeSpec::element(ModeId::new("workbench.files"), "Files");

/// The most the dropdown's tree stands before it scrolls: a folder or two
/// unfolded. In rems, so it follows the panel's zoom.
const DROPDOWN_MAX_H: gpui::Rems = gpui::rems(24.);

/// The Files mode: a view and nothing else.
///
/// Everything this mode works on lives in the view, so what is left here is the
/// declaration and the forwarding — which is the whole point of the shape.
pub struct Mode {
    view: Entity<FilesView>,
    /// A second tree over the same project, for a dropdown the mode beside
    /// the tree opens: an entity is drawn in one place, so the tree that
    /// stands in the panel cannot also stand in a popup. Told everything the
    /// first is told, it keeps its own folds and reads its own listings.
    dropdown: Entity<FilesView>,
}

impl Mode {
    pub fn new(ask: Asker, cx: &mut App) -> Self {
        Self {
            view: FilesView::new(ask.clone(), cx),
            dropdown: FilesView::capped(ask, Some(DROPDOWN_MAX_H), cx),
        }
    }

    /// Both trees, to tell each the same thing.
    fn each(&self, cx: &mut App, tell: impl Fn(&mut FilesView, &mut gpui::Context<FilesView>)) {
        for view in [&self.view, &self.dropdown] {
            view.update(cx, |view, cx| tell(view, cx));
        }
    }

    /// How the mode beside the tree says which file it has open, so both
    /// trees pick that file's row.
    pub fn active_setter(&self) -> impl Fn(Option<std::path::PathBuf>, &mut App) + 'static {
        let views = [self.view.clone(), self.dropdown.clone()];
        move |path, cx| {
            for view in &views {
                view.update(cx, |view, cx| view.set_active(path.clone(), cx));
            }
        }
    }

    /// The second tree, drawn wherever the mode beside it opens it.
    pub fn dropdown_view(&self) -> AnyView {
        self.dropdown.clone().into()
    }
}

impl WorkbenchMode for Mode {
    fn spec(&self) -> WorkbenchModeSpec {
        SPEC
    }

    fn view(&self) -> AnyView {
        self.view.clone().into()
    }

    fn set_root(&mut self, root: &Path, disk: &Disk, cx: &mut App) {
        self.each(cx, |view, cx| view.set_root(root, disk, cx));
    }

    fn forget_root(&mut self, root: &Path, cx: &mut App) {
        self.each(cx, |view, cx| view.forget_root(root, cx));
    }

    fn handle(&mut self, request: &Request<'_>, cx: &mut App) -> bool {
        match request {
            Request::SetGit(git) => {
                self.each(cx, |view, cx| view.set_git(git, cx));
                true
            }
            Request::Offloaded(roots) => {
                self.each(cx, |view, cx| view.set_offloaded(roots, cx));
                true
            }
            Request::Rescan => {
                self.each(cx, |view, cx| view.rescan(cx));
                true
            }
            // Not this mode's to answer.
            Request::Save
            | Request::Start
            | Request::Shown
            | Request::Reap
            | Request::SetStorage(_)
            | Request::SetFontSize(_)
            | Request::ShowIssue(_)
            | Request::AgentStarted(_)
            | Request::LiveConversations(_)
            | Request::Locked(_) => false,
        }
    }
}
