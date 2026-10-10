use super::popup::{edge_scrolled, more_text, popup_header, popup_surface};
use super::{CHIP_H, Composer, ComposerEvent, Overlay};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, Rems, StatefulInteractiveElement,
    Styled, Window, div, rems,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme, Icon, IconName, Selectable as _, Sizable as _, StyledExt};
use onehand_core::attachment::{
    AttachmentDelivery, AttachmentKind, AttachmentSource, StagedAttachment,
};

/// How much of an attachment's name is shown before it truncates.
const ATTACHMENT_MAX_W: Rems = rems(10.);
/// How far the cut end of the tray fades out over the chips it hides.
const TRAY_FADE: Rems = rems(3.);
/// Attachment chips drawn before the tray starts counting instead.
const MAX_TRAY_CHIPS: usize = 12;
/// Rows built at once in the expanded attachment manager. Removing a visible
/// row reveals the next one, so every item remains manageable without laying
/// out a dropped directory's entire contents on each frame.
const MAX_ATTACHMENT_MANAGER_ROWS: usize = 100;

impl Composer {
    /// Stage paths that arrived from somewhere other than the picker.
    pub(super) fn stage(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        source: AttachmentSource,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        self.attachments.extend(
            paths
                .into_iter()
                .map(|path| StagedAttachment::inspect(path, source)),
        );
        cx.notify();
    }

