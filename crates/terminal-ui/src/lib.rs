//! Shared PTY ownership and terminal-grid setup used by the shell dock and
//! built-in Neovim plugin. Rendering and input remain in the main GPUI process.

use gpui::{App, AppContext as _, Entity, SharedString, Window};
use gpui_component::ActiveTheme as _;
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SCROLLBACK: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Shell,
    Neovim,
}

pub struct PtyTab {
    view: Entity<TerminalView>,
    label: SharedString,
    _pty: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyTab {
    pub fn view(&self) -> &Entity<TerminalView> {
        &self.view
    }
    pub fn label(&self) -> SharedString {
        self.label.clone()
    }
    pub fn finished(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    pub fn set_font_size(&self, size: gpui::Pixels, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            view.update_config(
                TerminalConfig {
                    font_size: size,
                    ..view.config().clone()
                },
                cx,
            );
        });
    }

    pub fn set_palette(&self, colors: ColorPalette, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            view.update_config(
                TerminalConfig {
                    colors,
                    ..view.config().clone()
                },
                cx,
            );
        });
    }
}

impl Drop for PtyTab {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct TerminalThemeKey {
    dark: bool,
    colors: [gpui::Hsla; 17],
}

impl TerminalThemeKey {
    pub fn current(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            dark: theme.mode.is_dark(),
            colors: [
                theme.background,
                theme.foreground,
                theme.muted_foreground,
                theme.caret,
                theme.red,
                theme.red_light,
                theme.green,
                theme.green_light,
                theme.yellow,
                theme.yellow_light,
                theme.blue,
                theme.blue_light,
                theme.magenta,
                theme.magenta_light,
                theme.cyan,
                theme.cyan_light,
                theme.border,
            ],
        }
    }
}

fn command(program: Program) -> Result<(CommandBuilder, SharedString), String> {
    match program {
        Program::Shell => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let label = Path::new(&shell)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| shell.clone());
            Ok((CommandBuilder::new(&shell), label.into()))
        }
        Program::Neovim => {
            let found = std::env::var_os("PATH")
                .into_iter()
                .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
                .map(|dir| dir.join("nvim"))
                .find(|candidate| candidate.is_file())
                .ok_or_else(|| "Neovim is not installed, or `nvim` is not on PATH".to_string())?;
            Ok((CommandBuilder::new(found), "nvim".into()))
        }
    }
}

pub fn spawn_pty(
    cwd: &PathBuf,
    program: Program,
    font_size: gpui::Pixels,
    cx: &mut App,
    on_exit: impl Fn(&mut Window, &mut App) + 'static,
) -> Result<PtyTab, String> {
    let config = TerminalConfig {
        scrollback: SCROLLBACK,
        font_size,
        colors: terminal_palette(cx),
        font_family: cx.theme().mono_font_family.to_string(),
        ..TerminalConfig::default()
    };
    let pty = NativePtySystem::default()
        .openpty(PtySize {
            rows: config.rows as u16,
            cols: config.cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let (mut command, label) = command(program)?;
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    let child = pty
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
    let reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let master = Arc::new(Mutex::new(pty.master));
    let view = cx.new(|cx| {
        TerminalView::new(writer, reader, config, cx)
            .with_resize_callback({
                let master = master.clone();
                move |cols, rows| {
                    if let Ok(master) = master.lock() {
                        let _ = master.resize(PtySize {
                            rows: rows as u16,
                            cols: cols as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                }
            })
            .with_clipboard_store_callback(|_, _, text| {
                if let Ok(mut clipboard) = gpui_terminal::Clipboard::new() {
                    let _ = clipboard.copy(text);
                }
            })
            .with_exit_callback({
                let on_exit = std::rc::Rc::new(on_exit);
                move |window, cx| {
                    let on_exit = on_exit.clone();
                    window.defer(cx, move |window, cx| on_exit(window, cx));
                }
            })
    });
    Ok(PtyTab {
        view,
        label,
        _pty: master,
        child,
    })
}

pub fn terminal_palette(cx: &App) -> ColorPalette {
    fn rgb(color: gpui::Hsla) -> (u8, u8, u8) {
        let color = color.to_rgb();
        let c = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        (c(color.r), c(color.g), c(color.b))
    }
    let t = cx.theme();
    let background = rgb(t.background);
    let foreground = rgb(t.foreground);
    let muted = rgb(t.muted_foreground);
    let border = rgb(t.border);
    let black = if t.mode.is_dark() {
        background
    } else {
        foreground
    };
    let white = if t.mode.is_dark() { muted } else { border };
    let bright_white = if t.mode.is_dark() {
        foreground
    } else {
        background
    };
    let red = rgb(t.red);
    let red_b = rgb(t.red_light);
    let green = rgb(t.green);
    let green_b = rgb(t.green_light);
    let yellow = rgb(t.yellow);
    let yellow_b = rgb(t.yellow_light);
    let blue = rgb(t.blue);
    let blue_b = rgb(t.blue_light);
    let magenta = rgb(t.magenta);
    let magenta_b = rgb(t.magenta_light);
    let cyan = rgb(t.cyan);
    let cyan_b = rgb(t.cyan_light);
    let cursor = rgb(t.caret);
    ColorPalette::builder()
        .background(background.0, background.1, background.2)
        .foreground(foreground.0, foreground.1, foreground.2)
        .cursor(cursor.0, cursor.1, cursor.2)
        .black(black.0, black.1, black.2)
        .red(red.0, red.1, red.2)
        .green(green.0, green.1, green.2)
        .yellow(yellow.0, yellow.1, yellow.2)
        .blue(blue.0, blue.1, blue.2)
        .magenta(magenta.0, magenta.1, magenta.2)
        .cyan(cyan.0, cyan.1, cyan.2)
        .white(white.0, white.1, white.2)
        .bright_black(muted.0, muted.1, muted.2)
        .bright_red(red_b.0, red_b.1, red_b.2)
        .bright_green(green_b.0, green_b.1, green_b.2)
        .bright_yellow(yellow_b.0, yellow_b.1, yellow_b.2)
        .bright_blue(blue_b.0, blue_b.1, blue_b.2)
        .bright_magenta(magenta_b.0, magenta_b.1, magenta_b.2)
        .bright_cyan(cyan_b.0, cyan_b.1, cyan_b.2)
        .bright_white(bright_white.0, bright_white.1, bright_white.2)
        .build()
}
