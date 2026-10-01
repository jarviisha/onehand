use super::SELECTED_ALPHA;
use super::popup::POPUP_ROW_H;
use super::presentation::Row;
use gpui::prelude::FluentBuilder as _;
use gpui::{App, ParentElement, Rems, SharedString, Styled, div, rems};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable as _, StyledExt};

/// The leading column every popup row's name stands in.
///
/// **A constant and not the widest name in the list**, which was the obvious
/// alternative and is the wrong one here: the list is refiltered on every
/// keystroke, so a column measured from its contents is a column that changes
/// width while the query is being typed — and the detail beside it, the thing
/// this column exists to give a stable left edge, would move on every letter.
/// Measuring it would trade a column that is occasionally too narrow for one
/// that is never still.
///
/// Wide enough for the names it actually holds: a slash command is a word, and
/// a filename here has already had its folder split off into the column beside
/// it. What overruns is clipped, which costs that one row the tail of its name
/// and costs the column nothing.
const NAME_COLUMN: Rems = rems(14.);
/// How much room either option action may take before its current value truncates.
pub(super) const OPTION_MAX_W: Rems = rems(9.);

/// The shell both kinds of popup row are built from.
///
/// **Two facts, two ways of drawing them.** Which value is in force is a
/// property of the setting and outlives the popup; where the keyboard is
/// standing is a property of this moment. Drawn the same way they cannot be
/// told apart, and the list opens *on* the current value, so the one frame
/// where they coincide is the frame most people see.
///
/// **One fill, and it means one thing: the row about to be taken.** It follows
/// the pointer and the arrow keys alike -- whichever moved last -- so what is
/// lit is always what `Enter` or a click would pick. The value already in force
/// is said by the tick at the row's end and by nothing else.
///
/// The two were drawn apart once, a strong fill for the value in force and a
/// faint one for the cursor. Which is readable standing still and unreadable in
/// motion: a list opens *on* its current value, so the two coincide on the
/// first frame, and walking away from that row left a second fill behind that
/// looked exactly like a second candidate. A mark cannot be confused with a
/// fill however the two move.
///
/// The fill is `accent` with the ink that goes on it, the one spelling a
/// selected thing takes everywhere in this window -- a tab, a mode chip, a rail
/// row.
fn popup_row(id: usize, highlighted: bool, cx: &App) -> Button {
    crate::controls::action(("candidate", id))
        .ghost()
        // Not for the geometry, which is set outright below and lands after the
        // library's. This is what the row's own `text_sm` could not do: the
        // library letters a button from its `Size`, on the box holding the
        // words and so closer to them than anything the call site sets, and
        // with no size named that is a full 1rem -- so these rows were reading
        // a step larger than the line right here asks for.
        .small()
        .h_flex()
        .gap_2()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .px_2()
        .text_sm()
        .rounded(cx.theme().radius)
        // No border and no ring, on either state. The library leaves a ghost
        // button's border transparent already, so what is turned off here is
        // the focus ring it would draw over the top — a second rectangle around
        // the row, in a list where the keyboard is somewhere else entirely and
        // an outline would be claiming otherwise.
        .border_0()
        .when(highlighted, |el| {
            el.bg(cx.theme().accent.alpha(SELECTED_ALPHA))
                .text_color(cx.theme().accent_foreground)
        })
}

