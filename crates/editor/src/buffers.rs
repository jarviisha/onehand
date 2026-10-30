//! Editor mode: a quick file editor, not an IDE.
//!
//! The tab set, the size bound and the mtime guard are core's
//! (`onehand_core::editor`); what lives here is the `EditorState` buffer per
//! tab and the drawing. Highlighting comes from gpui-component's tree-sitter
//! feature over a deliberately small grammar set: highlighting is the whole
//! ambition here, and there is no language server behind it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Entity, IntoElement, ParentElement, Rems, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, rems,
};
use gpui_component::input::{Editor, EditorState};
use gpui_component::{ActiveTheme, IconName, StyledExt};
use onehand_core::editor::{RootEditors, SaveOutcome};
use onehand_ui::Named as _;
use onehand_ui::{
    Measured, TAB_MAX_W, TAB_MENU_CAP, TabStrip, TabWidths, measure_tab, menu_below, menu_row,
    tab_strip as tab_strip_rule, tab_width,
};
use std::collections::HashMap;
use std::path::Path;

/// Tabs stand flush, as a tab panel's do, each parted from the next by its
/// own line.
const TAB_GAP: Rems = rems(0.);
/// The unsaved-edits mark on a tab.
const DIRTY_DOT: Rems = rems(0.375);

/// Editor state for one project root: the tab set from core, plus this front
/// end's buffers.
#[derive(Default)]
pub(crate) struct RootBuffers {
    pub(crate) tabs: RootEditors,
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
    pub(crate) fn buffer(&self, uid: u64) -> Option<&Entity<EditorState>> {
        self.buffers.get(&uid)
    }

    pub(crate) fn insert(
        &mut self,
        uid: u64,
        state: Entity<EditorState>,
        watch: gpui::Subscription,
    ) {
        self.buffers.insert(uid, state);
        self.watches.insert(uid, watch);
    }

    pub(crate) fn forget(&mut self, uid: u64) {
        self.buffers.remove(&uid);
        self.watches.remove(&uid);
    }

    /// How many open tabs have edits a close would throw away.
    pub(crate) fn dirty_count(&self) -> usize {
        self.tabs.files.iter().filter(|f| f.dirty).count()
    }
}

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

/// What a press on the strip does, each handed a tab's index where it has one.
pub(crate) struct StripHandlers {
    pub(crate) toggle_tree: OnPress,
    pub(crate) back: OnPress,
    pub(crate) select: OnTab,
    pub(crate) close: OnTab,
    pub(crate) close_all: OnPress,
}

pub(crate) type OnPress = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;
/// Shared, because every tab's closure holds one.
pub(crate) type OnTab = std::rc::Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// What leads the strip, at the end nearest the tree.
pub(crate) enum Lead {
    /// The toggle that hides the tree beside the buffers, or brings it back:
    /// whether it is showing.
    Toggle(bool),
    /// The way back to the tree, while the two are shown one at a time.
    Back,
}

