//! Editor mode: a quick file editor, not an IDE.
//!
//! The tab set, the size bound and the mtime guard are core's
//! (`onehand_core::editor`); what lives here is the `EditorState` buffer per
//! tab and the drawing. Highlighting comes from gpui-component's tree-sitter
//! feature over a deliberately small grammar set: highlighting is the whole
//! ambition here, and there is no language server behind it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Entity, IntoElement, ParentElement, Rems, SharedString, Styled, Window, div,
    rems,
};
use gpui_component::input::{Editor, EditorState};
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_core::editor::{EditorFile, RootEditors, Standing};
use onehand_ui::Named as _;
use onehand_ui::{TAB_MENU_CAP, menu_below, menu_item, menu_row};
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

/// What a press on the bar does, each handed a file's index where it has one.
pub(crate) struct StripHandlers {
    pub(crate) toggle_tree: OnPress,
    pub(crate) back: OnPress,
    pub(crate) select: OnTab,
    pub(crate) close: OnTab,
    pub(crate) close_all: OnShared,
}

pub(crate) type OnPress = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;
/// Shared, because the menu holding it is built again each time it opens.
pub(crate) type OnShared = std::rc::Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;
/// Shared, because every menu row's closure holds one.
pub(crate) type OnTab = std::rc::Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// The tree's control on the bar: the toggle at its end, or the way back at its start.
pub(crate) enum Lead {
    /// The toggle that hides the tree beside the buffers, or brings it back:
    /// whether it is showing. It stands at the bar's end, by the tree.
    Toggle(bool),
    /// The way back to the tree, while the two are shown one at a time.
    Back,
}

/// One row of the open files' menu.
#[derive(Clone, Debug, PartialEq)]
struct FileRow {
    index: usize,
    label: SharedString,
    dirty: bool,
    chosen: bool,
}

/// The open files' menu: the first [`TAB_MENU_CAP`] in the order they were
/// opened, the current one chosen, and how many it leaves unlisted.
fn file_rows(files: &[EditorFile], active: usize) -> (Vec<FileRow>, usize) {
    let rows = files
        .iter()
        .enumerate()
        .take(TAB_MENU_CAP)
        .map(|(index, file)| FileRow {
            index,
            label: file.label.clone().into(),
            dirty: file.dirty,
            chosen: index == active,
        })
        .collect();
    (rows, files.len().saturating_sub(TAB_MENU_CAP))
}

/// The Editor's bar: the way back to the tree while one shows at a time, the
/// file open now as one control whose menu lists every open file and closes
/// them all, its cross, and the tree's toggle at the end, beside the tree.
///
/// One file and never a tab per file: the names never compete for the width,
/// so nothing is measured and nothing scrolls.
pub(crate) fn file_bar(
    root: &Path,
    tabs: &RootEditors,
    lead: Lead,
    on: StripHandlers,
    cx: &App,
) -> gpui::AnyElement {
    let StripHandlers {
        toggle_tree: on_toggle_tree,
        back: on_back,
        select: on_select,
        close: on_close,
        close_all: on_close_all,
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

    let files = tabs.files();
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
        let (rows, unlisted) = file_rows(files, active);
        // Keyed by the project and the count, so a menu left open over a bar
        // that changed under it is dropped rather than kept with rows aimed
        // at files that moved.
        let menu_id = SharedString::from(format!(
            "editor-file-menu-{}-{}",
            root.display(),
            files.len()
        ));
        let (select, close_all) = (on_select.clone(), on_close_all.clone());
        let trigger =
            onehand_ui::tab_select("editor-file-select", file.label.clone().into(), hint, cx)
                .min_w_0()
                // The dirty dot, not a modified-name convention: the label
                // truncates, and a marker inside it would go first.
                .when(file.dirty, |select| select.child(dirty_dot(cx)));
        let menu = menu_below(menu_id, trigger, move |mut menu, _, _| {
            for row in rows.iter().cloned() {
                let (select, index) = (select.clone(), row.index);
                menu = menu.item(
                    menu_row(move |_, cx| {
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .child(row.label.clone())
                            .when(row.dirty, |line| line.child(dirty_dot(cx)))
                    })
                    .checked(row.chosen)
                    .on_click(move |_, window, cx: &mut App| select(&index, window, cx)),
                );
            }
            if unlisted > 0 {
                menu = menu.item(onehand_ui::menu_label(format!(
                    "{unlisted} more files not listed"
                )));
            }
            let close_all = close_all.clone();
            menu.separator().item(
                menu_item("Close all")
                    .on_click(move |event, window, cx| close_all(event, window, cx)),
            )
        });
        div()
            .h_flex()
            .items_center()
            .min_w_0()
            .gap_1()
            .child(menu)
            .child(close_button(active, on_close.clone(), cx))
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

/// The cross that closes file `i`.
fn close_button(i: usize, close: OnTab, cx: &App) -> gpui_component::button::Button {
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
mod tests {
    use super::*;

    fn file(i: usize, dirty: bool) -> EditorFile {
        EditorFile {
            path: format!("/p/f{i}.rs").into(),
            label: format!("f{i}.rs"),
            dirty,
            disk_mtime: None,
            uid: i as u64,
        }
    }

    #[test]
    fn the_menu_lists_every_file_and_marks_the_current_one() {
        let files = vec![file(0, false), file(1, true), file(2, false)];
        let (rows, unlisted) = file_rows(&files, 1);
        assert_eq!(unlisted, 0);
        assert_eq!(rows.iter().map(|r| r.index).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(rows.iter().filter(|r| r.chosen).count(), 1);
        assert!(rows[1].chosen && rows[1].dirty);
        assert_eq!(rows[2].label, "f2.rs");
        assert!(file_rows(&[], 0).0.is_empty());
    }

    #[test]
    fn the_menu_stops_at_its_cap_and_counts_the_rest() {
        let files: Vec<_> = (0..TAB_MENU_CAP + 4).map(|i| file(i, false)).collect();
        let (rows, unlisted) = file_rows(&files, 0);
        assert_eq!(rows.len(), TAB_MENU_CAP);
        assert_eq!(unlisted, 4);
    }
}
