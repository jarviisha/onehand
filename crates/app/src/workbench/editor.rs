//! Editor mode: a quick file editor, not an IDE.
//!
//! The tab set, the size bound and the mtime guard are core's
//! (`onehand_core::editor`); what lives here is the `EditorState` buffer per
//! tab and the drawing. Highlighting comes from gpui-component's tree-sitter
//! feature over a deliberately small grammar set: highlighting is the whole
//! ambition here, and there is no language server behind it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Editor, EditorState};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::editor::{RootEditors, SaveOutcome};
use std::collections::HashMap;
use std::path::Path;

/// Editor state for one project root: the tab set from core, plus this front
/// end's buffers.
#[derive(Default)]
pub struct RootBuffers {
    pub tabs: RootEditors,
    /// Keyed by the tab's `uid` rather than by path: a path-keyed buffer would
    /// be shared across windows, and `uid` is what core hands out to prevent
    /// exactly that.
    buffers: HashMap<u64, Entity<EditorState>>,
    /// One per buffer, turning typing into `EditorFile::dirty`.
    ///
    /// Held here rather than dropped at the end of `open_file` because a
    /// `Subscription` unsubscribes on drop: a detached one stops delivering the
    /// moment the function that made it returns, which is indistinguishable
    /// from never having subscribed. Dropped with the buffer it
    /// watches, so a closed tab stops marking a `RootEditors` entry that is no
    /// longer there.
    watches: HashMap<u64, gpui::Subscription>,
}

impl RootBuffers {
    pub fn buffer(&self, uid: u64) -> Option<&Entity<EditorState>> {
        self.buffers.get(&uid)
    }

    pub fn insert(&mut self, uid: u64, state: Entity<EditorState>, watch: gpui::Subscription) {
        self.buffers.insert(uid, state);
        self.watches.insert(uid, watch);
    }

    pub fn forget(&mut self, uid: u64) {
        self.buffers.remove(&uid);
        self.watches.remove(&uid);
    }

    /// Whether any open tab has edits that a close would throw away.
    pub fn any_dirty(&self) -> bool {
        self.tabs.files.iter().any(|f| f.dirty)
    }
}

/// The tree-sitter language token for a path.
///
/// gpui-component keys its grammars by name, and only the ones enabled in
/// `Cargo.toml` resolve; an unknown token renders as plain text, which is the
/// right failure for a quick editor.
pub fn language_for(path: &Path) -> &'static str {
    match onehand_core::editor::syntax_token(path).as_str() {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "sh" | "bash" | "zsh" => "bash",
        "css" | "scss" => "css",
        "html" | "htm" => "html",
        "json" => "json",
        other => {
            // Borrowed for the whole program: the token set is closed above, so
            // anything else is plain text rather than a leaked allocation.
            let _ = other;
            "text"
        }
    }
}

/// The file-tab strip. A trailing close button shuts the whole set in one
/// touch, rather than making the user close tabs one at a time to get the room
/// back.
pub fn tab_strip(
    root: &RootBuffers,
    on_select: impl Fn(&usize, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&usize, &mut Window, &mut App) + 'static,
    on_close_all: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let active = root.tabs.active;
    let on_select = std::rc::Rc::new(on_select);
    let on_close = std::rc::Rc::new(on_close);

    div()
        .id("editor-tabs")
        .h_flex()
        .items_center()
        .gap_1()
        .w_full()
        .px_2()
        .py_1()
        .overflow_x_scroll()
        .border_b_1()
        .border_color(cx.theme().border)
        .children(root.tabs.files.iter().enumerate().map(|(i, file)| {
            let (select, close) = (on_select.clone(), on_close.clone());
            div()
                .id(("editor-tab", i))
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .px_2()
                .py_0p5()
                .rounded(cx.theme().radius)
                .text_xs()
                .cursor_pointer()
                .when(i == active, |tab| {
                    tab.bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .child(div().max_w(px(160.)).truncate().child(file.label.clone()))
                // The dirty dot, not a modified-name convention: the label is
                // already truncated, and a marker inside it would be the first
                // thing to disappear.
                .when(file.dirty, |tab| {
                    tab.child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(cx.theme().warning),
                    )
                })
                .on_click(move |_, window, cx: &mut App| select(&i, window, cx))
                .child(
                    div()
                        .id(("editor-tab-close", i))
                        .flex_none()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Close).size_3())
                        .on_click(move |_, window, cx: &mut App| close(&i, window, cx)),
                )
        }))
        .child(div().flex_1())
        .child(
            crate::controls::action("close-all-files")
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::Close))
                .on_click(on_close_all),
        )
        .into_any_element()
}

/// The editor body for the active tab.
pub fn body(state: &Entity<EditorState>) -> impl IntoElement + use<> {
    Editor::new(state).h_full()
}

/// Build a buffer for a newly opened file.
pub fn new_buffer(
    path: &Path,
    text: &str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<EditorState> {
    let language = language_for(path);
    let text = text.to_string();
    cx.new(|cx| {
        let mut state = EditorState::new(window, cx)
            .language(language)
            .line_number(true)
            .soft_wrap(false);
        state.set_value(text, window, cx);
        state
    })
}

/// Report a save. Returns the status line to surface, if any.
///
/// A conflict is *not* an error to shrug off: the agent edits files by design,
/// so this is the expected way a save fails and the user has to be told which
/// way it went.
pub fn save_status(outcome: &SaveOutcome, label: &str) -> Option<String> {
    match outcome {
        SaveOutcome::Saved { .. } => None,
        // No on-disk time means the file is *gone*, not merely different --
        // deleted or renamed under the buffer. Saying "changed on disk" there
        // sends the user looking for a diff that does not exist, and the second
        // press recreates the file rather than overwriting one.
        SaveOutcome::Conflict { disk_mtime: None } => Some(format!(
            "{label} no longer exists on disk — nothing was written. Save again to recreate it."
        )),
        SaveOutcome::Conflict { .. } => Some(format!(
            "{label} changed on disk — nothing was written. Save again to overwrite."
        )),
        SaveOutcome::Failed(e) => Some(format!("{label} not saved — {e}")),
    }
}