/// The file-tab strip: the tree's toggle or the way back to it, the tabs, and a
/// trailing close-all.
///
/// The tabs fit the width actually left to them, `measured` last frame (in
/// rems) by the box they sit in, each at the width it was last drawn at
/// (`widths`); either asks `view` to draw again when it changes. A tab is never
/// squeezed: those with no room, the oldest, go into an *N more* menu after the
/// rest. Nothing scrolls, so the active file's tab is always in reach and the
/// controls at the ends never move.
pub(crate) fn tab_strip<T: 'static>(
    root: &Path,
    buffers: &RootBuffers,
    (measured, widths): (&Measured, &TabWidths),
    view: WeakEntity<T>,
    lead: Lead,
    on: StripHandlers,
    cx: &App,
) -> gpui::AnyElement {
    let active = buffers.tabs.active;
    let StripHandlers {
        toggle_tree: on_toggle_tree,
        back: on_back,
        select: on_select,
        close: on_close,
        close_all: on_close_all,
    } = on;

    // At the end nearest the tree. The toggle's icon is the panel's *state*,
    // open or shut, and the tooltip says what a press does -- so the two never
    // disagree about which way round it is.
    let lead = match lead {
        Lead::Back => onehand_ui::back_link("editor-back", "Files", cx)
            .on_click(on_back)
            .into_any_element(),
        Lead::Toggle(shown) => {
            let (icon, hint) = match shown {
                true => (IconName::PanelLeftClose, "Hide the file tree"),
                false => (IconName::PanelLeftOpen, "Show the file tree"),
            };
            onehand_ui::strip_control("toggle-file-tree", icon, false, cx)
                .named(hint)
                .on_click(on_toggle_tree)
                .into_any_element()
        }
    };
    // The label is the file name alone, so three `mod.rs` tabs read the same;
    // the path relative to the project tells them apart, on hover. The dirty
    // dot is named there too, since a colour is a code somebody has to learn.
    let hint_of = |file: &onehand_core::editor::EditorFile| {
        let rel = file.path.strip_prefix(root).unwrap_or(&file.path);
        SharedString::from(match file.dirty {
            true => format!("{} — unsaved changes", rel.display()),
            false => rel.display().to_string(),
        })
    };
    let files = &buffers.tabs.files;

    // A tab's width follows what it shows: its name, and the dot while it
    // holds unsaved edits.
    let key = |file: &onehand_core::editor::EditorFile| {
        SharedString::from(format!(
            "{}{}",
            file.label,
            if file.dirty { " *" } else { "" }
        ))
    };
    let each: Vec<_> = files.iter().map(|f| tab_width(widths, &key(f))).collect();
    let TabStrip { shown, left_out } = tab_strip_rule(&each, active, measured.get(), TAB_GAP);
    let tabs = shown.into_iter().map(|i| {
        let file = &files[i];
        let (select, close) = (on_select.clone(), on_close.clone());
        let measure = measure_tab(widths.clone(), key(file), view.clone());
        onehand_ui::panel_tab(
            ("editor-tab", i),
            file.label.clone().into(),
            hint_of(file),
            i == active,
            cx,
        )
        .max_w(TAB_MAX_W)
        .relative()
        .child(measure)
        // The dirty dot, not a modified-name convention: the label
        // truncates, and a marker inside it would go first.
        .when(file.dirty, |tab| tab.child(dirty_dot(cx)))
        .on_click(move |_, window, cx: &mut App| select(&i, window, cx))
        .child(close_button(i, close, cx))
        .into_any_element()
    });
    // The older files the strip had no room for, in one menu after the tabs.
    let more = (!left_out.is_empty()).then(|| {
        let rows: Vec<(usize, SharedString, bool)> = left_out
            .iter()
            .map(|&i| {
                (
                    i,
                    SharedString::from(files[i].label.clone()),
                    files[i].dirty,
                )
            })
            .collect();
        // Keyed by the project and the count, so a menu left open over a
        // strip that changed under it is dropped rather than kept with rows
        // aimed at tabs that moved.
        let menu_id = SharedString::from(format!(
            "editor-tab-menu-{}-{}",
            root.display(),
            files.len()
        ));
        let select = on_select.clone();
        menu_below(
            menu_id,
            onehand_ui::tab_more("editor-tab-more", rows.len(), cx),
            move |mut menu, _, _| {
                for (i, label, dirty) in rows.iter().take(TAB_MENU_CAP).cloned() {
                    let select = select.clone();
                    menu = menu.item(
                        menu_row(move |_, cx| {
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .child(label.clone())
                                .when(dirty, |row| row.child(dirty_dot(cx)))
                        })
                        .on_click(move |_, window, cx: &mut App| select(&i, window, cx)),
                    );
                }
                let unlisted = rows.len().saturating_sub(TAB_MENU_CAP);
                if unlisted > 0 {
                    menu = menu.item(onehand_ui::menu_label(format!(
                        "{unlisted} more files not listed"
                    )));
                }
                menu
            },
        )
    });
    let tab_list = div()
        .h_full()
        .h_flex()
        .children(tabs)
        .children(more.map(|more| div().h_flex().items_center().px_1().child(more)));

    // The library's tab panel bar, a mode's bar a step lower than the
    // Workbench's over it, its line under it where the code begins.
    onehand_ui::tab_bar(cx)
        .h(onehand_ui::tokens::MODE_BAR_H)
        .child(div().h_flex().items_center().px_1().child(lead))
        // The one part of the row that gives way, and the box the tabs are
        // laid out to: `flex_1` + `min_w_0`, so its width is what is left
        // after the controls, never its content.
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(onehand_ui::measure_width(measured.clone(), view))
                .child(tab_list),
        )
        // Its tooltip is what tells it from a tab's own cross, which is the
        // same glyph.
        .when(!files.is_empty(), |strip| {
            strip.child(
                div().h_flex().items_center().child(
                    onehand_ui::strip_control("close-all-files", IconName::Close, false, cx)
                        .named("Close all files")
                        .on_click(on_close_all),
                ),
            )
        })
        .into_any_element()
}

/// The cross that closes tab `i`. `stop_propagation` keeps the press that
/// closes a tab from also selecting whatever slid into its place.
fn close_button(i: usize, close: OnTab, cx: &App) -> gpui_component::button::Button {
    onehand_ui::tab_close(("editor-tab-close", i), "Close this file", cx).on_click(
        move |_, window, cx: &mut App| {
            cx.stop_propagation();
            close(&i, window, cx);
        },
    )
}

/// A tab's unsaved-edits mark: a status *fill*, the theme's `warning`, where
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
pub(crate) fn body(state: &Entity<EditorState>) -> impl IntoElement + use<> {
    Editor::new(state)
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

/// Report a save. Returns the status line to surface, if any.
///
/// A conflict is *not* an error to shrug off: the agent edits files by design,
/// so this is the expected way a save fails and the user has to be told which
/// way it went.
pub(crate) fn save_status(outcome: &SaveOutcome, label: &str) -> Option<String> {
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
        // A failure is a toast, said once by the caller: a conflict stands
        // until a save answers it, and a write that failed does not.
        SaveOutcome::Failed(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RootBuffers;
    use std::path::PathBuf;

    #[test]
    fn dirty_tabs_are_counted() {
        let mut buffers = RootBuffers::default();
        assert_eq!(buffers.dirty_count(), 0);
        buffers.tabs.open(PathBuf::from("a.rs"), None, 1);
        buffers.tabs.open(PathBuf::from("b.rs"), None, 2);
        buffers.tabs.open(PathBuf::from("c.rs"), None, 3);
        buffers.tabs.files[0].dirty = true;
        buffers.tabs.files[2].dirty = true;
        assert_eq!(buffers.dirty_count(), 2);
    }
}
