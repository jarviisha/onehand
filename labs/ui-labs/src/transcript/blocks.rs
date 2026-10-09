//! How each transcript block draws. The canned turns that use them are in the
//! parent module; nothing here knows what they say.
use super::*;
use gpui::{ElementId, HighlightStyle, StyleRefinement, StyledText};
use gpui_component::spinner::Spinner;
use gpui_component::text::{TextView, TextViewStyle};

/// `text` with its backticks taken out, and where each backticked span fell
/// in what is left: inline code in a label too short to be markdown.
pub(super) fn code_spans(text: &str) -> (String, Vec<std::ops::Range<usize>>) {
    let mut out = String::new();
    let mut spans = Vec::new();
    for (i, part) in text.split('`').enumerate() {
        let start = out.len();
        out.push_str(part);
        if i % 2 == 1 {
            spans.push(start..out.len());
        }
    }
    (out, spans)
}

/// Each tool's kind: its glyph, and the words the summary counts it in.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Read,
    Search,
    Edit,
    Create,
    Delete,
    Move,
    Run,
    Fetch,
}

impl Kind {
    fn icon(self) -> Icon {
        match self {
            Kind::Read => Icon::new(IconName::File),
            Kind::Search => Icon::new(IconName::Search),
            Kind::Edit | Kind::Create => Icon::empty().path(crate::assets::SQUARE_PEN),
            Kind::Delete => Icon::new(IconName::Delete),
            Kind::Move => Icon::new(IconName::ArrowRight),
            Kind::Run => Icon::new(IconName::SquareTerminal),
            Kind::Fetch => Icon::new(IconName::Globe),
        }
    }

    /// The past verb and the thing counted, singular and plural.
    fn words(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Kind::Read => ("read", "file", "files"),
            Kind::Search => ("searched", "time", "times"),
            Kind::Edit => ("edited", "file", "files"),
            Kind::Create => ("created", "file", "files"),
            Kind::Delete => ("deleted", "file", "files"),
            Kind::Move => ("moved", "file", "files"),
            Kind::Run => ("ran", "command", "commands"),
            Kind::Fetch => ("fetched", "page", "pages"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum State {
    Done,
    Running,
    /// Cut off by Stop before it finished.
    Stopped,
    /// Failed, with what it said (`exit 101`).
    Failed(&'static str),
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Line {
    Same,
    Added,
    Removed,
}

/// What a tool opens to.
#[derive(Clone, Copy)]
pub(super) enum Detail {
    None,
    /// What it read or found, line by line.
    Lines(&'static [&'static str]),
    /// What a command printed; only the tail shows until asked.
    Output(&'static [&'static str]),
    /// A diff, its first line's number, and the rows.
    Diff(u32, &'static [(Line, &'static str)]),
    /// A diff too large to draw unasked: the lines it adds and removes, and
    /// the first of its rows, all that is drawn once it is asked for.
    Large(usize, usize, &'static [(Line, &'static str)]),
}

#[derive(Clone, Copy)]
pub(super) struct Tool {
    pub id: &'static str,
    pub kind: Kind,
    pub verb: &'static str,
    pub object: &'static str,
    pub state: State,
    pub detail: Detail,
}

impl Tool {
    fn counts(&self) -> (usize, usize) {
        match self.detail {
            Detail::Diff(_, rows) => (
                rows.iter().filter(|(l, _)| *l == Line::Added).count(),
                rows.iter().filter(|(l, _)| *l == Line::Removed).count(),
            ),
            Detail::Large(added, removed, _) => (added, removed),
            _ => (0, 0),
        }
    }
}

/// The sentence a run of tools folds into: each running tool named first,
/// then the finished ones, each kind counted in the order it first appears.
pub(super) fn summary(tools: &[Tool]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for t in tools.iter().filter(|t| t.state == State::Running) {
        parts.push(format!("running {}", t.object));
    }
    let mut seen: Vec<Kind> = Vec::new();
    for t in tools.iter().filter(|t| t.state != State::Running) {
        if !seen.contains(&t.kind) {
            seen.push(t.kind);
        }
    }
    for kind in seen {
        let n = tools
            .iter()
            .filter(|t| t.kind == kind && t.state != State::Running)
            .count();
        let (verb, one, many) = kind.words();
        parts.push(format!("{verb} {n} {}", if n == 1 { one } else { many }));
    }
    let mut s = parts.join(", ");
    if let Some(first) = s.get(..1) {
        s.replace_range(..1, &first.to_uppercase());
    }
    s
}

