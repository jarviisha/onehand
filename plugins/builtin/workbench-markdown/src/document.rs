//! Markdown mode: the project's documents on the left, the one being read on
//! the right.
//!
//! Read-only by design. Editing a file is what the quick editor is for, and a
//! second buffer here would be a second copy of its whole tab set, mtime guard
//! and unsaved-edit rules; the header's *Edit source* hands the document over
//! to it instead. What this mode owns is the opposite half — a rendered
//! document that **re-reads itself when the file changes on disk**, because in
//! this app the writer is usually the agent rather than the person watching.

use crate::index::{DocIndex, DocRow};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Entity, InteractiveElement, IntoElement, ParentElement, Rems,
    StatefulInteractiveElement, Styled, Window, div, relative, rems,
};
use gpui_component::button::ButtonVariants as _;
use gpui_component::text::{TextView, TextViewState, TextViewStyle};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The widest a document's column runs: a comfortable line of prose, so a
/// wide dock does not stretch a paragraph into lines too long to follow.
const DOC_MEASURE: Rems = rems(34.);
/// A document's line height as a multiple of its size: a shade more open than
/// the conversation's, because a document is read in long stretches.
const LEADING_DOC: f32 = 1.65;

/// A row of the tree: a little over a control's height, so a long list of
/// names reads as rows with air between them rather than a block of text.
const ROW_H: gpui::Rems = gpui::rems(1.875);
/// Where a row's content starts, and how far each level of the tree steps in.
const ROW_INSET: f32 = 0.5;
const TREE_INDENT: f32 = 0.875;

/// One project root's document view: what was found, what is folded away, and
/// what is being read.
///
/// Per root like everything else in this panel, so switching projects swaps the
/// list and the document together rather than leaving one project's reading
/// open over another project's index.
#[derive(Default)]
pub(crate) struct RootDocs {
    /// `None` until the first scan lands, which is what tells the empty list
    /// apart from the one still being walked.
    pub(crate) index: Option<DocIndex>,
    /// Directories folded away, the closed half rather than the open one (the
    /// reason is with [`DocIndex::rows`]).
    pub(crate) folded: HashSet<PathBuf>,
    pub(crate) open: Option<OpenDoc>,
}

/// The document on screen.
pub(crate) struct OpenDoc {
    pub(crate) path: PathBuf,
    /// The path relative to the root, which is what the header prints: a
    /// project's documents are mostly the same handful of names once per
    /// folder, so the name alone does not say which one is being read.
    pub(crate) label: String,
    /// The file's mtime as of the last read.
    ///
    /// This is the whole of the live reload: one `stat` against this says
    /// whether the file moved under the reader, and nothing is re-read or
    /// re-parsed while it has not.
    pub(crate) mtime: Option<SystemTime>,
    state: Entity<TextViewState>,
}

impl RootDocs {
    /// Fold or unfold `dir`.
    pub(crate) fn toggle(&mut self, dir: &Path) {
        if !self.folded.remove(dir) {
            self.folded.insert(dir.to_path_buf());
        }
    }

    /// Put a freshly read document on screen.
    pub(crate) fn show(
        &mut self,
        path: PathBuf,
        label: String,
        text: &str,
        mtime: Option<SystemTime>,
        cx: &mut App,
    ) {
        let state = cx.new(|cx| TextViewState::markdown(text, cx));
        self.open = Some(OpenDoc {
            path,
            label,
            mtime,
            state,
        });
    }

    /// Re-set the open document's text after the file changed underneath it.
    ///
    /// The same state entity is kept rather than a new one built: rebuilding it
    /// would put the reader back at the top of the document every time the
    /// agent touched the file, which for a file being written repeatedly is a
    /// document that cannot be read at all.
    pub(crate) fn refresh(
        &mut self,
        path: &Path,
        text: &str,
        mtime: Option<SystemTime>,
        cx: &mut App,
    ) {
        let Some(doc) = self.open.as_mut() else {
            return;
        };
        // The read was spawned against whatever was open when it started, and a
        // click can land while it is in flight.
        if doc.path != path {
            return;
        }
        doc.mtime = mtime;
        doc.state.update(cx, |state, cx| state.set_text(text, cx));
    }

