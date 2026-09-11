//! A real GPU window mounting the same PTY/view as both terminal consumers.
//! Run with `SHELL=/absolute/path/to/workload cargo run --release -p onehand
//! --example terminal_perf`. The workload must remain alive while being sampled.
use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Window, div, px};
use onehand_terminal_ui::{Program, PtyTab, spawn_pty};

struct TerminalPerf {
    tab: PtyTab,
}

impl Render for TerminalPerf {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.tab.view().clone())
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(onehand::assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            // Optional full-host check. Use an isolated XDG_CONFIG_HOME with
            // agents = [] so the probe opens no agent sessions. The normal public
            // shell methods mount the actual Workbench Neovim or Terminal dock.
            if let Ok(mode) = std::env::var("PERF_APP") {
                assert!(matches!(mode.as_str(), "neovim" | "terminal"));
                onehand::shell::boot(cx);
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(2))
                        .await;
                    let handle = cx.update(|cx| cx.windows()[0]);
                    handle
                        .update(cx, |root, window, cx| {
                            let root = root.downcast::<gpui_component::Root>().expect("app Root");
                            let shell = root
                                .read(cx)
                                .view()
                                .clone()
                                .downcast::<onehand::shell::Shell>()
                                .expect("app Shell");
                            shell.update(cx, |shell, cx| {
                                if mode == "neovim" {
                                    shell.show_neovim(window, cx);
                                } else {
                                    shell.show_terminal(window, cx);
                                }
                            });
                            sample_frames(handle, cx);
                        })
                        .unwrap();
                })
                .detach();
                cx.activate(true);
                return;
            }
            gpui_component::Theme::global_mut(cx).mono_font_family = "Liberation Mono".into();
            let handle = cx
                .open_window(
                    gpui::WindowOptions {
                        app_id: Some("onehand-terminal-perf".into()),
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                            gpui::point(px(0.), px(0.)),
                            gpui::size(px(960.), px(1000.)),
                        ))),
                        ..Default::default()
                    },
                    |window, cx| {
                        let tab = spawn_pty(
                            &std::env::current_dir().unwrap(),
                            Program::Shell,
                            px(14.),
                            cx,
                            |_, cx| cx.quit(),
                        )
                        .expect("start benchmark PTY");
                        tab.view().read(cx).focus_handle().clone().focus(window, cx);
                        cx.new(|_| TerminalPerf { tab })
                    },
                )
                .unwrap();
            sample_frames(handle.into(), cx);
            cx.activate(true);
        });
}

fn sample_frames(handle: gpui::AnyWindowHandle, cx: &mut App) {
    let started = std::time::Instant::now();
    // Reading histograms does not invalidate the window. The idle phase
    // therefore also checks that the probe itself causes no extra frames.
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            if handle
                .update(cx, |_, window, _| {
                    let frames = window.frame_duration_snapshot().draw_duration_histogram;
                    eprintln!(
                        "sample_s={:.3} frames={} draw_ns={:.0} mean_us={:.1} p95_us={:.1} max_us={:.1}",
                        started.elapsed().as_secs_f64(),
                        frames.len(),
                        frames.mean() * frames.len() as f64,
                        frames.mean() / 1000.,
                        frames.value_at_quantile(0.95) as f64 / 1000.,
                        frames.max() as f64 / 1000.
                    );
                    #[cfg(feature = "terminal-profiling")]
                    for (stage, sample) in gpui_terminal::profiling::snapshot() {
                        eprintln!(
                            "terminal_stage stage={stage} calls={} ns={} units={}",
                            sample.calls, sample.nanos, sample.units
                        );
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