    /// What `Ctrl+V` does in the composer.
    ///
    /// The clipboard holds *entries*, and only one kind of them is text. An
    /// image copied out of a screenshot tool and a file copied out of a file
    /// manager both arrive here, and both are things to attach rather than
    /// things to type — pasted into the buffer they produced nothing at all,
    /// which is a paste that looks broken.
    ///
    /// Anything else is the input's own business and is handed straight back to
    /// it, so ordinary text paste keeps working exactly as it did, undo history
    /// and all.
    pub(super) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mine = cx.read_from_clipboard().is_some_and(|item| {
            let mut mine = false;
            for entry in item.into_entries() {
                match entry {
                    gpui::ClipboardEntry::Image(image) => {
                        mine = true;
                        self.stage_pasted_image(image, window.window_handle(), cx);
                    }
                    gpui::ClipboardEntry::ExternalPaths(paths) => {
                        mine = true;
                        self.stage(paths.paths().to_vec(), AttachmentSource::Clipboard, cx);
                    }
                    gpui::ClipboardEntry::String(_) => {}
                }
            }
            mine
        });
        if !mine {
            window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
        }
    }

    /// Write a pasted image out and stage the file it became.
    ///
    /// The write is a real one and goes to the background executor; the id is
    /// the clipboard's own content hash, so pasting the same image twice
    /// rewrites one file instead of littering the temp directory.
    fn stage_pasted_image(
        &mut self,
        image: gpui::Image,
        window: gpui::AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |composer, cx| {
            let written = cx
                .background_executor()
                .spawn(async move {
                    onehand_core::attachment::write_clipboard_image(
                        image.id,
                        image.format.extension(),
                        &image.bytes,
                    )
                })
                .await;
            let Ok(path) = written else {
                // A toast and not a line in the card: nothing said about the
                // composer may move it.
                let _ = cx.update(|cx| {
                    window
                        .update(cx, |_, window, cx| {
                            gpui_component::WindowExt::push_notification(
                                window,
                                gpui_component::notification::Notification::warning(
                                    "Could not attach the pasted image",
                                ),
                                cx,
                            );
                        })
                        .ok()
                });
                return;
            };
            let _ = composer.update(cx, |composer: &mut Self, cx| {
                composer.stage(vec![path], AttachmentSource::Clipboard, cx);
            });
        })
        .detach();
    }

    /// Stage files through the native picker.
    ///
    /// Off the UI loop, like every other dialog in the app -- `pick_files` blocks
    /// until the user is done, which on this thread would freeze the window.
    pub(super) fn attach(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |composer, cx| {
            let picked = cx
                .background_executor()
                .spawn(async { rfd::FileDialog::new().pick_files() })
                .await;
            let Some(paths) = picked else {
                return;
            };
            let _ = composer.update(cx, |composer: &mut Self, cx| {
                composer.attachments.extend(
                    paths
                        .into_iter()
                        .map(|path| StagedAttachment::inspect(path, AttachmentSource::Picker)),
                );
                cx.notify();
            });
        })
        .detach();
    }

    fn unstage(&mut self, id: onehand_core::attachment::AttachmentId, cx: &mut Context<Self>) {
        self.attachments.retain(|a| a.id != id);
        if self.attachments.is_empty() && self.overlay == Some(Overlay::Attachments) {
            self.set_overlay(None);
        }
        cx.notify();
    }

    /// The staged files, as one row of chips.
    ///
    /// Bounded like everything else that grows with what the user did: a folder
    /// dropped on the card is however many files it held, and a tray of two
    /// hundred chips is two hundred elements laid out on every keystroke. What
    /// is over the bound is counted rather than dropped silently.
    ///
    /// **A chip names a staged file; it does not show it**, an image no more
    /// than a file. The picture is previewed once the prompt is sent, in the
    /// transcript. The tray rests above the composer card, not in it, so
    /// staging a file never moves the field being typed in.
    pub(in crate::chat) fn tray(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let (border, muted, danger_border, danger_text, radius, well) = (
            cx.theme().border,
            cx.theme().muted_foreground,
            cx.theme().danger,
            crate::theme::status_ink(cx).danger,
            cx.theme().radius,
            cx.theme().muted,
        );
        let hover = cx.theme().list_hover;
        let page = cx.theme().background;
        // **One row, never two.** Past the composer's width the row is cut,
        // the cut fades out, and *Show all* opens the list of every staged
        // file. Whether it was cut is last frame's measurement, a frame nobody
        // sees go by.
        let scroll = self.tray_scroll.clone();
        let overflowed = scroll.max_offset().x > gpui::px(0.);
        let cut = overflowed || self.attachments.len() > MAX_TRAY_CHIPS;
        // Laid out after the chips, so it reads this frame's measurement and
        // asks for one more frame only when it changed: nothing else redraws a
        // tray that has just been cut.
        let remeasure = {
            let scroll = scroll.clone();
            gpui::canvas(
                move |_, window, cx| {
                    if (scroll.max_offset().x > gpui::px(0.)) != overflowed {
                        // After this frame: a refresh asked for mid-draw is
                        // dropped.
                        window.defer(cx, |window, _| window.refresh());
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_0()
        };
        let chips = div()
            .id("attachment-tray")
            .h_flex()
            .gap_1()
            .overflow_x_scroll()
            .track_scroll(&scroll)
            .children(
                self.attachments
                    .iter()
                    .take(MAX_TRAY_CHIPS)
                    .enumerate()
                    .map(|(i, a)| {
                        let id = a.id;
                        // An unreadable file blocks Send entirely, so it has
                        // to look wrong here rather than fail silently at
                        // the moment the user hits Enter.
                        let unavailable = a.delivery == AttachmentDelivery::Unavailable;
                        let parts = [
                            Icon::new(match a.kind {
                                AttachmentKind::Image => IconName::Frame,
                                AttachmentKind::File => IconName::File,
                            })
                            .xsmall()
                            .text_color(muted)
                            .into_any_element(),
                            div()
                                .max_w(ATTACHMENT_MAX_W)
                                .truncate()
                                .text_color(cx.theme().foreground)
                                .when(unavailable, |el| el.text_color(danger_text))
                                .child(a.name.clone())
                                .into_any_element(),
                        ];
                        // The size, because two screenshots taken a minute
                        // apart have interchangeable names, and because it
                        // is the only warning that a large image will go as
                        // a link instead of inline.
                        let size = a.bytes.map(|bytes| {
                            div()
                                .flex_none()
                                .text_color(muted)
                                .child(onehand_core::attachment::size_label(bytes))
                                .into_any_element()
                        });
                        // A real button, not a bare glyph: this one is
                        // small, sits beside the name it destroys, and
                        // needs the hover and the focus ring that say which
                        // of the two the pointer is on.
                        //
                        // It is one clickable inside another wherever the
                        // chip itself opens, which is what the stop is for:
                        // without it the press that unstages a file also
                        // asks the Workbench to open the file just removed.
                        let unstage = crate::controls::action(("unstage", i))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Close))
                            .tooltip("Remove this attachment")
                            .on_click(cx.listener(move |composer: &mut Self, _, _, cx| {
                                cx.stop_propagation();
                                composer.unstage(id, cx);
                            }))
                            .into_any_element();

                        // **One chip for a file and an image alike**, the
                        // same element either way: only a file that opens
                        // adds the pointer, a hover and a press.
                        let open = openable(a);
                        attachment_shape(
                            div().id(("attachment", i)),
                            unavailable,
                            (border, danger_border, well),
                            radius,
                        )
                        .children(parts)
                        .children(size)
                        .child(unstage)
                        .when_some(open, |chip, path| {
                            chip.cursor_pointer()
                                .hover(move |chip| chip.bg(hover))
                                .tooltip(|window, cx| {
                                    gpui_component::tooltip::Tooltip::new(
                                        "Open this file in the Workbench",
                                    )
                                    .build(window, cx)
                                })
                                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::OpenFile(path.clone()));
                                }))
                        })
                        .into_any_element()
                    }),
            );
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .child(chips)
                        .child(remeasure)
                        .when(cut, |row| {
                            row.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .right_0()
                                    .w(TRAY_FADE)
                                    .bg(gpui::linear_gradient(
                                        90.,
                                        gpui::linear_color_stop(page.alpha(0.), 0.),
                                        gpui::linear_color_stop(page, 1.),
                                    )),
                            )
                        }),
                )
                .when(cut, |tray| {
                    tray.child(
                        crate::controls::action("all-attachments")
                            .ghost()
                            .xsmall()
                            .flex_none()
                            .label(format!("Show all {}", self.attachments.len()))
                            .tooltip("Review or remove staged attachments")
                            .on_click(cx.listener(|composer: &mut Self, _, window, cx| {
                                composer.toggle_attachments(window, cx);
                            })),
                    )
                })
                .into_any_element(),
        )
    }

    pub(super) fn attachments_popup(
        &self,
        room: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let muted = cx.theme().muted_foreground;
        let danger = crate::theme::status_ink(cx).danger;
        let hidden = self
            .attachments
            .len()
            .saturating_sub(MAX_ATTACHMENT_MANAGER_ROWS);

        let list = div()
            .id("attachment-manager")
            .v_flex()
            .w_full()
            .min_h_0()
            .max_h(room)
            .overflow_y_scroll()
            .track_scroll(&self.attachments_scroll)
            .children(
                self.attachments
                    .iter()
                    .take(MAX_ATTACHMENT_MANAGER_ROWS)
                    .enumerate()
                    .map(|(i, attachment)| {
                        let id = attachment.id;
                        let unavailable = attachment.delivery == AttachmentDelivery::Unavailable;
                        let path = openable(attachment);
                        let detail = attachment
                            .bytes
                            .map(onehand_core::attachment::size_label)
                            .unwrap_or_else(|| "Size unavailable".to_string());
                        let name = attachment.name.clone();
                        let remove = crate::controls::action(("remove-managed-attachment", i))
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Close))
                            .tooltip("Remove this attachment")
                            .on_click(cx.listener(move |composer: &mut Self, _, _, cx| {
                                cx.stop_propagation();
                                composer.unstage(id, cx);
                            }));
                        let row = div()
                            .h_flex()
                            .gap_2()
                            .w_full()
                            .min_w_0()
                            .px_2()
                            .h(CHIP_H)
                            .children(
                                unavailable
                                    .then(|| Icon::new(IconName::Info).size_3().text_color(danger)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .when(unavailable, |label| label.text_color(danger))
                                    .child(name),
                            )
                            .child(div().flex_none().text_xs().text_color(muted).child(detail))
                            .child(remove);
                        match path {
                            Some(path) => crate::controls::action(("open-managed-attachment", i))
                                .ghost()
                                .w_full()
                                .child(row)
                                .tooltip("Open this file in the Workbench")
                                .on_click(cx.listener(move |_: &mut Self, _, _, cx| {
                                    cx.emit(ComposerEvent::OpenFile(path.clone()));
                                }))
                                .into_any_element(),
                            None => row.into_any_element(),
                        }
                    }),
            );

        popup_surface(cx)
            // The count rides in the pinned title rather than in a row of its
            // own, which scrolled away once the list was long enough to need it.
            .child(popup_header(
                format!("Attachments · {}", self.attachments.len()).into(),
                cx,
            ))
            .child(edge_scrolled(&self.attachments_scroll, list))
            // Under the list, where the rows it stands for would be: it only
            // exists once there are more attachments than the manager draws.
            .when(hidden > 0, |popup| {
                popup.child(more_text(cx).child(format!(
                    "{hidden} more — remove visible items to reveal them"
                )))
            })
    }
}

