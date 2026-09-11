//! Terminal rendering module.
//!
//! This module provides [`TerminalRenderer`], which handles efficient rendering of
//! terminal content using GPUI's text and drawing systems.
//!
//! # Rendering Pipeline
//!
//! The renderer processes the terminal grid in several stages:
//!
//! ```text
//! Terminal Grid → Layout Phase → Paint Phase
//!                      │              │
//!                      └─ Collect backgrounds
//!                                     │
//!                                     ├─ Paint default background
//!                                     ├─ Paint non-default backgrounds
//!                                     ├─ Paint text characters
//!                                     └─ Paint cursor
//! ```
//!
//! # Optimizations
//!
//! The renderer includes several optimizations to minimize draw calls:
//!
//! 1. **Background Merging**: Adjacent cells with the same background color are
//!    merged into single rectangles, reducing the number of quads to paint.
//!
//! 2. **Default Background Skip**: Cells with the default background color don't
//!    generate separate background rectangles.
//!
//! 3. **Cell Measurement**: Font metrics are measured using the 'M' character and
//!    reused until the font changes.
//!
//! 4. *onehand patch* — **Text batching**: adjacent cells sharing a face, a
//!    colour and a decoration are shaped and painted as one run rather than one
//!    glyph at a time.
//!
//! *onehand patch*: the fourth is ours, and the reason it took two attempts is
//! worth stating. The batching that used to be here grouped the cells and was
//! then **discarded by its only caller**, which painted a glyph at a time
//! regardless — the cost paid, the benefit not. It was removed rather than wired
//! up, because the structure it produced could not express what the glyph pass
//! draws: an underline's own colour, a strikethrough, or which way round an
//! inverted cell is.
//!
//! What made batching unsafe was that a terminal has to put every glyph on its
//! own cell, and shaping several characters as one run hands the advances to the
//! font — where a ligature, a wide character or one glyph falling through to a
//! fallback face shifts everything after it. Three answers, one per hazard.
//! Shaping asks for a **forced cell width**, so gpui snaps each base glyph to
//! its own column instead of trusting the accumulated advances. The faces are
//! built with **contextual alternates off**, so a programming font cannot fuse
//! two cells into one ligature glyph and take the column after it with them —
//! which is also exactly what the one-glyph-at-a-time pass drew, so nothing on
//! screen changes. And a **double-width character is a run of its own**, since
//! it is the one thing that legitimately spans two columns.
//!
//! What this buys is the count: a screen of code costs a handful of shaping
//! calls per row instead of one per visible character, and each of those calls
//! is a hash into gpui's line-layout cache that a row nobody touched hits on
//! every frame. See [`split_row_runs`], and [`ascii_glyph`] for the single-glyph
//! run that still allocates nothing.
//!
//! # Cell Dimensions
//!
//! Cell size is calculated from actual font metrics using the '│' character,
//! which spans the full cell height in properly designed terminal fonts:
//!
//! - **Width**: Measured from shaped '│' character
//! - **Height**: `(ascent + descent) × line_height_multiplier`
//!
//! The `line_height_multiplier` (default 1.0) can be adjusted to add extra
//! vertical space if needed for specific fonts.
//!
//! # Example
//!
//! ```ignore
//! use gpui::px;
//! use gpui_terminal::{ColorPalette, TerminalRenderer};
//!
//! let renderer = TerminalRenderer::new(
//!     "JetBrains Mono".to_string(),
//!     px(14.0),
//!     1.0,  // line height multiplier
//!     ColorPalette::default(),
//! );
//! ```

use crate::box_drawing;
use crate::colors::ColorPalette;
use crate::event::GpuiEventProxy;
use alacritty_terminal::grid::{Dimensions, Row};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use gpui::{
    App, Bounds, Edges, Font, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, Point,
    SharedString, Size, StrikethroughStyle, TextRun, UnderlineStyle, Window, px, quad,
    transparent_black,
};

/// onehand patch: one `SharedString` per printable ASCII character, made once.
///
/// The glyph pass shapes a character at a time, and every call needs the text as
/// a [`SharedString`]. Built from the `char` each time — which is what this
/// replaces — that is a `String` allocation and then an `Arc` allocation, for
/// every visible character of every frame. On a full screen of code redrawn on
/// each keystroke, which is what a modal editor does, it is the largest single
/// source of allocation in the renderer.
///
/// Cloning out of this table is an `Arc` increment instead. Printable ASCII only:
/// everything else falls back to building one, since a table over the rest of
/// Unicode is not a table.
fn ascii_glyph(ch: char) -> SharedString {
    static PRINTABLE: std::sync::OnceLock<Vec<SharedString>> = std::sync::OnceLock::new();

    if ch.is_ascii_graphic() {
        let table = PRINTABLE.get_or_init(|| {
            ('!'..='~')
                .map(|c| SharedString::from(c.to_string()))
                .collect()
        });
        // `is_ascii_graphic` is exactly the range built above, so this indexes
        // rather than searches, and cannot miss.
        return table[ch as usize - '!' as usize].clone();
    }
    SharedString::from(ch.to_string())
}

/// Background rectangle to paint.
///
/// Represents a rectangular region with a solid color background.
#[derive(Debug, Clone)]
pub struct BackgroundRect {
    /// Starting column position
    pub start_col: usize,

    /// Ending column position (exclusive)
    pub end_col: usize,

    /// Row position
    pub row: usize,

    /// Background color
    pub color: Hsla,
}

/// onehand patch: the ink a cell is actually drawn with.
///
/// Three attributes change a cell's colours rather than its shape, and each one
/// is a thing a program *says with colour* -- so ignoring them does not lose a
/// flourish, it loses the meaning:
///
/// - **`INVERSE`** swaps foreground and background. It is how a great many
///   colour schemes draw a status line, a visual selection and a search hit;
///   left unswapped those come out as dark text on a dark background, which
///   reads as a broken theme rather than a missing attribute.
/// - **`DIM`** is the half-bright foreground old terminals had. Nothing else in
///   the palette expresses it, so it is applied to the lightness here.
/// - **`HIDDEN`** is text a program has drawn but does not want read -- a typed
///   password being the whole reason it exists. Showing it is the one failure in
///   this list that matters outside the screen.
///
/// Returned as a pair from **one** function on purpose. The colours are needed
/// by the background pass, by the glyph pass and by the cursor repainting the
/// character it sits on, and three places working the same rule out separately
/// is exactly how the cursor came to be drawn over the character underneath it.
///
/// Order matters: invert first, then dim, then hide. Dimming an inverted cell
/// has to dim what is now the foreground, and hiding is the last word whatever
/// the other two decided.
fn cell_ink(palette: &ColorPalette, cell: &Cell, colors: &Colors) -> (Hsla, Hsla) {
    let mut fg = palette.resolve(cell.fg, colors);
    let mut bg = palette.resolve(cell.bg, colors);

    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.flags.contains(Flags::DIM) {
        fg.l *= 0.7;
    }
    if cell.flags.contains(Flags::HIDDEN) {
        fg = bg;
    }

    (fg, bg)
}

/// onehand patch: how a cell's underline is drawn, if it has one.
///
/// **The colour is the half that carries information**, not the shape. A
/// language server marks an error and a warning with the same squiggle in
/// different colours, and it sets that colour per cell with its own escape
/// (`SGR 58`) rather than through the foreground — so a renderer that reads the
/// flag and not [`Cell::underline_color`] draws every diagnostic in the colour
/// of the text it is under, which is to say all of them alike.
///
/// The shape is the half that is approximated. GPUI's underline is a thickness,
/// a colour and a wavy flag, so a curly underline is exact and the double,
/// dotted and dashed forms all come out straight. That is a deliberate trade
/// against owning underline painting outright: a straight line where a dashed
/// one was asked for still says *there is something marked here*, which is the
/// whole job, while drawing nothing says the opposite.
fn underline_style(
    palette: &ColorPalette,
    cell: &Cell,
    colors: &Colors,
    fg: Hsla,
) -> Option<UnderlineStyle> {
    if !cell.flags.intersects(Flags::ALL_UNDERLINES) {
        return None;
    }
    Some(UnderlineStyle {
        thickness: px(1.0),
        color: Some(
            cell.underline_color()
                .map_or(fg, |color| palette.resolve(color, colors)),
        ),
        wavy: cell.flags.contains(Flags::UNDERCURL),
    })
}

/// onehand patch: the strikethrough, if the cell carries one.
///
/// Kept beside [`underline_style`] because it is the same shape of omission:
/// the flag was parsed and the renderer hard-coded `None` past it, so text a
/// program had struck out came through indistinguishable from text it had not.
fn strikethrough_style(cell: &Cell, fg: Hsla) -> Option<StrikethroughStyle> {
    cell.flags
        .contains(Flags::STRIKEOUT)
        .then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(fg),
        })
}

