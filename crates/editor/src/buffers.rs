//! Editor mode: a quick file editor, not an IDE.
//!
//! The tab set, the size bound and the mtime guard are core's
//! (`onehand_core::editor`); what lives here is the `EditorState` buffer per
//! tab and the drawing. Highlighting comes from gpui-component's tree-sitter
//! feature over a deliberately small grammar set: highlighting is the whole
//! ambition here, and there is no language server behind it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyView, App, AppContext, Entity, IntoElement, ParentElement, Rems, SharedString, Styled,
    Window, div, rems,
};
use gpui_component::input::{Editor, EditorState};
use gpui_component::popover::Popover;
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_core::editor::{RootEditors, Standing};
use onehand_ui::Named as _;
use std::path::Path;

/// The unsaved-edits mark on the open file.
const DIRTY_DOT: Rems = rems(0.375);

/// The tree-sitter language token for a path.
///
/// gpui-component keys its grammars by name, and only the ones enabled in
/// `Cargo.toml` resolve; an unknown token renders as plain text, which is the
/// right failure for a quick editor.
pub(crate) fn language_for(path: &Path) -> &'static str {
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

/// What a press on the bar does.
pub(crate) struct StripHandlers {
    pub(crate) toggle_tree: OnPress,
    pub(crate) back: OnPress,
    /// Whether the tree's dropdown is open, and what hears it open or close.
    pub(crate) picking: bool,
    pub(crate) pick: On<bool>,
    /// Handed the open file's index.
    pub(crate) close: On<usize>,
}

pub(crate) type OnPress = On<gpui::ClickEvent>;
pub(crate) type On<T> = Box<dyn Fn(&T, &mut Window, &mut App)>;

/// The tree's dropdown: wide enough for a nested name to be read and a
/// change mark beside it. In rems, so it follows the panel's zoom. Its
/// height is the tree's own, capped where the tree is built.
const TREE_DROPDOWN_W: Rems = rems(20.);

/// The tree's control on the bar: the toggle at its end, or the way back at its start.
pub(crate) enum Lead {
    /// The toggle that hides the tree beside the buffers, or brings it back:
    /// whether it is showing. It stands at the bar's end, by the tree.
    Toggle(bool),
    /// The way back to the tree, while the two are shown one at a time.
    Back,
}

/// The Editor's bar: the way back to the tree while one shows at a time, the
/// file open now as one control dropping down the project's tree, its cross,
/// and the tree's toggle at the end, beside the tree.
///
/// One file and never a tab per file: the names never compete for the width,
/// so nothing is measured and nothing scrolls. Another file is reached
/// through a tree, the one beside or the one this drops down, which is
/// `tree`: a view of its own, never the one drawn beside the buffers.
pub(crate) fn file_bar(
    root: &Path,
    tabs: &RootEditors,
    lead: Lead,
    on: StripHandlers,
    tree: &AnyView,
    cx: &App,
) -> gpui::AnyElement {
    let StripHandlers {
        toggle_tree: on_toggle_tree,
        back: on_back,
        picking,
        pick: on_pick,
        close: on_close,
    } = on;

    let (back, toggle) = match lead {
        Lead::Back => (
            Some(onehand_ui::back_link("editor-back", "Files", cx).on_click(on_back)),
            None,
        ),
        // The toggle's icon is the panel's *state*, open or shut, and the
        // tooltip says what a press does -- so the two never disagree about
        // which way round it is.
        Lead::Toggle(shown) => {
            let (icon, hint) = match shown {
                true => (IconName::PanelRightClose, "Hide the file tree"),
                false => (IconName::PanelRightOpen, "Show the file tree"),
            };
            let toggle = onehand_ui::strip_control("toggle-file-tree", icon, false, cx)
                .named(hint)
                .on_click(on_toggle_tree);
            (None, Some(toggle))
        }
    };

    let active = tabs.active();
    let current = tabs.active_file().map(|file| {
        // The label is the file name alone, so three `mod.rs` read the same;
        // the path relative to the project tells them apart, on hover. The
        // dirty dot is named there too, since a colour is a code somebody has
        // to learn.
        let rel = file.path.strip_prefix(root).unwrap_or(&file.path);
        let hint = SharedString::from(match file.dirty {
            true => format!("{} — unsaved changes", rel.display()),
            false => rel.display().to_string(),
        });
        let trigger =
            onehand_ui::tab_select("editor-file-select", file.label.clone().into(), hint, cx)
                .min_w_0()
                // The dirty dot, not a modified-name convention: the label
                // truncates, and a marker inside it would go first.
                .when(file.dirty, |select| select.child(dirty_dot(cx)));
        // Keyed by the project, so a dropdown left open as the project
        // changes is dropped rather than kept over the wrong tree. Escape and
        // a press outside close it and hand focus back where it was; a file
        // opened from it closes it too, through `picking`.
        let id = SharedString::from(format!("editor-file-tree-{}", root.display()));
        let tree = tree.clone();
        let dropdown = Popover::new(id)
            .trigger(trigger)
            .open(picking)
            .on_open_change(on_pick)
            .p_0()
            .w(TREE_DROPDOWN_W)
            .content(move |_, _, _| tree_dropdown(tree.clone()));
        div()
            .h_flex()
            .items_center()
            .min_w_0()
            .gap_1()
            .child(dropdown)
            .child(close_button(active, on_close, cx))
    });

    // A mode's bar, a step lower than the Workbench's over it.
    onehand_ui::tab_bar(cx)
        .h(onehand_ui::tokens::MODE_BAR_H)
        .children(back.map(|back| div().h_flex().items_center().px_1().child(back)))
        // The one part of the row that gives way: `flex_1` + `min_w_0`, so a
        // long name truncates rather than pushing the toggle out.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_flex()
                .items_center()
                .children(current),
        )
        .children(toggle.map(|toggle| div().h_flex().items_center().child(toggle)))
        .into_any_element()
}