impl Labs {
    pub(super) fn is_open(&self, id: &str, at_start: bool) -> bool {
        at_start != self.flipped.contains(id)
    }

    fn flip(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.flipped.remove(&id) {
            self.flipped.insert(id);
        }
        cx.notify();
    }

    /// A short label (a plan step) with its inline code in the accent.
    pub(super) fn label(&self, p: &Palette, text: &str) -> StyledText {
        let (text, spans) = code_spans(text);
        let ink = HighlightStyle {
            color: Some(p.accent),
            ..Default::default()
        };
        StyledText::new(text).with_highlights(spans.into_iter().map(|r| (r, ink)))
    }

    /// Prose as markdown, selectable so it can be copied, drawn by the
    /// library's text view as the app draws an answer. Its sizes follow the
    /// reading zoom: headings two steps up and one, then weight alone; inline
    /// code in the accent with nothing behind it; a fenced block in the same
    /// sunken well as a tool's output, its language in its corner.
    pub(crate) fn md(
        &self,
        p: &Palette,
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        window: &Window,
        cx: &App,
    ) -> TextView {
        let mut style = TextViewStyle::default()
            .paragraph_gap(rems(PARAGRAPH_GAP))
            .heading_font_size(|level, base| match level {
                1 => base * (TEXT_READ_H1 / TEXT_READ),
                2 => base * (TEXT_READ_H2 / TEXT_READ),
                _ => base,
            })
            .inline_code(HighlightStyle {
                color: Some(p.accent),
                // `None` brings back the library's filled fallback.
                background_color: Some(gpui::transparent_black()),
                ..Default::default()
            })
            .code_block(
                StyleRefinement::default()
                    .p_3()
                    .rounded(cx.theme().radius_lg)
                    .bg(p.sunken)
                    .text_size(self.read(TEXT_READ_SM))
                    .line_height(self.read(TEXT_READ_SM * LEADING_READ)),
            )
            .table_cell(StyleRefinement::default().px_3().py_1p5());
        style.heading_base_font_size = window.rem_size() * (TEXT_READ * self.zoom);
        let muted = p.muted;
        TextView::markdown(id, text)
            .selectable(true)
            .style(style)
            .code_block_actions(move |block, _, _| {
                div().text_xs().text_color(muted).children(block.lang())
            })
    }

    /// A sunken well for machine text at the reading size under the body.
    pub(super) fn read_well(&self, p: &Palette, mono: SharedString, cx: &App) -> gpui::Div {
        v_flex()
            .p_3()
            .rounded(cx.theme().radius_lg)
            .bg(p.sunken)
            .font_family(mono)
            .text_size(self.read(TEXT_READ_SM))
            .line_height(self.read(TEXT_READ_SM * LEADING_READ))
            // Machine lines never wrap; what runs past the well is cut at
            // its edge rather than drawn into the gutter.
            .overflow_x_hidden()
    }

    /// The line every folding block opens from: a chevron that turns, then
    /// what the block says about itself, lit under the pointer.
    pub(super) fn fold_line(
        &self,
        p: &Palette,
        id: &'static str,
        at_start: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let open = self.is_open(id, at_start);
        h_flex()
            .id(id)
            .gap_2()
            .min_w_0()
            .cursor_pointer()
            .text_size(self.read(TEXT_READ_SM))
            .text_color(p.text2)
            .hover(|d| d.text_color(p.text))
            .on_click(cx.listener(move |this, _, _, cx| this.flip(id.to_string(), cx)))
            .child(chevron(open))
    }

    /// The agent's reasoning: *Thought for Ns* once done, a spinner and
    /// *Thinking…* while it runs.
    pub(super) fn thought(
        &self,
        p: &Palette,
        id: &'static str,
        secs: Option<u32>,
        text: &'static str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let running = secs.is_none();
        let line = self.fold_line(p, id, running, cx);
        let line = match secs {
            Some(s) => line.child(format!("Thought for {s}s")),
            None => line
                .text_color(p.accent)
                .child(Spinner::new().xsmall())
                .child("Thinking…"),
        };
        v_flex()
            .gap_2()
            .child(line)
            .when(self.is_open(id, running), |d| {
                d.child(
                    div()
                        .pl_5()
                        .text_size(self.read(TEXT_READ_SM))
                        .text_color(p.muted)
                        .child(self.md(
                            p,
                            SharedString::from(format!("{id}/text")),
                            text,
                            window,
                            cx,
                        )),
                )
            })
    }