/// onehand patch: what the glyph pass has to do with a cell.
///
/// Written down as a type because the answer decides where a shaped run may
/// start and where it has to stop, and getting that wrong does not look like a
/// styling mistake — it slides every character after it one column left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellGlyph {
    /// Nothing is drawn and no run may cross it: an empty cell, or the second
    /// half of a double-width character, which is a column the text of a run
    /// cannot account for.
    Blank,
    /// A space. Nothing is drawn for it either, but a run may carry it so that
    /// the words on both sides stay one run — see [`RunStyle::carries_spaces`].
    Space,
    /// Drawn as lines by the box-drawing pass, not shaped.
    Boxed,
    /// A double-width character. Shaped alone, because it is the one thing that
    /// legitimately covers two columns.
    Wide(char),
    /// An ordinary single-width character.
    Glyph(char),
}

/// onehand patch: which of the five a cell is.
fn classify(cell: &Cell) -> CellGlyph {
    if cell
        .flags
        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
    {
        return CellGlyph::Blank;
    }
    match cell.c {
        '\0' => CellGlyph::Blank,
        ' ' => CellGlyph::Space,
        ch if box_drawing::is_box_drawing_char(ch) => CellGlyph::Boxed,
        ch if cell.flags.contains(Flags::WIDE_CHAR) => CellGlyph::Wide(ch),
        ch => CellGlyph::Glyph(ch),
    }
}

/// onehand patch: everything about a cell that a shaped run has to hold constant.
///
/// A [`TextRun`] carries one face, one colour and one decoration for its whole
/// length, so two cells may only be shaped together when all of those agree.
/// Nothing about the *background* is in here: that is painted as quads before
/// the glyphs, so a run may cross a change of background colour without being
/// broken.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RunStyle {
    /// Index into [`TerminalRenderer::font_variants`].
    font: usize,
    color: Hsla,
    underline: Option<UnderlineStyle>,
    strikethrough: Option<StrikethroughStyle>,
}

impl RunStyle {
    fn of(palette: &ColorPalette, cell: &Cell, colors: &Colors) -> Self {
        let (fg, _) = cell_ink(palette, cell, colors);
        Self {
            font: TerminalRenderer::font_index(cell.flags),
            color: fg,
            underline: underline_style(palette, cell, colors, fg),
            strikethrough: strikethrough_style(cell, fg),
        }
    }

    /// Whether a space may be carried inside a run of this style.
    ///
    /// A space draws no glyph, so carrying one costs nothing and joins the words
    /// on either side of it into a single shaping call — which is most of what
    /// batching buys on a screen of prose or code. It is refused for a decorated
    /// run because a decoration is *not* nothing: an underline or a strikethrough
    /// runs the full width of the run it belongs to, so carrying a space would
    /// draw a line under a cell the one-glyph-at-a-time pass left bare.
    fn carries_spaces(&self) -> bool {
        self.underline.is_none() && self.strikethrough.is_none()
    }
}

/// onehand patch: a stretch of one row that is shaped and painted in one call.
#[derive(Debug)]
struct GlyphRun {
    /// The column the run's first character sits in. Every character in `text`
    /// occupies exactly one column from here on, which is what lets the whole
    /// run be placed from a single origin.
    start_col: usize,
    style: RunStyle,
    text: String,
}

/// onehand patch: the runs of one row, in a buffer that outlives the row.
///
/// Held across the whole frame and reset per row rather than rebuilt, because
/// the `String` inside each run is the allocation this pass exists to avoid: a
/// grid is forty rows and a frame is however often the child writes. `len` is
/// how many of `runs` this row is using — the rest keep their capacity for the
/// row after it.
#[derive(Default)]
struct RowRuns {
    runs: Vec<GlyphRun>,
    len: usize,
    /// Whether the last run is still accepting characters. A double-width
    /// character closes its own run, so "there is a run" and "a run may be
    /// appended to" are different questions.
    open: bool,
}

impl RowRuns {
    fn reset(&mut self) {
        self.len = 0;
        self.open = false;
    }

    /// Open a run at `start_col` and hand it back to be written into.
    ///
    /// Returning the run rather than only opening it is what keeps the caller
    /// from having to ask for it again and unwrap an `Option` it just created.
    fn start(&mut self, start_col: usize, style: RunStyle) -> &mut GlyphRun {
        if self.len == self.runs.len() {
            self.runs.push(GlyphRun {
                start_col,
                style,
                text: String::new(),
            });
        } else {
            let run = &mut self.runs[self.len];
            run.start_col = start_col;
            run.style = style;
            run.text.clear();
        }
        self.len += 1;
        self.open = true;
        &mut self.runs[self.len - 1]
    }

    fn open_run(&mut self) -> Option<&mut GlyphRun> {
        if self.open {
            self.runs[..self.len].last_mut()
        } else {
            None
        }
    }

    fn close(&mut self) {
        self.open = false;
    }

    /// Finish the row: trailing spaces are dropped from every run.
    ///
    /// They draw nothing, and a run that ends in twenty of them is a different
    /// cache key from the same words without them — which on a screen where only
    /// the trailing whitespace moved would miss the cache on every row.
    fn finish(&mut self) {
        for run in &mut self.runs[..self.len] {
            while run.text.ends_with(' ') {
                run.text.pop();
            }
        }
        self.open = false;
    }

    /// The runs that have text left to draw, in column order.
    ///
    /// A run can end up empty: one made of nothing but spaces has them all
    /// trimmed by [`Self::finish`], and there is nothing to shape.
    fn drawable(&self) -> impl Iterator<Item = &GlyphRun> {
        self.runs[..self.len]
            .iter()
            .filter(|run| !run.text.is_empty())
    }
}

/// onehand patch: split one row into the runs the glyph pass shapes.
///
/// Free rather than a method, and taking cells as an iterator rather than a
/// grid row, so the whole rule can be tested — placing the runs needs a window
/// and deciding where they begin and end does not. The property that matters is
/// that a run's *n*th character is in its `start_col + n`th column: every arm
/// below either pushes exactly one character for the cell it is looking at or
/// pushes none and closes the run.
fn split_row_runs<'a>(
    palette: &ColorPalette,
    cells: impl Iterator<Item = &'a Cell>,
    colors: &Colors,
    out: &mut RowRuns,
) {
    out.reset();

    for (col, cell) in cells.enumerate() {
        match classify(cell) {
            CellGlyph::Blank | CellGlyph::Boxed => out.close(),
            CellGlyph::Space => match out.open_run() {
                Some(run) if run.style.carries_spaces() => run.text.push(' '),
                _ => out.close(),
            },
            CellGlyph::Wide(ch) => {
                out.start(col, RunStyle::of(palette, cell, colors))
                    .text
                    .push(ch);
                out.close();
            }
            CellGlyph::Glyph(ch) => {
                let style = RunStyle::of(palette, cell, colors);
                match out.open_run() {
                    Some(run) if run.style == style => run.text.push(ch),
                    _ => out.start(col, style).text.push(ch),
                }
            }
        }
    }

    out.finish();
}

impl BackgroundRect {
    /// Check if this rectangle can be merged with another.
    ///
    /// Two rectangles can be merged if they:
    /// - Are on the same row
    /// - Have the same color
    /// - Are horizontally adjacent
    fn can_merge_with(&self, other: &Self) -> bool {
        self.row == other.row && self.color == other.color && self.end_col == other.start_col
    }
}

/// Terminal renderer with font settings and cell dimensions.
///
/// This struct manages the rendering of terminal content, including text,
/// backgrounds, and cursor. It maintains font metrics and provides the
/// [`paint`](Self::paint) method for drawing the terminal grid.
///
/// # Font Metrics
///
/// Cell dimensions are calculated from actual font measurements via
/// [`measure_cell`](Self::measure_cell). This ensures accurate character
/// positioning regardless of the font used.
///
/// # Usage
///
/// The renderer is typically used internally by [`TerminalView`](crate::TerminalView),
/// but can also be used directly for custom rendering:
///
/// ```ignore
/// // Measure cell dimensions (call once per font change)
/// renderer.measure_cell(window);
///
/// // Paint the terminal grid
/// renderer.paint(bounds, padding, &term, true, window, cx);
/// ```
///
/// # Performance
///
/// For optimal performance:
/// - Call `measure_cell` only when font settings change
/// - The `paint` method is designed to be called every frame
/// - Background and text batching minimize GPU draw calls
#[derive(Clone)]
pub struct TerminalRenderer {
    /// Font family name (e.g., "Fira Code", "Menlo")
    pub font_family: String,