/// Where a staged attachment leads, if it leads anywhere.
///
/// **Only a text file, and only one that could be read.** Three files named
/// `main.rs` are three chips that say `main.rs`, and the only way to tell which
/// one is staged is to look at it — so the chip carries the way to. But the
/// Workbench's editor reads a file as text, so an image handed to it comes back
/// as a decoding error naming a file the user can see is right there, and a
/// file already marked unreadable would fail for the reason the chip is already
/// showing in the danger tint. Neither is worth a second telling, so neither
/// chip offers the press: the pointer appears over the ones that open and
/// nowhere else, which is the only warning a control of this size can carry.
fn openable(attachment: &StagedAttachment) -> Option<std::path::PathBuf> {
    let openable = attachment.kind == AttachmentKind::File
        && attachment.delivery != AttachmentDelivery::Unavailable;
    openable.then(|| attachment.path.clone())
}

/// The chip an attachment is drawn as, applied to whichever container carries
/// it.
///
/// Two containers, because only some of these do something when pressed. The
/// shape is shared rather than written twice so that being pressable stays the
/// *only* difference between them: the two things this tray has to say — the
/// file's name, and whether it can be read — are said the same way whether or
/// not there is anywhere to go, and a chip that changed size or inset on
/// becoming clickable would be saying a third thing nobody meant.
fn attachment_shape<E: Styled>(
    el: E,
    unavailable: bool,
    (border, danger, well): (gpui::Hsla, gpui::Hsla, gpui::Hsla),
    radius: gpui::Pixels,
) -> E {
    el.h_flex()
        .items_center()
        .gap_2()
        .flex_none()
        .h(CHIP_H)
        .pl_2()
        .pr_1()
        .rounded(radius)
        .border_1()
        .border_color(if unavailable { danger } else { border })
        .bg(well)
        .text_xs()
}