    /// A run of tools folded into one sentence; opened, a row per tool.
    pub(super) fn activity(
        &self,
        p: &Palette,
        mono: SharedString,
        id: &'static str,
        elapsed: Option<&'static str>,
        tools: &[Tool],
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let running = tools.iter().any(|t| t.state == State::Running);
        let failed = tools
            .iter()
            .filter(|t| matches!(t.state, State::Failed(_)))
            .count();
        let (added, removed) = tools
            .iter()
            .map(Tool::counts)
            .fold((0, 0), |(a, r), (x, y)| (a + x, r + y));
        v_flex()
            .gap_2()
            .child(
                self.fold_line(p, id, running, cx)
                    .when(running, |d| {
                        d.child(Spinner::new().xsmall().color(p.accent))
                    })
                    .child(div().truncate().child(summary(tools)))
                    .when(failed > 0, |d| {
                        d.child(div().text_color(p.muted).child("·"))
                            .child(div().text_color(p.danger).child(format!("{failed} failed")))
                    })
                    .when(added + removed > 0, |d| d.child(counts(p, added, removed)))
                    .when_some(elapsed, |d, e| {
                        d.child(div().font_family(mono.clone()).text_color(p.muted).child(e))
                    }),
            )
            .when(self.is_open(id, running), |d| {
                d.child(
                    v_flex()
                        .pl_5()
                        .gap_2()
                        .children(tools.iter().map(|t| self.tool(p, mono.clone(), t, cx))),
                )
            })
    }