    /// Font size in pixels
    pub font_size: Pixels,

    /// Width of a single character cell
    pub cell_width: Pixels,

    /// Height of a single character cell (line height)
    pub cell_height: Pixels,

    /// Multiplier for line height to accommodate tall glyphs
    pub line_height_multiplier: f32,

    /// Color palette for resolving terminal colors
    pub palette: ColorPalette,

    /// onehand patch: the font [`Self::cell_width`] and [`Self::cell_height`]
    /// were last measured for, or `None` if they are still the constructor's
    /// guesses.
    ///
    /// Measuring means shaping a glyph through the text system, and the answer
    /// only changes when the family, the size or the line-height multiplier
    /// does — none of which happens while somebody is typing. Without this the
    /// measurement ran on **every frame**, which for a modal editor is every
    /// keystroke.
    ///
    /// Private, unlike everything above it, because it is a claim about two
    /// other fields rather than a setting: anything that could set it out of
    /// step with them would make the grid lay out at a size it never measured.
    measured_for: Option<(String, Pixels, f32)>,
}

impl TerminalRenderer {
    /// Creates a new terminal renderer with the given font settings and color palette.
    ///
    /// # Arguments
    ///
    /// * `font_family` - The name of the font family to use
    /// * `font_size` - The font size in pixels
    /// * `line_height_multiplier` - Multiplier for line height (e.g., 1.2 for 20% extra)
    /// * `palette` - The color palette to use for terminal colors
    ///
    /// # Returns
    ///
    /// A new `TerminalRenderer` instance with default cell dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use gpui::px;
    /// use gpui_terminal::render::TerminalRenderer;
    /// use gpui_terminal::ColorPalette;
    ///
    /// let renderer = TerminalRenderer::new("Fira Code".to_string(), px(14.0), 1.0, ColorPalette::default());
    /// ```
    pub fn new(
        font_family: String,
        font_size: Pixels,
        line_height_multiplier: f32,
        palette: ColorPalette,
    ) -> Self {
        // Default cell dimensions - will be measured on first paint
        // Using 0.6 as approximate em-width ratio for monospace fonts
        let cell_width = font_size * 0.6;
        let cell_height = font_size * 1.4; // Line height with some spacing

        Self {
            font_family,
            font_size,
            cell_width,
            cell_height,
            line_height_multiplier,
            palette,
            measured_for: None,
        }
    }

    /// onehand patch: measure the cell, but only when the font it describes has
    /// changed.
    ///
    /// The gate rather than [`Self::measure_cell`] is what the paint should
    /// call. Shaping a probe glyph is not free, and the answer is the same on
    /// every frame between one font change and the next.
    pub fn ensure_measured(&mut self, window: &mut Window) {
        if !self.needs_measure() {
            return;
        }
        self.measure_cell(window);
        self.measured_for = Some((
            self.font_family.clone(),
            self.font_size,
            self.line_height_multiplier,
        ));
    }

    /// Whether [`Self::cell_width`] and [`Self::cell_height`] still describe the
    /// font this renderer is set to draw with.
    ///
    /// Split from [`Self::ensure_measured`] so the rule can be tested: measuring
    /// itself needs a window, and deciding whether to does not.
    fn needs_measure(&self) -> bool {
        !self
            .measured_for
            .as_ref()
            .is_some_and(|(family, size, mult)| {
                family == &self.font_family
                    && *size == self.font_size
                    && *mult == self.line_height_multiplier
            })
    }

    /// onehand patch: take another renderer's measurements as this one's.
    ///
    /// The measuring happens on a clone inside the paint, because it needs the
    /// window and the window is only there; this is how the answer gets back to
    /// the renderer the *view* holds, which is what every pixel-to-cell
    /// conversion divides by. The staleness key travels with the numbers it
    /// describes, so the next frame knows it has nothing to re-measure.
    pub fn adopt_metrics(&mut self, measured: &Self) {
        self.cell_width = measured.cell_width;
        self.cell_height = measured.cell_height;
        self.measured_for = measured.measured_for.clone();
    }

    /// onehand patch: the four faces a cell can ask for, built once per paint.
    ///
    /// Indexed by [`Self::font_index`]. This exists because a [`Font`] is not
    /// cheap to make: its `family` is a [`SharedString`] built here from a
    /// `String`, and `FontFeatures::default()` is an `Arc<Vec<_>>` — so
    /// constructing one per glyph, which is what the glyph pass used to do, is
    /// three heap allocations for every visible character of every frame.
    /// Cloning one of these four is a pair of `Arc` increments instead.
    fn font_variants(&self) -> [Font; 4] {
        [
            self.face(FontWeight::NORMAL, FontStyle::Normal),
            self.face(FontWeight::NORMAL, FontStyle::Italic),
            self.face(FontWeight::BOLD, FontStyle::Normal),
            self.face(FontWeight::BOLD, FontStyle::Italic),
        ]
    }

    /// onehand patch: where a glyph's baseline sits for a given row.
    ///
    /// The line-height multiplier adds height around the text, and the text is
    /// centred in what it added. One function because two places need the
    /// answer and have to agree: the glyph pass, and the cursor repainting the
    /// character it is sitting on. Worked out separately, the character the
    /// block puts back lands a pixel or two off the one it covered.
    fn glyph_baseline(&self, line_idx: usize) -> Pixels {
        let base_height = self.cell_height / self.line_height_multiplier;
        let vertical_offset = (self.cell_height - base_height) / 2.0;
        self.cell_height * (line_idx as f32) + vertical_offset
    }

    /// onehand patch: one face, built the one way the grid ever wants one.
    ///
    /// **Contextual alternates are off**, and that is the load-bearing part. A
    /// programming font uses them to fuse `->` or `!=` into a single ligature
    /// glyph, which in a document is a nicety and in a cell grid is a character
    /// that has eaten the column after it: the glyph pass shapes several cells
    /// as one run and places each glyph on its own column, so two characters
    /// arriving as one glyph move everything after them one cell left. Nothing
    /// on screen changes by turning it off — a pass that shaped a single
    /// character at a time never had a neighbour to ligate with in the first
    /// place.
    ///
    /// Shared by every shaping this module does, so the grid is measured, drawn
    /// and repainted under the cursor through exactly one description of the
    /// font. Two of them is a cell measured in one face and filled in another.
    fn face(&self, weight: FontWeight, style: FontStyle) -> Font {
        Font {
            family: self.font_family.clone().into(),
            features: FontFeatures::disable_ligatures(),
            fallbacks: None,
            weight,
            style,
        }
    }

    /// Which of [`Self::font_variants`] a cell wants.
    fn font_index(flags: Flags) -> usize {
        usize::from(flags.contains(Flags::BOLD)) << 1 | usize::from(flags.contains(Flags::ITALIC))
    }