    /// Record that the file moved without its new contents being readable, and
    /// say whether it is still the document being read.
    ///
    /// What the stamp buys is that a document deleted or grown past the read's
    /// size bound is complained about once. Without it the mtime on record stays
    /// the one that was read, so every check afterwards sees a change and
    /// reports the same failure again for as long as the document is open.
    ///
    /// The answer matters as much as the stamp: a failed read that landed after
    /// the reader moved on belongs to a document nobody is looking at, and a
    /// caller that complained about it anyway would leave a standing warning
    /// naming a file that is no longer on screen.
    pub(crate) fn stamp(&mut self, path: &Path, mtime: Option<SystemTime>) -> bool {
        match self.open.as_mut() {
            Some(doc) if doc.path == path => {
                doc.mtime = mtime;
                true
            }
            _ => false,
        }
    }

    /// The document on screen, for a row to draw itself as the selected one.
    pub(crate) fn showing(&self) -> Option<&Path> {
        self.open.as_ref().map(|doc| doc.path.as_path())
    }
}

/// The document list.
pub(crate) fn list(
    root: &Path,
    docs: &RootDocs,
    on_toggle: impl Fn(&PathBuf, &mut Window, &mut App) + 'static,
    on_open: impl Fn(&PathBuf, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    let Some(index) = docs.index.as_ref() else {
        return note("Looking for documents…", cx);
    };
    if index.docs.is_empty() {
        return note("No markdown in this project", cx);
    }

    let rows = index.rows(root, &docs.folded);
    let showing = docs.showing().map(|p| p.to_path_buf());
    let on_toggle = std::rc::Rc::new(on_toggle);
    let on_open = std::rc::Rc::new(on_open);

    div()
        .id("markdown-list")
        .v_flex()
        .size_full()
        .p_2()
        .gap_0p5()
        .overflow_y_scroll()
        .children(
            rows.into_iter()
                .enumerate()
                .map(|(i, row)| {
                    doc_row(
                        i,
                        row,
                        &docs.folded,
                        showing.as_deref(),
                        on_toggle.clone(),
                        on_open.clone(),
                        cx,
                    )
                })
                .collect::<Vec<_>>(),
        )
        .when(index.truncated, |list| {
            // The cap is the index's, and a list that silently stops is one the
            // reader believes they have seen the whole of.
            list.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("… more documents not shown"),
            )
        })
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn doc_row(
    i: usize,
    row: DocRow,
    folded: &HashSet<PathBuf>,
    showing: Option<&Path>,
    on_toggle: std::rc::Rc<impl Fn(&PathBuf, &mut Window, &mut App) + 'static>,
    on_open: std::rc::Rc<impl Fn(&PathBuf, &mut Window, &mut App) + 'static>,
    cx: &App,
) -> gpui::AnyElement {
    let selected = !row.is_dir && showing == Some(row.path.as_path());
    let shut = folded.contains(&row.path);
    let path = row.path.clone();
    let is_dir = row.is_dir;

    div()
        .id(("markdown-row", i))
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .h(ROW_H)
        .px_1()
        .rounded(cx.theme().radius)
        .text_sm()
        .cursor_pointer()
        .when(selected, |row| row.bg(cx.theme().accent))
        .when(!selected, |row| {
            row.hover(|row| row.bg(cx.theme().list_hover))
        })
        .tooltip({
            let name = row.name.clone();
            move |window, cx| Tooltip::new(name.clone()).build(window, cx)
        })
        // Indent by depth rather than by nested containers, for the same reason
        // the file tree does: the cap here is 400 documents, and that many
        // nested elements is that many wasted.
        .pl(rems(ROW_INSET + row.depth as f32 * TREE_INDENT))
        .child(
            Icon::new(if is_dir {
                if shut {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                }
            } else {
                IconName::File
            })
            .size_3(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                // A directory here is a heading over what it holds, not a place
                // to go: the documents are the point of the list, so the
                // folders stay quieter than they do in the file tree.
                .when(is_dir, |name| name.text_color(cx.theme().muted_foreground))
                .child(row.name),
        )
        .on_click(move |_, window, cx: &mut App| {
            if is_dir {
                on_toggle(&path, window, cx);
            } else {
                on_open(&path, window, cx);
            }
        })
        .into_any_element()
}

/// The reading side: the document, under a header that is always drawn,
/// led by `lead`: the toggle that hides the list beside it, or the way back to
/// the list while the two show one at a time.
///
/// **Always**, and that is what makes the list hideable at all. The control
/// that brings the list back lives in this header, so a header that appeared
/// only once a document was open would let somebody hide the list with nothing
/// open and be left facing a panel with no way back to either.
///
/// `rem` is the base the headings are scaled off. Asked for rather than read
/// from the window here, although the caller reads it straight off the window:
/// what this needs is the base *in force*, and a function that went and got it
/// itself would be right only for as long as every caller happened to draw
/// inside the same override. Handed one, the caller has to say which it means.
pub(crate) fn reader(
    doc: Option<&OpenDoc>,
    lead: gpui::AnyElement,
    rem: gpui::Pixels,
    on_edit: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_w_0()
        // The column takes the panel's whole height on its own account rather
        // than leaving it to the row: the body below is sized from what the
        // header leaves over, and a column as tall as its content has nothing
        // to leave.
        .h_full()
        .v_flex()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .flex_none()
                .px_3()
                .py_1p5()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(lead)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(match doc {
                            Some(doc) => doc.label.clone(),
                            None => String::new(),
                        }),
                )
                // Read-only is the rule, so the way out of it is said rather
                // than left to be discovered: this is the one control that
                // turns a document being read into a file being changed. Only
                // where there is a document for it to act on.
                .children(doc.map(|_| {
                    onehand_plugin_host::action("markdown-edit")
                        .xsmall()
                        .ghost()
                        .label("Edit source")
                        .tooltip("Open this file in the editor")
                        .on_click(on_edit)
                })),
        )
        .child(match doc {
            // The column at the app's reading size, a step over its chrome,
            // since a document is read the way the conversation is.
            Some(doc) => div()
                .flex_1()
                .min_h_0()
                .px_4()
                .py_4()
                .child(
                    div()
                        .h_full()
                        .max_w(DOC_MEASURE)
                        .text_base()
                        .line_height(relative(LEADING_DOC))
                        .child(
                            TextView::new(&doc.state)
                                .selectable(true)
                                // Virtualized: a long document draws the rows
                                // on screen rather than all of it.
                                .scrollable(true)
                                .style(doc_style(rem, cx)),
                        ),
                )
                .into_any_element(),
            None => div()
                .flex_1()
                .min_h_0()
                .child(note("Pick a document to read it", cx))
                .into_any_element(),
        })
        .into_any_element()
}