/// A completion candidate: the name, and the part that tells two of the same
/// name apart beside it.
///
/// One line and side by side, unlike a choice below. A file candidate's detail
/// is its folder, which is *where the name is* rather than something about it —
/// stacked under the name it would double the height of a list whose whole job
/// is to put fifty paths in front of somebody typing.
pub(super) fn candidate_row(id: usize, row: Row, highlighted: bool, cx: &App) -> Button {
    // **The name is one ink, and the match moved off the ramp onto weight.**
    //
    // There are four things to tell apart here and three ink steps to do it
    // with, so something has to give, and the name is what cannot: a row is
    // read as a name first and everything else second. Every arrangement that
    // spent two ink steps on the name paid for the highlight by dimming the
    // word carrying it — the match intact and the thing it was marking faded
    // out. Drawn at one strength the name is finally as bright as it should
    // have been, and the ramp has a step spare for the detail.
    //
    // So the matched run is bold rather than brighter. That is a rule this
    // popup set out *not* to use — the match was to be carried by ink alone, no
    // colour, no weight — and the reason it held was that ink was free to carry
    // it. It is not any more. Weight is the next axis that is still not a hue,
    // it survives a reader who does not separate colours, and it does not
    // compete with the one fill here that means something.
    //
    // What is left below the name: the detail at the middle step, the run label
    // at the bottom one. They are the safe pair to have adjacent, and the pair
    // that could never share — the detail and the *name lying beside it on the
    // same line* — is now two whole steps apart.
    let name_ink = match highlighted {
        true => cx.theme().accent_foreground,
        false => cx.theme().foreground,
    };
    // The middle step, for the detail and for the type icon. Both are context
    // for the name rather than part of it, and both sit on the same line as it,
    // so they take the step directly under it — far enough down that the name
    // leads, not so far that a path is hard to read.
    let second = match highlighted {
        true => cx.theme().accent_foreground.alpha(0.75),
        false => crate::theme::meta_ink(cx),
    };
    popup_row(id, highlighted, cx)
        .h(POPUP_ROW_H)
        // **The icon is what holds the name's left edge still.** Without it a
        // row with no folder and a row with a long one start their names in
        // different places, and the column a reader scans down stops being a
        // column. It is drawn for every mention row and for none of the command
        // rows, so neither list has a gap in it where the other has a glyph.
        .children(
            row.mark
                .map(|mark| Icon::new(mark).size_3().flex_none().text_color(second)),
        )
        // **A column of a fixed width, not a box that fits its name.** The name
        // is what the eye runs down, so it has to start at the same x on every
        // row — which `flex_none` already gave — but so does the *detail*, and
        // that is what a name-sized box cannot do: sized to its content, every
        // row hands the detail a different left edge and the second column
        // stops being a column at all.
        //
        // A name past the boundary is clipped rather than allowed to push, for
        // the same reason. The row that is too long is one row; the column is
        // every row.
        .child(
            div()
                .flex_none()
                .w(NAME_COLUMN)
                .overflow_hidden()
                .child(marked(&row.label, row.label_span, name_ink, None)),
        )
        // **The detail is set against the row's right edge, so every detail in
        // the list ends at the same x.** The slack is between the two columns
        // rather than after them.
        //
        // This is the opposite end to the one the name is pinned to, and both
        // are pinned on purpose: the name column is fixed-width, so the detail
        // starts no further left than that boundary however long a name is, and
        // ends no further right than the row does however long a detail is. Two
        // fixed edges with the give in the middle — which is the arrangement
        // that keeps a ragged column from being ragged at *both* ends, which is
        // what a content-sized name beside a right-set detail used to be.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_flex()
                .justify_end()
                .overflow_hidden()
                .text_xs()
                .children(
                    row.detail
                        .map(|detail| marked(&detail, row.detail_span, second, Some(name_ink))),
                ),
        )
}