    /// Measure cell dimensions based on actual font metrics.
    ///
    /// This method measures the actual width and height of characters
    /// using the GPUI text system. It uses the '│' (BOX DRAWINGS LIGHT VERTICAL)
    /// character which spans the full cell height in properly designed terminal fonts.
    ///
    /// # Arguments
    ///
    /// * `window` - The GPUI window for text system access
    pub fn measure_cell(&mut self, window: &mut Window) {
        // onehand patch: measured from 'M', not from '│' (U+2502).
        //
        // Upstream picks the box-drawing character because it spans the full
        // cell in a terminal face. But shaping falls back per *glyph*: a family
        // that does not carry U+2502 -- which plenty of otherwise perfectly good
        // monospace faces do not -- silently yields the advance and the metrics
        // of some other font, and the grid is then laid out at one font's cell
        // size while every row is drawn in another. That reads as a broken
        // renderer rather than a missing glyph. 'M' exists in every face that
        // could be chosen here, and ascent/descent come from the font rather
        // than from the glyph's ink, so nothing is lost by not measuring a
        // full-height character.
        const PROBE: &str = "M";

        // onehand patch: the grid's own face, not one built here. The cell is
        // measured in whatever the glyph pass will draw in, features included,
        // or the lattice is laid out from one description of the font and
        // filled from another.
        let font = self.face(FontWeight::NORMAL, FontStyle::Normal);

        let text_run = TextRun {
            len: PROBE.len(),
            font,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        // Shape the probe character to get cell metrics
        let shaped =
            window
                .text_system()
                .shape_line(PROBE.into(), self.font_size, &[text_run], None);

        // Get the width from the shaped line
        if shaped.width > px(0.0) {
            self.cell_width = shaped.width;
        }

        // Calculate height from ascent + descent with optional multiplier
        let line_height = (shaped.ascent + shaped.descent).ceil();
        if line_height > px(0.0) {
            self.cell_height = line_height * self.line_height_multiplier;
        }
    }

    /// Lay a row's cells out into merged background rectangles.
    ///
    /// onehand patch: **backgrounds only.** This used to return batched text
    /// runs as well, and its one caller discarded them — so every row of every
    /// frame grouped the row's characters into runs, allocated a `String` for
    /// each, and threw the lot away before painting one glyph at a time anyway.
    ///
    /// # Arguments
    ///
    /// * `row` - The row number
    /// * `cells` - The row's cells, as (column, cell) pairs
    /// * `colors` - Terminal color configuration
    ///
    /// # Returns
    ///
    /// The row's background rectangles, with horizontally adjacent runs of the
    /// same colour merged into one.
    pub fn layout_backgrounds(
        &self,
        row: usize,
        cells: &[(usize, Cell)],
        colors: &Colors,
    ) -> Vec<BackgroundRect> {
        let mut backgrounds = Vec::new();
        self.collect_backgrounds(
            row,
            cells.iter().map(|(col, cell)| (*col, cell)),
            colors,
            &mut backgrounds,
        );
        self.merge_backgrounds(backgrounds)
    }

    /// onehand patch: the same rule, writing into a buffer the caller keeps.
    ///
    /// What [`Self::layout_backgrounds`] does, minus the `Vec` — the paint runs
    /// this once per row per frame, and a grid is forty rows.
    fn collect_backgrounds<'a>(
        &self,
        row: usize,
        cells: impl Iterator<Item = (usize, &'a Cell)>,
        colors: &Colors,
        out: &mut Vec<BackgroundRect>,
    ) {
        let mut current_bg: Option<BackgroundRect> = None;

        for (col, cell) in cells {
            // Skip wide character spacers
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }

            // onehand patch: through `cell_ink`, so the background this pass
            // paints and the foreground the glyph pass draws agree about which
            // way round an inverted cell is.
            let (_, bg_color) = cell_ink(&self.palette, cell, colors);

            let started = BackgroundRect {
                start_col: col,
                end_col: col + 1,
                row,
                color: bg_color,
            };

            // Handle background rectangles
            if let Some(bg_rect) = current_bg.as_mut() {
                if bg_rect.color == bg_color && bg_rect.end_col == col {
                    // Extend current background
                    bg_rect.end_col = col + 1;
                } else {
                    // Save current background and start new one
                    out.push(std::mem::replace(bg_rect, started));
                }
            } else {
                // Start new background
                current_bg = Some(started);
            }
        }

        if let Some(bg) = current_bg {
            out.push(bg);
        }
    }

    /// Merge adjacent background rects with same color.
    ///
    /// This optimization reduces the number of rectangles to paint by
    /// combining horizontally adjacent rectangles that share the same color.
    ///
    /// # Arguments
    ///
    /// * `rects` - Vector of background rectangles to merge
    ///
    /// # Returns
    ///
    /// A new vector with merged rectangles
    fn merge_backgrounds(&self, mut rects: Vec<BackgroundRect>) -> Vec<BackgroundRect> {
        Self::merge_in_place(&mut rects);
        rects
    }

    /// onehand patch: the merge, in place and in one pass.
    ///
    /// It used to build a second `Vec` and take its first element with
    /// `Vec::remove(0)`, which shifts the whole row's rectangles down one slot
    /// before the merging even starts.
    fn merge_in_place(rects: &mut Vec<BackgroundRect>) {
        if rects.len() < 2 {
            return;
        }

        let mut kept = 0;
        for next in 1..rects.len() {
            if rects[kept].can_merge_with(&rects[next]) {
                let end_col = rects[next].end_col;
                rects[kept].end_col = end_col;
            } else {
                kept += 1;
                rects.swap(kept, next);
            }
        }
        rects.truncate(kept + 1);
    }

    /// Paint terminal content to the window.
    ///
    /// This is the main rendering method that draws the terminal grid,
    /// including backgrounds, text, and cursor.
    ///
    /// # Arguments
    ///
    /// * `bounds` - The bounding box to render within
    /// * `padding` - Padding around the terminal content
    /// * `term` - The terminal state
    /// * `focused` - Whether the grid currently holds keyboard focus
    /// * `window` - The GPUI window
    /// * `cx` - The application context
    ///
    /// onehand patch: `focused` is ours. Only the view knows the answer, and the
    /// cursor is drawn differently without the keyboard -- see
    /// [`Self::paint_cursor`]. The app context is ours to the extent that it is
    /// now *used*: upstream took it as `_cx` and painted no text through it,
    /// because painting a shaped line needs it and upstream shaped none here.
    pub fn paint(
        &self,
        bounds: Bounds<Pixels>,
        padding: Edges<Pixels>,
        term: &Term<GpuiEventProxy>,
        focused: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        #[cfg(feature = "profiling")]
        let _paint = crate::profiling::Span::new(crate::profiling::Stage::Paint, 1);
        // Get terminal dimensions
        let grid = term.grid();
        let num_lines = grid.screen_lines();
        let num_cols = grid.columns();
        let colors = term.colors();

        // Calculate default background color
        let default_bg = self.palette.resolve(
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background),
            colors,
        );

        // Paint default background (covers full bounds including padding)
        window.paint_quad(quad(
            bounds,
            px(0.0),
            default_bg,
            Edges::<Pixels>::default(),
            transparent_black(),
            Default::default(),
        ));

        // Calculate origin offset (content starts after padding)
        let origin = Point {
            x: bounds.origin.x + padding.left,
            y: bounds.origin.y + padding.top,
        };

        // onehand patch: honour the scrollback viewport.
        //
        // `Grid: Index<Line>` addresses the *live* screen; `display_offset` is
        // how far the viewport has been scrolled up from it, and alacritty's own
        // conversion is `viewport = buffer + display_offset` (term/mod.rs:125).
        // Without this the grid renders the live screen no matter where the
        // scrollback sits, so scrolling appeared to do nothing.
        let display_offset = grid.display_offset() as i32;
        let selection = term.selection.as_ref().and_then(|s| s.to_range(term));

        // onehand patch: the buffers the row loop reuses.
        //
        // Each was allocated fresh per row per frame, so a forty-row grid paid
        // a hundred and twenty allocations a frame before drawing anything —
        // and a modal editor redraws the whole grid on every keystroke. The row
        // of cells that used to be *cloned* into a `Vec` is gone entirely: the
        // grid row is borrowed instead, so a screen of two thousand cells is no
        // longer copied to be looked at.
        let mut backgrounds: Vec<BackgroundRect> = Vec::with_capacity(num_cols);
        let mut spanned = vec![false; num_cols];
        let mut runs = RowRuns::default();
        // The four faces, made once for the whole grid rather than once per
        // character -- see `font_variants`.
        let fonts = self.font_variants();

        // Iterate over visible lines
        for line_idx in 0..num_lines {
            let line = Line(line_idx as i32 - display_offset);
            let row = &grid[line];

            // Lay the row out for backgrounds. onehand patch: into a buffer the
            // frame owns, over borrowed cells. The row used to be cloned once to
            // be collected and a second time to be handed over.
            backgrounds.clear();
            #[cfg(feature = "profiling")]
            let background_span = crate::profiling::Span::new(crate::profiling::Stage::Backgrounds, num_cols);
            self.collect_backgrounds(
                line_idx,
                (0..num_cols).map(|col| (col, &row[Column(col)])),
                colors,
                &mut backgrounds,
            );
            Self::merge_in_place(&mut backgrounds);

            // Paint backgrounds
            for bg_rect in &backgrounds {
                // Skip if it's the default background color
                if bg_rect.color == default_bg {
                    continue;
                }

                let x = origin.x + self.cell_width * (bg_rect.start_col as f32);
                let y = origin.y + self.cell_height * (bg_rect.row as f32);
                let width = self.cell_width * ((bg_rect.end_col - bg_rect.start_col) as f32);
                let height = self.cell_height;

                let rect_bounds = Bounds {
                    origin: Point { x, y },
                    size: Size { width, height },
                };

                window.paint_quad(quad(
                    rect_bounds,
                    px(0.0),
                    bg_rect.color,
                    Edges::<Pixels>::default(),
                    transparent_black(),
                    Default::default(),
                ));
            }

            // onehand patch: selection highlight.
            //
            // Painted per row after the backgrounds so it wins over them, and
            // before the glyph pass so the text stays on top. Upstream has no
            // selection at all -- `term.selection` was never read.
            if let Some(range) = selection {
                if line >= range.start.line && line <= range.end.line {
                    let first = if line == range.start.line {
                        range.start.column.0
                    } else {
                        0
                    };
                    let last = if line == range.end.line {
                        range.end.column.0
                    } else {
                        num_cols.saturating_sub(1)
                    };
                    if first <= last {
                        let x = origin.x + self.cell_width * (first as f32);
                        let y = origin.y + self.cell_height * (line_idx as f32);
                        let width = self.cell_width * ((last - first + 1) as f32);
                        window.paint_quad(quad(
                            Bounds {
                                origin: Point { x, y },
                                size: Size {
                                    width,
                                    height: self.cell_height,
                                },
                            },
                            px(0.0),
                            self.palette
                                .resolve(
                                    Color::Named(
                                        alacritty_terminal::vte::ansi::NamedColor::Foreground,
                                    ),
                                    colors,
                                )
                                .opacity(0.30),
                            Edges::<Pixels>::default(),
                            transparent_black(),
                            Default::default(),
                        ));
                    }
                }
            }

            #[cfg(feature = "profiling")]
            drop(background_span);
            #[cfg(feature = "profiling")]
            let boxes_span = crate::profiling::Span::new(crate::profiling::Stage::Boxes, num_cols);
            self.paint_box_drawing(origin, line_idx, row, colors, &mut spanned, window);
            #[cfg(feature = "profiling")]
            drop(boxes_span);

            // onehand patch: the glyphs, as runs rather than one call per
            // visible character -- see `split_row_runs`.
            #[cfg(feature = "profiling")]
            let runs_span = crate::profiling::Span::new(crate::profiling::Stage::Runs, num_cols);
            split_row_runs(
                &self.palette,
                (0..num_cols).map(|col| &row[Column(col)]),
                colors,
                &mut runs,
            );
            #[cfg(feature = "profiling")]
            drop(runs_span);
            self.paint_row_runs(origin, line_idx, &runs, &fonts, window, cx);
        }