    /// One tool: its glyph (a spinner while it runs), the verb, what it
    /// worked on, and its outcome; opened, what it read, printed or changed.
    fn tool(&self, p: &Palette, mono: SharedString, t: &Tool, cx: &mut Context<Self>) -> gpui::Div {
        let folds = !matches!(t.detail, Detail::None);
        let running = t.state == State::Running;
        let open = folds && self.is_open(t.id, running);
        let (added, removed) = t.counts();
        let id = t.id;
        let row = h_flex()
            .id(id)
            .gap_2()
            .min_w_0()
            .text_size(self.read(TEXT_READ_SM))
            .text_color(p.text2)
            .when(folds, |d| {
                d.cursor_pointer()
                    .hover(|d| d.text_color(p.text))
                    .on_click(cx.listener(move |this, _, _, cx| this.flip(id.to_string(), cx)))
            })
            .child(
                div()
                    .w_3()
                    .flex_none()
                    .when(folds, |d| d.child(chevron(open))),
            )
            .child(if running {
                Spinner::new().xsmall().color(p.accent).into_any_element()
            } else {
                t.kind
                    .icon()
                    .xsmall()
                    .text_color(p.muted)
                    .into_any_element()
            })
            .child(t.verb)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(mono.clone())
                    .text_color(p.text)
                    .when(t.kind == Kind::Delete, |d| {
                        d.line_through().text_color(p.muted)
                    })
                    .child(t.object),
            )
            .when(added + removed > 0, |d| d.child(counts(p, added, removed)))
            .map(|d| match t.state {
                State::Failed(why) => d.child(div().flex_none().text_color(p.danger).child(why)),
                State::Stopped => d.child(div().flex_none().text_color(p.muted).child("stopped")),
                State::Done | State::Running => d,
            });
        v_flex().gap_1().child(row).when(open, |d| {
            d.child(div().pl_5().child(self.detail(p, mono, t, cx)))
        })
    }

    fn detail(
        &self,
        p: &Palette,
        mono: SharedString,
        t: &Tool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match t.detail {
            Detail::None => div().into_any_element(),
            Detail::Lines(lines) => self
                .read_well(p, mono, cx)
                .text_color(p.text2)
                .children(lines.iter().map(|l| div().whitespace_nowrap().child(*l)))
                .into_any_element(),
            Detail::Output(lines) => {
                let all_id = format!("{}/all", t.id);
                let all = self.is_open(&all_id, false);
                let hidden = lines.len().saturating_sub(OUTPUT_TAIL);
                let shown = if all { lines } else { &lines[hidden..] };
                self.read_well(p, mono, cx)
                    .text_color(p.text2)
                    .when(hidden > 0, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(all_id.clone()))
                                .pb_1()
                                .cursor_pointer()
                                .font_family(cx.theme().font_family.clone())
                                .text_xs()
                                .text_color(p.muted)
                                .hover(|d| d.text_color(p.text))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.flip(all_id.clone(), cx)
                                    }),
                                )
                                .child(if all {
                                    "Show less".to_string()
                                } else {
                                    format!("Show {hidden} earlier lines")
                                }),
                        )
                    })
                    .children(shown.iter().map(|l| {
                        div()
                            .whitespace_nowrap()
                            .when(failing(l), |d| d.text_color(p.danger))
                            .child(*l)
                    }))
                    .into_any_element()
            }
            Detail::Diff(first, rows) => self.diff(p, mono, first, rows, cx).into_any_element(),
            Detail::Large(added, removed, rows) => {
                let load_id = format!("{}/load", t.id);
                if self.is_open(&load_id, false) {
                    let left = (added + removed).saturating_sub(rows.len());
                    return v_flex()
                        .gap_1()
                        .child(self.diff(p, mono, 1, rows, cx))
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted)
                                .child(format!("{left} more changed lines not shown")),
                        )
                        .into_any_element();
                }
                let load = action(SharedString::from(load_id.clone()))
                    .ghost()
                    .xsmall()
                    .label("Load diff")
                    .on_click(cx.listener(move |this, _, _, cx| this.flip(load_id.clone(), cx)));
                self.read_well(p, mono, cx)
                    .font_family(cx.theme().font_family.clone())
                    .child(
                        h_flex()
                            .gap_2()
                            .text_color(p.muted)
                            .child(format!(
                                "Large diff · {} changed lines, not drawn",
                                added + removed
                            ))
                            .child(div().flex_1())
                            .child(load),
                    )
                    .into_any_element()
            }
        }
    }

    fn diff(
        &self,
        p: &Palette,
        mono: SharedString,
        first: u32,
        rows: &[(Line, &'static str)],
        cx: &App,
    ) -> gpui::Div {
        let mut number = first;
        self.read_well(p, mono, cx)
            .px_0()
            .children(rows.iter().map(|(kind, text)| {
                let (sign, ink) = match kind {
                    Line::Same => (" ", p.muted),
                    Line::Added => ("+", p.success),
                    Line::Removed => ("−", p.danger),
                };
                let shown = (*kind != Line::Removed).then(|| {
                    number += 1;
                    (number - 1).to_string()
                });
                h_flex()
                    .px_3()
                    .when(*kind != Line::Same, |d| d.bg(ink.opacity(STATE_TINT)))
                    .child(
                        div()
                            .w_8()
                            .flex_none()
                            .text_right()
                            .pr_3()
                            .text_color(p.muted)
                            .child(shown.unwrap_or_default()),
                    )
                    .child(div().w_4().flex_none().text_color(ink).child(sign))
                    .child(div().whitespace_nowrap().text_color(p.text).child(*text))
            }))
    }

    /// The agent's plan: each step's state as a glyph, the done ones struck.
    pub(super) fn plan(
        &self,
        p: &Palette,
        id: &'static str,
        steps: &[(State, &'static str)],
        pending: &[&'static str],
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let done = steps.iter().filter(|(s, _)| *s == State::Done).count();
        let total = steps.len() + pending.len();
        let row = |icon: Icon, ink: Hsla, text: &str, struck: bool| {
            h_flex().gap_2().child(icon.xsmall().text_color(ink)).child(
                div()
                    .text_color(if struck { p.muted } else { p.text })
                    .when(struck, |d| d.line_through())
                    .child(self.label(p, text)),
            )
        };
        v_flex()
            .gap_2()
            .child(
                self.fold_line(p, id, true, cx)
                    .child("Plan")
                    .child(div().text_color(p.muted).child(format!("{done}/{total}"))),
            )
            .when(self.is_open(id, true), |d| {
                d.child(
                    v_flex()
                        .pl_5()
                        .gap_1()
                        .text_size(self.read(TEXT_READ_SM))
                        .children(steps.iter().map(|(state, text)| match state {
                            State::Done => row(Icon::new(IconName::Check), p.success, text, true),
                            State::Running => {
                                row(Icon::new(IconName::LoaderCircle), p.accent, text, false)
                            }
                            State::Failed(_) => {
                                row(Icon::new(IconName::CircleX), p.danger, text, false)
                            }
                            State::Stopped => row(Icon::new(IconName::Dash), p.muted, text, false),
                        }))
                        .children(
                            pending
                                .iter()
                                .map(|text| row(Icon::new(IconName::Dash), p.muted, text, false)),
                        ),
                )
            })
    }

    /// A permission or a question once answered: no fold, the answer said
    /// after what was asked.
    pub(super) fn settled(
        &self,
        p: &Palette,
        mono: Option<SharedString>,
        verb: &'static str,
        asked: &'static str,
        answer: &'static str,
        refused: bool,
    ) -> gpui::Div {
        h_flex()
            .gap_2()
            .min_w_0()
            .text_size(self.read(TEXT_READ_SM))
            .text_color(p.text2)
            .child(Icon::new(IconName::User).xsmall().text_color(p.muted))
            .child(div().flex_none().child(verb))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(if refused { p.muted } else { p.text })
                    .when(refused, |d| d.line_through())
                    .when_some(mono, |d, m| d.font_family(m))
                    .child(asked),
            )
            .child(div().text_color(p.muted).child("·"))
            .child(
                div()
                    .flex_none()
                    .when(refused, |d| d.text_color(p.danger))
                    .child(answer),
            )
    }

    /// The prompt: the one filled bubble, right-aligned, with what was
    /// attached under its words.
    pub(crate) fn bubble(
        &self,
        p: &Palette,
        text: impl Into<SharedString>,
        files: &[&str],
        cx: &App,
    ) -> gpui::Div {
        h_flex().justify_end().child(
            v_flex()
                .max_w(rems(BUBBLE_MAX))
                .gap_1()
                .px_3()
                .py_2()
                .rounded(cx.theme().radius_lg)
                .bg(p.sunken)
                .child(text.into())
                .when(!files.is_empty(), |d| {
                    d.child(h_flex().flex_wrap().gap_2().children(files.iter().map(|f| {
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(p.muted)
                            .child(Icon::new(IconName::File).xsmall())
                            .child(f.to_string())
                    })))
                }),
        )
    }

    /// Under a finished turn: copy the answer, and how long the turn took.
    pub(super) fn footer(
        &self,
        p: &Palette,
        id: &'static str,
        took: &'static str,
        answer: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        h_flex()
            .gap_2()
            .text_xs()
            .text_color(p.muted)
            .child(
                action(id)
                    .ghost()
                    .xsmall()
                    .icon(IconName::Copy)
                    .tooltip("Copy this answer")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(answer.to_string()))
                    })),
            )
            .child(format!("Processed in {took}"))
    }

    pub(crate) fn notice(&self, p: &Palette, text: &'static str) -> gpui::Div {
        h_flex()
            .justify_center()
            .text_xs()
            .text_color(p.muted)
            .child(text)
    }

    pub(super) fn error(&self, p: &Palette, text: &'static str, cx: &App) -> gpui::Div {
        h_flex()
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(p.danger)
            .bg(p.danger.opacity(STATE_TINT))
            .text_size(self.read(TEXT_READ_SM))
            .text_color(p.danger)
            .child(
                div()
                    .h(self.read(TEXT_READ_SM * LEADING_READ))
                    .flex()
                    .items_center()
                    .child(Icon::new(IconName::TriangleAlert).xsmall()),
            )
            .child(text)
    }
}

/// A line of output that reports a failure: one that starts with what a
/// failure starts with, or a test marked `FAILED`. A count such as
/// `0 failed` in a passing summary is not one.
fn failing(line: &str) -> bool {
    let start = line.trim_start().to_lowercase();
    ["error", "failed", "fatal", "panic", "assertion", "thread '"]
        .iter()
        .any(|w| start.starts_with(w))
        || line.ends_with(" FAILED")
        || line.contains("result: FAILED")
}

fn chevron(open: bool) -> Icon {
    Icon::new(IconName::ChevronRight)
        .xsmall()
        .rotate(gpui::radians(if open {
            std::f32::consts::FRAC_PI_2
        } else {
            0.0
        }))
}

/// `+N −N` in their states' ink, a zero left out.
fn counts(p: &Palette, added: usize, removed: usize) -> gpui::Div {
    h_flex()
        .flex_none()
        .gap_1()
        .when(added > 0, |d| {
            d.child(div().text_color(p.success).child(format!("+{added}")))
        })
        .when(removed > 0, |d| {
            d.child(div().text_color(p.danger).child(format!("−{removed}")))
        })
}

#[cfg(test)]
mod tests;