/// Document styling.
///
/// The renderer's own defaults are a document's, which is what is wanted here —
/// unlike in the transcript, where the same renderer is drawing one message and
/// its headings have to be pulled back down. Two things still have to be said.
/// The heading base is an absolute pixel value in the renderer, which is exactly
/// what a rem-base override cannot reach, so the panel's zoom has to be written
/// into it by hand — that is what `rem` is, and why it is a parameter rather
/// than a reading off the window. And the code block's text size is in rems,
/// which the override *does* reach.
fn doc_style(rem: gpui::Pixels, cx: &App) -> TextViewStyle {
    let mut style = TextViewStyle::default().code_block(
        gpui::StyleRefinement::default()
            .p(rems(0.75))
            .text_size(rems(0.8125))
            // **The fill is named here rather than left to the library**, and
            // it is the step past the well: the dock this document is read on
            // sits so near the well that a block filled with it showed only its
            // padding. Written out, a panel that changes surface again is one
            // edit in one place rather than a block that quietly disappears.
            .bg(cx.theme().secondary),
    );
    style.heading_base_font_size = rem;
    style
}

/// A line where the list would be, for the two states that are not a list.
fn note(text: &'static str, cx: &App) -> gpui::AnyElement {
    div()
        .size_full()
        .v_flex()
        .items_center()
        .justify_center()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