        #[cfg(feature = "profiling")]
        let _cursor = crate::profiling::Span::new(crate::profiling::Stage::Cursor, 1);
        self.paint_cursor(origin, term, focused, window, cx);
        #[cfg(feature = "profiling")]
        crate::profiling::event("paint", 0, false);
    }

    /// onehand patch: the box-drawing passes for one row.
    ///
    /// Two of them, and the order is the point: a horizontal line is drawn
    /// across every cell it runs through in one span, so a border does not come
    /// out as a row of dashes with a gap at each cell boundary, and the cells
    /// that span covered are then drawn again for their *vertical* parts alone.
    ///
    /// `spanned` is the frame's buffer rather than this row's, and it is a
    /// column-indexed `Vec<bool>` where it used to be a `HashSet<usize>` — the
    /// question asked of it is "was this column covered", forty times a row, and
    /// hashing an integer to answer it is work with nothing to show for it.
    fn paint_box_drawing(
        &self,
        origin: Point<Pixels>,
        line_idx: usize,
        row: &Row<Cell>,
        colors: &Colors,
        spanned: &mut [bool],
        window: &mut Window,
    ) {
        let num_cols = row.len();
        let y_base = origin.y + self.cell_height * (line_idx as f32);
        // Vertically centred, which is where a horizontal line belongs.
        let cy = y_base + self.cell_height / 2.0;

        spanned.fill(false);

        // First pass: find and draw horizontal spans of box-drawing characters
        // This draws continuous lines across multiple cells to avoid gaps
        let mut col = 0;
        while col < num_cols {
            let cell = &row[Column(col)];
            let Some(weight) = box_drawing::get_horizontal_weight(cell.c) else {
                col += 1;
                continue;
            };

            let fg_color = self.palette.resolve(cell.fg, colors);
            let start_col = col;
            let mut end_col = col;

            // Look ahead for consecutive cells with same horizontal weight
            let mut next = col + 1;
            while next < num_cols {
                let next_cell = &row[Column(next)];
                // Must have same horizontal weight and same color
                if box_drawing::get_horizontal_weight(next_cell.c) != Some(weight)
                    || self.palette.resolve(next_cell.fg, colors) != fg_color
                {
                    break;
                }
                end_col = next;
                next += 1;
            }

            // Draw the horizontal span
            let start_x = origin.x + self.cell_width * (start_col as f32);
            let end_x = origin.x + self.cell_width * ((end_col + 1) as f32);

            box_drawing::draw_horizontal_span(
                start_x,
                end_x,
                cy,
                weight,
                self.cell_width,
                fg_color,
                window,
            );

            // Mark these columns as having horizontal drawn
            spanned[start_col..=end_col].fill(true);

            // Skip past this span
            col = next;
        }

        // Second pass: draw vertical components and non-horizontal box chars
        for col in 0..num_cols {
            let cell = &row[Column(col)];
            let ch = cell.c;

            if !box_drawing::is_box_drawing_char(ch) {
                continue;
            }

            let x = origin.x + self.cell_width * (col as f32);
            let fg_color = self.palette.resolve(cell.fg, colors);
            let cell_bounds = Bounds {
                origin: Point { x, y: y_base },
                size: Size {
                    width: self.cell_width,
                    height: self.cell_height,
                },
            };

            if spanned[col] {
                // Horizontal already drawn, just draw vertical components
                box_drawing::draw_vertical_components(
                    ch,
                    cell_bounds,
                    fg_color,
                    self.cell_width,
                    window,
                );
            } else {
                // Not part of a horizontal span, draw the whole character
                box_drawing::draw_box_character(ch, cell_bounds, fg_color, self.cell_width, window);
            }
        }
    }

    /// onehand patch: shape and paint one row's runs.
    ///
    /// Everything about *where* the runs came from is in [`split_row_runs`],
    /// which can be tested; what is left here is the half that needs a window.
    ///
    /// The forced cell width handed to `shape_line` is what keeps this a grid:
    /// gpui snaps each base glyph of the run to its own column rather than
    /// letting the font's accumulated advances decide, so a character that fell
    /// through to a fallback face lands on its cell like every other one.
    ///
    /// **What makes an untouched row cheap is gpui's, not ours**, and the
    /// dependency is worth naming rather than assuming: `shape_line` goes
    /// through `LineLayoutCache`, which carries entries from the previous frame
    /// into the current one on a hit, keyed by the text, the size, the resolved
    /// faces and the forced width. So a row nobody wrote to shapes nothing on
    /// the second frame and every frame after it — the call is a hash and an
    /// `Arc` clone. Batching is what makes that lookup cheap enough to matter:
    /// it is paid per run rather than per character. If that cache ever stops
    /// promoting across frames, this pass silently goes back to reshaping every
    /// visible row on every frame, with nothing here to say so.
    fn paint_row_runs(
        &self,
        origin: Point<Pixels>,
        line_idx: usize,
        runs: &RowRuns,
        fonts: &[Font; 4],
        window: &mut Window,
        cx: &mut App,
    ) {
        let y = origin.y + self.glyph_baseline(line_idx);

        for run in runs.drawable() {
            #[cfg(feature = "profiling")]
            let shape_span = crate::profiling::Span::new(crate::profiling::Stage::Shape, run.text.len());
            // onehand patch: a run one ASCII character long is the common case
            // in a column of tool output, and the table answers it without
            // allocating at all.
            let text: SharedString = match run.text.chars().next() {
                Some(ch) if ch.len_utf8() == run.text.len() => ascii_glyph(ch),
                _ => SharedString::from(run.text.clone()),
            };

            let text_run = TextRun {
                len: text.len(),
                font: fonts[run.style.font].clone(),
                color: run.style.color,
                background_color: None,
                // onehand patch: every underline the protocol has, in the
                // colour the program chose for it, plus the strikethrough
                // that used to be hard-coded away.
                underline: run.style.underline,
                strikethrough: run.style.strikethrough,
            };

            let shaped = window.text_system().shape_line(
                text,
                self.font_size,
                &[text_run],
                Some(self.cell_width),
            );
            #[cfg(feature = "profiling")]
            drop(shape_span);
            #[cfg(feature = "profiling")]
            let _glyphs = crate::profiling::Span::new(crate::profiling::Stage::Glyphs, run.text.len());

            // Paint at exact cell position (ignore errors)
            // onehand patch: gpui grew `TextAlign` + a wrap-width argument
            // since the crates.io release this was written against. A run is
            // laid out on the cell lattice and there is nothing to wrap.
            let _ = shaped.paint(
                Point {
                    x: origin.x + self.cell_width * (run.start_col as f32),
                    y,
                },
                self.cell_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }

    /// onehand patch: draw the cursor the child asked for, where the viewport
    /// actually is.
    ///
    /// Four things upstream did not do, and all four show up the moment a
    /// full-screen editor is running.
    ///
    /// The **shape** is the child's to choose. `DECSCUSR` is already parsed --
    /// `Term::cursor_style` answers with a shape and a blink flag -- and drawing
    /// a block regardless throws that away, which in a modal editor erases the
    /// one signal that says which mode it is in: insert asks for a beam,
    /// replace for an underline, and normal for the block.
    ///
    /// The **character underneath** has to survive. A filled quad painted after
    /// the glyph pass covers the glyph, so the cursor does not sit *on* a
    /// character, it hides one -- and what it hides is the character the person
    /// is about to act on. Repainting that one cell in the background colour on
    /// top of the block is the inversion every other terminal draws.
    ///
    /// **Hiding** has to be honoured. `DECTCEM` off means the child is drawing
    /// its own cursor or wants none, and a program that hides the cursor to
    /// redraw a frame will otherwise flicker a block through every repaint.
    ///
    /// And the **viewport** has to be taken into account: `grid.cursor.point` is
    /// a live-screen coordinate, so drawn without the scroll offset the cursor
    /// stays pinned where it would have been while the reader scrolls the text
    /// out from under it, and is drawn over unrelated scrollback.
    ///
    /// Blinking is deliberately not implemented: it would need a repaint on a
    /// timer for the life of every tab, in a view that otherwise only draws when
    /// bytes arrive. A steady cursor is a setting people choose on purpose;
    /// a terminal that repaints twice a second while idle is not.
    fn paint_cursor(
        &self,
        origin: Point<Pixels>,
        term: &Term<GpuiEventProxy>,
        focused: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        use alacritty_terminal::vte::ansi::CursorShape;

        if !term.mode().contains(TermMode::SHOW_CURSOR) {
            return;
        }

        let grid = term.grid();
        let colors = term.colors();
        let point = grid.cursor.point;

        // Same conversion the glyph pass uses, in the other direction: the grid
        // is addressed from the live screen and rows are drawn from the top of
        // the viewport.
        let row = point.line.0 + grid.display_offset() as i32;
        if row < 0 || row >= grid.screen_lines() as i32 {
            return;
        }

        let shape = match term.cursor_style().shape {
            CursorShape::Hidden => return,
            // A grid that does not hold focus draws an outline whatever the
            // child asked for. Two terminals side by side both drawing a solid
            // cursor is two claims on the keyboard, and only one of them is
            // true.
            _ if !focused => CursorShape::HollowBlock,
            shape => shape,
        };

        let cell_origin = Point {
            x: origin.x + self.cell_width * (point.column.0 as f32),
            y: origin.y + self.cell_height * (row as f32),
        };
        let color = self
            .palette
            .resolve(Color::Named(NamedColor::Cursor), colors);

        // Thickness for the two thin shapes. Derived from the cell rather than
        // fixed, so a beam stays visible when the grid is zoomed out and does
        // not become a second block when it is zoomed in.
        let stroke = (self.cell_width * 0.15).max(px(1.0));

        let bounds = match shape {
            CursorShape::Block | CursorShape::HollowBlock => Bounds {
                origin: cell_origin,
                size: Size {
                    width: self.cell_width,
                    height: self.cell_height,
                },
            },
            CursorShape::Beam => Bounds {
                origin: cell_origin,
                size: Size {
                    width: stroke,
                    height: self.cell_height,
                },
            },
            CursorShape::Underline => Bounds {
                origin: Point {
                    x: cell_origin.x,
                    y: cell_origin.y + self.cell_height - stroke,
                },
                size: Size {
                    width: self.cell_width,
                    height: stroke,
                },
            },
            CursorShape::Hidden => return,
        };

        if shape == CursorShape::HollowBlock {
            window.paint_quad(quad(
                bounds,
                px(0.0),
                transparent_black(),
                Edges::all(px(1.0)),
                color,
                Default::default(),
            ));
            return;
        }

        window.paint_quad(quad(
            bounds,
            px(0.0),
            color,
            Edges::<Pixels>::default(),
            transparent_black(),
            Default::default(),
        ));

        // Only the block covers its cell; a beam and an underline leave the
        // glyph where it was drawn.
        if shape != CursorShape::Block {
            return;
        }

        let cell = &grid[point];
        let ch = cell.c;
        if ch == ' ' || ch == '\0' {
            return;
        }

        // Drawn in the cell's own *background* rather than its foreground,
        // which would be invisible against a block painted in a colour derived
        // from that foreground. Asked of `cell_ink` and not of the palette
        // directly, because on an inverted cell the effective background is the
        // colour the text would otherwise have been -- and a cursor sitting on a
        // status line or a search hit is exactly where that happens.
        let (_, ink) = cell_ink(&self.palette, cell, colors);
        let flags = cell.flags;
        let text: SharedString = ascii_glyph(ch);
        let run = TextRun {
            len: text.len(),
            // onehand patch: built the same way the glyph pass builds its four,
            // so the character repainted over the block is the character that
            // was under it. One face and not the whole table: asking for all
            // four here would make three of them to throw away, on a path that
            // runs once a frame in a module whose point is not to.
            font: self.face(
                if flags.contains(Flags::BOLD) {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
                if flags.contains(Flags::ITALIC) {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
            ),
            color: ink,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // Shaped on the cell lattice like the glyph pass, so the character put
        // back over the block lands exactly where it was taken from.
        let shaped =
            window
                .text_system()
                .shape_line(text, self.font_size, &[run], Some(self.cell_width));
        let _ = shaped.paint(
            Point {
                x: cell_origin.x,
                // The glyph pass's own baseline, asked of the one function that
                // works it out.
                y: origin.y + self.glyph_baseline(row as usize),
            },
            self.cell_height,
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    /// onehand patch: draw the input method's in-progress composition.
    ///
    /// The grid holds nothing for this text -- it has not been committed, so it
    /// has never been near the child process -- which is why it is painted over
    /// the grid at the cursor rather than written into a cell. Drawing it is not
    /// a nicety: the input method hands its composition to the *application* to
    /// display, and an application that ignores it leaves someone typing a
    /// language that composes -- Vietnamese, Japanese, Korean, Chinese -- with
    /// no way to see the syllable they are halfway through.
    ///
    /// Returns the rectangle it drew into, which is where the candidate window
    /// belongs.
    pub fn paint_preedit(
        &self,
        bounds: Bounds<Pixels>,
        padding: Edges<Pixels>,
        term: &Term<GpuiEventProxy>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Bounds<Pixels> {
        let grid = term.grid();
        let colors = term.colors();
        let origin = Point {
            x: bounds.origin.x + padding.left,
            y: bounds.origin.y + padding.top,
        };
        let cursor = grid.cursor.point;
        let mut area = Bounds {
            origin: Point {
                x: origin.x + self.cell_width * (cursor.column.0 as f32),
                y: origin.y + self.cell_height * (cursor.line.0 as f32),
            },
            size: Size {
                width: self.cell_width,
                height: self.cell_height,
            },
        };
        if text.is_empty() {
            return area;
        }

        let foreground = self.palette.resolve(
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Foreground),
            colors,
        );
        let background = self.palette.resolve(
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background),
            colors,
        );

        // onehand patch: **not** `face`, which turns contextual alternates off.
        //
        // That is a rule about the *grid*: a cell may hold one character and no
        // font may fuse two of them into one glyph. A composition is not on the
        // grid — it is a run of ordinary text painted over it, several
        // characters long, and it is being read by somebody halfway through a
        // syllable. Contextual alternates are how a font joins letters that have
        // to be joined, so borrowing the grid's rule here would break the
        // shaping of the very scripts this is drawn for.
        let font = Font {
            family: self.font_family.clone().into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };
        let run = TextRun {
            len: text.len(),
            font,
            color: foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(text.into(), self.font_size, &[run], None);
        area.size.width = shaped.width.max(self.cell_width);

        // Painted over whatever the grid put there, because the composition
        // occupies the screen the cursor is sitting on and the two would
        // otherwise overlap into an unreadable pile.
        window.paint_quad(quad(
            area,
            px(0.0),
            background,
            Edges::<Pixels>::default(),
            transparent_black(),
            Default::default(),
        ));
        let _ = shaped.paint(
            area.origin,
            self.cell_height,
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        // The underline is the convention every terminal and text field uses to
        // say "this is not typed yet".
        window.paint_quad(quad(
            Bounds {
                origin: Point {
                    x: area.origin.x,
                    y: area.origin.y + self.cell_height - px(1.0),
                },
                size: Size {
                    width: area.size.width,
                    height: px(1.0),
                },
            },
            px(0.0),
            foreground,
            Edges::<Pixels>::default(),
            transparent_black(),
            Default::default(),
        ));
        area
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = TerminalRenderer::new(
            "Fira Code".to_string(),
            px(14.0),
            1.0,
            ColorPalette::default(),
        );
        assert_eq!(renderer.font_family, "Fira Code");
        assert_eq!(renderer.font_size, px(14.0));
        assert_eq!(renderer.line_height_multiplier, 1.0);
    }

    #[test]
    fn test_background_rect_merge() {
        let black = Hsla::black();

        let rect1 = BackgroundRect {
            start_col: 0,
            end_col: 5,
            row: 0,
            color: black,
        };

        let rect2 = BackgroundRect {
            start_col: 5,
            end_col: 10,
            row: 0,
            color: black,
        };

        assert!(rect1.can_merge_with(&rect2));

        let rect3 = BackgroundRect {
            start_col: 5,
            end_col: 10,
            row: 1,
            color: black,
        };

        assert!(!rect1.can_merge_with(&rect3));
    }

    #[test]
    fn test_merge_backgrounds() {
        let renderer = TerminalRenderer::new(
            "monospace".to_string(),
            px(14.0),
            1.0,
            ColorPalette::default(),
        );
        let black = Hsla::black();

        let rects = vec![
            BackgroundRect {
                start_col: 0,
                end_col: 5,
                row: 0,
                color: black,
            },
            BackgroundRect {
                start_col: 5,
                end_col: 10,
                row: 0,
                color: black,
            },
        ];

        let merged = renderer.merge_backgrounds(rects);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start_col, 0);
        assert_eq!(merged[0].end_col, 10);
    }

    // ── onehand patch: the attributes that change a cell's colours ──────────
    //
    // Testable at all because they were pulled out of the paint into functions
    // of (flags, palette): the drawing needs a window and cannot be reached
    // from here, while the *rule* it follows can.

    use alacritty_terminal::vte::ansi::NamedColor;

    fn cell_with(flags: Flags) -> Cell {
        let mut cell = Cell::default();
        cell.c = 'x';
        cell.fg = Color::Named(NamedColor::Red);
        cell.bg = Color::Named(NamedColor::Blue);
        cell.flags = flags;
        cell
    }

    #[test]
    fn inverse_swaps_the_two_colours() {
        let palette = ColorPalette::default();
        let colors = Colors::default();

        let (fg, bg) = cell_ink(&palette, &cell_with(Flags::empty()), &colors);
        let (inv_fg, inv_bg) = cell_ink(&palette, &cell_with(Flags::INVERSE), &colors);

        assert_eq!(inv_fg, bg);
        assert_eq!(inv_bg, fg);
    }

    /// Hiding is the last word: a cell that is both inverted and hidden must
    /// still be unreadable, because what it is hiding is usually a password.
    #[test]
    fn hidden_wins_over_inverse() {
        let palette = ColorPalette::default();
        let colors = Colors::default();

        let (fg, bg) = cell_ink(
            &palette,
            &cell_with(Flags::INVERSE | Flags::HIDDEN),
            &colors,
        );
        assert_eq!(fg, bg);
    }

    /// And dimming an inverted cell dims what is now in front, not what used to
    /// be.
    #[test]
    fn dim_applies_after_the_swap() {
        let palette = ColorPalette::default();
        let colors = Colors::default();

        let (plain_fg, _) = cell_ink(&palette, &cell_with(Flags::INVERSE), &colors);
        let (dim_fg, _) = cell_ink(&palette, &cell_with(Flags::INVERSE | Flags::DIM), &colors);

        assert!(dim_fg.l < plain_fg.l);
        assert_eq!(dim_fg.h, plain_fg.h);
    }

    #[test]
    fn only_undercurl_is_wavy() {
        let palette = ColorPalette::default();
        let colors = Colors::default();
        let ink = gpui::black();

        assert!(underline_style(&palette, &cell_with(Flags::empty()), &colors, ink).is_none());

        let curl = underline_style(&palette, &cell_with(Flags::UNDERCURL), &colors, ink).unwrap();
        assert!(curl.wavy);

        let straight =
            underline_style(&palette, &cell_with(Flags::UNDERLINE), &colors, ink).unwrap();
        assert!(!straight.wavy);

        // The three shapes GPUI cannot draw are still drawn, as straight lines:
        // saying "something is marked here" wrongly beats not saying it.
        for flag in [
            Flags::DOUBLE_UNDERLINE,
            Flags::DOTTED_UNDERLINE,
            Flags::DASHED_UNDERLINE,
        ] {
            let style = underline_style(&palette, &cell_with(flag), &colors, ink).unwrap();
            assert!(!style.wavy, "{flag:?} should fall back to a straight line");
        }
    }

    /// The half that carries the meaning. A language server marks an error and a
    /// warning with the same squiggle and a different colour, set per cell with
    /// its own escape rather than through the foreground.
    #[test]
    fn an_underline_keeps_the_colour_the_program_chose() {
        let palette = ColorPalette::default();
        let colors = Colors::default();
        let ink = gpui::black();

        let mut cell = cell_with(Flags::UNDERCURL);
        cell.set_underline_color(Some(Color::Named(NamedColor::Green)));

        let style = underline_style(&palette, &cell, &colors, ink).unwrap();
        assert_eq!(
            style.color,
            Some(palette.resolve(Color::Named(NamedColor::Green), &colors))
        );

        // With none of its own it follows the text, which is what a terminal
        // that never had `SGR 58` always did.
        let plain = underline_style(&palette, &cell_with(Flags::UNDERLINE), &colors, ink).unwrap();
        assert_eq!(plain.color, Some(ink));
    }

    #[test]
    fn strikeout_is_no_longer_dropped() {
        let ink = gpui::black();
        assert!(strikethrough_style(&cell_with(Flags::empty()), ink).is_none());
        assert!(strikethrough_style(&cell_with(Flags::STRIKEOUT), ink).is_some());
    }

    // ── onehand patch: the per-glyph path allocating nothing ────────────────

    /// Cloning out of the table has to give the same text as building one, or
    /// the fast path draws a different character from the slow one.
    #[test]
    fn the_ascii_table_agrees_with_building_a_string() {
        for ch in '!'..='~' {
            assert_eq!(ascii_glyph(ch).as_ref(), ch.to_string());
        }
        // Outside it, and still correct.
        for ch in ['é', '→', '中', '\u{1f600}'] {
            assert_eq!(ascii_glyph(ch).as_ref(), ch.to_string());
        }
    }

    /// Four faces, and each combination of bold and italic picks a different
    /// one. An index that collided would draw bold text upright, or italics
    /// heavy, with nothing to show for it but a font that looks wrong.
    #[test]
    fn each_weight_and_slant_gets_its_own_face() {
        let renderer = TerminalRenderer::new(
            "Fira Code".to_string(),
            px(14.0),
            1.0,
            ColorPalette::default(),
        );
        let fonts = renderer.font_variants();

        let plain = &fonts[TerminalRenderer::font_index(Flags::empty())];
        let bold = &fonts[TerminalRenderer::font_index(Flags::BOLD)];
        let italic = &fonts[TerminalRenderer::font_index(Flags::ITALIC)];
        let both = &fonts[TerminalRenderer::font_index(Flags::BOLD | Flags::ITALIC)];

        assert_eq!(plain.weight, FontWeight::NORMAL);
        assert_eq!(plain.style, FontStyle::Normal);
        assert_eq!(bold.weight, FontWeight::BOLD);
        assert_eq!(bold.style, FontStyle::Normal);
        assert_eq!(italic.weight, FontWeight::NORMAL);
        assert_eq!(italic.style, FontStyle::Italic);
        assert_eq!(both.weight, FontWeight::BOLD);
        assert_eq!(both.style, FontStyle::Italic);

        // All four indices are distinct, which is the property the table needs.
        let indices = [
            TerminalRenderer::font_index(Flags::empty()),
            TerminalRenderer::font_index(Flags::BOLD),
            TerminalRenderer::font_index(Flags::ITALIC),
            TerminalRenderer::font_index(Flags::BOLD | Flags::ITALIC),
        ];
        let mut seen = indices;
        seen.sort_unstable();
        assert_eq!(seen, [0, 1, 2, 3]);
    }

    /// The whole point of the measurement key: a renderer that has measured
    /// nothing is stale, and one whose font has since changed is stale again.
    /// Getting this wrong either re-measures every frame, which is the cost this
    /// removes, or never re-measures, which lays the grid out at the wrong size.
    #[test]
    fn the_measurement_is_stale_only_when_the_font_moved() {
        let mut renderer = TerminalRenderer::new(
            "Fira Code".to_string(),
            px(14.0),
            1.0,
            ColorPalette::default(),
        );
        assert!(renderer.needs_measure(), "nothing has been measured yet");

        // Standing in for `measure_cell`, which needs a window.
        renderer.measured_for = Some((
            renderer.font_family.clone(),
            renderer.font_size,
            renderer.line_height_multiplier,
        ));
        assert!(!renderer.needs_measure());

        renderer.font_size = px(15.0);
        assert!(renderer.needs_measure(), "a new size is a new cell");

        renderer.measured_for = Some((renderer.font_family.clone(), px(15.0), 1.0));
        renderer.font_family = "JetBrains Mono".to_string();
        assert!(renderer.needs_measure(), "a new family is a new cell");
    }

    // ── onehand patch: splitting a row into shaped runs ─────────────────────
    //
    // Testable because the splitting was kept apart from the painting: where a
    // run starts and stops is a function of the cells, and only placing it needs
    // a window. The property every one of these is really checking is that a
    // run's nth character is in its start_col + nth column — a run that has
    // swallowed a column it does not draw slides the rest of the line left, and
    // that is the failure the whole arrangement exists to make impossible.

    fn cells(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|c| {
                let mut cell = Cell::default();
                cell.c = c;
                cell
            })
            .collect()
    }

    fn split(cells: &[Cell]) -> Vec<(usize, String)> {
        let mut runs = RowRuns::default();
        split_row_runs(
            &ColorPalette::default(),
            cells.iter(),
            &Colors::default(),
            &mut runs,
        );
        runs.drawable()
            .map(|run| (run.start_col, run.text.clone()))
            .collect()
    }

    /// Every run has to land where its first character was, and hold one
    /// character per column from there.
    fn columns_line_up(cells: &[Cell]) {
        for (start_col, text) in split(cells) {
            for (offset, ch) in text.chars().enumerate() {
                assert_eq!(
                    cells[start_col + offset].c,
                    ch,
                    "column {} of the row is not what the run puts there",
                    start_col + offset
                );
            }
        }
    }

    #[test]
    fn one_style_is_one_run() {
        let cells = cells("hello");
        assert_eq!(split(&cells), vec![(0, "hello".to_string())]);
        columns_line_up(&cells);
    }

    /// The case batching is for: a space costs nothing to draw, so carrying it
    /// turns a line of prose into one shaping call rather than one per word.
    #[test]
    fn a_run_carries_the_spaces_inside_it() {
        let cells = cells("let x = 5;");
        assert_eq!(split(&cells), vec![(0, "let x = 5;".to_string())]);
        columns_line_up(&cells);
    }

    /// Trailing spaces are dropped, so the run's cache key is the words and not
    /// the width of the terminal.
    #[test]
    fn trailing_spaces_are_not_shaped() {
        let cells = cells("hi        ");
        assert_eq!(split(&cells), vec![(0, "hi".to_string())]);
    }

    /// A leading space starts nothing: the run has to begin at the column its
    /// first character is in.
    #[test]
    fn a_run_starts_at_its_first_character() {
        let cells = cells("    indented");
        assert_eq!(split(&cells), vec![(4, "indented".to_string())]);
        columns_line_up(&cells);
    }

    /// A colour change is a new run, and it starts at the column where the
    /// colour changed.
    #[test]
    fn a_change_of_colour_splits_the_run() {
        let mut cells = cells("redblue");
        for cell in &mut cells[3..] {
            cell.fg = Color::Named(NamedColor::Blue);
        }
        assert_eq!(
            split(&cells),
            vec![(0, "red".to_string()), (3, "blue".to_string())]
        );
        columns_line_up(&cells);
    }

    /// A decorated run refuses spaces, because an underline runs the width of
    /// the run it belongs to and would otherwise be drawn under a cell the
    /// character-at-a-time pass left bare.
    #[test]
    fn a_decorated_run_stops_at_a_space() {
        let mut cells = cells("a b");
        for cell in &mut cells {
            cell.flags = Flags::UNDERLINE;
        }
        assert_eq!(
            split(&cells),
            vec![(0, "a".to_string()), (2, "b".to_string())]
        );
        columns_line_up(&cells);
    }

    /// A box-drawing character is painted as lines by another pass, so it is a
    /// hole in the row that no run may be shaped across.
    #[test]
    fn a_box_drawing_character_breaks_the_run() {
        let cells = cells("ab│cd");
        assert_eq!(
            split(&cells),
            vec![(0, "ab".to_string()), (3, "cd".to_string())]
        );
        columns_line_up(&cells);
    }

    /// A double-width character covers two columns, so it is shaped alone and
    /// the cell after it — its spacer — belongs to nobody.
    #[test]
    fn a_wide_character_is_a_run_of_its_own() {
        let mut cells = cells("a中b");
        cells[1].flags = Flags::WIDE_CHAR;
        // The spacer alacritty writes into the column the wide character covers.
        let mut spacer = Cell::default();
        spacer.c = ' ';
        spacer.flags = Flags::WIDE_CHAR_SPACER;
        cells.insert(2, spacer);

        assert_eq!(
            split(&cells),
            vec![
                (0, "a".to_string()),
                (1, "中".to_string()),
                (3, "b".to_string())
            ]
        );
        columns_line_up(&cells);
    }

    /// An empty row asks for no shaping at all, which is most of a terminal most
    /// of the time.
    #[test]
    fn an_empty_row_is_no_runs() {
        assert!(split(&cells("          ")).is_empty());
        assert!(split(&vec![Cell::default(); 10]).is_empty());
    }

    /// And the buffer is reused across rows without carrying one row's text into
    /// the next, which is the one way a per-frame buffer goes wrong.
    #[test]
    fn the_run_buffer_does_not_leak_between_rows() {
        let mut runs = RowRuns::default();
        let palette = ColorPalette::default();
        let colors = Colors::default();

        let long = cells("a much longer first row");
        split_row_runs(&palette, long.iter(), &colors, &mut runs);

        let short = cells("hi");
        split_row_runs(&palette, short.iter(), &colors, &mut runs);

        let got: Vec<_> = runs
            .drawable()
            .map(|run| (run.start_col, run.text.clone()))
            .collect();
        assert_eq!(got, vec![(0, "hi".to_string())]);
    }

    /// The measurement the batching exists for, in the only form this crate can
    /// take without a window: how many shaping calls a row of ordinary source
    /// costs. One per visible character is what it used to be, and it is what a
    /// full-screen editor pays on every keystroke.
    #[test]
    fn a_row_of_source_costs_a_handful_of_shaping_calls() {
        let mut cells = cells("    let shaped = window.text_system().shape_line(text, size);");
        let printed = cells.iter().filter(|cell| cell.c != ' ').count();
        // Syntax highlighting is what splits a row in practice, so colour a few
        // stretches of it rather than measuring one flat colour.
        for cell in &mut cells[4..7] {
            cell.fg = Color::Named(NamedColor::Magenta);
        }
        for cell in &mut cells[21..40] {
            cell.fg = Color::Named(NamedColor::Yellow);
        }

        let runs = split(&cells).len();
        assert!(
            runs * 4 < printed,
            "{runs} runs for {printed} characters is not worth the machinery"
        );
    }

    #[test]
    fn a_cell_is_classified_by_what_would_draw_it() {
        assert_eq!(classify(&cells("x")[0]), CellGlyph::Glyph('x'));
        // A cell nobody has written to holds a space, which is why an untouched
        // screen costs no shaping at all.
        assert_eq!(classify(&Cell::default()), CellGlyph::Space);
        assert_eq!(classify(&cells("\0")[0]), CellGlyph::Blank);
        assert_eq!(classify(&cells("│")[0]), CellGlyph::Boxed);

        let mut wide = cells("中")[0].clone();
        wide.flags = Flags::WIDE_CHAR;
        assert_eq!(classify(&wide), CellGlyph::Wide('中'));

        let mut spacer = cells(" ")[0].clone();
        spacer.flags = Flags::WIDE_CHAR_SPACER;
        assert_eq!(classify(&spacer), CellGlyph::Blank);
    }

    /// And the answer has to reach the renderer the view holds, key included —
    /// carrying the numbers without it would re-measure on every frame anyway.
    #[test]
    fn adopting_metrics_carries_the_key_with_them() {
        let mut view_side = TerminalRenderer::new(
            "Fira Code".to_string(),
            px(14.0),
            1.0,
            ColorPalette::default(),
        );
        let mut painted = view_side.clone();
        painted.cell_width = px(8.0);
        painted.cell_height = px(17.0);
        painted.measured_for = Some(("Fira Code".to_string(), px(14.0), 1.0));

        view_side.adopt_metrics(&painted);

        assert_eq!(view_side.cell_width, px(8.0));
        assert_eq!(view_side.cell_height, px(17.0));
        assert!(!view_side.needs_measure());
    }
}
