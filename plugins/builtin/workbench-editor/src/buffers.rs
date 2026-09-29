//! Editor mode: a quick file editor, not an IDE.
//!
//! The tab set, the size bound and the mtime guard are core's
//! (`onehand_core::editor`); what lives here is the `EditorState` buffer per
//! tab and the drawing. Highlighting comes from gpui-component's tree-sitter
//! feature over a deliberately small grammar set: highlighting is the whole
//! ambition here, and there is no language server behind it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, Entity, InteractiveElement, IntoElement, ParentElement, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::input::{Editor, EditorState};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use onehand_core::editor::{RootEditors, SaveOutcome};
use onehand_plugin_host::status_ink;
use std::collections::HashMap;
use std::path::Path;

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
    pub(crate) select: OnTab,
    pub(crate) close: OnTab,
    pub(crate) close_all: OnPress,
}

pub(crate) type OnPress = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>;
/// Shared, because every tab's closure holds one.
pub(crate) type OnTab = std::rc::Rc<dyn Fn(&usize, &mut Window, &mut App)>;

/// The file-tab strip: the tabs in a box of their own that scrolls, and a
/// trailing close-all that stays put.
///
/// **The tabs' box is the only part of the row that gives way.** The close-all
/// used to sit inside the scrolling box, so enough open files pushed it past the
/// panel's edge — the one control wanted precisely when there are too many tabs
/// was the one the tabs took away. `flex_1` + `min_w_0` on the box and
/// `flex_none` on the control is what keeps it on screen at any count.
pub(crate) fn tab_strip(
    root: &Path,
    buffers: &RootBuffers,
    scroll: &ScrollHandle,
    tree_shown: bool,
    on: StripHandlers,
    cx: &App,
) -> gpui::AnyElement {
    let active = buffers.tabs.active;
    let StripHandlers {
        toggle_tree: on_toggle_tree,
        select: on_select,
        close: on_close,
        close_all: on_close_all,
    } = on;

    let (offset, max) = (scroll.offset().x, scroll.max_offset().x);
    let (before, after) = (offset < px(0.), offset > -max);

    let tab_list = div()
        .id("editor-tabs")
        .track_scroll(scroll)
        .h_flex()
        .items_center()
        .gap_1()
        .w_full()
        .overflow_x_scroll()
        .children(buffers.tabs.files.iter().enumerate().map(|(i, file)| {
            let (select, close) = (on_select.clone(), on_close.clone());
            // One group per tab: a name shared by the strip would light
            // every tab's cross the moment the pointer entered any of
            // them.
            let hovered = SharedString::from(format!("editor-tab-{i}"));
            // The label is the file name alone, so three `mod.rs` tabs
            // read the same; the path relative to the project is what
            // tells them apart, and the hover is where it goes. The
            // dirty dot is named there too, since a colour is a code
            // somebody has to have learnt first.
            let rel = file.path.strip_prefix(root).unwrap_or(&file.path);
            let hint = SharedString::from(match file.dirty {
                true => format!("{} — unsaved changes", rel.display()),
                false => rel.display().to_string(),
            });
            div()
                .id(("editor-tab", i))
                .group(hovered.clone())
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .max_w(px(220.))
                .px_2()
                // A height of its own rather than padding around the line.
                // `text_xs` sets the font size alone, and gpui's default line
                // is 1.618 times that — about 1.2rem for 0.75rem glyphs — so
                // padded, the chip stood nearly twice as tall as its letters.
                // The line comes down with the box, or the label's own line
                // would hold the tab open; the font size does not move.
                .h(rems(1.125))
                .line_height(rems(1.125))
                .rounded(cx.theme().radius)
                .text_xs()
                .cursor_pointer()
                .when(i == active, |tab| {
                    tab.bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                })
                // The well, the same hover the terminal's tabs and the
                // mode strip take, so one strip does not answer the
                // pointer differently from the two beside it.
                .when(i != active, |tab| tab.hover(|tab| tab.bg(cx.theme().muted)))
                .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                .child(div().min_w_0().truncate().child(file.label.clone()))
                // The dirty dot, not a modified-name convention: the
                // label is already truncated, and a marker inside it
                // would be the first thing to disappear. Status ink, the
                // same value the file tree uses for "changed".
                .when(file.dirty, |tab| {
                    tab.child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(status_ink(cx).warning),
                    )
                })
                .on_click(move |_, window, cx: &mut App| select(&i, window, cx))
                // **Shown on hover alone**, `invisible` rather than
                // absent so a tab does not change width under the
                // pointer. `stop_propagation` is what keeps the press
                // that closes a tab from also selecting whatever slid
                // into its place — which switched the file on screen
                // when a background tab was closed.
                //
                // `size_4`: `xsmall` gives an icon button a 1.25rem box, taller
                // than the tab's own height, and a child that tall would hold
                // the tab open whatever the tab asks for. The library applies a
                // caller's style after its size preset, so this takes the box
                // down while the glyph keeps its `xsmall` size.
                .child(
                    onehand_plugin_host::action(("editor-tab-close", i))
                        .ghost()
                        .xsmall()
                        .size_4()
                        .icon(Icon::new(IconName::Close))
                        .invisible()
                        .group_hover(hovered, |style| style.visible())
                        .on_click(move |_, window, cx: &mut App| {
                            cx.stop_propagation();
                            close(&i, window, cx);
                        }),
                )
        }));

    div()
        .h_flex()
        .items_center()
        .gap_1()
        .w_full()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
        // The tree's toggle, at the end nearest the tree. The icon is the
        // panel's *state*, open or shut, and the tooltip says what a press
        // does -- so the two never disagree about which way round it is.
        .child(
            onehand_plugin_host::action("toggle-file-tree")
                .ghost()
                .xsmall()
                .flex_none()
                .text_color(cx.theme().muted_foreground)
                .icon(Icon::new(match tree_shown {
                    true => IconName::PanelLeftClose,
                    false => IconName::PanelLeftOpen,
                }))
                .tooltip(match tree_shown {
                    true => "Hide the file tree",
                    false => "Show the file tree",
                })
                .on_click(on_toggle_tree),
        )
        .child(
            // **Each end fades while there is more past it**, into the surface
            // the strip sits on. A hard clip cuts a tab mid-letter, which reads
            // as a label that was drawn wrong rather than as a row that goes on;
            // a fade says the row goes on. Shown only on a side with something
            // scrolled out, so a strip that fits looks exactly as it did. Read
            // off the handle, which holds the last frame's layout — a wheel
            // notifies the view, so the next frame has the new offset.
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .child(tab_list)
                .when(before, |list| list.child(fade(Side::Start, cx)))
                .when(after, |list| list.child(fade(Side::End, cx))),
        )
        // Muted like the other strips' controls: a ghost button in full ink is
        // the brightest thing on the row, out-shouting the file names beside it.
        // Its tooltip is what tells it from a tab's own cross, which is the
        // same glyph.
        .when(!buffers.tabs.files.is_empty(), |strip| {
            strip.child(
                onehand_plugin_host::action("close-all-files")
                    .ghost()
                    .xsmall()
                    .flex_none()
                    .text_color(cx.theme().muted_foreground)
                    .icon(Icon::new(IconName::Close))
                    .tooltip("Close all files")
                    .on_click(on_close_all),
            )
        })
        .into_any_element()
}

