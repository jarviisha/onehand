//! A vertical stepper, as the library's draws one: a dot per step, its
//! number in it (a check once done), on a rail running from dot to dot, the
//! step's own content beside it. The rows are the caller's, so a step can
//! open, be picked or hold controls, which the library's stepper cannot.

use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, Div, IntoElement, ParentElement as _, Rems, Styled as _, div, rems};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _};

/// How tall a step's first line stands, its dot centred on it: the line the
/// rail runs through from one step to the next.
pub const STEP_LINE_H: Rems = rems(2.25);

/// A step's dot, its number inside it.
const DOT: Rems = rems(1.375);

/// Where the rail runs: through the dots' centres.
const RAIL_X: Rems = rems(0.6875);

/// Where the rail meets a dot's centre: the line's gutter above it, then
/// half the line.
const RAIL_JOIN: Rems = rems(1.25);

/// What a step's dot says.
#[derive(Clone, Copy)]
pub enum StepMark {
    /// Not reached, or left off: its number, muted.
    Waiting(usize),
    /// Under way: its number, ringed in the running ink.
    Current(usize),
    /// Done: a check on the primary fill.
    Done,
    /// The one picked to show beside: its number on the selected fill.
    Picked(usize),
}

/// A step's dot.
pub fn step_dot(mark: StepMark, cx: &App) -> Div {
    let theme = cx.theme();
    let (fill, edge, ink) = match mark {
        StepMark::Waiting(_) => (theme.background, theme.input, theme.muted_foreground),
        StepMark::Current(_) => (theme.background, theme.info, theme.info),
        StepMark::Done => (theme.primary, theme.primary, theme.primary_foreground),
        StepMark::Picked(_) => (theme.accent, theme.accent, theme.accent_foreground),
    };
    let inside = match mark {
        StepMark::Done => Icon::new(IconName::Check).xsmall().into_any_element(),
        StepMark::Waiting(n) | StepMark::Current(n) | StepMark::Picked(n) => {
            format!("{n}").into_any_element()
        }
    };
    div()
        .flex_none()
        .h(STEP_LINE_H)
        .h_flex()
        .items_center()
        .child(
            div()
                .size(DOT)
                .rounded_full()
                .border_1()
                .border_color(edge)
                .bg(fill)
                .h_flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(ink)
                .child(inside),
        )
}

/// One step: its `dot` on the rail, `content` beside it, its first line
/// [`STEP_LINE_H`] tall. The rail comes in from the step before unless
/// `first` and goes on to the next unless `last`, so it starts and ends on a
/// dot however tall an open step stands.
pub fn step_line(
    first: bool,
    last: bool,
    dot: impl IntoElement,
    content: impl IntoElement,
    cx: &App,
) -> AnyElement {
    let rail = cx.theme().input;
    // A hairline-wide fill: a zero-width box's edge draws nothing.
    let half = || div().absolute().left(RAIL_X).w(gpui::px(1.)).bg(rail);
    div()
        .relative()
        .h_flex()
        .items_start()
        .gap_2()
        .when(!first, |line| line.child(half().top_0().h(RAIL_JOIN)))
        .when(!last, |line| line.child(half().top(RAIL_JOIN).bottom_0()))
        // A gutter above and below each line, inside the step's own height
        // so the rail runs on through it: a picked step's fill and a hovered
        // neighbour's never touch.
        .child(div().flex_none().py_0p5().child(dot))
        .child(div().flex_1().min_w_0().py_0p5().v_flex().child(content))
        .into_any_element()
}
