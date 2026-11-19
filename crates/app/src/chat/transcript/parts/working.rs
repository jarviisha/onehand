//! The words of what is still running: a line in its own quiet ink that a
//! band of brighter ink crosses, so what is alive is told by its own words and
//! the mark beside it never has to change.

use gpui::{
    Animation, AnimationExt as _, App, HighlightStyle, Hsla, IntoElement, ParentElement,
    SharedString, Styled, StyledText, div,
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
/// How far into a line the band travels.
///
/// **A bound on the sweep, and so on its speed.** The pass takes the same time
/// whatever the line, so over a whole long command the band would cross a
/// hundred characters a second, and most of them past where the line is cut
/// off: a flick at its head and then nothing. Kept to a line's opening, the
/// band is slow enough to follow and stays where the line is drawn.
const SWEEP: usize = 48;
/// Often enough to read as a sweep, seldom enough that a long turn with
/// several lines running costs little.
const FPS: f32 = 30.;

/// A line of what is running: its words, which of them are in the machine's
/// face, and which keep an ink of their own.
pub(in crate::chat) struct WorkingLine {
    text: String,
    faces: Vec<Range<usize>>,
    tints: Vec<(Range<usize>, Hsla)>,
}

impl WorkingLine {
    /// A line opening on `text` in the reading face, taken as it is, so a
    /// span counted in it still falls where it was counted.
    pub(in crate::chat) fn words(text: impl AsRef<str>) -> Self {
        Self {
            text: text.as_ref().to_string(),
            faces: Vec::new(),
            tints: Vec::new(),
        }
    }

    /// `text` after a space, in the reading face; nothing for an empty one.
    pub(in crate::chat) fn and(mut self, text: impl AsRef<str>) -> Self {
        self.push(text.as_ref());
        self
    }

    /// `text` after a space, in the machine's face: a path, a command.
    pub(in crate::chat) fn mono(mut self, text: impl AsRef<str>) -> Self {
        if let Some(added) = self.push(text.as_ref()) {
            self.faces.push(added);
        }
        self
    }

    /// `spans` of the line in `ink`, kept under the band's rest: the code a
    /// plan's step sets in backticks.
    pub(in crate::chat) fn tinted(
        mut self,
        spans: impl IntoIterator<Item = Range<usize>>,
        ink: Hsla,
    ) -> Self {
        self.tints.extend(spans.into_iter().map(|span| (span, ink)));
        self
    }

    /// Add `text` after a space, and say where it landed.
    fn push(&mut self, text: &str) -> Option<Range<usize>> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        let from = self.text.len();
        self.text.push_str(text);
        Some(from..self.text.len())
    }
}