/// What the tree's dropdown holds: the tree, which sizes and scrolls itself.
fn tree_dropdown(tree: AnyView) -> gpui::Div {
    div().v_flex().child(tree)
}

/// The cross that closes file `i`.
fn close_button(i: usize, close: On<usize>, cx: &App) -> gpui_component::button::Button {
    onehand_ui::tab_close("editor-file-close", "Close this file", cx)
        .on_click(move |_, window, cx: &mut App| close(&i, window, cx))
}

/// A file's unsaved-edits mark: a status *fill*, the theme's `warning`, where
/// `status_ink` is for text.
fn dirty_dot(cx: &App) -> gpui::Div {
    div()
        .size(DIRTY_DOT)
        .flex_none()
        .rounded_full()
        .bg(cx.theme().warning)
}

/// The editor body for the active tab.
///
/// `appearance(false)`: by default the component draws itself as a form field —
/// its own fill, border and radius — which inside the Workbench is a rounded
/// box laid on the dock. The dock is the frame; the code sits on its surface
/// the way the file tree beside it does.
///
/// At the code size in rems, over the library's in pixels, so the panel's
/// zoom reaches it.
///
/// `locked`: read only, its folder's code being elsewhere for now.
pub(crate) fn body(state: &Entity<EditorState>, locked: bool) -> impl IntoElement + use<> {
    Editor::new(state)
        .readonly(locked)
        .appearance(false)
        .h_full()
        .text_size(onehand_ui::tokens::CODE_TEXT)
}

/// Build a buffer for a newly opened file.
pub(crate) fn new_buffer(
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

/// What stands to be said about a save, in words.
///
/// A conflict is *not* an error to shrug off: the agent edits files by design,
/// so this is the expected way a save fails and the user has to be told which
/// way it went.
pub(crate) fn standing_line(standing: &Standing) -> String {
    match standing {
        // No on-disk time means the file is *gone*, not merely different --
        // deleted or renamed under the buffer. Saying "changed on disk" there
        // sends the user looking for a diff that does not exist, and the second
        // press recreates the file rather than overwriting one.
        Standing::Gone { label } => format!(
            "{label} no longer exists on disk — nothing was written. Save again to recreate it."
        ),
        Standing::Changed { label } => {
            format!("{label} changed on disk — nothing was written. Save again to overwrite.")
        }
    }
}

#[cfg(test)]
mod tests;
