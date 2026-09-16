//! Compare retained terminal rows (left) with a fresh renderer (right).
//! PERF_CACHE_STEP optionally names a file containing the desired case number.
use alacritty_terminal::{
    grid::Scroll,
    index::{Column, Line, Point as GridPoint, Side},
    selection::{Selection, SelectionType},
};
use gpui::{
    App, AppContext, Bounds, Context, Edges, IntoElement, ParentElement, Render, Styled, Window,
    canvas, div, point, px, size,
};
use gpui_terminal::{ColorPalette, GpuiEventProxy, TerminalRenderer, TerminalState};

struct Comparison {
    state: TerminalState,
    renderer: TerminalRenderer,
    case: usize,
    focused: bool,
    preedit: String,
}

impl Comparison {
    fn new() -> Self {
        let (tx, _) = std::sync::mpsc::channel();
        let mut state = TerminalState::with_scrollback(40, 8, 100, GpuiEventProxy::new(tx));
        state.process_bytes(b"\x1b[2J\x1b[H");
        state.process_bytes("plain -> != text\r\n\x1b[1;31mBOLD red\x1b[0m\r\n\x1b[4:3;58;5;2mcurly underline\x1b[0m\r\nwide: 界 combining: e\u{301}\r\n╭────────╮\r\n│ border │\r\n╰────────╯".as_bytes());
        Self {
            state,
            renderer: TerminalRenderer::new(
                "Liberation Mono".into(),
                px(14.),
                1.,
                ColorPalette::default(),
            ),
            case: 0,
            focused: true,
            preedit: String::new(),
        }
    }

    fn advance(&mut self, case: usize) {
        // Apply in sequence so every transition exercises already cached rows.
        match case {
            1 => self.state.process_bytes(b"\x1b[1;4H"),
            2 => self.state.with_term_mut(|term| {
                let mut selection = Selection::new(
                    SelectionType::Simple,
                    GridPoint::new(Line(0), Column(1)),
                    Side::Left,
                );
                selection.update(GridPoint::new(Line(2), Column(12)), Side::Right);
                term.selection = Some(selection);
            }),
            3 => {
                self.focused = false;
                self.preedit = "composition".into();
            }
            4 => {
                self.focused = true;
                self.preedit.clear();
                self.state.with_term_mut(|term| term.selection = None);
            }
            5 => self
                .state
                .process_bytes(b"\x1b[2;1H\x1b[7;32mINVERSE green\x1b[0m"),
            6 => self.state.process_bytes(
                b"\x1b]4;1;rgb:11/cc/ee\x07\x1b]10;rgb:ee/cc/11\x07\x1b]11;rgb:20/30/40\x07",
            ),
            7 => {
                self.renderer.palette = ColorPalette::builder()
                    .background(60, 30, 20)
                    .green(255, 100, 50)
                    .build()
            }
            8 => {
                self.renderer.font_size = px(18.);
                self.renderer.line_height_multiplier = 1.2;
            }
            9 => self.renderer.font_family = "DejaVu Sans Mono".into(),
            10 => {
                self.state.resize(32, 6);
                self.state
                    .process_bytes(b"\x1b[6;1H\r\nline 9\r\nline 10\r\nline 11");
            }
            11 => self
                .state
                .with_term_mut(|term| term.scroll_display(Scroll::Delta(2))),
            12 => self
                .state
                .process_bytes(b"\x1b[?1049h\x1b[2J\x1b[Halternate screen"),
            13 => self.state.process_bytes(b"\x1b[?1049l"),
            _ => {}
        }
        self.case = case;
    }
}

impl Render for Comparison {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.term_arc();
        let renderer = self.renderer.clone();
        let focused = self.focused;
        let preedit = self.preedit.clone();
        let case = self.case;
        div().size_full().flex().children([false, true].map(move |fresh| {
            let state = state.clone();
            let mut renderer = renderer.clone();
            let preedit = preedit.clone();
            canvas(|_, _, _| (), move |bounds, _, window, cx| {
                if fresh {
                    renderer = TerminalRenderer::new(renderer.font_family.clone(), renderer.font_size,
                        renderer.line_height_multiplier, renderer.palette.clone());
                }
                renderer.ensure_measured(window);
                let term = state.lock();
                let padding = Edges::all(px(8.));
                renderer.paint(bounds, padding, &term, focused, window, cx);
                renderer.paint_preedit(bounds, padding, &term, &preedit, window, cx);
                eprintln!("cache_comparison case={case} fresh={fresh} scale={} x={} y={} width={} height={}",
                    window.scale_factor(), f32::from(bounds.origin.x), f32::from(bounds.origin.y),
                    f32::from(bounds.size.width), f32::from(bounds.size.height));
            }).w(px(448.)).h(px(320.))
        }))
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(onehand::assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            let window = cx
                .open_window(
                    gpui::WindowOptions {
                        app_id: Some("onehand-terminal-cache".into()),
                        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
                            point(px(0.), px(0.)),
                            size(px(896.), px(320.)),
                        ))),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| Comparison::new()),
                )
                .unwrap();
            if let Ok(path) = std::env::var("PERF_CACHE_STEP") {
                cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(100))
                            .await;
                        let next = std::fs::read_to_string(&path)
                            .ok()
                            .and_then(|s| s.trim().parse::<usize>().ok())
                            .unwrap_or(0)
                            .min(13);
                        if window
                            .update(cx, |view, _, cx| {
                                if next > view.case {
                                    for case in view.case + 1..=next {
                                        view.advance(case);
                                    }
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            cx.activate(true);
        });
}