/// `line` drawn as one text, swept by the band.
///
/// **One text, not a child per stretch**: the band crosses from the verb into
/// what it is done to without a seam, and the line truncates as one. **In the
/// ink of the line it stands on**, so it lifts with that line under the
/// pointer; only the band and what is tinted bring an ink of their own.
pub(in crate::chat) fn working_text(
    id: impl Into<gpui::ElementId>,
    line: WorkingLine,
    cx: &App,
) -> gpui::AnyElement {
    let family = cx.theme().mono_font_family.clone();
    let WorkingLine { text, faces, tints } = line;
    let text = SharedString::from(text);
    let chars = text.chars().count();
    let lit = cx.theme().foreground;
    let soft = crate::theme::meta_ink(cx);

    div()
        .min_w_0()
        .truncate()
        .with_animation(
            id,
            Animation::new(PERIOD).repeat().with_max_fps(FPS),
            move |element, t| {
                let lights: Vec<(Range<usize>, Hsla)> = band(chars, t)
                    .into_iter()
                    .flat_map(|band| {
                        // A softer character each side, so the band has no edge.
                        let before = band.start.checked_sub(1).map(|at| (at..band.start, soft));
                        let after = (band.end < chars).then(|| (band.end..band.end + 1, soft));
                        before.into_iter().chain([(band.clone(), lit)]).chain(after)
                    })
                    .map(|(chars, color)| (byte_range(&text, chars), color))
                    .collect();
                // **Every stretch is its own run, the plain ones too.** A
                // family is applied only to a run it covers whole, and the
                // runs are whatever the inks cut the line into: left to the
                // band alone, a resting line was one run in the reading face,
                // and its command changed face as the band crossed it.
                let edges = faces
                    .iter()
                    .chain(tints.iter().map(|(span, _)| span))
                    .chain(lights.iter().map(|(span, _)| span));
                let inks = cuts(text.len(), edges)
                    .into_iter()
                    .map(|stretch| {
                        let within = |(span, color): &(Range<usize>, Hsla)| {
                            (span.start <= stretch.start && stretch.end <= span.end)
                                .then_some(*color)
                        };
                        let color = lights
                            .iter()
                            .find_map(within)
                            .or_else(|| tints.iter().find_map(within));
                        (
                            stretch,
                            HighlightStyle {
                                color,
                                ..Default::default()
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                element.child(
                    StyledText::new(text.clone())
                        .with_font_family_overrides(
                            faces.iter().map(|face| (face.clone(), family.clone())),
                        )
                        .with_highlights(inks),
                )
            },
        )
        .into_any_element()
}

/// The characters lit at `t` of the period (`0.0..1.0`): a band entering from
/// before the first character and leaving past the last it sweeps, then
/// nothing while the line rests.
fn band(chars: usize, t: f32) -> Option<Range<usize>> {
    let swept = chars.min(SWEEP);
    if swept == 0 || !(0.0..PASS).contains(&t) {
        return None;
    }
    // Where the band's leading edge stands, counted from before the line.
    let travelled = (t / PASS * (swept + BAND) as f32) as usize;
    let end = travelled.min(swept);
    let start = travelled.saturating_sub(BAND);
    (start < end).then_some(start..end)
}

/// `len` bytes cut at every edge of `spans`, in order: the stretches that
/// each lie wholly inside or outside every one of them.
fn cuts<'a>(len: usize, spans: impl IntoIterator<Item = &'a Range<usize>>) -> Vec<Range<usize>> {
    let mut edges: Vec<usize> = spans
        .into_iter()
        .flat_map(|span| [span.start, span.end])
        .chain([0, len])
        .filter(|edge| *edge <= len)
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges.windows(2).map(|pair| pair[0]..pair[1]).collect()
}

/// The bytes `chars` covers in `text`, on character boundaries.
fn byte_range(text: &str, chars: Range<usize>) -> Range<usize> {
    let at = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(at, _)| at);
    at(chars.start)..at(chars.end)
}

#[cfg(test)]
mod tests {
    use super::{BAND, SWEEP, WorkingLine, band, byte_range, cuts};
    use std::ops::Range;

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

    /// A long line is swept over its opening only, at the pace of a line that
    /// long and no faster.
    #[test]
    fn the_band_stays_in_a_long_lines_opening() {
        for step in 0..100 {
            let t = step as f32 / 100.;
            assert_eq!(band(400, t), band(SWEEP, t));
            if let Some(lit) = band(400, t) {
                assert!(lit.end <= SWEEP);
            }
        }
    }

    /// Every stretch lies wholly inside or wholly outside a face, a tint and
    /// a light: the text is cut at all of them, since a family is only
    /// applied to a stretch it covers whole.
    #[test]
    fn the_line_is_cut_at_every_face_and_every_light() {
        // "Running cargo test": mono from 8 to 18, nothing lit.
        let face = Range { start: 8, end: 18 };
        assert_eq!(cuts(18, [&face]), [0..8, 8..18]);
        // The band straddles the face's edge.
        let band = Range { start: 6, end: 10 };
        assert_eq!(cuts(18, [&face, &band]), [0..6, 6..8, 8..10, 10..18]);
        assert_eq!(cuts(5, []).len(), 1, "one stretch, whole");
        assert!(cuts(0, []).is_empty());
    }

    #[test]
    fn a_line_is_joined_by_single_spaces_and_knows_its_mono_stretch() {
        let line = WorkingLine::words("Running").mono(" cargo test ").and("");
        assert_eq!(line.text, "Running cargo test");
        assert_eq!(line.faces, [Range { start: 8, end: 18 }]);
        let bare = WorkingLine::words("Running").mono("");
        assert_eq!(bare.text, "Running", "no gap for a step with no subject");
        assert!(bare.faces.is_empty());
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