/// A string with the run the query matched picked out of the rest of it.
///
/// **Weight always, ink only where there is room for it.** This started as ink
/// alone — matched at full strength, the rest a step down — and that held while
/// the ramp had a step to spare. It stopped holding when the name went to full
/// strength: there is nothing above full strength to climb to, so ink can say
/// nothing there and `lit` is `None`. A detail still has room, because it sits
/// a step below the name, so its matched run climbs *and* takes the weight.
///
/// What weight buys is what the ink was chosen for in the first place: it is
/// not a hue, so it survives a reader who does not separate colours, and it
/// does not compete with the one fill in this popup that means something — the
/// row about to be taken.
///
/// Three spans and not one styled run, because the range is a byte range into
/// this exact string: slicing is safe only because core found the range against
/// the same text and on character boundaries.
fn marked(
    text: &SharedString,
    at: Option<std::ops::Range<usize>>,
    base: gpui::Hsla,
    lit: Option<gpui::Hsla>,
) -> gpui::Div {
    let Some(at) = at.filter(|at| text.is_char_boundary(at.start) && text.is_char_boundary(at.end))
    else {
        // One text child, so the ellipsis the column needs is available: a
        // string too long for its column ends in `…` at the column's own edge
        // rather than at the popup's.
        //
        // **Not `w_full`.** A box told to fill its parent sits at both edges of
        // it, so a detail set against the right edge of the row would be pushed
        // back to the left one by its own width: the alignment undone by the
        // thing being aligned. `min_w_0` lets it shrink to its text and still
        // give way when the text is longer than the room.
        return div()
            .min_w_0()
            .truncate()
            .text_color(base)
            .child(text.clone());
    };
    // Three children cannot share one ellipsis — the run that overflows is
    // whichever one the boundary falls in, and gpui has no way to say "put the
    // mark at the end of this box whichever child reaches it". So the split
    // string clips instead, and the tail is the piece that takes it: the head
    // and the lit run are the part answering why this row is here, and the tail
    // is what is left over.
    div()
        .h_flex()
        .min_w_0()
        .overflow_hidden()
        .text_color(base)
        .child(div().flex_none().child(text[..at.start].to_string()))
        .child(
            div()
                .flex_none()
                // Weight, and an ink only where there is one above the base
                // to step up to. In a name there is not — the name is already
                // at full strength — so `lit` is `None` there and weight is the
                // whole of the affordance. In a detail there is, because the
                // detail sits a step down, so the matched run climbs to the
                // name's own ink *and* takes the weight.
                //
                // Written as an `Option` rather than two colours because two
                // colours that had to be equal is what this was: the caller
                // passed the same value twice and the comment here claimed a
                // step between them that was not drawn.
                .font_semibold()
                .text_color(lit.unwrap_or(base))
                .child(text[at.start..at.end].to_string()),
        )
        .child(div().min_w_0().truncate().child(text[at.end..].to_string()))
}

/// One value of an agent-advertised setting: its name, the agent's sentence
/// about it underneath, and a tick where it is the one in force.
///
/// **Stacked, and that is the whole difference from a candidate.** A model's
/// description is a sentence and the names it tells apart are two words each,
/// so beside the name it either pushes the name off the row or truncates to the
/// three words every model's description opens with. Under it, at the quieter
/// size, the names stay a column that can be scanned and the sentences are
/// there for the one being considered.
///
/// The tick comes back here because the objection to it does not hold in this
/// shape: it used to pull a *centred* label off centre, and the content of this
/// row is pinned to the start by a `flex_1` of its own. What the fill alone
/// cannot do is survive a reader who does not separate its colour from the row
/// above — so the answer is said twice, in the fill and in a mark.
pub(super) fn choice_row(id: usize, row: Row, highlighted: bool, cx: &App) -> Button {
    let detail_ink = match highlighted {
        // On the fill, the muted ink of an unlit row is close to unreadable;
        // this is the same relationship one step down from the ink that belongs
        // on this fill.
        true => cx.theme().accent_foreground.alpha(0.75),
        false => cx.theme().muted_foreground,
    };
    let checked = row.checked;
    popup_row(id, highlighted, cx)
        // **The height has to be taken back from the library, explicitly.** A
        // `Button` writes a fixed height per size -- 1.5rem at this one -- and
        // then wraps everything the call site gave it in a box set to the full
        // height of that, centred. A second line does not make the button
        // taller: it overflows the box it was centred in and is painted across
        // the rows either side of it, which is two lines of one choice sitting
        // on top of the next choice's name. Nothing about it looks like a
        // height; it looks like the list has been drawn twice.
        //
        // The floor keeps a choice the agent sent no sentence for standing at
        // exactly the height every other one-line row in this popup does.
        .h_auto()
        .min_h(POPUP_ROW_H)
        .py_1()
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .child(div().w_full().truncate().child(row.label))
                .children(row.detail.map(|detail| {
                    div()
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(detail_ink)
                        .child(detail)
                })),
        )
        .children(checked.then(|| Icon::new(IconName::Check).size_4().flex_none()))
}