/// The shell every control in the composer's row is built from.
///
/// One shape, because they are one *rank* of control: small, quiet things
/// acting on the message being written, sitting in a row under it. Built two
/// ways -- a library `Button` for the icons and a hand-made chip for the
/// selectors -- they came out at two sizes, two inks and two hover fills, and
/// the icons, which carry the smaller job, read as the louder half. Sharing the
/// shell makes them one family structurally rather than by two sets of style
/// rules kept in step by hand.
///
/// A ghost control at a small button's height, its words lettered at
/// `text_xs` by each chip on the child that carries them, in full ink, while
/// its glyphs and its caret take the chip's muted ink. The chip whose popup is
/// open takes the selected fill, and keeps it under the pointer; its caller
/// leaves its tooltip off meanwhile, since the menu opens where it would show.
pub(super) fn chip(id: impl Into<gpui::ElementId>, open: bool, cx: &App) -> Button {
    let (open_fill, fg, radius) = (
        cx.theme().accent,
        cx.theme().muted_foreground,
        cx.theme().radius,
    );
    crate::controls::action(id)
        .ghost()
        // Not for the geometry -- the height and padding below are set outright
        // and land after the library's own, so they win either way. This is for
        // the **caret**, which takes its size from the button's size rather than
        // from the text beside it: left at the default it is a chevron a third
        // taller than the word it belongs to, on a control whose whole job is to
        // be quiet.
        .xsmall()
        .selected(open)
        .h_flex()
        .items_center()
        .gap_1()
        .flex_none()
        .h(CHIP_H)
        .px_1p5()
        .rounded(radius)
        // Ink here, but **not the text size**: the library sets that on the box
        // holding the words, from the button's `Size` and not from anything the
        // call site asks for -- so a size set out here is overridden by one set
        // closer to the text, and setting it looks like it worked while nothing
        // moves. Whatever wants a size of its own says so on the child that
        // carries the words.
        .text_color(fg)
        .when(open, |chip| chip.bg(open_fill))
}
