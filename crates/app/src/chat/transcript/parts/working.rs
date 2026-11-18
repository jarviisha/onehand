//! The words of what is still running: a line in the quiet ink that a band of
//! brighter ink crosses, so what is alive is told by its own words and the
//! mark beside it never has to change.

use gpui::{
    Animation, AnimationExt as _, App, HighlightStyle, IntoElement, ParentElement, SharedString,
    Styled, StyledText, div,
};
use gpui_component::ActiveTheme;
use std::ops::Range;
use std::time::Duration;

/// One pass and the rest after it.
const PERIOD: Duration = Duration::from_millis(2200);
/// The share of the period the pass takes; what is left is the rest.
const PASS: f32 = 0.75;
/// How many characters the band lights at once.
const BAND: usize = 4;
/// Often enough to read as a sweep, seldom enough that a long turn with
/// several lines running costs little.
const FPS: f32 = 30.;

/// A stretch of a working line, in the reading face or the machine's.
pub(in crate::chat) struct WorkingRun {
    pub text: SharedString,
    pub mono: bool,
}

impl WorkingRun {
    pub(in crate::chat) fn words(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            mono: false,
        }
    }

    pub(in crate::chat) fn mono(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            mono: true,
        }
    }
}

/// `runs` as one line, a space between them, swept by the band.
///
/// **One text, not a child per run**: the band crosses from the verb into
/// what it is done to without a seam, and the line truncates as one. With
/// motion reduced the line stands still in its quiet ink.
pub(in crate::chat) fn working_text(
    id: impl Into<gpui::ElementId>,
    runs: Vec<WorkingRun>,
    cx: &App,
) -> gpui::AnyElement {
    let mut text = String::new();
    let mut mono: Vec<(Range<usize>, SharedString)> = Vec::new();
    let family = cx.theme().mono_font_family.clone();
    for run in runs.iter().filter(|run| !run.text.trim().is_empty()) {
        if !text.is_empty() {
            text.push(' ');
        }
        let from = text.len();
        text.push_str(&run.text);
        if run.mono {
            mono.push((from..text.len(), family.clone()));
        }
    }
    let text = SharedString::from(text);
    let chars = text.chars().count();
    let lit = cx.theme().foreground;
    let soft = crate::theme::meta_ink(cx);
    let ink = |color| HighlightStyle {
        color: Some(color),
        ..Default::default()
    };

    div()
        .min_w_0()
        .truncate()
        .text_color(cx.theme().muted_foreground)
        .with_animation(
            id,
            Animation::new(PERIOD).repeat().with_max_fps(FPS),
            move |line, t| {
                let lights = band(chars, t).into_iter().flat_map(|band| {
                    // A softer character each side, so the band has no edge.
                    let before = band.start.checked_sub(1).map(|at| (at..band.start, soft));
                    let after = (band.end < chars).then(|| (band.end..band.end + 1, soft));
                    before.into_iter().chain([(band.clone(), lit)]).chain(after)
                });
                line.child(
                    StyledText::new(text.clone())
                        .with_font_family_overrides(mono.clone())
                        .with_highlights(
                            lights
                                .map(|(chars, color)| (byte_range(&text, chars), ink(color)))
                                .collect::<Vec<_>>(),
                        ),
                )
            },
        )
        .into_any_element()
}

/// The characters lit at `t` of the period (`0.0..1.0`): a band entering from
/// before the first character and leaving past the last, then nothing while
/// the line rests.
fn band(chars: usize, t: f32) -> Option<Range<usize>> {
    if chars == 0 || !(0.0..PASS).contains(&t) {
        return None;
    }
    // Where the band's first character stands, counted from `-BAND`.
    let travelled = (t / PASS * (chars + BAND) as f32) as usize;
    let end = travelled.min(chars);
    let start = travelled.saturating_sub(BAND);
    (start < end).then_some(start..end)
}

/// The bytes `chars` covers in `text`, on character boundaries.
fn byte_range(text: &str, chars: Range<usize>) -> Range<usize> {
    let at = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(at, _)| at);
    at(chars.start)..at(chars.end)
}

#[cfg(test)]
mod tests {
    use super::{BAND, band, byte_range};

    #[test]
    fn the_band_crosses_the_text_then_rests() {
        assert_eq!(band(20, 0.0), None, "before the first character");
        let mid = band(20, 0.375).expect("half way across");
        assert_eq!(mid.len(), BAND);
        assert!(mid.start > 0 && mid.end < 20);
        assert_eq!(band(20, 0.8), None, "resting");
        assert_eq!(band(0, 0.3), None, "nothing to light");
        assert!(
            band(2, 0.375).expect("a short word").end <= 2,
            "never past the end"
        );
    }

    #[test]
    fn the_band_is_cut_on_characters() {
        let text = "Đang chạy 日本語 🙂 xong";
        let chars = text.chars().count();
        for step in 0..100 {
            if let Some(lit) = band(chars, step as f32 / 100.) {
                let bytes = byte_range(text, lit);
                assert!(text.is_char_boundary(bytes.start) && text.is_char_boundary(bytes.end));
                assert!(bytes.end <= text.len());
            }
        }
    }
}
