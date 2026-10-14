//! The command a permission asks to run, as the card and its record draw it.

use super::*;

/// The command a permission is asking to run.
///
/// **An element of its own because it needs the window.** How far it may open
/// is a share of the viewport, and the keys that answer the card hang off a
/// focus handle the window owns — neither is reachable from a plain builder,
/// and both belong to the one box that holds the command rather than to the
/// pane four levels up.
///
/// What it draws is the agent's text and nothing else: the lines are the
/// newlines the agent wrote, in the order it wrote them, wrapped where the box
/// runs out rather than reflowed. A command edited on its way to the screen is
/// a command approved in one form and run in another.
#[derive(IntoElement)]
pub(in crate::chat::transcript) struct CommandBlock {
    pub(in crate::chat::transcript) target: TranscriptItemId,
    /// Every line of it: the box scrolls rather than holding lines back.
    pub(in crate::chat::transcript) lines: Vec<SharedString>,
    /// How tall the panel this is drawn in was last frame, which is what the
    /// box is bounded against. `None` before the list has measured itself,
    /// where the window is the only answer there is.
    pub(in crate::chat::transcript) well: Option<gpui::Pixels>,
}

impl RenderOnce for CommandBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = fold_key(self.target);
        // **Always a gutter, on every command.** It was drawn only past the
        // second line, on the reasoning that a single line has nothing to be
        // told apart from -- which is true of the numbering and false of
        // everything else the column does. A block with no gutter is a
        // different-looking card, and which card a permission gets was decided
        // by whether the agent happened to put a newline in it: two grants an
        // inch apart in one transcript, drawn as two kinds of thing, neither of
        // them the reader's doing. The width is the digits of the count, so the
        // one-line case costs a single character.
        let total = self.lines.len().max(1);
        let gutter =
            rems(MONO_ADVANCE * crate::tokens::CODE_TEXT.0 * total.to_string().len() as f32);
        // **A share of the panel this is drawn in, not of the window.** What
        // the bound is for is the card's own heading staying on screen with the
        // command it belongs to, and the card is in the conversation -- so with
        // a dock open, half the window is taller than the whole panel and an
        // opened command pushes *Permission required* off the top, which is the
        // one thing the share was put here to stop. The window is the fallback
        // for the frame before the list has measured itself, where it is the
        // only answer there is.
        let ceiling = self.well.unwrap_or_else(|| window.viewport_size().height) * COMMAND_SHARE;
        // The command scrolls inside a `gpui::list` row, so it needs a handle
        // of its own and a mask over it: a bubble listener runs too late there,
        // the transcript having already spent the same wheel delta scrolling
        // itself. Without this an opened command is a box the wheel slides the
        // conversation behind.
        let scroll = window
            .use_keyed_state(("perm-command-scroll-state", key), cx, |_, _| {
                ScrollHandle::default()
            })
            .read(cx)
            .clone();
        // Last frame's, like everything else measured here. The first frame of
        // a command draws without these and the second has them, which is a
        // frame nobody can see.
        let overflows = scroll.max_offset().y > gpui::px(0.);
        let more_below = scroll.max_offset().y - scroll.offset().y.abs() > SCROLL_SLACK;

        let well = crate::theme::floating_well(cx);
        plain_box(cx)
            .relative()
            .h_flex()
            .items_start()
            // **No edge, and a step of its own**: the card floats on the
            // well's own grey in the dark palette, so its fill alone sets the
            // command off the card.
            .bg(well)
            .py_2()
            .pl_3()
            .child(
                div()
                    .id(("perm-command-scroll", key))
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    // The scrollbar's gutter while there is one, held inside
                    // the scrolled box: the block itself keeps no right
                    // padding, because the scrollbar lays itself out in the
                    // block's content box, and a gutter of padding there moved
                    // the scrollbar in with the text rather than clear of it.
                    .map(|area| match overflows {
                        true => area.pr(SCROLLBAR_W),
                        false => area.pr_3(),
                    })
                    // **A bound on drawn rows rather than on the agent's
                    // newlines**, which is the only kind that holds for a
                    // command of one very long line: one line of a base64 blob
                    // is a screenful of wrapped rows. Past it the box scrolls.
                    .max_h(COMMAND_H.to_pixels(window.rem_size()).min(ceiling))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .children(self.lines.into_iter().enumerate().map(|(n, line)| {
                        let row = div().h_flex().items_start().gap_3().w_full();
                        row.child(
                            div()
                                .flex_none()
                                .w(gutter)
                                .text_right()
                                // The numbers are a ruler and have to stay one
                                // column: wrapped, they would renumber
                                // themselves down the side of the command.
                                .whitespace_nowrap()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{}", n + 1)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                // A blank line in a script is a line, and an
                                // empty box is no rows tall — which slides
                                // every number after it up against the wrong
                                // line of the command.
                                .child(match line.is_empty() {
                                    true => SharedString::from(" "),
                                    false => line,
                                }),
                        )
                    })),
            )
            // The mask takes the wheel in the capture phase and nothing else,
            // so a press still reaches whatever is drawn on top of it.
            .child(ScrollableMask::new(Axis::Vertical, &scroll).id(("perm-command-mask", key)))
            // **What says the command does not end where the box does.** It goes
            // as soon as the end is reached, so
            // it is never a gradient laid over the last line of a command
            // somebody is being asked to approve.
            .when(more_below, |block| {
                block.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(COMMAND_FADE)
                        .bg(gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(well.alpha(0.), 0.),
                            gpui::linear_color_stop(well, 0.75),
                        )),
                )
            })
            // Over the fade: the one thing that says how far the command runs.
            .when(overflows, |block| {
                block.child(Scrollbar::vertical(&scroll).mode(ScrollbarMode::Always))
            })
    }
}

/// The path a permission's command would run in, shortened from the front.
///
/// **From the front, because a path is read from its tail**: the last
/// components are what tell two checkouts of one project apart, and they are
/// exactly what trimming the end throws away. The whole of it is in the
/// tooltip, so the line identifies the directory and the hover confirms it.
pub(in crate::chat::transcript) fn command_cwd(
    root: &Path,
) -> Option<(SharedString, SharedString)> {
    let full = root.to_string_lossy().into_owned();
    if full.is_empty() {
        return None;
    }
    // `~` where the home directory is: a line that is mostly somebody's user
    // name is a line spent saying where the machine keeps its accounts.
    let shown = std::env::home_dir()
        .and_then(|home| root.strip_prefix(&home).ok())
        .map(|rest| format!("~/{}", rest.to_string_lossy()))
        .unwrap_or_else(|| full.clone());
    Some((
        crate::rail::ellipsize_front(&shown, MAX_CWD),
        SharedString::from(full),
    ))
}

/// Characters of the working directory the meta line carries.
pub(super) const MAX_CWD: usize = 48;