/// Which end of the tab list a fade sits on.
#[derive(Debug, Clone, Copy)]
enum Side {
    Start,
    End,
}

/// A band at one end of the tab list, from the dock's surface to nothing.
///
/// Carries no handler, so it takes no hitbox: a tab under it still answers the
/// pointer, its hover and its tooltip included.
fn fade(side: Side, cx: &App) -> gpui::Div {
    let surface = onehand_plugin_host::dock_surface(cx);
    let (from, to) = match side {
        Side::Start => (surface, surface.alpha(0.)),
        Side::End => (surface.alpha(0.), surface),
    };
    let band = div()
        .absolute()
        .top_0()
        .bottom_0()
        .w(rems(1.5))
        .bg(gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(from, 0.),
            gpui::linear_color_stop(to, 1.),
        ));
    match side {
        Side::Start => band.left_0(),
        Side::End => band.right_0(),
    }
}

/// The editor body for the active tab.
///
/// `appearance(false)`: by default the component draws itself as a form field —
/// its own fill, border and radius — which inside the Workbench card is a
/// second, smaller rounded box nested in the first. The card is the frame; the
/// code sits on its surface the way the file tree beside it does.
pub(crate) fn body(state: &Entity<EditorState>) -> impl IntoElement + use<> {
    Editor::new(state).appearance(false).h_full()
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
        SaveOutcome::Failed(e) => Some(format!("{label} not saved — {e}")),
    }
}
